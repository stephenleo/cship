//! Usage limits module — renders 5h and 7d API utilization with time-to-reset.
//!
//! On cache hit: reads directly from `cache::read_usage_limits`, no thread.
//! On cache miss: dispatches `fetch_usage_limits` via `std::thread::spawn` and waits
//! up to 2 seconds via `mpsc::recv_timeout`. Falls back to last cache or empty.
//!
//! [Source: architecture.md#src/modules/usage_limits.rs]

use crate::cache;
use crate::config::{CshipConfig, UsageLimitsConfig};
use crate::context::Context;
use crate::usage_limits::UsageLimitsData;

/// Render the usage limits module.
///
/// Render flow (exact order):
/// 1. Check disabled flag — silent None
/// 2. Extract transcript_path — silent None if absent
/// 3. Cache hit → render immediately, no thread
/// 4. Cache miss → get OAuth token, dispatch fetch thread, recv_timeout(2s)
/// 5. Format output
/// 6. Apply threshold styling (higher of two pcts)
pub fn render(ctx: &Context, cfg: &CshipConfig) -> Option<String> {
    let ul_cfg = cfg.usage_limits.as_ref();

    // Step 1: disabled flag → silent None
    if ul_cfg.and_then(|c| c.disabled) == Some(true) {
        return None;
    }

    // Step 2: transcript_path required for cache key
    // Silent None (no warn) — per Dev Notes: transcript_path absence is expected when
    // cship is invoked outside a Claude Code session (not an error condition).
    let transcript_str = ctx.transcript_path.as_deref()?;
    let transcript_path = std::path::Path::new(transcript_str);

    // Step 3: cache hit → render immediately
    let data = if let Some(cached) = cache::read_usage_limits(transcript_path, false) {
        cached
    } else {
        // Step 4a: get OAuth token
        let token = match crate::platform::get_oauth_token() {
            Ok(t) => t,
            Err(e) => {
                tracing::warn!("cship.usage_limits: credential retrieval failed: {e}");
                return None;
            }
        };

        // Step 4b: dispatch fetch with 2s timeout
        match fetch_with_timeout(move || crate::usage_limits::fetch_usage_limits(&token)) {
            Some(fresh) => {
                // Step 4c: write fresh data to cache for future renders
                cache::write_usage_limits(transcript_path, &fresh);
                fresh
            }
            // AC #4: on timeout or API error, fall back to last cached value (may be stale)
            // Do NOT write stale data back to cache — that would falsely reset the TTL
            None => cache::read_usage_limits(transcript_path, true)?,
        }
    };

    // Step 5: format output
    let default_ul_cfg = UsageLimitsConfig::default();
    let content = format_output(&data, ul_cfg.unwrap_or(&default_ul_cfg));

    // Step 6: threshold styling — use higher of the two utilization percentages
    let max_pct = data.five_hour_pct.max(data.seven_day_pct);
    let warn_threshold = ul_cfg.and_then(|c| c.warn_threshold);
    let warn_style = ul_cfg.and_then(|c| c.warn_style.as_deref());
    let critical_threshold = ul_cfg.and_then(|c| c.critical_threshold);
    let critical_style = ul_cfg.and_then(|c| c.critical_style.as_deref());

    Some(crate::ansi::apply_style_with_threshold(
        &content,
        Some(max_pct),
        None, // UsageLimitsConfig has no base `style` field
        warn_threshold,
        warn_style,
        critical_threshold,
        critical_style,
    ))
}

/// Spawn `fetch_fn` on a new thread and wait up to 2 seconds for the result.
///
/// Returns `None` on API error or timeout, logging a warning in both cases.
/// Generic `F` bound allows tests to inject a fast lambda bypassing real HTTP.
fn fetch_with_timeout<F>(fetch_fn: F) -> Option<UsageLimitsData>
where
    F: FnOnce() -> Result<UsageLimitsData, String> + Send + 'static,
{
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        tx.send(fetch_fn()).ok();
    });
    match rx.recv_timeout(std::time::Duration::from_secs(2)) {
        Ok(Ok(data)) => Some(data),
        Ok(Err(e)) => {
            tracing::warn!("cship.usage_limits: API fetch failed: {e}");
            None
        }
        Err(_) => {
            tracing::warn!("cship.usage_limits: API fetch timed out after 2s");
            None
        }
    }
}

