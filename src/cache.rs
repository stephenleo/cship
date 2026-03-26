//! File-based cache for cship module results.
//!
//! ## Passthrough cache (Story 4.2)
//! Path: `{dirname(transcript_path)}/cship/{transcript_stem}-starship-{module_name}`
//! TTL: 5 seconds via file mtime. Format: raw UTF-8 text.
//!
//! ## Usage limits cache (Story 5.2)
//! Path: `{dirname(transcript_path)}/cship/{transcript_stem}-usage-limits`
//! TTL: 60 seconds + early invalidation when a usage window resets.
//! Format: JSON envelope `{ "data": {...}, "expires_at": u64, "five_hour_resets_at": u64, "seven_day_resets_at": u64 }`
//! The OAuth token is NEVER written to any cache file (NFR-S3).

use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::usage_limits::UsageLimitsData;

const PASSTHROUGH_TTL: Duration = Duration::from_secs(5);

/// Derive the cache file path for a passthrough module.
/// Sanitizes `module_name` by replacing `/` and space with `_`.
fn passthrough_cache_path(module_name: &str, transcript_path: &Path) -> Option<std::path::PathBuf> {
    let dir = transcript_path.parent()?;
    let stem = transcript_path.file_stem()?.to_str()?;
    let safe_name = module_name.replace(['/', ' '], "_");
    Some(
        dir.join("cship")
            .join(format!("{stem}-starship-{safe_name}")),
    )
}

/// Read a cached passthrough value if it exists and is < 5 seconds old.
/// Returns None on cache miss, stale entry, or any I/O error.
pub fn read_passthrough(module_name: &str, transcript_path: &Path) -> Option<String> {
    let path = passthrough_cache_path(module_name, transcript_path)?;
    let metadata = std::fs::metadata(&path).ok()?;
    let modified = metadata.modified().ok()?;
    let age = SystemTime::now().duration_since(modified).ok()?;
    if age >= PASSTHROUGH_TTL {
        return None; // stale
    }
    std::fs::read_to_string(&path).ok()
}

/// Write a passthrough value to the cache file, creating the cache directory if needed.
/// Silently no-ops on any I/O error — cache write failure must never surface to the user.
pub fn write_passthrough(module_name: &str, transcript_path: &Path, content: &str) {
    if let Some(path) = passthrough_cache_path(module_name, transcript_path) {
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(path, content);
    }
}

// ── Usage limits cache ────────────────────────────────────────────────────────

/// Cache envelope stored on disk for usage limits data.
/// Envelope timestamps are Unix epoch seconds for cheap comparison.
/// The `data` field preserves ISO 8601 strings for rendering (Story 5.3).
#[derive(serde::Serialize, serde::Deserialize)]
struct UsageLimitsCacheEnvelope {
    data: UsageLimitsData,
    expires_at: u64,
    five_hour_resets_at: u64,
    seven_day_resets_at: u64,
}

/// Derive the cache file path for usage limits.
/// Example: `.../session.jsonl` → `.../cship/session-usage-limits`
fn usage_limits_cache_path(transcript_path: &Path) -> Option<std::path::PathBuf> {
    let dir = transcript_path.parent()?;
    let stem = transcript_path.file_stem()?.to_str()?;
    Some(dir.join("cship").join(format!("{stem}-usage-limits")))
}

/// Parse "YYYY-MM-DDTHH:MM:SSZ" to Unix epoch seconds using the Howard Hinnant
/// civil-date algorithm. Returns `None` on any parse failure.
pub(crate) fn iso8601_to_epoch(s: &str) -> Option<u64> {
    // Accept both 'Z' (e.g. "...T00:00:00Z") and '+00:00' (e.g. "...T04:59:59.943648+00:00")
    // — both are UTC. The Anthropic API uses '+00:00' in practice.
    let s = s
        .strip_suffix('Z')
        .or_else(|| s.strip_suffix("+00:00"))
        .unwrap_or(s);
    let (date_s, time_s) = s.split_once('T')?;
    let mut dp = date_s.split('-');
    let year: i64 = dp.next()?.parse().ok()?;
    let month: i64 = dp.next()?.parse().ok()?;
    let day: i64 = dp.next()?.parse().ok()?;
    let mut tp = time_s.split(':');
    let hour: i64 = tp.next()?.parse().ok()?;
    let min: i64 = tp.next()?.parse().ok()?;
    let sec: i64 = tp.next()?.split('.').next()?.parse().ok()?;
    // Howard Hinnant civil-to-days algorithm
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (month + if month > 2 { -3 } else { 9 }) + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146097 + doe - 719468;
    let total = days * 86400 + hour * 3600 + min * 60 + sec;
    u64::try_from(total).ok()
}

