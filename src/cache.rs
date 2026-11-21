//! File-based cache for cship module results.
//!
//! ## Passthrough cache (Story 4.2)
//! Path: `{dirname(transcript_path)}/cship/{transcript_stem}-starship-{module_name}`
//! TTL: 5 seconds via file mtime. Format: raw UTF-8 text.
//!
//! ## Account profile cache
//! Path: `{dirname(transcript_path)}/cship/{transcript_stem}-account-profile`
//! TTL: caller-supplied (default 24h) via `write_account_profile`'s `ttl_secs`.
//! Format: JSON envelope `{ "data": {...}, "expires_at": u64, "token_fingerprint": Option<String> }`
//! The OAuth token is NEVER written to any cache file (NFR-S3).
//!
//! Note: account-wide OAuth usage-limits data is no longer cached here — see
//! `usage_limits_state.rs` for that cross-process coordinator's own on-disk
//! state file and TTL/backoff semantics.

use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::account::AccountProfile;

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

// ── Account profile cache ────────────────────────────────────────────────────
//
// The `/api/oauth/profile` endpoint returns static-ish data (organization name,
// account display name, tier). It only changes when the user switches orgs, so
// this cache uses a long default TTL of 24h.
//
// Cache layout mirrors `usage_limits`: a JSON envelope keyed by transcript_path,
// stored alongside other cship caches in `{dirname}/cship/{stem}-account-profile`.
// The OAuth token is NEVER written to the cache (NFR-S3).

#[derive(serde::Serialize, serde::Deserialize)]
struct AccountProfileCacheEnvelope {
    data: AccountProfile,
    expires_at: u64,
    #[serde(default)]
    token_fingerprint: Option<String>,
}

/// Derive the cache file path for an account profile.
/// Example: `.../session.jsonl` → `.../cship/session-account-profile`
fn account_profile_cache_path(transcript_path: &Path) -> Option<std::path::PathBuf> {
    let dir = transcript_path.parent()?;
    let stem = transcript_path.file_stem()?.to_str()?;
    Some(dir.join("cship").join(format!("{stem}-account-profile")))
}

/// Read a cached account profile.
///
/// When `allow_stale` is `false`, returns `None` if the envelope's `expires_at`
/// has passed. When `allow_stale` is `true`, returns the most recent cached data
/// regardless of TTL — used as a fallback when a live fetch times out.
pub fn read_account_profile(
    transcript_path: &Path,
    allow_stale: bool,
    expected_fingerprint: Option<&str>,
) -> Option<AccountProfile> {
    let path = account_profile_cache_path(transcript_path)?;
    let raw = std::fs::read_to_string(&path).ok()?;
    let envelope: AccountProfileCacheEnvelope = serde_json::from_str(&raw).ok()?;
    if let Some(expected) = expected_fingerprint
        && envelope.token_fingerprint.as_deref() != Some(expected)
    {
        return None;
    }
    if allow_stale {
        return Some(envelope.data);
    }
    if now_epoch() >= envelope.expires_at {
        return None;
    }
    Some(envelope.data)
}