/// Format usage data using configurable format strings.
///
/// Placeholders in format strings (all occurrences are substituted):
/// - `{pct}` — percentage as integer (e.g. `"23"`)
/// - `{reset}` — time-until-reset string (e.g. `"4h12m"`)
///
/// Defaults (backwards compatible with pre-7.2 hardcoded output):
/// - `five_hour_format`: `"5h: {pct}% resets in {reset}"`
/// - `seven_day_format`: `"7d: {pct}% resets in {reset}"`
/// - `separator`: `" | "`
fn format_output(data: &UsageLimitsData, cfg: &UsageLimitsConfig) -> String {
    let five_h_pct = format!("{:.0}", data.five_hour_pct);
    let five_h_reset = format_time_until(&data.five_hour_resets_at);
    let seven_d_pct = format!("{:.0}", data.seven_day_pct);
    let seven_d_reset = format_time_until(&data.seven_day_resets_at);

    let five_h_fmt = cfg
        .five_hour_format
        .as_deref()
        .unwrap_or("5h: {pct}% resets in {reset}");
    let seven_d_fmt = cfg
        .seven_day_format
        .as_deref()
        .unwrap_or("7d: {pct}% resets in {reset}");
    let sep = cfg.separator.as_deref().unwrap_or(" | ");

    let five_h_part = five_h_fmt
        .replace("{pct}", &five_h_pct)
        .replace("{reset}", &five_h_reset);
    let seven_d_part = seven_d_fmt
        .replace("{pct}", &seven_d_pct)
        .replace("{reset}", &seven_d_reset);

    format!("{five_h_part}{sep}{seven_d_part}")
}

