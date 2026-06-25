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

thread_local! {
    /// Per-render-cycle memo for `resolve_data`. cship invocations are short-lived
    /// (one process per prompt render), so a thread-local cache effectively means
    /// "compute once per render cycle." When a status bar references multiple
    /// sub-tokens ($cship.usage_limits.opus, .sonnet, etc.), each token invokes
    /// `render` independently; without this cache each call would pay the full
    /// OAuth/cache lookup cost.
    static RESOLVE_CACHE: std::cell::RefCell<Option<Option<UsageLimitsData>>> =
        const { std::cell::RefCell::new(None) };
}

/// Shared data-fetch logic for the usage limits module and its sub-field renderers.
///
/// In production builds, memoized per process invocation via `RESOLVE_CACHE` so the
/// OAuth/cache lookup runs at most once per render cycle even when the user's
/// status bar references multiple sub-tokens (`$cship.usage_limits.opus`, `.sonnet`,
/// etc.). In test builds, memoization is bypassed: cargo's worker threads are
/// reused across tests, so a thread-local cache would leak state between unrelated
/// test cases. The memoization mechanism itself is exercised by
/// `resolve_data_memoized` in the test module.
#[cfg(not(test))]
fn resolve_data(ctx: &Context, cfg: &CshipConfig) -> Option<UsageLimitsData> {
    RESOLVE_CACHE.with(|c| {
        if let Some(cached) = c.borrow().as_ref() {
            return cached.clone();
        }
        let computed = resolve_data_uncached(ctx, cfg);
        *c.borrow_mut() = Some(computed.clone());
        computed
    })
}

#[cfg(test)]
fn resolve_data(ctx: &Context, cfg: &CshipConfig) -> Option<UsageLimitsData> {
    resolve_data_uncached(ctx, cfg)
}

/// Stdin `rate_limits` always provides the freshest 5h/7d values (sent every render
/// by Claude Code). Cache/OAuth provide per-model + extra usage data.
///
/// Token is read up front for fingerprint computation (cache identity check).
/// Token failure is non-fatal — stdin data is still usable.
fn resolve_data_uncached(ctx: &Context, cfg: &CshipConfig) -> Option<UsageLimitsData> {
    let ul_cfg = cfg.usage_limits.as_ref();

    if ul_cfg.and_then(|c| c.disabled) == Some(true) {
        return None;
    }

    let transcript_path = ctx.transcript_path.as_deref().map(std::path::Path::new);

    // Stdin provides the freshest 5h/7d values
    let stdin_data = data_from_stdin_rate_limits(ctx);

    // Read OAuth token up front for fingerprint (cache identity check).
    // Token failure is non-fatal here — stdin data is still usable.
    let token_and_fp = match crate::platform::get_oauth_token() {
        Ok(token) => {
            let fp = crate::platform::token_fingerprint(&token);
            Some((token, fp))
        }
        Err(e) => {
            tracing::warn!("cship.usage_limits: credential retrieval failed: {e}");
            if let Some(tp) = transcript_path {
                cache::write_negative_marker(tp, 30);
            }
            None
        }
    };

    // Try to get full data (per-model + extra) from cache or OAuth
    let full_data = transcript_path.and_then(|tp| {
        let fp_ref = token_and_fp.as_ref().map(|(_, fp)| fp.as_str());
        // Fresh cache?
        if let Some(cached) = cache::read_usage_limits(tp, false, fp_ref) {
            return Some(cached);
        }
        // Check negative cache — avoid retrying immediately after a failure
        if cache::read_negative_marker(tp) {
            tracing::debug!("cship.usage_limits: skipping OAuth (recent failure cooldown)");
            if let Some((_, ref fp)) = token_and_fp {
                return cache::read_usage_limits(tp, true, Some(fp));
            }
            return cache::read_usage_limits(tp, true, None);
        }
        // OAuth fetch? (needs token)
        if let Some((token, fp)) = token_and_fp {
            if let Some(fresh) = fetch_and_cache(tp, ul_cfg, token, &fp) {
                return Some(fresh);
            }
            // Stale cache as last resort for per-model/extra
            cache::read_usage_limits(tp, true, Some(&fp))
        } else {
            // No token available — try stale cache without fingerprint check
            cache::read_usage_limits(tp, true, None)
        }
    });

    match (stdin_data, full_data) {
        // Merge: stdin 5h/7d (fresh) + cache/OAuth per-model/extra
        (Some(stdin), Some(full)) => Some(UsageLimitsData {
            five_hour_pct: stdin.five_hour_pct,
            seven_day_pct: stdin.seven_day_pct,
            five_hour_resets_at_epoch: stdin.five_hour_resets_at_epoch,
            seven_day_resets_at_epoch: stdin.seven_day_resets_at_epoch,
            ..full
        }),
        // Stdin only (no per-model/extra)
        (Some(stdin), None) => Some(stdin),
        // No stdin, use cache/OAuth data as-is
        (None, Some(full)) => Some(full),
        // Nothing available
        (None, None) => None,
    }
}

/// Fetch usage limits via OAuth and write to cache with fingerprint.
///
/// Accepts a pre-fetched token and fingerprint — the caller handles credential
/// retrieval so this function only deals with the fetch + cache write.
fn fetch_and_cache(
    transcript_path: &std::path::Path,
    ul_cfg: Option<&UsageLimitsConfig>,
    token: String,
    fingerprint: &str,
) -> Option<UsageLimitsData> {
    let ttl_secs = ul_cfg.and_then(|c| c.ttl).unwrap_or(60);
    let fp = fingerprint.to_string();

    match fetch_with_timeout(move || crate::usage_limits::fetch_usage_limits(&token)) {
        Some(fresh) => {
            cache::write_usage_limits(transcript_path, &fresh, ttl_secs, Some(&fp));
            Some(fresh)
        }
        None => {
            cache::write_negative_marker(transcript_path, 30);
            None
        }
    }
}

/// True when the response carries no 5h or 7d signal — i.e. percentages and
/// reset markers are all at their `Default` zero state. On Claude Enterprise
/// the API returns these fields as `null`, so they remain at default values
/// after `parse_api_response`. Used to switch the renderer into "extra-usage
/// only" mode.
pub(crate) fn lacks_standard_signal(data: &UsageLimitsData) -> bool {
    data.five_hour_pct == 0.0
        && data.seven_day_pct == 0.0
        && data.five_hour_resets_at_epoch.is_none()
        && data.seven_day_resets_at_epoch.is_none()
        && data.five_hour_resets_at.is_empty()
        && data.seven_day_resets_at.is_empty()
}

/// Apply threshold styling using the higher of 5h/7d utilization.
/// Falls back to `extra_usage_utilization` when both standard signals are zero
/// (Enterprise plans where 5h/7d data is absent).
fn apply_threshold(content: &str, data: &UsageLimitsData, cfg: &CshipConfig) -> String {
    let ul_cfg = cfg.usage_limits.as_ref();
    let standard_max = data.five_hour_pct.max(data.seven_day_pct);
    let pct = if standard_max > 0.0 {
        standard_max
    } else {
        data.extra_usage_utilization.unwrap_or(0.0)
    };
    crate::ansi::apply_style_with_threshold(
        content,
        Some(pct),
        ul_cfg.and_then(|c| c.style.as_deref()),
        ul_cfg.and_then(|c| c.warn_threshold),
        ul_cfg.and_then(|c| c.warn_style.as_deref()),
        ul_cfg.and_then(|c| c.critical_threshold),
        ul_cfg.and_then(|c| c.critical_style.as_deref()),
    )
}

/// Render the usage limits module.
///
/// On Pro/Max plans this is the full 5h + 7d (+ optional per-model + optional extra)
/// composition produced by `format_output`. On Enterprise plans (where the API returns
/// 5h/7d as null), `render` short-circuits to the `extra_usage` section only.
pub fn render(ctx: &Context, cfg: &CshipConfig) -> Option<String> {
    let data = resolve_data(ctx, cfg)?;
    let default_ul_cfg = UsageLimitsConfig::default();
    let ul_cfg = cfg.usage_limits.as_ref().unwrap_or(&default_ul_cfg);

    let content = if lacks_standard_signal(&data) {
        format_extra_usage(&data, ul_cfg)?
    } else {
        format_output(&data, ul_cfg)
    };
    Some(apply_threshold(&content, &data, cfg))
}

/// Render only the per-model breakdown (opus, sonnet, cowork, oauth_apps).
pub fn render_per_model(ctx: &Context, cfg: &CshipConfig) -> Option<String> {
    let data = resolve_data(ctx, cfg)?;
    let default_ul_cfg = UsageLimitsConfig::default();
    let ul_cfg = cfg.usage_limits.as_ref().unwrap_or(&default_ul_cfg);
    let content = format_per_model(&data, ul_cfg);
    if content.is_empty() {
        return None;
    }
    Some(apply_threshold(&content, &data, cfg))
}

/// Render a single model's usage (opus, sonnet, cowork, or oauth).
fn render_model(
    ctx: &Context,
    cfg: &CshipConfig,
    name: &str,
    pct: impl FnOnce(&UsageLimitsData) -> Option<f64>,
    resets_at: impl FnOnce(&UsageLimitsData) -> &Option<String>,
    fmt: impl FnOnce(&UsageLimitsConfig) -> Option<&str>,
) -> Option<String> {
    let data = resolve_data(ctx, cfg)?;
    let default_ul_cfg = UsageLimitsConfig::default();
    let ul_cfg = cfg.usage_limits.as_ref().unwrap_or(&default_ul_cfg);
    let content = format_single_model(name, pct(&data), resets_at(&data), fmt(ul_cfg))?;
    Some(apply_threshold(&content, &data, cfg))
}