fn now_epoch() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Convert an ISO 8601 `resets_at` string to epoch seconds for cache comparison.
/// Returns `u64::MAX` when the input is empty or unparseable — meaning "no reset
/// scheduled, never trigger early invalidation via this field."
fn epoch_or_never(s: &str) -> u64 {
    if s.is_empty() {
        return u64::MAX;
    }
    iso8601_to_epoch(s).filter(|&e| e > 0).unwrap_or(u64::MAX)
}

/// Read a cached usage limits value.
///
/// When `allow_stale` is `false`, the cache is invalid (returns `None`) if:
/// 1. Current time ≥ `expires_at` (60 s TTL since last write), OR
/// 2. Current time ≥ `five_hour_resets_at` OR `seven_day_resets_at`
///    (ensures the display refreshes immediately when a usage window resets)
///
/// When `allow_stale` is `true`, returns the most recently written data regardless
/// of TTL or reset timestamps — used as a fallback when a live API fetch times out
/// so the statusline shows something meaningful rather than going blank.
pub fn read_usage_limits(transcript_path: &Path, allow_stale: bool) -> Option<UsageLimitsData> {
    let path = usage_limits_cache_path(transcript_path)?;
    let raw = std::fs::read_to_string(&path).ok()?;
    let envelope: UsageLimitsCacheEnvelope = serde_json::from_str(&raw).ok()?;
    if allow_stale {
        return Some(envelope.data);
    }
    let now = now_epoch();
    if now >= envelope.expires_at {
        return None; // TTL expired
    }
    if now >= envelope.five_hour_resets_at || now >= envelope.seven_day_resets_at {
        return None; // usage window reset — stale data
    }
    Some(envelope.data)
}