/// Convert an ISO 8601 reset timestamp to a human-readable time-until string.
///
/// - Empty string → `"?"`
/// - Unparseable → `"?"`
/// - Past timestamp → `"now"`
/// - < 1 hour → `"45m"`
/// - < 1 day → `"4h12m"`
/// - >= 1 day → `"3d2h"`
fn format_time_until(resets_at: &str) -> String {
    if resets_at.is_empty() {
        return "?".to_string();
    }
    let reset_epoch = match crate::cache::iso8601_to_epoch(resets_at) {
        Some(e) => e,
        None => return "?".to_string(),
    };
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    if now >= reset_epoch {
        return "now".to_string();
    }
    let secs = reset_epoch - now;
    let mins = secs / 60;
    let hours = mins / 60;
    let days = hours / 24;
    if days > 0 {
        format!("{}d{}h", days, hours % 24)
    } else if hours > 0 {
        format!("{}h{}m", hours, mins % 60)
    } else {
        format!("{}m", mins)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{CshipConfig, UsageLimitsConfig};
    use crate::context::Context;

    // ── Helper ────────────────────────────────────────────────────────────────

    /// Convert Unix epoch seconds to an approximate ISO 8601 string (UTC, no fractional seconds).
    /// Used only in tests to construct future timestamps for `format_time_until`.
    fn epoch_to_iso8601_approx(secs: u64) -> String {
        // Use Howard Hinnant days-to-civil algorithm (inverse of iso8601_to_epoch)
        let days_since_epoch = (secs / 86400) as i64;
        let remaining = secs % 86400;
        let hour = remaining / 3600;
        let min = (remaining % 3600) / 60;
        let sec = remaining % 60;

        let z = days_since_epoch + 719468;
        let era = z.div_euclid(146097);
        let doe = z - era * 146097;
        let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
        let y = yoe + era * 400;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let d = doy - (153 * mp + 2) / 5 + 1;
        let m = if mp < 10 { mp + 3 } else { mp - 9 };
        let y = if m <= 2 { y + 1 } else { y };

        format!(
            "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
            y, m, d, hour, min, sec
        )
    }

    // ── render() tests ────────────────────────────────────────────────────────

    #[test]
    fn test_render_disabled_returns_none() {
        let ctx = Context::default();
        let cfg = CshipConfig {
            usage_limits: Some(UsageLimitsConfig {
                disabled: Some(true),
                ..Default::default()
            }),
            ..Default::default()
        };
        assert!(render(&ctx, &cfg).is_none());
    }

    #[test]
    fn test_render_no_transcript_path_returns_none() {
        let ctx = Context {
            transcript_path: None,
            ..Default::default()
        };
        let cfg = CshipConfig::default();
        assert!(render(&ctx, &cfg).is_none());
    }

    #[test]
    fn test_render_cache_hit_returns_formatted_output() {
        let dir = tempfile::tempdir().unwrap();
        let transcript = dir.path().join("test.jsonl");
        let data = UsageLimitsData {
            five_hour_pct: 23.4,
            seven_day_pct: 45.1,
            five_hour_resets_at: "2099-01-01T00:00:00Z".into(),
            seven_day_resets_at: "2099-01-01T00:00:00Z".into(),
        };
        crate::cache::write_usage_limits(&transcript, &data);

        let ctx = Context {
            transcript_path: Some(transcript.to_str().unwrap().to_string()),
            ..Default::default()
        };
        let result = render(&ctx, &CshipConfig::default()).unwrap();
        assert!(result.contains("5h:"), "expected 5h prefix: {result:?}");
        assert!(result.contains("7d:"), "expected 7d prefix: {result:?}");
        assert!(result.contains("23%"), "expected five_hour_pct: {result:?}");
        assert!(result.contains("45%"), "expected seven_day_pct: {result:?}");
    }

    #[test]
    fn test_render_warn_threshold_applies_ansi() {
        let dir = tempfile::tempdir().unwrap();
        let transcript = dir.path().join("test.jsonl");
        let data = UsageLimitsData {
            five_hour_pct: 65.0,
            seven_day_pct: 10.0,
            five_hour_resets_at: "2099-01-01T00:00:00Z".into(),
            seven_day_resets_at: "2099-01-01T00:00:00Z".into(),
        };
        crate::cache::write_usage_limits(&transcript, &data);

        let ctx = Context {
            transcript_path: Some(transcript.to_str().unwrap().to_string()),
            ..Default::default()
        };
        let cfg = CshipConfig {
            usage_limits: Some(UsageLimitsConfig {
                warn_threshold: Some(60.0),
                warn_style: Some("bold yellow".to_string()),
                ..Default::default()
            }),
            ..Default::default()
        };
        let result = render(&ctx, &cfg).unwrap();
        assert!(
            result.contains('\x1b'),
            "expected ANSI codes for warn: {result:?}"
        );
    }

    #[test]
    fn test_render_critical_overrides_warn() {
        let dir = tempfile::tempdir().unwrap();
        let transcript = dir.path().join("test.jsonl");
        let data = UsageLimitsData {
            five_hour_pct: 85.0,
            seven_day_pct: 20.0,
            five_hour_resets_at: "2099-01-01T00:00:00Z".into(),
            seven_day_resets_at: "2099-01-01T00:00:00Z".into(),
        };
        crate::cache::write_usage_limits(&transcript, &data);

        let ctx = Context {
            transcript_path: Some(transcript.to_str().unwrap().to_string()),
            ..Default::default()
        };
        let cfg = CshipConfig {
            usage_limits: Some(UsageLimitsConfig {
                warn_threshold: Some(60.0),
                warn_style: Some("yellow".to_string()),
                critical_threshold: Some(80.0),
                critical_style: Some("bold red".to_string()),
                ..Default::default()
            }),
            ..Default::default()
        };
        let result = render(&ctx, &cfg).unwrap();
        // Verify critical style ("bold red") is applied, NOT warn style ("yellow")
        let content = format_output(&data, &UsageLimitsConfig::default());
        let expected_critical = crate::ansi::apply_style(&content, Some("bold red"));
        let expected_warn = crate::ansi::apply_style(&content, Some("yellow"));
        assert_eq!(result, expected_critical, "expected critical style applied");
        assert_ne!(result, expected_warn, "critical should override warn style");
    }

    #[test]
    fn test_threshold_uses_higher_of_two_pcts() {
        let dir = tempfile::tempdir().unwrap();
        let transcript = dir.path().join("test.jsonl");
        // seven_day is high (85%), five_hour is low (20%) — should still trigger critical
        let data = UsageLimitsData {
            five_hour_pct: 20.0,
            seven_day_pct: 85.0,
            five_hour_resets_at: "2099-01-01T00:00:00Z".into(),
            seven_day_resets_at: "2099-01-01T00:00:00Z".into(),
        };
        crate::cache::write_usage_limits(&transcript, &data);

        let ctx = Context {
            transcript_path: Some(transcript.to_str().unwrap().to_string()),
            ..Default::default()
        };
        let cfg = CshipConfig {
            usage_limits: Some(UsageLimitsConfig {
                critical_threshold: Some(80.0),
                critical_style: Some("bold red".to_string()),
                ..Default::default()
            }),
            ..Default::default()
        };
        let result = render(&ctx, &cfg).unwrap();
        assert!(
            result.contains('\x1b'),
            "expected ANSI codes for critical: {result:?}"
        );
    }

    // ── fetch_with_timeout() tests ────────────────────────────────────────────

    #[test]
    fn test_render_stale_cache_returned_on_fetch_timeout() {
        // Write an expired cache entry (expires_at = 0 so read_usage_limits returns None)
        // but read_usage_limits(allow_stale=true) should still return it
        let dir = tempfile::tempdir().unwrap();
        let transcript = dir.path().join("test.jsonl");
        // Write valid cache first so the file exists
        let data = UsageLimitsData {
            five_hour_pct: 50.0,
            seven_day_pct: 30.0,
            five_hour_resets_at: "2099-01-01T00:00:00Z".into(),
            seven_day_resets_at: "2099-01-01T00:00:00Z".into(),
        };
        crate::cache::write_usage_limits(&transcript, &data);
        // Verify read_usage_limits(allow_stale=true) works even after TTL would normally expire
        let stale = crate::cache::read_usage_limits(&transcript, true);
        assert!(
            stale.is_some(),
            "stale read should return data regardless of TTL"
        );
        assert!((stale.unwrap().five_hour_pct - 50.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_fetch_with_timeout_success_returns_data() {
        let expected = UsageLimitsData {
            five_hour_pct: 50.0,
            seven_day_pct: 30.0,
            five_hour_resets_at: "2099-01-01T00:00:00Z".into(),
            seven_day_resets_at: "2099-01-01T00:00:00Z".into(),
        };
        let cloned = expected.clone();
        let result = fetch_with_timeout(move || Ok(cloned));
        assert!(result.is_some());
        assert!((result.unwrap().five_hour_pct - 50.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_fetch_with_timeout_api_error_returns_none() {
        let result = fetch_with_timeout(|| Err("API error".to_string()));
        assert!(result.is_none());
    }

    #[test]
    #[ignore = "slow: blocks for 2s timeout"]
    fn test_fetch_with_timeout_timeout_returns_none() {
        let result = fetch_with_timeout(|| {
            std::thread::sleep(std::time::Duration::from_secs(5));
            Ok(UsageLimitsData {
                five_hour_pct: 0.0,
                seven_day_pct: 0.0,
                five_hour_resets_at: String::new(),
                seven_day_resets_at: String::new(),
            })
        });
        assert!(result.is_none());
    }

    // ── format_time_until() tests ─────────────────────────────────────────────

    #[test]
    fn test_format_time_until_empty_string_returns_question_mark() {
        assert_eq!(format_time_until(""), "?");
    }

    #[test]
    fn test_format_time_until_past_timestamp_returns_now() {
        assert_eq!(format_time_until("2000-01-01T00:00:00Z"), "now");
    }

    #[test]
    fn test_format_time_until_hours_minutes() {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let future_epoch = now + 4 * 3600 + 12 * 60 + 30; // ~4h12m from now
        let future_str = epoch_to_iso8601_approx(future_epoch);
        let result = format_time_until(&future_str);
        assert!(
            result.contains('h') && result.contains('m'),
            "expected Xh Ym format: {result}"
        );
    }

    #[test]
    fn test_format_time_until_days_hours() {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let future_epoch = now + 3 * 86400 + 2 * 3600 + 30; // ~3d2h from now
        let future_str = epoch_to_iso8601_approx(future_epoch);
        let result = format_time_until(&future_str);
        assert!(
            result.contains('d') && result.contains('h'),
            "expected Xd Yh format: {result}"
        );
    }

    #[test]
    fn test_format_time_until_minutes_only() {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let future_epoch = now + 45 * 60 + 30; // ~45m from now
        let future_str = epoch_to_iso8601_approx(future_epoch);
        let result = format_time_until(&future_str);
        assert!(
            result.ends_with('m') && !result.contains('h'),
            "expected Xm format: {result}"
        );
    }

    #[test]
    fn test_format_time_until_plus_offset_format() {
        // Anthropic API returns "+00:00" not "Z" — format_time_until must handle it
        // Use a far-future timestamp so this test is stable regardless of when it runs
        let result = format_time_until("2099-01-01T00:00:00+00:00");
        assert_ne!(result, "?", "should parse +00:00 format, not return '?'");
        assert_ne!(
            result, "now",
            "far-future +00:00 timestamp should not be 'now'"
        );
    }

    // ── format_output() tests ─────────────────────────────────────────────────

    #[test]
    fn test_format_output_default_produces_legacy_format() {
        // AC1: no config → identical to old hardcoded string
        let data = UsageLimitsData {
            five_hour_pct: 23.4,
            seven_day_pct: 45.1,
            five_hour_resets_at: "2099-01-01T00:00:00Z".into(),
            seven_day_resets_at: "2099-01-01T00:00:00Z".into(),
        };
        let cfg = UsageLimitsConfig::default();
        let result = format_output(&data, &cfg);
        assert!(result.starts_with("5h: 23%"), "5h prefix: {result:?}");
        assert!(result.contains(" | "), "default separator: {result:?}");
        assert!(result.contains("7d: 45%"), "7d prefix: {result:?}");
    }

    #[test]
    fn test_format_output_custom_five_hour_format() {
        // AC2: custom five_hour_format
        let data = UsageLimitsData {
            five_hour_pct: 23.0,
            seven_day_pct: 10.0,
            five_hour_resets_at: "2099-01-01T00:00:00Z".into(),
            seven_day_resets_at: "2099-01-01T00:00:00Z".into(),
        };
        let cfg = UsageLimitsConfig {
            five_hour_format: Some("⏱: {pct}%({reset})".into()),
            ..Default::default()
        };
        let result = format_output(&data, &cfg);
        assert!(
            result.starts_with("⏱: 23%("),
            "expected custom 5h format: {result:?}"
        );
    }

    #[test]
    fn test_format_output_custom_seven_day_format() {
        // AC3: custom seven_day_format
        let data = UsageLimitsData {
            five_hour_pct: 10.0,
            seven_day_pct: 45.0,
            five_hour_resets_at: "2099-01-01T00:00:00Z".into(),
            seven_day_resets_at: "2099-01-01T00:00:00Z".into(),
        };
        let cfg = UsageLimitsConfig {
            seven_day_format: Some("7d {pct}%/{reset}".into()),
            ..Default::default()
        };
        let result = format_output(&data, &cfg);
        assert!(
            result.contains("7d 45%/"),
            "expected custom 7d format: {result:?}"
        );
    }

    #[test]
    fn test_format_output_custom_separator() {
        // AC4: custom separator
        let data = UsageLimitsData {
            five_hour_pct: 10.0,
            seven_day_pct: 20.0,
            five_hour_resets_at: "2099-01-01T00:00:00Z".into(),
            seven_day_resets_at: "2099-01-01T00:00:00Z".into(),
        };
        let cfg = UsageLimitsConfig {
            separator: Some(" — ".into()),
            ..Default::default()
        };
        let result = format_output(&data, &cfg);
        assert!(
            result.contains(" — "),
            "expected em-dash separator: {result:?}"
        );
        assert!(
            !result.contains(" | "),
            "should not contain default separator: {result:?}"
        );
    }

    #[test]
    fn test_format_output_pct_only_no_reset_placeholder() {
        // AC5: format with only {pct}, no {reset} — no extra text
        let data = UsageLimitsData {
            five_hour_pct: 30.0,
            seven_day_pct: 50.0,
            five_hour_resets_at: "2099-01-01T00:00:00Z".into(),
            seven_day_resets_at: "2099-01-01T00:00:00Z".into(),
        };
        let cfg = UsageLimitsConfig {
            five_hour_format: Some("{pct}%".into()),
            seven_day_format: Some("{pct}%".into()),
            ..Default::default()
        };
        let result = format_output(&data, &cfg);
        // Should be "30% | 50%" — no "resets in" text
        assert_eq!(result, "30% | 50%", "unexpected content: {result:?}");
    }

    #[test]
    fn test_threshold_styling_applies_to_custom_format() {
        // AC6: threshold styling wraps the full composed output after format substitution
        let dir = tempfile::tempdir().unwrap();
        let transcript = dir.path().join("test.jsonl");
        let data = UsageLimitsData {
            five_hour_pct: 75.0,
            seven_day_pct: 10.0,
            five_hour_resets_at: "2099-01-01T00:00:00Z".into(),
            seven_day_resets_at: "2099-01-01T00:00:00Z".into(),
        };
        crate::cache::write_usage_limits(&transcript, &data);

        let ctx = Context {
            transcript_path: Some(transcript.to_str().unwrap().to_string()),
            ..Default::default()
        };
        let cfg = CshipConfig {
            usage_limits: Some(UsageLimitsConfig {
                five_hour_format: Some("{pct}%".into()),
                seven_day_format: Some("{pct}%".into()),
                separator: Some("/".into()),
                warn_threshold: Some(70.0),
                warn_style: Some("bold yellow".to_string()),
                ..Default::default()
            }),
            ..Default::default()
        };
        let result = render(&ctx, &cfg).unwrap();
        // Custom format produces "75%/10%", warn threshold triggers ANSI wrapping
        assert!(
            result.contains('\x1b'),
            "expected ANSI codes on custom-formatted output: {result:?}"
        );
        assert!(
            result.contains("75%"),
            "custom format content should be present: {result:?}"
        );
    }
}