/// Render only the opus 7-day usage.
pub fn render_opus(ctx: &Context, cfg: &CshipConfig) -> Option<String> {
    render_model(
        ctx,
        cfg,
        "opus",
        |d| d.seven_day_opus_pct,
        |d| &d.seven_day_opus_resets_at,
        |c| c.opus_format.as_deref(),
    )
}

/// Render only the sonnet 7-day usage.
pub fn render_sonnet(ctx: &Context, cfg: &CshipConfig) -> Option<String> {
    render_model(
        ctx,
        cfg,
        "sonnet",
        |d| d.seven_day_sonnet_pct,
        |d| &d.seven_day_sonnet_resets_at,
        |c| c.sonnet_format.as_deref(),
    )
}

/// Render only the cowork 7-day usage.
pub fn render_cowork(ctx: &Context, cfg: &CshipConfig) -> Option<String> {
    render_model(
        ctx,
        cfg,
        "cowork",
        |d| d.seven_day_cowork_pct,
        |d| &d.seven_day_cowork_resets_at,
        |c| c.cowork_format.as_deref(),
    )
}

/// Render only the oauth_apps 7-day usage.
pub fn render_oauth_apps(ctx: &Context, cfg: &CshipConfig) -> Option<String> {
    render_model(
        ctx,
        cfg,
        "oauth",
        |d| d.seven_day_oauth_apps_pct,
        |d| &d.seven_day_oauth_apps_resets_at,
        |c| c.oauth_apps_format.as_deref(),
    )
}

/// Render only the extra usage section.
pub fn render_extra_usage(ctx: &Context, cfg: &CshipConfig) -> Option<String> {
    let data = resolve_data(ctx, cfg)?;
    let default_ul_cfg = UsageLimitsConfig::default();
    let ul_cfg = cfg.usage_limits.as_ref().unwrap_or(&default_ul_cfg);
    let content = format_extra_usage(&data, ul_cfg)?;
    Some(apply_threshold(&content, &data, cfg))
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

/// Extract usage limits from the `rate_limits` field Claude Code sends via stdin.
/// This avoids the OAuth API call entirely when the data is available.
/// `resets_at` is a Unix epoch in the stdin payload; stored directly to avoid a
/// round-trip through ISO 8601 formatting and re-parsing.
///
/// Only falls back to `None` (triggering the OAuth path) when `ctx.rate_limits` is absent.
/// Partial data (absent `five_hour`, `seven_day`, or `used_percentage`) uses placeholder values:
/// - absent period → pct `0.0` (renders as `"0%"`) and epoch `None` (renders as `"?"`)
/// - absent `used_percentage` within a present period → pct `0.0`
fn data_from_stdin_rate_limits(ctx: &Context) -> Option<UsageLimitsData> {
    let rl = ctx.rate_limits.as_ref()?; // None here → use OAuth path

    let (five_pct, five_epoch) = match rl.five_hour.as_ref() {
        Some(five) => (five.used_percentage.unwrap_or(0.0), five.resets_at),
        None => {
            tracing::warn!(
                "rate_limits.five_hour absent from stdin; rendering with placeholder values"
            );
            (0.0, None)
        }
    };
    let (seven_pct, seven_epoch) = match rl.seven_day.as_ref() {
        Some(seven) => (seven.used_percentage.unwrap_or(0.0), seven.resets_at),
        None => {
            tracing::warn!(
                "rate_limits.seven_day absent from stdin; rendering with placeholder values"
            );
            (0.0, None)
        }
    };

    Some(UsageLimitsData {
        five_hour_pct: five_pct,
        seven_day_pct: seven_pct,
        five_hour_resets_at_epoch: five_epoch,
        seven_day_resets_at_epoch: seven_epoch,
        ..Default::default()
    })
}

/// Format usage data using configurable format strings.
///
/// Placeholders in format strings (all occurrences are substituted):
/// - `{pct}` — percentage used as integer (e.g. `"23"`)
/// - `{remaining}` — percentage remaining as integer (e.g. `"77"`)
/// - `{reset}` — time-until-reset string (e.g. `"4h12m"`)
/// - `{reset_at}` — absolute local reset time (e.g. `"7:42 PM"` or `"Mon 9:00 AM"`)
/// - `{pace}` — signed pace string (e.g. `"+20%"`, `"-15%"`, `"?"`)
///
/// Per-model breakdowns (opus, sonnet, cowork, oauth_apps) are appended when
/// the API returns non-null data. Extra usage is appended when enabled.
///
/// Defaults (backwards compatible with pre-7.2 hardcoded output):
/// - `five_hour_format`: `"5h: {pct}% resets in {reset}"`
/// - `seven_day_format`: `"7d: {pct}% resets in {reset}"`
/// - `separator`: `" | "`
fn format_output(data: &UsageLimitsData, cfg: &UsageLimitsConfig) -> String {
    let sep = cfg.separator.as_deref().unwrap_or(" | ");
    let now = now_epoch();

    const FIVE_HOUR_SECS: u64 = 18_000;
    const SEVEN_DAY_SECS: u64 = 604_800;

    let five_h_epoch = resolve_epoch(data.five_hour_resets_at_epoch, &data.five_hour_resets_at);
    let seven_d_epoch = resolve_epoch(data.seven_day_resets_at_epoch, &data.seven_day_resets_at);

    let five_h_pct = format!("{:.0}", data.five_hour_pct);
    let five_h_remaining = format!("{:.0}", (100.0 - data.five_hour_pct).max(0.0));
    let five_h_reset = format_reset(five_h_epoch, now);
    let five_h_reset_at = format_reset_at(five_h_epoch, now);
    let five_h_pace = format_pace(calculate_pace(
        data.five_hour_pct,
        five_h_epoch,
        FIVE_HOUR_SECS,
        now,
    ));

    let seven_d_pct = format!("{:.0}", data.seven_day_pct);
    let seven_d_remaining = format!("{:.0}", (100.0 - data.seven_day_pct).max(0.0));
    let seven_d_reset = format_reset(seven_d_epoch, now);
    let seven_d_reset_at = format_reset_at(seven_d_epoch, now);
    let seven_d_pace = format_pace(calculate_pace(
        data.seven_day_pct,
        seven_d_epoch,
        SEVEN_DAY_SECS,
        now,
    ));

    let five_h_fmt = cfg
        .five_hour_format
        .as_deref()
        .unwrap_or("5h: {pct}% resets in {reset}");
    let seven_d_fmt = cfg
        .seven_day_format
        .as_deref()
        .unwrap_or("7d: {pct}% resets in {reset}");

    let five_h_part = five_h_fmt
        .replace("{pct}", &five_h_pct)
        .replace("{remaining}", &five_h_remaining)
        .replace("{reset}", &five_h_reset)
        .replace("{reset_at}", &five_h_reset_at)
        .replace("{pace}", &five_h_pace);
    let seven_d_part = seven_d_fmt
        .replace("{pct}", &seven_d_pct)
        .replace("{remaining}", &seven_d_remaining)
        .replace("{reset}", &seven_d_reset)
        .replace("{reset_at}", &seven_d_reset_at)
        .replace("{pace}", &seven_d_pace);

    let mut parts: Vec<String> = vec![five_h_part, seven_d_part];

    if cfg.show_per_model.unwrap_or(false) {
        let per_model = format_per_model(data, cfg);
        if !per_model.is_empty() {
            parts.push(per_model);
        }
    }

    if let Some(extra) = format_extra_usage(data, cfg) {
        parts.push(extra);
    }

    parts.join(sep)
}

/// Format a single model's usage into a string. Returns `None` if the model has no data.
fn format_single_model(
    name: &str,
    pct: Option<f64>,
    resets_at: &Option<String>,
    fmt_override: Option<&str>,
) -> Option<String> {
    let pct = pct?;
    let now = now_epoch();
    const SEVEN_DAY_SECS: u64 = 604_800;

    let pct_str = format!("{:.0}", pct);
    let remaining_str = format!("{:.0}", (100.0 - pct).max(0.0));
    let model_epoch = resets_at
        .as_ref()
        .and_then(|s| crate::cache::iso8601_to_epoch(s));
    let reset_str = match model_epoch {
        Some(_) => format_reset(model_epoch, now),
        None => "?".to_string(),
    };
    let reset_at_str = format_reset_at(model_epoch, now);
    let pace_str = format_pace(calculate_pace(pct, model_epoch, SEVEN_DAY_SECS, now));
    let default_fmt;
    let fmt: &str = match fmt_override {
        Some(f) => f,
        None => {
            default_fmt = format!("{name} {{pct}}%");
            &default_fmt
        }
    };
    Some(
        fmt.replace("{pct}", &pct_str)
            .replace("{remaining}", &remaining_str)
            .replace("{reset}", &reset_str)
            .replace("{reset_at}", &reset_at_str)
            .replace("{pace}", &pace_str),
    )
}

/// Format the per-model breakdown (opus, sonnet, cowork, oauth_apps).
/// Returns an empty string when no per-model data is present.
fn format_per_model(data: &UsageLimitsData, cfg: &UsageLimitsConfig) -> String {
    let sep = cfg.separator.as_deref().unwrap_or(" | ");
    let parts: Vec<String> = model_entries(data, cfg)
        .into_iter()
        .filter_map(|(name, pct, resets_at, fmt)| format_single_model(name, pct, resets_at, fmt))
        .collect();
    parts.join(sep)
}

/// Return the list of (name, pct, resets_at, format_override) for all per-model entries.
#[allow(clippy::type_complexity)]
fn model_entries<'a>(
    data: &'a UsageLimitsData,
    cfg: &'a UsageLimitsConfig,
) -> Vec<(&'a str, Option<f64>, &'a Option<String>, Option<&'a str>)> {
    vec![
        (
            "opus",
            data.seven_day_opus_pct,
            &data.seven_day_opus_resets_at,
            cfg.opus_format.as_deref(),
        ),
        (
            "sonnet",
            data.seven_day_sonnet_pct,
            &data.seven_day_sonnet_resets_at,
            cfg.sonnet_format.as_deref(),
        ),
        (
            "cowork",
            data.seven_day_cowork_pct,
            &data.seven_day_cowork_resets_at,
            cfg.cowork_format.as_deref(),
        ),
        (
            "oauth",
            data.seven_day_oauth_apps_pct,
            &data.seven_day_oauth_apps_resets_at,
            cfg.oauth_apps_format.as_deref(),
        ),
    ]
}