/// Write account profile data to the cache file.
/// Sets `expires_at` to now + `ttl_secs`. Silently no-ops on any I/O error —
/// cache write failure must never surface to the user.
pub fn write_account_profile(
    transcript_path: &Path,
    data: &AccountProfile,
    ttl_secs: u64,
    token_fingerprint: Option<&str>,
) {
    let Some(path) = account_profile_cache_path(transcript_path) else {
        return;
    };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let envelope = AccountProfileCacheEnvelope {
        data: data.clone(),
        expires_at: now_epoch() + ttl_secs,
        token_fingerprint: token_fingerprint.map(String::from),
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

    // ── iso8601_to_epoch() tests ──────────────────────────────────────────────
    // (still used by usage_limits_state::earliest_future_reset and
    // modules::usage_limits::resolve_epoch)

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

    // ── Account profile cache tests ───────────────────────────────────────────

    fn sample_profile() -> AccountProfile {
        AccountProfile {
            account_display_name: Some("Nils".into()),
            account_email: Some("nils@example.com".into()),
            organization_name: Some("Example Team".into()),
            organization_tier: Some("default_claude_max_5x".into()),
            organization_type: Some("claude_team".into()),
        }
    }

    #[test]
    fn test_account_profile_cache_hit_within_ttl() {
        let (dir, transcript) = temp_transcript("acct_hit");
        write_account_profile(&transcript, &sample_profile(), 86_400, None);
        let result = read_account_profile(&transcript, false, None);
        assert_eq!(result, Some(sample_profile()));
        drop(dir);
    }

    #[test]
    fn test_account_profile_cache_miss_nonexistent_file() {
        let (_dir, transcript) = temp_transcript("acct_miss");
        assert!(read_account_profile(&transcript, false, None).is_none());
    }

    #[test]
    fn test_account_profile_cache_file_path_and_json_structure() {
        let dir = tempfile::tempdir().expect("tempdir");
        let transcript = dir.path().join("transcript.jsonl");
        write_account_profile(&transcript, &sample_profile(), 86_400, None);
        let expected = dir.path().join("cship").join("transcript-account-profile");
        assert!(expected.exists(), "cache file at: {expected:?}");
        let raw = std::fs::read_to_string(&expected).unwrap();
        let v: serde_json::Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(v["data"]["account_display_name"], "Nils");
        assert_eq!(v["data"]["organization_name"], "Example Team");
        assert!(v["expires_at"].is_number());
        drop(dir);
    }

    #[test]
    fn test_account_profile_ttl_invalidation() {
        let dir = tempfile::tempdir().expect("tempdir");
        let transcript = dir.path().join("transcript.jsonl");
        write_account_profile(&transcript, &sample_profile(), 86_400, None);
        // Overwrite with expired envelope
        let path = dir.path().join("cship").join("transcript-account-profile");
        let expired = serde_json::json!({
            "data": {
                "account_display_name": "Nils",
                "account_email": null,
                "organization_name": "Example Team",
                "organization_tier": null,
                "organization_type": null
            },
            "expires_at": 0_u64
        });
        std::fs::write(&path, serde_json::to_string(&expired).unwrap()).unwrap();
        assert!(read_account_profile(&transcript, false, None).is_none());
        // Allow stale recovers data regardless
        let stale = read_account_profile(&transcript, true, None).unwrap();
        assert_eq!(stale.organization_name.as_deref(), Some("Example Team"));
        drop(dir);
    }

    #[test]
    fn test_account_profile_write_creates_directory() {
        let dir = tempfile::tempdir().expect("tempdir");
        let transcript = dir.path().join("deep").join("nested").join("t.jsonl");
        write_account_profile(&transcript, &sample_profile(), 86_400, None);
        let cache_file = dir
            .path()
            .join("deep")
            .join("nested")
            .join("cship")
            .join("t-account-profile");
        assert!(cache_file.exists(), "dir should be created: {cache_file:?}");
        drop(dir);
    }

    // ── Token fingerprint tests (account profile) ────────────────────────────

    #[test]
    fn test_account_profile_fingerprint_match_returns_data() {
        let dir = tempfile::tempdir().expect("tempdir");
        let transcript = dir.path().join("transcript.jsonl");
        write_account_profile(&transcript, &sample_profile(), 86_400, Some("fp_work"));
        let result = read_account_profile(&transcript, false, Some("fp_work"));
        assert!(result.is_some(), "matching fingerprint should return data");
        drop(dir);
    }

    #[test]
    fn test_account_profile_fingerprint_mismatch_returns_none() {
        let dir = tempfile::tempdir().expect("tempdir");
        let transcript = dir.path().join("transcript.jsonl");
        write_account_profile(&transcript, &sample_profile(), 86_400, Some("fp_work"));
        let result = read_account_profile(&transcript, false, Some("fp_personal"));
        assert!(
            result.is_none(),
            "mismatched fingerprint should return None"
        );
        drop(dir);
    }

    #[test]
    fn test_account_profile_fingerprint_mismatch_even_when_stale_allowed() {
        let dir = tempfile::tempdir().expect("tempdir");
        let transcript = dir.path().join("transcript.jsonl");
        write_account_profile(&transcript, &sample_profile(), 86_400, Some("fp_work"));
        let result = read_account_profile(&transcript, true, Some("fp_personal"));
        assert!(
            result.is_none(),
            "fingerprint mismatch should invalidate even with allow_stale"
        );
        drop(dir);
    }
}