/// Write usage limits data to the cache file.
/// Sets `expires_at` to now + `ttl_secs` seconds (default 60).
/// Silently no-ops on any I/O error — cache write failure must never surface to the user.
/// The OAuth token is never present in the written data (NFR-S3).
pub fn write_usage_limits(transcript_path: &Path, data: &UsageLimitsData, ttl_secs: u64) {
    let Some(path) = usage_limits_cache_path(transcript_path) else {
        return;
    };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let now = now_epoch();
    let envelope = UsageLimitsCacheEnvelope {
        data: data.clone(),
        expires_at: now + ttl_secs,
        five_hour_resets_at: epoch_or_never(&data.five_hour_resets_at),
        seven_day_resets_at: epoch_or_never(&data.seven_day_resets_at),
    };
    if let Ok(json) = serde_json::to_string(&envelope) {
        let _ = std::fs::write(path, json);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn temp_transcript(subdir: &str) -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().expect("tempdir");
        let transcript = dir.path().join(subdir).join("test_transcript.jsonl");
        (dir, transcript)
    }

    #[test]
    fn test_cache_miss_returns_none_for_nonexistent_file() {
        let (_dir, transcript) = temp_transcript("session1");
        let result = read_passthrough("git_branch", &transcript);
        assert!(result.is_none());
    }

    #[test]
    fn test_cache_hit_returns_content_within_ttl() {
        let (dir, transcript) = temp_transcript("session2");
        // Write directly to the expected cache path
        write_passthrough("git_branch", &transcript, "main");
        // Immediately read back — should be within TTL
        let result = read_passthrough("git_branch", &transcript);
        assert_eq!(result, Some("main".to_string()));
        drop(dir);
    }

    #[test]
    fn test_write_creates_directory_if_missing() {
        let dir = tempfile::tempdir().expect("tempdir");
        // transcript in a subdir that doesn't exist yet
        let transcript = dir
            .path()
            .join("deep")
            .join("nested")
            .join("transcript.jsonl");
        write_passthrough("directory", &transcript, "/home/user");
        // Verify the cache file was created
        let cache_file = dir
            .path()
            .join("deep")
            .join("nested")
            .join("cship")
            .join("transcript-starship-directory");
        assert!(cache_file.exists(), "cache file should have been created");
        let content = std::fs::read_to_string(&cache_file).unwrap();
        assert_eq!(content, "/home/user");
    }

    #[test]
    fn test_path_derivation() {
        // Verify the derived path matches the expected scheme
        let dir = tempfile::tempdir().expect("tempdir");
        let transcript = dir.path().join("transcript.jsonl");
        write_passthrough("git_branch", &transcript, "main");
        let expected = dir
            .path()
            .join("cship")
            .join("transcript-starship-git_branch");
        assert!(
            expected.exists(),
            "cache file at expected path: {expected:?}"
        );
    }

    #[test]
    fn test_module_name_sanitization() {
        let dir = tempfile::tempdir().expect("tempdir");
        let transcript = dir.path().join("transcript.jsonl");
        // Module name with slash and space
        write_passthrough("node/js lang", &transcript, "v20");
        let expected = dir
            .path()
            .join("cship")
            .join("transcript-starship-node_js_lang");
        assert!(
            expected.exists(),
            "sanitized path should exist: {expected:?}"
        );
        let content = std::fs::read_to_string(&expected).unwrap();
        assert_eq!(content, "v20");
    }

    #[test]
    fn test_stale_cache_returns_none() {
        use std::time::{Duration, SystemTime};

        let dir = tempfile::tempdir().expect("tempdir");
        let transcript = dir.path().join("transcript.jsonl");
        write_passthrough("git_branch", &transcript, "main");

        // Manually set the file mtime to 10 seconds in the past
        let cache_file = dir
            .path()
            .join("cship")
            .join("transcript-starship-git_branch");
        let stale_time = SystemTime::now() - Duration::from_secs(10);
        filetime::set_file_mtime(
            &cache_file,
            filetime::FileTime::from_system_time(stale_time),
        )
        .expect("set mtime");

        let result = read_passthrough("git_branch", &transcript);
        assert!(result.is_none(), "stale cache should return None");
    }

    // ── Usage limits cache tests ──────────────────────────────────────────────

    fn sample_data() -> UsageLimitsData {
        UsageLimitsData {
            five_hour_pct: 23.4,
            seven_day_pct: 45.1,
            five_hour_resets_at: "2099-01-01T00:00:00Z".into(),
            seven_day_resets_at: "2099-01-01T00:00:00Z".into(),
            five_hour_resets_at_epoch: None,
            seven_day_resets_at_epoch: None,
        }
    }

    #[test]
    fn test_usage_limits_cache_hit_within_ttl() {
        let (dir, transcript) = temp_transcript("s5_2_hit");
        write_usage_limits(&transcript, &sample_data(), 60);
        let result = read_usage_limits(&transcript, false);
        assert!(result.is_some(), "fresh cache should return Some");
        let data = result.unwrap();
        assert!((data.five_hour_pct - 23.4).abs() < f64::EPSILON);
        assert!((data.seven_day_pct - 45.1).abs() < f64::EPSILON);
        drop(dir);
    }

    #[test]
    fn test_usage_limits_cache_miss_nonexistent_file() {
        let (_dir, transcript) = temp_transcript("s5_2_miss");
        let result = read_usage_limits(&transcript, false);
        assert!(
            result.is_none(),
            "nonexistent cache file should return None"
        );
    }

    #[test]
    fn test_usage_limits_cache_file_path_and_json_structure() {
        let dir = tempfile::tempdir().expect("tempdir");
        let transcript = dir.path().join("transcript.jsonl");
        write_usage_limits(&transcript, &sample_data(), 60);
        let expected_path = dir.path().join("cship").join("transcript-usage-limits");
        assert!(expected_path.exists(), "cache file at: {expected_path:?}");
        let raw = std::fs::read_to_string(&expected_path).unwrap();
        let v: serde_json::Value = serde_json::from_str(&raw).unwrap();
        assert!(v["data"]["five_hour_pct"].is_number());
        assert!(v["data"]["seven_day_pct"].is_number());
        assert!(v["data"]["five_hour_resets_at"].is_string());
        assert!(v["data"]["seven_day_resets_at"].is_string());
        assert!(v["expires_at"].is_number());
        drop(dir);
    }

    #[test]
    fn test_usage_limits_ttl_invalidation() {
        let dir = tempfile::tempdir().expect("tempdir");
        let transcript = dir.path().join("transcript.jsonl");
        // Write a valid cache entry first
        write_usage_limits(&transcript, &sample_data(), 60);
        // Overwrite with an expired envelope (expires_at = 0, resets_at far future)
        let path = dir.path().join("cship").join("transcript-usage-limits");
        let expired = serde_json::json!({
            "data": {
                "five_hour_pct": 23.4,
                "seven_day_pct": 45.1,
                "five_hour_resets_at": "2099-01-01T00:00:00Z",
                "seven_day_resets_at": "2099-01-01T00:00:00Z"
            },
            "expires_at": 0_u64,
            "five_hour_resets_at": 9_999_999_999_u64,
            "seven_day_resets_at": 9_999_999_999_u64
        });
        std::fs::write(&path, serde_json::to_string(&expired).unwrap()).unwrap();
        let result = read_usage_limits(&transcript, false);
        assert!(result.is_none(), "expired TTL should return None");
        drop(dir);
    }

    #[test]
    fn test_usage_limits_resets_at_early_invalidation() {
        let dir = tempfile::tempdir().expect("tempdir");
        let transcript = dir.path().join("transcript.jsonl");
        // five_hour_resets_at is in the past — should invalidate even within 60s TTL
        let data = UsageLimitsData {
            five_hour_pct: 50.0,
            seven_day_pct: 10.0,
            five_hour_resets_at: "2000-01-01T00:00:00Z".into(), // past
            seven_day_resets_at: "2099-01-01T00:00:00Z".into(), // future
            five_hour_resets_at_epoch: None,
            seven_day_resets_at_epoch: None,
        };
        write_usage_limits(&transcript, &data, 60);
        let result = read_usage_limits(&transcript, false);
        assert!(
            result.is_none(),
            "past five_hour_resets_at should invalidate cache"
        );
        drop(dir);
    }

    #[test]
    fn test_usage_limits_write_creates_directory() {
        let dir = tempfile::tempdir().expect("tempdir");
        let transcript = dir.path().join("deep").join("nested").join("t.jsonl");
        write_usage_limits(&transcript, &sample_data(), 60);
        let cache_file = dir
            .path()
            .join("deep")
            .join("nested")
            .join("cship")
            .join("t-usage-limits");
        assert!(
            cache_file.exists(),
            "directory should be created: {cache_file:?}"
        );
        drop(dir);
    }

    #[test]
    fn test_usage_limits_seven_day_resets_at_early_invalidation() {
        let dir = tempfile::tempdir().expect("tempdir");
        let transcript = dir.path().join("transcript.jsonl");
        // seven_day_resets_at is in the past, five_hour is in the future
        let data = UsageLimitsData {
            five_hour_pct: 50.0,
            seven_day_pct: 10.0,
            five_hour_resets_at: "2099-01-01T00:00:00Z".into(), // future
            seven_day_resets_at: "2000-01-01T00:00:00Z".into(), // past
            five_hour_resets_at_epoch: None,
            seven_day_resets_at_epoch: None,
        };
        write_usage_limits(&transcript, &data, 60);
        let result = read_usage_limits(&transcript, false);
        assert!(
            result.is_none(),
            "past seven_day_resets_at should invalidate cache"
        );
        drop(dir);
    }

    #[test]
    fn test_usage_limits_empty_resets_at_does_not_invalidate() {
        let dir = tempfile::tempdir().expect("tempdir");
        let transcript = dir.path().join("transcript.jsonl");
        // Both resets_at are empty (API returned null) — cache should still be valid within TTL
        let data = UsageLimitsData {
            five_hour_pct: 50.0,
            seven_day_pct: 10.0,
            five_hour_resets_at: String::new(),
            seven_day_resets_at: String::new(),
            five_hour_resets_at_epoch: None,
            seven_day_resets_at_epoch: None,
        };
        write_usage_limits(&transcript, &data, 60);
        let result = read_usage_limits(&transcript, false);
        assert!(
            result.is_some(),
            "empty resets_at should not trigger early invalidation"
        );
        drop(dir);
    }

    #[test]
    fn test_read_usage_limits_allow_stale_returns_expired_data() {
        let dir = tempfile::tempdir().expect("tempdir");
        let transcript = dir.path().join("transcript.jsonl");
        // Write a valid cache entry, then overwrite with expired TTL
        write_usage_limits(&transcript, &sample_data(), 60);
        let path = dir.path().join("cship").join("transcript-usage-limits");
        let expired = serde_json::json!({
            "data": {
                "five_hour_pct": 77.0,
                "seven_day_pct": 88.0,
                "five_hour_resets_at": "2099-01-01T00:00:00Z",
                "seven_day_resets_at": "2099-01-01T00:00:00Z"
            },
            "expires_at": 0_u64,           // expired
            "five_hour_resets_at": 9_999_999_999_u64,
            "seven_day_resets_at": 9_999_999_999_u64
        });
        std::fs::write(&path, serde_json::to_string(&expired).unwrap()).unwrap();
        // Normal read returns None (TTL expired)
        assert!(
            read_usage_limits(&transcript, false).is_none(),
            "normal read should be None"
        );
        // Stale read returns data regardless
        let stale = read_usage_limits(&transcript, true);
        assert!(stale.is_some(), "stale read should return data");
        assert!((stale.unwrap().five_hour_pct - 77.0).abs() < f64::EPSILON);
        drop(dir);
    }

    #[test]
    fn test_read_usage_limits_allow_stale_returns_none_when_no_file() {
        let (_dir, transcript) = temp_transcript("stale_miss");
        assert!(read_usage_limits(&transcript, true).is_none());
    }

    #[test]
    fn test_iso8601_to_epoch_known_value() {
        // 2000-01-01T00:00:00Z = 946,684,800 seconds since epoch
        assert_eq!(iso8601_to_epoch("2000-01-01T00:00:00Z"), Some(946_684_800));
    }

    #[test]
    fn test_iso8601_to_epoch_invalid_returns_none() {
        assert_eq!(iso8601_to_epoch("not-a-date"), None);
        assert_eq!(iso8601_to_epoch(""), None);
    }

    #[test]
    fn test_iso8601_to_epoch_plus_offset_format() {
        // Anthropic API returns "+00:00" suffix, not "Z" — must parse to same epoch as Z form
        assert_eq!(
            iso8601_to_epoch("2000-01-01T00:00:00+00:00"),
            Some(946_684_800),
            "+00:00 format should parse to same epoch as Z form"
        );
        assert_eq!(
            iso8601_to_epoch("2000-01-01T00:00:01.943648+00:00"),
            Some(946_684_801),
            "fractional seconds with +00:00 should be truncated"
        );
    }

    #[test]
    fn test_usage_limits_early_invalidation_with_plus_offset_resets_at() {
        // Real API returns "+00:00" format — early invalidation must fire correctly
        let dir = tempfile::tempdir().expect("tempdir");
        let transcript = dir.path().join("transcript.jsonl");
        // five_hour_resets_at is in the past but uses +00:00 format
        let data = UsageLimitsData {
            five_hour_pct: 50.0,
            seven_day_pct: 10.0,
            five_hour_resets_at: "2000-01-01T00:00:00+00:00".into(), // past, +00:00 format
            seven_day_resets_at: "2099-01-01T00:00:00+00:00".into(), // future
            five_hour_resets_at_epoch: None,
            seven_day_resets_at_epoch: None,
        };
        write_usage_limits(&transcript, &data, 60);
        let result = read_usage_limits(&transcript, false);
        assert!(
            result.is_none(),
            "past five_hour_resets_at (+00:00 format) should invalidate cache"
        );
        drop(dir);
    }

    #[test]
    fn test_iso8601_to_epoch_fractional_seconds() {
        // Sub-second precision must parse to the same epoch as the whole-second form
        assert_eq!(
            iso8601_to_epoch("2000-01-01T00:00:01.000Z"),
            Some(946_684_801),
            "fractional-second timestamp should parse correctly"
        );
        assert_eq!(
            iso8601_to_epoch("2000-01-01T00:00:01.999Z"),
            Some(946_684_801),
            "fractional seconds are truncated, not rounded"
        );
    }

    #[test]
    fn test_usage_limits_custom_ttl_sets_expires_at() {
        // Issue #95: configurable TTL — verify custom TTL is respected in cache envelope
        let dir = tempfile::tempdir().expect("tempdir");
        let transcript = dir.path().join("transcript.jsonl");
        write_usage_limits(&transcript, &sample_data(), 300);
        let path = dir.path().join("cship").join("transcript-usage-limits");
        let raw = std::fs::read_to_string(&path).unwrap();
        let v: serde_json::Value = serde_json::from_str(&raw).unwrap();
        let expires_at = v["expires_at"].as_u64().unwrap();
        let now = now_epoch();
        // expires_at should be approximately now + 300 (±2s tolerance for test execution)
        assert!(
            expires_at >= now + 298 && expires_at <= now + 302,
            "expected expires_at ~now+300, got delta={}",
            expires_at.saturating_sub(now)
        );
        // Cache should still be valid (not expired within the custom window)
        let result = read_usage_limits(&transcript, false);
        assert!(result.is_some(), "cache with 300s TTL should be valid");
        drop(dir);
    }
}