/// Format the extra usage section. Returns `None` when extra usage is absent or disabled.
///
/// `{active}` glyph rules:
/// - Pro/Max: `⚡` when either 5h or 7d utilization is at 100%, else `💤`.
/// - Enterprise (no 5h/7d signal): `⚡` once `extra_usage_utilization` exceeds
///   `warn_threshold`, or for any non-zero utilization when no threshold is
///   configured. Otherwise `💤`.
fn format_extra_usage(data: &UsageLimitsData, cfg: &UsageLimitsConfig) -> Option<String> {
    if data.extra_usage_enabled != Some(true) {
        return None;
    }
    let eu_pct = data
        .extra_usage_utilization
        .map(|v| format!("{v:.0}"))
        .unwrap_or_else(|| "?".into());
    // The Anthropic OAuth usage API reports `used_credits` and `monthly_limit`
    // in cents (smallest currency unit). Divide by 100 for dollar display.
    let eu_used = data
        .extra_usage_used_credits
        .map(|v| format!("{:.2}", v / 100.0))
        .unwrap_or_else(|| "?".into());
    let eu_limit = data
        .extra_usage_monthly_limit
        .map(|v| format!("{:.2}", v / 100.0))
        .unwrap_or_else(|| "?".into());
    let eu_remaining_credits = match (
        data.extra_usage_monthly_limit,
        data.extra_usage_used_credits,
    ) {
        (Some(limit), Some(used)) => format!("{:.2}", (limit - used).max(0.0) / 100.0),
        _ => "?".into(),
    };
    let active = if data.five_hour_pct >= 100.0 || data.seven_day_pct >= 100.0 {
        "\u{26a1}" // ⚡
    } else if lacks_standard_signal(data) {
        let threshold = cfg.warn_threshold.unwrap_or(0.0);
        match data.extra_usage_utilization {
            Some(u) if u > threshold => "\u{26a1}", // ⚡
            _ => "\u{1f4a4}",                       // 💤
        }
    } else {
        "\u{1f4a4}" // 💤
    };
    let eu_fmt = cfg
        .extra_usage_format
        .as_deref()
        .unwrap_or("{active} extra: {pct}% (${used}/${limit})");
    if eu_fmt.contains("{remaining}") {
        tracing::warn!(
            "`extra_usage_format` contains `{{remaining}}` which is no longer substituted; \
             use `{{remaining_credits}}` for the dollar amount remaining"
        );
    }
    Some(
        eu_fmt
            .replace("{active}", active)
            .replace("{pct}", &eu_pct)
            .replace("{used}", &eu_used)
            .replace("{limit}", &eu_limit)
            .replace("{remaining_credits}", &eu_remaining_credits),
    )
}

fn now_epoch() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Resolve a reset epoch: prefer a pre-computed epoch, fall back to parsing an ISO 8601 string.
fn resolve_epoch(epoch: Option<u64>, iso: &str) -> Option<u64> {
    epoch.or_else(|| crate::cache::iso8601_to_epoch(iso))
}

/// Format a resolved epoch as a human-readable time-until string.
/// `None` → `"?"`, past → `"now"`, otherwise `"4h12m"` / `"3d2h"` / `"45m"`.
fn format_reset(epoch: Option<u64>, now: u64) -> String {
    match epoch {
        None => "?".to_string(),
        Some(e) if now >= e => "now".to_string(),
        Some(e) => format_remaining_secs(e - now),
    }
}

/// Format a resolved epoch as an absolute local clock time.
/// `None` → `"?"`, past → `"now"`, same local date as `now` → `"7:42 PM"`,
/// a later local date → `"Mon 9:00 AM"`.
fn format_reset_at(epoch: Option<u64>, now: u64) -> String {
    match epoch {
        None => "?".to_string(),
        Some(e) if now >= e => "now".to_string(),
        Some(e) => match (epoch_to_local(e), epoch_to_local(now)) {
            (Some(reset), Some(today)) if reset.date_naive() == today.date_naive() => {
                reset.format("%-I:%M %p").to_string()
            }
            (Some(reset), _) => reset.format("%a %-I:%M %p").to_string(),
            _ => "?".to_string(),
        },
    }
}

/// Convert Unix epoch seconds to the local-timezone equivalent.
fn epoch_to_local(secs: u64) -> Option<chrono::DateTime<chrono::Local>> {
    chrono::DateTime::from_timestamp(secs as i64, 0).map(|utc| utc.with_timezone(&chrono::Local))
}

/// Format a number of remaining seconds as a compact human-readable string.
///
/// Shared arithmetic used by `format_reset`.
///
/// - < 1 hour → `"45m"`
/// - < 1 day → `"4h12m"`
/// - >= 1 day → `"3d2h"`
fn format_remaining_secs(secs: u64) -> String {
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

/// Calculate pace: how far ahead or behind linear consumption the user is.
///
/// Returns `Some(pace)` where positive = headroom, negative = over-pace.
/// Returns `None` when `resets_at_epoch` is unavailable or already in the past
/// (an expired window has no meaningful pace — `format_pace(None)` renders `"?"`).
fn calculate_pace(
    used_pct: f64,
    resets_at_epoch: Option<u64>,
    window_secs: u64,
    now: u64,
) -> Option<f64> {
    let reset = resets_at_epoch?;
    if reset <= now {
        return None;
    }
    let remaining = reset - now;
    let elapsed = window_secs.saturating_sub(remaining);
    let elapsed_fraction = elapsed as f64 / window_secs as f64;
    let expected_pct = elapsed_fraction * 100.0;
    Some(expected_pct - used_pct)
}

/// Format a pace value as a signed percentage string.
/// Positive → "+20%", negative → "-15%", None → "?".
fn format_pace(pace: Option<f64>) -> String {
    match pace {
        Some(p) => {
            let rounded = p.round() as i64;
            if rounded >= 0 {
                format!("+{rounded}%")
            } else {
                format!("{rounded}%")
            }
        }
        None => "?".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{CshipConfig, UsageLimitsConfig};
    use crate::context::Context;

    /// Convert a Unix epoch to ISO 8601 UTC string: "YYYY-MM-DDTHH:MM:SSZ".
    /// Returns an empty string for `None` input.
    /// Test-only helper — used by epoch_to_iso tests and format_time_until tests.
    fn epoch_to_iso(epoch: Option<u64>) -> String {
        match epoch {
            Some(e) => {
                let days_since_epoch = (e / 86400) as i64;
                let remaining = e % 86400;
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
            None => String::new(),
        }
    }

    fn sample_data() -> UsageLimitsData {
        UsageLimitsData {
            five_hour_pct: 23.4,
            seven_day_pct: 45.1,
            five_hour_resets_at: "2099-01-01T00:00:00Z".into(),
            seven_day_resets_at: "2099-01-01T00:00:00Z".into(),
            ..Default::default()
        }
    }

    // ── lacks_standard_signal() tests ────────────────────────────────────────

    #[test]
    fn test_lacks_standard_signal_returns_true_for_default() {
        let data = UsageLimitsData::default();
        assert!(lacks_standard_signal(&data));
    }

    #[test]
    fn test_lacks_standard_signal_returns_false_when_pct_set() {
        let data = UsageLimitsData {
            five_hour_pct: 1.0,
            ..Default::default()
        };
        assert!(!lacks_standard_signal(&data));
    }

    #[test]
    fn test_lacks_standard_signal_returns_false_when_reset_iso_set() {
        let data = UsageLimitsData {
            seven_day_resets_at: "2099-01-01T00:00:00+00:00".into(),
            ..Default::default()
        };
        assert!(!lacks_standard_signal(&data));
    }

    #[test]
    fn test_lacks_standard_signal_returns_false_when_reset_epoch_set() {
        let data = UsageLimitsData {
            five_hour_resets_at_epoch: Some(1_700_000_000),
            ..Default::default()
        };
        assert!(!lacks_standard_signal(&data));
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
        use crate::context::{RateLimitPeriod, RateLimits};
        let dir = tempfile::tempdir().unwrap();
        let transcript = dir.path().join("test.jsonl");
        let ctx = Context {
            transcript_path: Some(transcript.to_str().unwrap().to_string()),
            rate_limits: Some(RateLimits {
                five_hour: Some(RateLimitPeriod {
                    used_percentage: Some(23.4),
                    resets_at: Some(9_999_999_999),
                }),
                seven_day: Some(RateLimitPeriod {
                    used_percentage: Some(45.1),
                    resets_at: Some(9_999_999_999),
                }),
            }),
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
        use crate::context::{RateLimitPeriod, RateLimits};
        let dir = tempfile::tempdir().unwrap();
        let transcript = dir.path().join("test.jsonl");
        let ctx = Context {
            transcript_path: Some(transcript.to_str().unwrap().to_string()),
            rate_limits: Some(RateLimits {
                five_hour: Some(RateLimitPeriod {
                    used_percentage: Some(65.0),
                    resets_at: Some(9_999_999_999),
                }),
                seven_day: Some(RateLimitPeriod {
                    used_percentage: Some(10.0),
                    resets_at: Some(9_999_999_999),
                }),
            }),
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
        use crate::context::{RateLimitPeriod, RateLimits};
        let dir = tempfile::tempdir().unwrap();
        let transcript = dir.path().join("test.jsonl");
        let ctx = Context {
            transcript_path: Some(transcript.to_str().unwrap().to_string()),
            rate_limits: Some(RateLimits {
                five_hour: Some(RateLimitPeriod {
                    used_percentage: Some(85.0),
                    resets_at: Some(9_999_999_999),
                }),
                seven_day: Some(RateLimitPeriod {
                    used_percentage: Some(20.0),
                    resets_at: Some(9_999_999_999),
                }),
            }),
            ..Default::default()
        };
        let cfg = CshipConfig {
            usage_limits: Some(UsageLimitsConfig {
                warn_threshold: Some(60.0),
                warn_style: Some("bold yellow".to_string()),
                critical_threshold: Some(80.0),
                critical_style: Some("bold red".to_string()),
                ..Default::default()
            }),
            ..Default::default()
        };
        let result = render(&ctx, &cfg).unwrap();
        assert!(
            result.contains("31m") || result.contains("red"),
            "expected critical/red ANSI codes: {result:?}"
        );
    }

    #[test]
    fn test_threshold_uses_higher_of_two_pcts() {
        use crate::context::{RateLimitPeriod, RateLimits};
        let dir = tempfile::tempdir().unwrap();
        let transcript = dir.path().join("test.jsonl");
        // seven_day is high (85%), five_hour is low (20%) — should still trigger critical
        let ctx = Context {
            transcript_path: Some(transcript.to_str().unwrap().to_string()),
            rate_limits: Some(RateLimits {
                five_hour: Some(RateLimitPeriod {
                    used_percentage: Some(20.0),
                    resets_at: Some(9_999_999_999),
                }),
                seven_day: Some(RateLimitPeriod {
                    used_percentage: Some(85.0),
                    resets_at: Some(9_999_999_999),
                }),
            }),
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

    // ── apply_threshold() extra_usage fallback tests ─────────────────────────

    #[test]
    fn test_apply_threshold_uses_extra_usage_when_standard_absent() {
        use crate::config::CshipConfig;
        let data = UsageLimitsData {
            extra_usage_utilization: Some(85.0),
            ..Default::default()
        };
        let cfg = CshipConfig {
            usage_limits: Some(UsageLimitsConfig {
                critical_threshold: Some(80.0),
                critical_style: Some("bold red".into()),
                ..Default::default()
            }),
            ..Default::default()
        };
        let styled = apply_threshold("X", &data, &cfg);
        // Critical style should have wrapped the content with ANSI escapes.
        assert_ne!(styled, "X", "expected critical styling to apply");
        assert!(
            styled.contains('\x1b'),
            "expected ANSI escape; got {styled:?}"
        );
    }

    #[test]
    fn test_apply_threshold_prefers_standard_when_present() {
        use crate::config::CshipConfig;
        let data = UsageLimitsData {
            five_hour_pct: 30.0,
            extra_usage_utilization: Some(90.0),
            ..Default::default()
        };
        let cfg = CshipConfig {
            usage_limits: Some(UsageLimitsConfig {
                critical_threshold: Some(80.0),
                critical_style: Some("bold red".into()),
                ..Default::default()
            }),
            ..Default::default()
        };
        let styled = apply_threshold("X", &data, &cfg);
        // Standard pct (30%) is below threshold; extra_usage (90%) is ignored.
        assert_eq!(styled, "X", "expected no styling; standard signal must win");
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
            ..Default::default()
        };
        crate::cache::write_usage_limits(&transcript, &data, 60, None);
        // Verify read_usage_limits(allow_stale=true) works even after TTL would normally expire
        let stale = crate::cache::read_usage_limits(&transcript, true, None);
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
            ..Default::default()
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
            Ok(UsageLimitsData::default())
        });
        assert!(result.is_none());
    }

    // ── epoch_to_iso() tests ──────────────────────────────────────────────────

    #[test]
    fn test_epoch_to_iso_zero() {
        assert_eq!(epoch_to_iso(Some(0)), "1970-01-01T00:00:00Z");
    }

    #[test]
    fn test_epoch_to_iso_none() {
        assert_eq!(epoch_to_iso(None), "");
    }

    #[test]
    fn test_epoch_to_iso_known_value() {
        // 2000-01-01T00:00:00Z = 946684800
        assert_eq!(epoch_to_iso(Some(946_684_800)), "2000-01-01T00:00:00Z");
    }

    #[test]
    fn test_epoch_to_iso_far_future() {
        // 2099-12-31T00:00:00Z = 4102358400
        assert_eq!(epoch_to_iso(Some(4_102_358_400)), "2099-12-31T00:00:00Z");
    }

    // ── format_reset() tests ─────────────────────────────────────────────────

    #[test]
    fn test_format_reset_none_returns_question_mark() {
        assert_eq!(format_reset(None, now_epoch()), "?");
    }

    #[test]
    fn test_format_reset_past_returns_now() {
        let now = now_epoch();
        assert_eq!(format_reset(Some(0), now), "now");
        assert_eq!(format_reset(Some(now.saturating_sub(1)), now), "now");
    }

    #[test]
    fn test_format_reset_hours_minutes() {
        let now = now_epoch();
        let result = format_reset(Some(now + 4 * 3600 + 12 * 60 + 30), now);
        assert!(
            result.contains('h') && result.contains('m'),
            "expected Xh Ym format: {result}"
        );
    }

    #[test]
    fn test_format_reset_days_hours() {
        let now = now_epoch();
        let result = format_reset(Some(now + 3 * 86400 + 2 * 3600 + 30), now);
        assert!(
            result.contains('d') && result.contains('h'),
            "expected Xd Yh format: {result}"
        );
    }

    #[test]
    fn test_format_reset_minutes_only() {
        let now = now_epoch();
        let result = format_reset(Some(now + 45 * 60 + 30), now);
        assert!(
            result.ends_with('m') && !result.contains('h'),
            "expected Xm format: {result}"
        );
    }

    // ── format_reset_at() tests ──────────────────────────────────────────────

    #[test]
    fn test_format_reset_at_none_returns_question_mark() {
        assert_eq!(format_reset_at(None, now_epoch()), "?");
    }

    #[test]
    fn test_format_reset_at_past_returns_now() {
        let now = now_epoch();
        assert_eq!(format_reset_at(Some(0), now), "now");
        assert_eq!(format_reset_at(Some(now.saturating_sub(1)), now), "now");
    }

    #[test]
    fn test_format_reset_at_same_day_is_clock_only() {
        use chrono::Timelike;
        let now = now_epoch();
        let local_now = epoch_to_local(now).unwrap();
        // One minute before local midnight on `now`'s own date — guaranteed same
        // calendar date as `now` regardless of system timezone, and always in the
        // future unless the test runs in the literal last minute of the day.
        let end_of_day = local_now
            .date_naive()
            .and_hms_opt(23, 59, 0)
            .unwrap()
            .and_local_timezone(chrono::Local)
            .single()
            .unwrap();
        if local_now.hour() == 23 && local_now.minute() >= 59 {
            return; // last minute of the day — skip to avoid the unrepresentable edge case
        }
        let result = format_reset_at(Some(end_of_day.timestamp() as u64), now);
        assert!(
            result.contains(':') && (result.contains("AM") || result.contains("PM")),
            "expected clock-only format: {result}"
        );
        assert!(
            !result
                .split(' ')
                .next()
                .unwrap()
                .chars()
                .all(char::is_alphabetic),
            "same-day format should not start with a weekday: {result}"
        );
    }

    #[test]
    fn test_format_reset_at_later_day_includes_weekday() {
        let now = now_epoch();
        // 30 days out: outside any plausible UTC offset, guaranteed different local date.
        let result = format_reset_at(Some(now + 30 * 86400), now);
        assert!(
            result.contains(':') && (result.contains("AM") || result.contains("PM")),
            "expected clock portion: {result}"
        );
        let weekday = result.split(' ').next().unwrap();
        assert!(
            weekday.chars().all(char::is_alphabetic) && weekday.len() == 3,
            "expected 3-letter weekday prefix: {result}"
        );
    }

    #[test]
    fn test_resolve_epoch_from_iso_plus_offset() {
        // Anthropic API returns "+00:00" not "Z" — resolve_epoch must handle it
        let epoch = resolve_epoch(None, "2099-01-01T00:00:00+00:00");
        assert!(epoch.is_some(), "should parse +00:00 format");
        let now = now_epoch();
        let result = format_reset(epoch, now);
        assert_ne!(result, "?");
        assert_ne!(result, "now");
    }

    // ── format_output() tests ─────────────────────────────────────────────────

    #[test]
    fn test_format_output_default_produces_legacy_format() {
        // AC1: no config → identical to old hardcoded string
        let data = sample_data();
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
            ..Default::default()
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
            ..Default::default()
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
    fn test_format_output_reset_at_placeholder() {
        let data = sample_data();
        let cfg = UsageLimitsConfig {
            five_hour_format: Some("5h {pct}% at {reset_at}".into()),
            seven_day_format: Some("7d {pct}% at {reset_at}".into()),
            ..Default::default()
        };
        let result = format_output(&data, &cfg);
        assert!(
            !result.contains("{reset_at}"),
            "placeholder should be substituted: {result:?}"
        );
        let weekday_count = result.matches(" at ").count();
        assert_eq!(
            weekday_count, 2,
            "both sections should have a reset_at: {result:?}"
        );
    }

    #[test]
    fn test_format_single_model_reset_at_placeholder() {
        let result = format_single_model(
            "opus",
            Some(50.0),
            &Some("2099-01-01T00:00:00Z".into()),
            Some("opus {pct}% at {reset_at}"),
        )
        .unwrap();
        assert!(
            !result.contains("{reset_at}"),
            "placeholder should be substituted: {result:?}"
        );
        assert!(
            result.contains(" at "),
            "expected reset_at content: {result:?}"
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
            ..Default::default()
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
            ..Default::default()
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
        use crate::context::{RateLimitPeriod, RateLimits};
        let dir = tempfile::tempdir().unwrap();
        let transcript = dir.path().join("test.jsonl");
        let ctx = Context {
            transcript_path: Some(transcript.to_str().unwrap().to_string()),
            rate_limits: Some(RateLimits {
                five_hour: Some(RateLimitPeriod {
                    used_percentage: Some(75.0),
                    resets_at: Some(9_999_999_999),
                }),
                seven_day: Some(RateLimitPeriod {
                    used_percentage: Some(10.0),
                    resets_at: Some(9_999_999_999),
                }),
            }),
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

    // ── stdin rate limits path tests ──────────────────────────────────────────

    #[test]
    fn test_data_from_stdin_rate_limits_uses_epoch_directly() {
        // Verify no ISO string is produced — five_hour_resets_at remains empty
        // and five_hour_resets_at_epoch carries the raw epoch value.
        let ctx = Context {
            rate_limits: Some(crate::context::RateLimits {
                five_hour: Some(crate::context::RateLimitPeriod {
                    used_percentage: Some(23.0),
                    resets_at: Some(9_999_999_999),
                }),
                seven_day: Some(crate::context::RateLimitPeriod {
                    used_percentage: Some(45.0),
                    resets_at: Some(9_999_999_999),
                }),
            }),
            ..Default::default()
        };
        let data = data_from_stdin_rate_limits(&ctx).unwrap();
        assert_eq!(
            data.five_hour_resets_at, "",
            "ISO field must be empty on stdin path"
        );
        assert_eq!(
            data.seven_day_resets_at, "",
            "ISO field must be empty on stdin path"
        );
        assert_eq!(
            data.five_hour_resets_at_epoch,
            Some(9_999_999_999),
            "epoch field must carry raw resets_at value"
        );
        assert_eq!(
            data.seven_day_resets_at_epoch,
            Some(9_999_999_999),
            "epoch field must carry raw resets_at value"
        );
    }

    #[test]
    fn test_render_stdin_rate_limits_produces_output() {
        // render() uses stdin path (no transcript_path needed)
        let ctx = Context {
            rate_limits: Some(crate::context::RateLimits {
                five_hour: Some(crate::context::RateLimitPeriod {
                    used_percentage: Some(23.0),
                    resets_at: Some(9_999_999_999),
                }),
                seven_day: Some(crate::context::RateLimitPeriod {
                    used_percentage: Some(45.0),
                    resets_at: Some(9_999_999_999),
                }),
            }),
            ..Default::default()
        };
        let result = render(&ctx, &CshipConfig::default()).unwrap();
        assert!(result.contains("5h:"), "expected 5h prefix: {result:?}");
        assert!(result.contains("7d:"), "expected 7d prefix: {result:?}");
        assert!(result.contains("23%"), "expected five_hour_pct: {result:?}");
        assert!(result.contains("45%"), "expected seven_day_pct: {result:?}");
    }

    // ── partial stdin data tests (story 2-3) ──────────────────────────────────

    #[test]
    fn test_stdin_only_five_hour_present_seven_day_absent() {
        // AC1: absent seven_day → Some with placeholder values (0% / "?")
        let ctx = Context {
            rate_limits: Some(crate::context::RateLimits {
                five_hour: Some(crate::context::RateLimitPeriod {
                    used_percentage: Some(42.0),
                    resets_at: None,
                }),
                seven_day: None,
            }),
            ..Default::default()
        };
        let result = data_from_stdin_rate_limits(&ctx);
        assert!(
            result.is_some(),
            "should return Some with only five_hour present"
        );
        let data = result.unwrap();
        assert!(
            (data.five_hour_pct - 42.0).abs() < f64::EPSILON,
            "five_hour_pct should be 42.0"
        );
        assert!(
            (data.seven_day_pct - 0.0).abs() < f64::EPSILON,
            "seven_day_pct should be 0.0 placeholder"
        );
        assert_eq!(
            data.seven_day_resets_at_epoch, None,
            "absent seven_day epoch should be None"
        );
    }

    #[test]
    fn test_stdin_seven_day_used_percentage_absent_uses_zero() {
        // AC5: absent used_percentage within a present period → 0.0 fallback
        let ctx = Context {
            rate_limits: Some(crate::context::RateLimits {
                five_hour: Some(crate::context::RateLimitPeriod {
                    used_percentage: Some(10.0),
                    resets_at: Some(9_999_999_999),
                }),
                seven_day: Some(crate::context::RateLimitPeriod {
                    used_percentage: None,
                    resets_at: Some(9_999_999_999),
                }),
            }),
            ..Default::default()
        };
        let result = data_from_stdin_rate_limits(&ctx);
        assert!(
            result.is_some(),
            "should return Some when seven_day.used_percentage is None"
        );
        let data = result.unwrap();
        assert!(
            (data.seven_day_pct - 0.0).abs() < f64::EPSILON,
            "absent used_percentage should fall back to 0.0"
        );
    }

    #[test]
    fn test_stdin_rate_limits_entirely_absent_returns_none() {
        // AC3: entirely absent rate_limits → None (OAuth fallback)
        let ctx = Context::default();
        assert!(
            data_from_stdin_rate_limits(&ctx).is_none(),
            "absent rate_limits should return None"
        );
    }

    #[test]
    fn test_stdin_both_periods_present_full_data_happy_path() {
        // AC4: regression guard — full data still works as before
        let ctx = Context {
            rate_limits: Some(crate::context::RateLimits {
                five_hour: Some(crate::context::RateLimitPeriod {
                    used_percentage: Some(55.0),
                    resets_at: Some(9_999_999_999),
                }),
                seven_day: Some(crate::context::RateLimitPeriod {
                    used_percentage: Some(80.0),
                    resets_at: Some(9_999_999_999),
                }),
            }),
            ..Default::default()
        };
        let result = data_from_stdin_rate_limits(&ctx);
        assert!(
            result.is_some(),
            "both periods present → should return Some"
        );
        let data = result.unwrap();
        assert!(
            (data.five_hour_pct - 55.0).abs() < f64::EPSILON,
            "five_hour_pct should be 55.0"
        );
        assert!(
            (data.seven_day_pct - 80.0).abs() < f64::EPSILON,
            "seven_day_pct should be 80.0"
        );
        assert_eq!(data.five_hour_resets_at_epoch, Some(9_999_999_999));
        assert_eq!(data.seven_day_resets_at_epoch, Some(9_999_999_999));
    }

    // ── additional stdin unit tests (story 2-4) ───────────────────────────────

    #[test]
    fn test_stdin_period_with_resets_at_none_uses_none_epoch() {
        // AC1: present period with resets_at: None → five_hour_resets_at_epoch is None
        let ctx = Context {
            rate_limits: Some(crate::context::RateLimits {
                five_hour: Some(crate::context::RateLimitPeriod {
                    used_percentage: Some(30.0),
                    resets_at: None, // ← absent resets_at
                }),
                seven_day: Some(crate::context::RateLimitPeriod {
                    used_percentage: Some(50.0),
                    resets_at: Some(9_999_999_999),
                }),
            }),
            ..Default::default()
        };
        let data = data_from_stdin_rate_limits(&ctx).unwrap();
        assert_eq!(
            data.five_hour_resets_at_epoch, None,
            "absent resets_at → None epoch"
        );
        assert_eq!(data.seven_day_resets_at_epoch, Some(9_999_999_999));
    }

    #[test]
    fn test_stdin_only_seven_day_present_five_hour_absent() {
        // AC1: only seven_day present — mirror of existing only_five_hour test
        let ctx = Context {
            rate_limits: Some(crate::context::RateLimits {
                five_hour: None, // ← absent period
                seven_day: Some(crate::context::RateLimitPeriod {
                    used_percentage: Some(75.0),
                    resets_at: Some(9_999_999_999),
                }),
            }),
            ..Default::default()
        };
        let result = data_from_stdin_rate_limits(&ctx);
        assert!(result.is_some());
        let data = result.unwrap();
        assert!(
            (data.five_hour_pct - 0.0).abs() < f64::EPSILON,
            "absent five_hour → 0.0 placeholder"
        );
        assert!((data.seven_day_pct - 75.0).abs() < f64::EPSILON);
        assert_eq!(data.five_hour_resets_at_epoch, None);
    }

    // ── render() stdin integration tests (story 2-4, AC2 & AC4) ──────────────

    #[test]
    fn test_render_stdin_path_no_transcript_needed() {
        // AC4: transcript_path: None + valid rate_limits → render() returns Some
        // Proves stdin path bypasses the transcript_path requirement
        let ctx = Context {
            transcript_path: None, // ← no transcript
            rate_limits: Some(crate::context::RateLimits {
                five_hour: Some(crate::context::RateLimitPeriod {
                    used_percentage: Some(40.0),
                    resets_at: Some(9_999_999_999),
                }),
                seven_day: Some(crate::context::RateLimitPeriod {
                    used_percentage: Some(60.0),
                    resets_at: Some(9_999_999_999),
                }),
            }),
            ..Default::default()
        };
        let result = render(&ctx, &CshipConfig::default());
        assert!(result.is_some(), "stdin path must not need transcript_path");
        let output = result.unwrap();
        assert!(
            output.contains("40%"),
            "expected five_hour_pct 40%: {output:?}"
        );
        assert!(
            output.contains("60%"),
            "expected seven_day_pct 60%: {output:?}"
        );
    }

    #[test]
    fn test_render_stdin_takes_priority_over_cache() {
        // AC2: stdin rate_limits present + cache present → stdin wins
        let dir = tempfile::tempdir().unwrap();
        let transcript = dir.path().join("test.jsonl");
        let cache_data = UsageLimitsData {
            five_hour_pct: 99.0, // ← cache has high value
            seven_day_pct: 99.0,
            five_hour_resets_at: "2099-01-01T00:00:00Z".into(),
            seven_day_resets_at: "2099-01-01T00:00:00Z".into(),
            ..Default::default()
        };
        crate::cache::write_usage_limits(&transcript, &cache_data, 60, None);

        let ctx = Context {
            transcript_path: Some(transcript.to_str().unwrap().to_string()),
            rate_limits: Some(crate::context::RateLimits {
                // ← stdin has different value
                five_hour: Some(crate::context::RateLimitPeriod {
                    used_percentage: Some(23.0),
                    resets_at: Some(9_999_999_999),
                }),
                seven_day: Some(crate::context::RateLimitPeriod {
                    used_percentage: Some(45.0),
                    resets_at: Some(9_999_999_999),
                }),
            }),
            ..Default::default()
        };
        let result = render(&ctx, &CshipConfig::default()).unwrap();
        // Stdin value (23%) must win over cache value (99%)
        assert!(
            result.contains("23%"),
            "stdin must override cache: {result:?}"
        );
        assert!(
            !result.contains("99%"),
            "cache value must not appear: {result:?}"
        );
    }

    #[test]
    fn test_render_falls_back_to_oauth_path_when_rate_limits_absent() {
        // AC2: rate_limits: None, transcript_path: None → render() returns None
        // Proves OAuth path is triggered when stdin data is absent
        let ctx = Context {
            transcript_path: None,
            rate_limits: None,
            ..Default::default()
        };
        assert!(
            render(&ctx, &CshipConfig::default()).is_none(),
            "absent rate_limits with no transcript → None (OAuth path triggered, no token available)"
        );
    }

    #[test]
    fn test_format_reset_epoch_past_returns_now() {
        let now = now_epoch();
        assert_eq!(format_reset(Some(0), now), "now");
        assert_eq!(format_reset(Some(1), now), "now");
    }

    #[test]
    fn test_format_reset_epoch_hours_minutes() {
        let now = now_epoch();
        let result = format_reset(Some(now + 4 * 3600 + 12 * 60 + 30), now);
        assert!(
            result.contains('h') && result.contains('m'),
            "expected Xh Ym format: {result}"
        );
    }

    #[test]
    fn test_format_reset_epoch_days_hours() {
        let now = now_epoch();
        let result = format_reset(Some(now + 3 * 86400 + 2 * 3600 + 30), now);
        assert!(
            result.contains('d') && result.contains('h'),
            "expected Xd Yh format: {result}"
        );
    }

    #[test]
    fn test_format_reset_epoch_minutes_only() {
        let now = now_epoch();
        let result = format_reset(Some(now + 45 * 60 + 30), now);
        assert!(
            result.ends_with('m') && !result.contains('h'),
            "expected Xm format: {result}"
        );
    }

    // ── calculate_pace() tests ───────────────────────────────────────────────

    #[test]
    fn test_calculate_pace_headroom() {
        let now = now_epoch();
        let pace = calculate_pace(30.0, Some(now + 9000), 18000, now);
        let p = pace.unwrap();
        assert!(p > 15.0 && p < 25.0, "expected ~+20 headroom, got {p}");
    }

    #[test]
    fn test_calculate_pace_over_pace() {
        let now = now_epoch();
        let pace = calculate_pace(70.0, Some(now + 9000), 18000, now);
        let p = pace.unwrap();
        assert!(p < -15.0 && p > -25.0, "expected ~-20 over-pace, got {p}");
    }

    #[test]
    fn test_calculate_pace_no_reset_returns_none() {
        let pace = calculate_pace(50.0, None, 18000, now_epoch());
        assert!(pace.is_none());
    }

    #[test]
    fn test_calculate_pace_zero_elapsed() {
        let now = now_epoch();
        let pace = calculate_pace(10.0, Some(now + 18000), 18000, now);
        let p = pace.unwrap();
        assert!(
            p > -15.0 && p < -5.0,
            "expected ~-10 over-pace at zero elapsed, got {p}"
        );
    }

    #[test]
    fn test_calculate_pace_reset_in_past() {
        let now = now_epoch();
        // An expired window has no meaningful pace: pre-fix the function returned
        // a spurious "+70 headroom" because the clamped elapsed_fraction hit 1.0.
        let pace = calculate_pace(30.0, Some(now.saturating_sub(100)), 18000, now);
        assert!(pace.is_none(), "expired window should produce no pace");
    }

    #[test]
    fn test_calculate_pace_reset_equals_now() {
        let now = now_epoch();
        let pace = calculate_pace(30.0, Some(now), 18000, now);
        assert!(
            pace.is_none(),
            "reset == now is end of window; treat as expired"
        );
    }

    // ── format_pace() tests ──────────────────────────────────────────────────

    #[test]
    fn test_format_pace_positive() {
        assert_eq!(format_pace(Some(20.3)), "+20%");
    }

    #[test]
    fn test_format_pace_negative() {
        assert_eq!(format_pace(Some(-15.7)), "-16%");
    }

    #[test]
    fn test_format_pace_zero() {
        assert_eq!(format_pace(Some(0.0)), "+0%");
    }

    #[test]
    fn test_format_pace_none() {
        assert_eq!(format_pace(None), "?");
    }

    // ── pace in format_output() tests ────────────────────────────────────────

    #[test]
    fn test_format_output_pace_placeholder_five_hour() {
        let now_epoch = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let data = UsageLimitsData {
            five_hour_pct: 30.0,
            seven_day_pct: 10.0,
            five_hour_resets_at_epoch: Some(now_epoch + 9000),
            seven_day_resets_at_epoch: Some(now_epoch + 302400),
            ..Default::default()
        };
        let cfg = UsageLimitsConfig {
            five_hour_format: Some("5h {pct}% pace:{pace}".into()),
            seven_day_format: Some("7d {pct}%".into()),
            ..Default::default()
        };
        let result = format_output(&data, &cfg);
        assert!(
            result.contains("pace:+"),
            "expected positive pace in: {result:?}"
        );
        assert!(result.contains("5h 30%"), "expected 5h 30% in: {result:?}");
    }

    #[test]
    fn test_format_output_pace_placeholder_no_epoch() {
        let data = UsageLimitsData {
            five_hour_pct: 30.0,
            seven_day_pct: 10.0,
            ..Default::default()
        };
        let cfg = UsageLimitsConfig {
            five_hour_format: Some("5h {pct}% pace:{pace}".into()),
            seven_day_format: Some("7d {pct}%".into()),
            ..Default::default()
        };
        let result = format_output(&data, &cfg);
        assert!(
            result.contains("pace:?"),
            "expected ? for unknown pace in: {result:?}"
        );
    }

    // ── extra usage in format_output() tests ─────────────────────────────────

    #[test]
    fn test_format_output_extra_usage_enabled() {
        let data = UsageLimitsData {
            five_hour_pct: 100.0,
            seven_day_pct: 50.0,
            five_hour_resets_at: "2099-01-01T00:00:00Z".into(),
            seven_day_resets_at: "2099-01-01T00:00:00Z".into(),
            extra_usage_enabled: Some(true),
            extra_usage_monthly_limit: Some(20000.0),
            extra_usage_used_credits: Some(6195.0),
            extra_usage_utilization: Some(31.0),
            ..Default::default()
        };
        let cfg = UsageLimitsConfig {
            five_hour_format: Some("{pct}%".into()),
            seven_day_format: Some("{pct}%".into()),
            show_per_model: Some(true),
            ..Default::default()
        };
        let result = format_output(&data, &cfg);
        assert!(result.contains("100%"), "five_hour in: {result:?}");
        assert!(result.contains("50%"), "seven_day in: {result:?}");
        assert!(
            result.contains("extra:"),
            "extra usage default format in: {result:?}"
        );
        assert!(
            result.contains('\u{26a1}'),
            "active indicator (⚡) when 5h at 100%: {result:?}"
        );
        assert!(result.contains("31%"), "extra pct in: {result:?}");
        assert!(result.contains("61.95"), "used credits in: {result:?}");
        assert!(result.contains("200.00"), "monthly limit in: {result:?}");
    }

    #[test]
    fn test_format_output_extra_usage_inactive() {
        let data = UsageLimitsData {
            five_hour_pct: 50.0,
            seven_day_pct: 40.0,
            five_hour_resets_at: "2099-01-01T00:00:00Z".into(),
            seven_day_resets_at: "2099-01-01T00:00:00Z".into(),
            extra_usage_enabled: Some(true),
            extra_usage_monthly_limit: Some(20000.0),
            extra_usage_used_credits: Some(1000.0),
            extra_usage_utilization: Some(5.0),
            ..Default::default()
        };
        let cfg = UsageLimitsConfig {
            five_hour_format: Some("{pct}%".into()),
            seven_day_format: Some("{pct}%".into()),
            show_per_model: Some(true),
            ..Default::default()
        };
        let result = format_output(&data, &cfg);
        assert!(
            result.contains('\u{1f4a4}'),
            "inactive indicator (💤) when not rate-limited: {result:?}"
        );
        assert!(
            !result.contains('\u{26a1}'),
            "should not contain ⚡ when not rate-limited: {result:?}"
        );
    }

    #[test]
    fn test_format_output_extra_usage_disabled() {
        let data = UsageLimitsData {
            five_hour_pct: 50.0,
            seven_day_pct: 20.0,
            five_hour_resets_at: "2099-01-01T00:00:00Z".into(),
            seven_day_resets_at: "2099-01-01T00:00:00Z".into(),
            extra_usage_enabled: Some(false),
            extra_usage_monthly_limit: Some(20000.0),
            extra_usage_used_credits: Some(0.0),
            extra_usage_utilization: Some(0.0),
            ..Default::default()
        };
        let cfg = UsageLimitsConfig::default();
        let result = format_output(&data, &cfg);
        assert!(
            !result.contains("extra"),
            "extra usage should be hidden when disabled: {result:?}"
        );
    }

    #[test]
    fn test_format_output_extra_usage_absent() {
        let data = UsageLimitsData {
            five_hour_pct: 50.0,
            seven_day_pct: 20.0,
            five_hour_resets_at: "2099-01-01T00:00:00Z".into(),
            seven_day_resets_at: "2099-01-01T00:00:00Z".into(),
            ..Default::default()
        };
        let cfg = UsageLimitsConfig::default();
        let result = format_output(&data, &cfg);
        assert!(
            !result.contains("extra"),
            "extra usage should be absent: {result:?}"
        );
    }

    #[test]
    fn test_format_output_extra_usage_custom_format() {
        let data = UsageLimitsData {
            five_hour_pct: 100.0,
            seven_day_pct: 50.0,
            five_hour_resets_at: "2099-01-01T00:00:00Z".into(),
            seven_day_resets_at: "2099-01-01T00:00:00Z".into(),
            extra_usage_enabled: Some(true),
            extra_usage_monthly_limit: Some(20000.0),
            extra_usage_used_credits: Some(6195.0),
            extra_usage_utilization: Some(31.0),
            ..Default::default()
        };
        let cfg = UsageLimitsConfig {
            five_hour_format: Some("{pct}%".into()),
            seven_day_format: Some("{pct}%".into()),
            extra_usage_format: Some("EXTRA {pct}% rem:{remaining_credits}".into()),
            show_per_model: Some(true),
            ..Default::default()
        };
        let result = format_output(&data, &cfg);
        assert!(result.contains("EXTRA 31%"), "custom format: {result:?}");
        assert!(
            result.contains("rem:138.05"),
            "remaining credits: {result:?}"
        );
    }

    // ── per-model in format_output() tests ───────────────────────────────────

    #[test]
    fn test_format_output_per_model_present() {
        let data = UsageLimitsData {
            five_hour_pct: 50.0,
            seven_day_pct: 30.0,
            five_hour_resets_at: "2099-01-01T00:00:00Z".into(),
            seven_day_resets_at: "2099-01-01T00:00:00Z".into(),
            seven_day_opus_pct: Some(12.0),
            seven_day_opus_resets_at: Some("2099-02-01T00:00:00Z".into()),
            seven_day_sonnet_pct: Some(3.0),
            seven_day_sonnet_resets_at: Some("2099-03-01T00:00:00Z".into()),
            ..Default::default()
        };
        let cfg = UsageLimitsConfig {
            five_hour_format: Some("{pct}%".into()),
            seven_day_format: Some("{pct}%".into()),
            show_per_model: Some(true),
            ..Default::default()
        };
        let result = format_output(&data, &cfg);
        assert!(result.contains("opus 12%"), "opus breakdown in: {result:?}");
        assert!(
            result.contains("sonnet 3%"),
            "sonnet breakdown in: {result:?}"
        );
        assert!(
            !result.contains("cowork"),
            "null cowork should be omitted: {result:?}"
        );
        assert!(
            !result.contains("oauth"),
            "null oauth_apps should be omitted: {result:?}"
        );
    }

    #[test]
    fn test_format_output_per_model_custom_format() {
        let data = UsageLimitsData {
            five_hour_pct: 50.0,
            seven_day_pct: 30.0,
            five_hour_resets_at: "2099-01-01T00:00:00Z".into(),
            seven_day_resets_at: "2099-01-01T00:00:00Z".into(),
            seven_day_opus_pct: Some(12.0),
            seven_day_opus_resets_at: Some("2099-02-01T00:00:00Z".into()),
            ..Default::default()
        };
        let cfg = UsageLimitsConfig {
            five_hour_format: Some("{pct}%".into()),
            seven_day_format: Some("{pct}%".into()),
            opus_format: Some("OP:{pct}%/{remaining}%".into()),
            show_per_model: Some(true),
            ..Default::default()
        };
        let result = format_output(&data, &cfg);
        assert!(
            result.contains("OP:12%/88%"),
            "custom opus format: {result:?}"
        );
    }

    #[test]
    fn test_format_output_per_model_pace_placeholder() {
        let now_epoch = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let data = UsageLimitsData {
            five_hour_pct: 50.0,
            seven_day_pct: 30.0,
            five_hour_resets_at: "2099-01-01T00:00:00Z".into(),
            seven_day_resets_at: "2099-01-01T00:00:00Z".into(),
            seven_day_opus_pct: Some(12.0),
            seven_day_opus_resets_at: Some(epoch_to_iso(Some(now_epoch + 302_400))),
            ..Default::default()
        };
        let cfg = UsageLimitsConfig {
            five_hour_format: Some("{pct}%".into()),
            seven_day_format: Some("{pct}%".into()),
            opus_format: Some("opus {pct}% pace:{pace}".into()),
            show_per_model: Some(true),
            ..Default::default()
        };
        let result = format_output(&data, &cfg);
        assert!(
            !result.contains("{pace}"),
            "literal {{pace}} should be substituted: {result:?}"
        );
        assert!(
            result.contains("pace:+"),
            "opus pace should show headroom: {result:?}"
        );
    }

    // ── resolve_data memoization tests (Fix #2) ──────────────────────────────

    /// Test-only mirror of the production memoization wrapper. Lets us exercise
    /// the cache mechanism (which is `cfg(not(test))`-gated in `resolve_data`)
    /// without re-enabling it for unrelated tests on the same worker thread.
    fn resolve_data_memoized(
        compute: impl FnOnce() -> Option<UsageLimitsData>,
    ) -> Option<UsageLimitsData> {
        RESOLVE_CACHE.with(|c| {
            if let Some(cached) = c.borrow().as_ref() {
                return cached.clone();
            }
            let computed = compute();
            *c.borrow_mut() = Some(computed.clone());
            computed
        })
    }

    fn reset_resolve_cache() {
        RESOLVE_CACHE.with(|c| *c.borrow_mut() = None);
    }

    #[test]
    fn test_resolve_data_memoizes_across_calls() {
        reset_resolve_cache();
        let counter = std::sync::atomic::AtomicUsize::new(0);
        let make_compute = || {
            counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Some(UsageLimitsData {
                five_hour_pct: 42.0,
                ..Default::default()
            })
        };

        let first = resolve_data_memoized(make_compute);
        let second = resolve_data_memoized(make_compute);
        let third = resolve_data_memoized(make_compute);

        assert_eq!(
            counter.load(std::sync::atomic::Ordering::SeqCst),
            1,
            "compute closure should run exactly once"
        );
        assert!((first.unwrap().five_hour_pct - 42.0).abs() < f64::EPSILON);
        assert!((second.unwrap().five_hour_pct - 42.0).abs() < f64::EPSILON);
        assert!((third.unwrap().five_hour_pct - 42.0).abs() < f64::EPSILON);
        reset_resolve_cache(); // courtesy: don't leak to whatever runs next on this thread
    }

    #[test]
    fn test_resolve_data_memoizes_none_results() {
        reset_resolve_cache();
        let counter = std::sync::atomic::AtomicUsize::new(0);
        let compute = || {
            counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            None
        };
        let first = resolve_data_memoized(compute);
        let second = resolve_data_memoized(compute);
        assert!(first.is_none());
        assert!(second.is_none());
        assert_eq!(
            counter.load(std::sync::atomic::Ordering::SeqCst),
            1,
            "None results should also be memoized — avoids re-fetching on cold cache"
        );
        reset_resolve_cache();
    }

    // ── show_per_model toggle tests (Fix #4) ─────────────────────────────────

    #[test]
    fn test_format_output_default_omits_per_model_only() {
        // show_per_model defaults to false, so per-model sections are hidden.
        // Extra-usage is always appended when enabled, regardless of show_per_model.
        let data = UsageLimitsData {
            five_hour_pct: 50.0,
            seven_day_pct: 30.0,
            five_hour_resets_at: "2099-01-01T00:00:00Z".into(),
            seven_day_resets_at: "2099-01-01T00:00:00Z".into(),
            seven_day_opus_pct: Some(12.0),
            seven_day_opus_resets_at: Some("2099-02-01T00:00:00Z".into()),
            extra_usage_enabled: Some(true),
            extra_usage_monthly_limit: Some(20000.0),
            extra_usage_used_credits: Some(6195.0),
            extra_usage_utilization: Some(31.0),
            ..Default::default()
        };
        let cfg = UsageLimitsConfig {
            five_hour_format: Some("{pct}%".into()),
            seven_day_format: Some("{pct}%".into()),
            // show_per_model omitted → defaults to false
            ..Default::default()
        };
        let result = format_output(&data, &cfg);
        assert!(
            result.starts_with("50% | 30%"),
            "legacy 5h/7d prefix: {result:?}"
        );
        assert!(!result.contains("opus"), "opus must be hidden by default");
        assert!(
            result.contains("extra"),
            "extra-usage must show when enabled: {result:?}"
        );
    }

    #[test]
    fn test_format_output_show_per_model_true_includes_sections() {
        let data = UsageLimitsData {
            five_hour_pct: 50.0,
            seven_day_pct: 30.0,
            five_hour_resets_at: "2099-01-01T00:00:00Z".into(),
            seven_day_resets_at: "2099-01-01T00:00:00Z".into(),
            seven_day_opus_pct: Some(12.0),
            seven_day_opus_resets_at: Some("2099-02-01T00:00:00Z".into()),
            ..Default::default()
        };
        let cfg = UsageLimitsConfig {
            five_hour_format: Some("{pct}%".into()),
            seven_day_format: Some("{pct}%".into()),
            show_per_model: Some(true),
            ..Default::default()
        };
        let result = format_output(&data, &cfg);
        assert!(result.contains("opus 12%"), "opus visible: {result:?}");
    }

    // ── extra_usage placeholder tests (Fix #3) ───────────────────────────────

    #[test]
    fn test_format_extra_usage_remaining_credits_substituted() {
        let data = UsageLimitsData {
            five_hour_pct: 0.0,
            seven_day_pct: 0.0,
            extra_usage_enabled: Some(true),
            extra_usage_monthly_limit: Some(20000.0),
            extra_usage_used_credits: Some(6195.0),
            extra_usage_utilization: Some(31.0),
            ..Default::default()
        };
        let cfg = UsageLimitsConfig {
            extra_usage_format: Some("rem={remaining_credits} pct={pct}".into()),
            ..Default::default()
        };
        let out = format_extra_usage(&data, &cfg).expect("extra_usage should render");
        assert!(out.contains("rem=138.05"), "remaining_credits: {out:?}");
        assert!(out.contains("pct=31"), "pct still works: {out:?}");
    }

    #[test]
    fn test_format_extra_usage_remaining_no_longer_substituted_in_extra() {
        // Guards against regression: the old `{remaining}` placeholder must NOT
        // be substituted in extra_usage_format (where it had a confusing
        // dollar-amount semantic). It's reserved for the percentage-based
        // formatters now.
        let data = UsageLimitsData {
            extra_usage_enabled: Some(true),
            extra_usage_monthly_limit: Some(20000.0),
            extra_usage_used_credits: Some(6195.0),
            extra_usage_utilization: Some(31.0),
            ..Default::default()
        };
        let cfg = UsageLimitsConfig {
            extra_usage_format: Some("rem={remaining}".into()),
            ..Default::default()
        };
        let out = format_extra_usage(&data, &cfg).expect("extra_usage renders");
        assert_eq!(
            out, "rem={remaining}",
            "literal {{remaining}} should pass through untouched"
        );
    }

    #[test]
    fn test_format_extra_usage_renders_dollars_from_cents() {
        // The Anthropic OAuth usage API reports `used_credits` and
        // `monthly_limit` in cents. Verified against a real Claude Enterprise
        // response: monthly_limit=30000 cents = $300.00, used=23076 cents =
        // $230.76. The default format must render those as $230.76 / $300.00.
        let data = UsageLimitsData {
            extra_usage_enabled: Some(true),
            extra_usage_monthly_limit: Some(30000.0),
            extra_usage_used_credits: Some(23076.0),
            extra_usage_utilization: Some(76.92),
            ..Default::default()
        };
        let cfg = UsageLimitsConfig::default();
        let out = format_extra_usage(&data, &cfg).expect("extra_usage enabled => Some");
        assert!(out.contains("230.76"), "expected $230.76 in output: {out}");
        assert!(out.contains("300.00"), "expected $300.00 in output: {out}");
        assert!(
            !out.contains("23076"),
            "raw cents must not appear in output: {out}"
        );
        assert!(
            !out.contains("30000"),
            "raw cents must not appear in output: {out}"
        );
    }

    #[test]
    fn test_format_extra_usage_remaining_credits_dollars_from_cents() {
        // monthly_limit=30000c, used=23076c → remaining=6924c → $69.24
        let data = UsageLimitsData {
            extra_usage_enabled: Some(true),
            extra_usage_monthly_limit: Some(30000.0),
            extra_usage_used_credits: Some(23076.0),
            extra_usage_utilization: Some(76.92),
            ..Default::default()
        };
        let cfg = UsageLimitsConfig {
            extra_usage_format: Some("rem ${remaining_credits}".into()),
            ..Default::default()
        };
        let out = format_extra_usage(&data, &cfg).expect("extra_usage enabled => Some");
        assert_eq!(out, "rem $69.24");
    }

    #[test]
    fn test_active_glyph_on_enterprise_above_warn() {
        let data = UsageLimitsData {
            extra_usage_enabled: Some(true),
            extra_usage_monthly_limit: Some(20000.0),
            extra_usage_used_credits: Some(17000.0),
            extra_usage_utilization: Some(85.0),
            ..Default::default()
        };
        let cfg = UsageLimitsConfig {
            warn_threshold: Some(80.0),
            extra_usage_format: Some("{active}".into()),
            ..Default::default()
        };
        let out = format_extra_usage(&data, &cfg).expect("Some");
        assert_eq!(
            out, "\u{26a1}", /* ⚡ */
            "above warn => lightning bolt"
        );
    }

    #[test]
    fn test_active_glyph_on_enterprise_below_warn() {
        let data = UsageLimitsData {
            extra_usage_enabled: Some(true),
            extra_usage_monthly_limit: Some(20000.0),
            extra_usage_used_credits: Some(10000.0),
            extra_usage_utilization: Some(50.0),
            ..Default::default()
        };
        let cfg = UsageLimitsConfig {
            warn_threshold: Some(80.0),
            extra_usage_format: Some("{active}".into()),
            ..Default::default()
        };
        let out = format_extra_usage(&data, &cfg).expect("Some");
        assert_eq!(
            out, "\u{1f4a4}", /* 💤 */
            "below warn => sleep emoji"
        );
    }

    #[test]
    fn test_active_glyph_on_enterprise_no_threshold_any_usage() {
        // No warn_threshold configured — ANY non-zero utilization shows ⚡.
        let data = UsageLimitsData {
            extra_usage_enabled: Some(true),
            extra_usage_utilization: Some(0.1),
            extra_usage_monthly_limit: Some(20000.0),
            extra_usage_used_credits: Some(20.0),
            ..Default::default()
        };
        let cfg = UsageLimitsConfig {
            extra_usage_format: Some("{active}".into()),
            ..Default::default()
        };
        let out = format_extra_usage(&data, &cfg).expect("Some");
        assert_eq!(out, "\u{26a1}" /* ⚡ */);
    }

    #[test]
    fn test_active_glyph_pro_max_unchanged_at_5h_full() {
        // Pre-existing semantic: ⚡ when 5h or 7d at 100%, regardless of extra_usage.
        let data = UsageLimitsData {
            five_hour_pct: 100.0,
            extra_usage_enabled: Some(true),
            extra_usage_utilization: Some(0.0),
            extra_usage_monthly_limit: Some(20000.0),
            extra_usage_used_credits: Some(0.0),
            ..Default::default()
        };
        let cfg = UsageLimitsConfig {
            extra_usage_format: Some("{active}".into()),
            ..Default::default()
        };
        let out = format_extra_usage(&data, &cfg).expect("Some");
        assert_eq!(out, "\u{26a1}" /* ⚡ */);
    }

    #[test]
    fn test_format_output_no_dangling_separators() {
        let data = UsageLimitsData {
            five_hour_pct: 50.0,
            seven_day_pct: 30.0,
            five_hour_resets_at: "2099-01-01T00:00:00Z".into(),
            seven_day_resets_at: "2099-01-01T00:00:00Z".into(),
            ..Default::default()
        };
        let cfg = UsageLimitsConfig {
            five_hour_format: Some("{pct}%".into()),
            seven_day_format: Some("{pct}%".into()),
            separator: Some(" | ".into()),
            ..Default::default()
        };
        let result = format_output(&data, &cfg);
        assert_eq!(result, "50% | 30%", "no trailing separator: {result:?}");
        assert!(!result.ends_with(" | "), "no dangling sep: {result:?}");
    }

    fn current_fingerprint() -> Option<String> {
        crate::platform::get_oauth_token()
            .ok()
            .map(|t| crate::platform::token_fingerprint(&t))
    }

    #[test]
    fn test_render_enterprise_extra_usage_only() {
        let tmp = tempfile::tempdir().unwrap();
        let transcript_path = tmp.path().join("transcript.jsonl");
        std::fs::write(&transcript_path, "").unwrap();

        let data = UsageLimitsData {
            extra_usage_enabled: Some(true),
            extra_usage_monthly_limit: Some(30000.0),
            extra_usage_used_credits: Some(23076.0),
            extra_usage_utilization: Some(76.92),
            ..Default::default()
        };
        let fp = current_fingerprint();
        crate::cache::write_usage_limits(&transcript_path, &data, 600, fp.as_deref());

        let ctx = Context {
            transcript_path: Some(transcript_path.to_string_lossy().into()),
            ..Default::default()
        };
        let cfg = CshipConfig::default();
        let out = render(&ctx, &cfg).expect("Enterprise: extra_usage_only must render");
        assert!(!out.contains("5h:"), "must not include 5h section: {out}");
        assert!(!out.contains("7d:"), "must not include 7d section: {out}");
        assert!(out.contains("230.76"), "must include used dollars: {out}");
        assert!(out.contains("300.00"), "must include limit dollars: {out}");
    }

    #[test]
    fn test_render_enterprise_extra_disabled_returns_none() {
        let tmp = tempfile::tempdir().unwrap();
        let transcript_path = tmp.path().join("transcript.jsonl");
        std::fs::write(&transcript_path, "").unwrap();

        let data = UsageLimitsData {
            extra_usage_enabled: Some(false),
            ..Default::default()
        };
        let fp = current_fingerprint();
        crate::cache::write_usage_limits(&transcript_path, &data, 600, fp.as_deref());

        let ctx = Context {
            transcript_path: Some(transcript_path.to_string_lossy().into()),
            ..Default::default()
        };
        let cfg = CshipConfig::default();
        assert!(render(&ctx, &cfg).is_none());
    }
}
