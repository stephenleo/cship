//! Usage limits module — renders 5h and 7d API utilization with time-to-reset.
//!
//! Per-model + extra-usage data comes from the cross-process shared
//! coordinator (`crate::usage_limits_state::resolve_shared_usage`), which
//! fetches from the Anthropic API at most once per the configured interval
//! (60s default, user-configurable via `ttl`) across all concurrent
//! `cship` processes. This module only merges that with the
//! freshest 5h/7d values from stdin and formats the result — it does not
//! drive scheduling or HTTP calls itself.
//!
//! [Source: architecture.md#src/modules/usage_limits.rs]

use crate::config::{CshipConfig, UsageLimitsConfig};
use crate::context::Context;
use crate::usage_limits::UsageLimitsData;

/// A single window's percentage + reset, tagged with which source produced it.
/// Reset and percentage always travel together so a merge can never pair one
/// source's pct with another source's reset (the bug this type exists to prevent).
#[derive(Debug, Clone, PartialEq)]
struct UsageWindowSnapshot {
    pct: f64,
    resets_at_epoch: Option<u64>,
    resets_at_iso: String,
}

impl UsageWindowSnapshot {
    fn new(pct: f64, epoch: Option<u64>, iso: &str) -> Self {
        let resets_at_epoch = resolve_epoch(epoch, iso);
        Self {
            pct,
            resets_at_epoch,
            resets_at_iso: iso.to_string(),
        }
    }
}

/// Result of merging a stdin and an API window snapshot for one period (5h or 7d).
#[derive(Debug, Clone, PartialEq, Default)]
struct MergedUsageWindow {
    pct: f64,
    resets_at_epoch: Option<u64>,
    resets_at_iso: String,
}

impl MergedUsageWindow {
    fn empty() -> Self {
        Self::default()
    }
}

impl From<&UsageWindowSnapshot> for MergedUsageWindow {
    fn from(s: &UsageWindowSnapshot) -> Self {
        Self {
            pct: s.pct,
            resets_at_epoch: s.resets_at_epoch,
            resets_at_iso: s.resets_at_iso.clone(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WindowState {
    Live,
    Expired,
    Unknown,
}

fn classify_window(snapshot: &UsageWindowSnapshot, now: u64) -> WindowState {
    match snapshot.resets_at_epoch {
        Some(e) if e > now => WindowState::Live,
        Some(_) => WindowState::Expired,
        None => WindowState::Unknown,
    }
}

/// Same-window tolerance: reset timestamps within this many seconds of each
/// other are treated as the same underlying window (clock/rounding slop
/// between stdin's epoch and the API's ISO-parsed epoch).
const SAME_WINDOW_TOLERANCE_SECS: u64 = 60;

/// Merge a stdin and an API window snapshot for one period into a single
/// authoritative percentage + reset pair. See module docs / plan doc for the
/// full decision table (Phase 1, "Required Merge Rules").
fn merge_window(
    stdin: Option<UsageWindowSnapshot>,
    api: Option<UsageWindowSnapshot>,
    now: u64,
) -> MergedUsageWindow {
    let stdin_state = stdin.as_ref().map(|s| classify_window(s, now));
    let api_state = api.as_ref().map(|s| classify_window(s, now));

    match (&stdin, stdin_state, &api, api_state) {
        (Some(s), Some(WindowState::Live), Some(a), Some(WindowState::Live)) => {
            let s_reset = s.resets_at_epoch.unwrap();
            let a_reset = a.resets_at_epoch.unwrap();
            if s_reset.abs_diff(a_reset) <= SAME_WINDOW_TOLERANCE_SECS {
                if a.pct >= s.pct { a.into() } else { s.into() }
            } else if a_reset > s_reset {
                a.into()
            } else {
                s.into()
            }
        }
        (Some(s), Some(WindowState::Live), _, _) => s.into(),
        (_, _, Some(a), Some(WindowState::Live)) => a.into(),
        // API's unknown-reset pct is preferred over stdin's (API is account-wide),
        // so check it first — both arms produce the same shape, so one helper
        // covers "whichever side is Unknown" instead of two near-identical arms.
        (_, _, Some(a), Some(WindowState::Unknown)) => unknown_result(a.pct),
        (Some(s), Some(WindowState::Unknown), _, _) => unknown_result(s.pct),
        _ => MergedUsageWindow::empty(),
    }
}

/// A pct with no trustworthy reset time attached — used whenever the
/// selected source's window state is `Unknown`.
fn unknown_result(pct: f64) -> MergedUsageWindow {
    MergedUsageWindow {
        pct,
        resets_at_epoch: None,
        resets_at_iso: String::new(),
    }
}

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

/// Mirrors the pre-coordinator behavior: no live OAuth fetch without a
/// transcript_path, even if a token is available. A real Claude Code session
/// always sends one; input that doesn't (e.g. an integration test driving
/// the compiled binary directly with synthetic stdin) gets no credential
/// read and no HTTP call.
fn full_data_eligible(ctx: &Context) -> bool {
    ctx.transcript_path.is_some()
}

/// Per-model + extra-usage data from the cross-process shared coordinator.
///
/// Reads the real OAuth credential and, when eligible, drives a real HTTP
/// call and real cross-process cache file via
/// `usage_limits_state::resolve_shared_usage`. Token failure is non-fatal —
/// stdin data is still usable.
///
/// Three independent layers keep this from touching real credentials/network
/// where it shouldn't: `full_data_eligible` gates on `transcript_path` (this
/// function, both real and test contexts); the integration test harness
/// (`tests/cli.rs`) isolates `CLAUDE_HOME` so `get_oauth_token()` can't find
/// a real credential even when `transcript_path` is set; and `#[cfg(test)]`
/// replaces this whole function with a stub for `cargo test --lib`, since
/// `get_oauth_token()` reads whatever the *host machine* running unit tests
/// happens to have, with no test-injectable seam at that layer.
/// `resolve_shared_usage`'s own logic is covered directly by
/// `usage_limits_state::tests`; the full integration is a manual-verification
/// item (see the Task 4 plan, Phase 7).
#[cfg(not(test))]
fn resolve_full_data(ctx: &Context, ul_cfg: Option<&UsageLimitsConfig>) -> Option<UsageLimitsData> {
    if !full_data_eligible(ctx) {
        return None;
    }
    let token = match crate::platform::get_oauth_token() {
        Ok(t) => Some(t),
        Err(e) => {
            tracing::warn!("cship.usage_limits: credential retrieval failed: {e}");
            None
        }
    };
    crate::usage_limits_state::resolve_shared_usage(
        token.as_deref(),
        ul_cfg.and_then(|c| c.ttl),
        now_epoch(),
    )
    .usage
}

#[cfg(test)]
thread_local! {
    /// What the test-build `resolve_full_data` returns in place of the real
    /// OAuth/coordinator data. `None` (the default) means "no API data".
    /// Set through `tests::FullDataGuard`, which restores it on drop because
    /// cargo reuses worker threads between tests.
    static TEST_FULL_DATA: std::cell::RefCell<Option<UsageLimitsData>> =
        const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
fn resolve_full_data(
    _ctx: &Context,
    _ul_cfg: Option<&UsageLimitsConfig>,
) -> Option<UsageLimitsData> {
    TEST_FULL_DATA.with(|d| d.borrow().clone())
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

    // Stdin provides the freshest 5h/7d values
    let stdin_data = data_from_stdin_rate_limits(ctx);
    let full_data = resolve_full_data(ctx, ul_cfg);

    let now = now_epoch();
    match (stdin_data, full_data) {
        (None, None) => None,
        (stdin, full) => {
            let five_h = merge_window(
                stdin.as_ref().map(|s| {
                    UsageWindowSnapshot::new(
                        s.five_hour_pct,
                        s.five_hour_resets_at_epoch,
                        &s.five_hour_resets_at,
                    )
                }),
                full.as_ref().map(|f| {
                    UsageWindowSnapshot::new(
                        f.five_hour_pct,
                        f.five_hour_resets_at_epoch,
                        &f.five_hour_resets_at,
                    )
                }),
                now,
            );
            let seven_d = merge_window(
                stdin.as_ref().map(|s| {
                    UsageWindowSnapshot::new(
                        s.seven_day_pct,
                        s.seven_day_resets_at_epoch,
                        &s.seven_day_resets_at,
                    )
                }),
                full.as_ref().map(|f| {
                    UsageWindowSnapshot::new(
                        f.seven_day_pct,
                        f.seven_day_resets_at_epoch,
                        &f.seven_day_resets_at,
                    )
                }),
                now,
            );
            // Non-window fields (extra_usage_*, per-model breakdowns) only
            // ever come from `full` — prefer it as the base when present,
            // matching the previous `..full` spread; fall back to `stdin`
            // (whose non-window fields are all `Default`) when `full` is
            // absent, so the arm still produces a value in the
            // single-source stdin-only case.
            let five_hour_available = stdin.as_ref().is_some_and(|d| d.five_hour_available)
                || full.as_ref().is_some_and(|d| d.five_hour_available);
            let seven_day_available = stdin.as_ref().is_some_and(|d| d.seven_day_available)
                || full.as_ref().is_some_and(|d| d.seven_day_available);
            let base = full.unwrap_or_else(|| stdin.unwrap_or_default());
            Some(UsageLimitsData {
                five_hour_pct: five_h.pct,
                seven_day_pct: seven_d.pct,
                five_hour_resets_at: five_h.resets_at_iso,
                seven_day_resets_at: seven_d.resets_at_iso,
                five_hour_resets_at_epoch: five_h.resets_at_epoch,
                seven_day_resets_at_epoch: seven_d.resets_at_epoch,
                five_hour_available,
                seven_day_available,
                ..base
            })
        }
    }
}

/// True when neither source ever carried a 5h or 7d window at all — i.e. on
/// Claude Enterprise, where the API returns these fields as `null`/absent.
/// Used to switch the renderer into "extra-usage only" mode.
///
/// Checks presence (`*_available`), not value: a normal account whose 5h
/// window just reset reports `{utilization: 0, resets_at: null}` — present,
/// just idle — and must still render as a normal zero-usage window rather
/// than being mistaken for a plan where standard windows don't exist.
pub(crate) fn lacks_standard_signal(data: &UsageLimitsData) -> bool {
    !data.five_hour_available && !data.seven_day_available
}

/// Apply threshold styling using the higher of 5h/7d utilization.
/// Falls back to `extra_usage_utilization` whenever both standard percentages
/// are zero — either because neither window is available (Enterprise plans,
/// where 5h/7d data is absent) or because a normal account's windows are
/// genuinely idle at 0%. `lacks_standard_signal` is presence-based and not
/// checked here; this fallback is purely value-based, so it applies in both
/// cases.
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

/// Which standard usage window a per-period token renders.
enum Period {
    FiveHour,
    SevenDay,
}

/// Render a single standard window (5h or 7d) as its own token, colored by *that
/// window's* utilization — unlike `$cship.usage_limits`, whose threshold color keys
/// off `max(5h, 7d)`. This lets `session` and `weekly` sit on separate lines, each
/// escalating independently. Returns `None` on plans without standard 5h/7d
/// windows (e.g. Enterprise) — unlike `render`, it does not fall back to the
/// extra-usage section, since this token is specifically the 5h/7d window.
fn render_period(ctx: &Context, cfg: &CshipConfig, period: Period) -> Option<String> {
    let data = resolve_data(ctx, cfg)?;
    if lacks_standard_signal(&data) {
        return None;
    }
    let default_ul_cfg = UsageLimitsConfig::default();
    let ul_cfg = cfg.usage_limits.as_ref().unwrap_or(&default_ul_cfg);
    let now = now_epoch();

    let (pct, epoch, window_secs, fmt) = match period {
        Period::FiveHour => (
            data.five_hour_pct,
            resolve_epoch(data.five_hour_resets_at_epoch, &data.five_hour_resets_at),
            FIVE_HOUR_SECS,
            ul_cfg
                .five_hour_format
                .as_deref()
                .unwrap_or("5h: {pct}% resets in {reset}"),
        ),
        Period::SevenDay => (
            data.seven_day_pct,
            resolve_epoch(data.seven_day_resets_at_epoch, &data.seven_day_resets_at),
            SEVEN_DAY_SECS,
            ul_cfg
                .seven_day_format
                .as_deref()
                .unwrap_or("7d: {pct}% resets in {reset}"),
        ),
    };

    let content = format_period_part(fmt, pct, epoch, window_secs, now, ul_cfg);
    Some(crate::ansi::apply_style_with_threshold(
        &content,
        Some(pct),
        ul_cfg.style.as_deref(),
        ul_cfg.warn_threshold,
        ul_cfg.warn_style.as_deref(),
        ul_cfg.critical_threshold,
        ul_cfg.critical_style.as_deref(),
    ))
}

/// Render only the 5-hour (session) window, colored by its own utilization.
pub fn render_five_hour(ctx: &Context, cfg: &CshipConfig) -> Option<String> {
    render_period(ctx, cfg, Period::FiveHour)
}

/// Render only the 7-day (weekly) window, colored by its own utilization.
pub fn render_seven_day(ctx: &Context, cfg: &CshipConfig) -> Option<String> {
    render_period(ctx, cfg, Period::SevenDay)
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
        five_hour_available: rl.five_hour.is_some(),
        seven_day_available: rl.seven_day.is_some(),
        ..Default::default()
    })
}

/// Reset-window lengths, used for pace calculation.
const FIVE_HOUR_SECS: u64 = 18_000;
const SEVEN_DAY_SECS: u64 = 604_800;

/// Default `{bar}` width in characters when `bar_width` is unset.
const DEFAULT_BAR_WIDTH: usize = 10;

/// Upper bound on `{bar}` width. A statusline is narrow, and this caps the
/// string `render_bar` allocates from an over-large `bar_width`.
const MAX_BAR_WIDTH: usize = 200;

/// Render a proportional Unicode bar for `pct` (0..=100) using the module's bar config.
///
/// Mirrors `context_bar`'s fill math (kept in sync deliberately): the fill count
/// floors via `as usize` truncation (a visual approximation) while the `{pct}` text
/// rounds. Width is clamped to `MAX_BAR_WIDTH`. Fill/empty glyphs and width are
/// configurable via `bar_filled_char` / `bar_empty_char` / `bar_width`.
fn render_bar(pct: f64, cfg: &UsageLimitsConfig) -> String {
    let width = cfg
        .bar_width
        .map(|w| (w as usize).min(MAX_BAR_WIDTH))
        .unwrap_or(DEFAULT_BAR_WIDTH);
    let filled = (((pct / 100.0) * width as f64) as usize).min(width);
    let empty = width - filled;
    let filled_char = cfg.bar_filled_char.as_deref().unwrap_or("█");
    let empty_char = cfg.bar_empty_char.as_deref().unwrap_or("░");
    filled_char.repeat(filled) + &empty_char.repeat(empty)
}

/// Substitute all per-period placeholders (including `{bar}`) into `fmt`.
///
/// `window_secs` is the reset-window length used for the `{pace}` calculation
/// (`FIVE_HOUR_SECS` for the 5h window, `SEVEN_DAY_SECS` for the 7d window).
fn format_period_part(
    fmt: &str,
    pct: f64,
    epoch: Option<u64>,
    window_secs: u64,
    now: u64,
    cfg: &UsageLimitsConfig,
) -> String {
    let pct_str = format!("{:.0}", pct);
    let remaining_str = format!("{:.0}", (100.0 - pct).max(0.0));
    let reset_str = format_reset(epoch, now);
    let reset_at_str = format_reset_at(epoch, now);
    let pace_str = format_pace(calculate_pace(pct, epoch, window_secs, now));
    let out = fmt
        .replace("{pct}", &pct_str)
        .replace("{remaining}", &remaining_str)
        .replace("{reset}", &reset_str)
        .replace("{reset_at}", &reset_at_str)
        .replace("{pace}", &pace_str);
    // Substitute `{bar}` last, and only when referenced: the (potentially large)
    // bar string is then built solely when needed, and its glyphs are never
    // re-scanned for the other placeholders.
    if out.contains("{bar}") {
        out.replace("{bar}", &render_bar(pct, cfg))
    } else {
        out
    }
}

/// Format usage data using configurable format strings.
///
/// Placeholders in format strings (all occurrences are substituted):
/// - `{pct}` — percentage used as integer (e.g. `"23"`)
/// - `{remaining}` — percentage remaining as integer (e.g. `"77"`)
/// - `{reset}` — time-until-reset string (e.g. `"4h12m"`)
/// - `{reset_at}` — absolute local reset time (e.g. `"7:42 PM"` or `"Mon 9:00 AM"`)
/// - `{pace}` — signed pace string (e.g. `"+20%"`, `"-15%"`, `"?"`)
/// - `{bar}` — proportional Unicode bar for this period (e.g. `"████░░░░░░"`)
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

    let five_h_epoch = resolve_epoch(data.five_hour_resets_at_epoch, &data.five_hour_resets_at);
    let seven_d_epoch = resolve_epoch(data.seven_day_resets_at_epoch, &data.seven_day_resets_at);

    let five_h_fmt = cfg
        .five_hour_format
        .as_deref()
        .unwrap_or("5h: {pct}% resets in {reset}");
    let seven_d_fmt = cfg
        .seven_day_format
        .as_deref()
        .unwrap_or("7d: {pct}% resets in {reset}");

    let five_h_part = format_period_part(
        five_h_fmt,
        data.five_hour_pct,
        five_h_epoch,
        FIVE_HOUR_SECS,
        now,
        cfg,
    );
    let seven_d_part = format_period_part(
        seven_d_fmt,
        data.seven_day_pct,
        seven_d_epoch,
        SEVEN_DAY_SECS,
        now,
        cfg,
    );

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
            five_hour_available: true,
            seven_day_available: true,
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
    fn test_lacks_standard_signal_returns_false_when_five_hour_available() {
        let data = UsageLimitsData {
            five_hour_pct: 1.0,
            five_hour_available: true,
            ..Default::default()
        };
        assert!(!lacks_standard_signal(&data));
    }

    #[test]
    fn test_lacks_standard_signal_returns_false_when_seven_day_available() {
        let data = UsageLimitsData {
            seven_day_resets_at: "2099-01-01T00:00:00+00:00".into(),
            seven_day_available: true,
            ..Default::default()
        };
        assert!(!lacks_standard_signal(&data));
    }

    #[test]
    fn test_lacks_standard_signal_returns_false_when_only_epoch_and_available_set() {
        let data = UsageLimitsData {
            five_hour_resets_at_epoch: Some(1_700_000_000),
            five_hour_available: true,
            ..Default::default()
        };
        assert!(!lacks_standard_signal(&data));
    }

    #[test]
    fn test_lacks_standard_signal_returns_false_for_idle_but_available_window() {
        // The core regression case: a supported window that just reset
        // (utilization 0, resets_at null) is still `available` — must not be
        // mistaken for a plan where the window doesn't exist (Enterprise).
        let data = UsageLimitsData {
            five_hour_pct: 0.0,
            seven_day_pct: 62.0,
            five_hour_available: true,
            seven_day_available: true,
            ..Default::default()
        };
        assert!(!lacks_standard_signal(&data));
    }

    #[test]
    fn test_lacks_standard_signal_returns_true_when_unavailable_despite_stale_pct() {
        // Presence, not value, is authoritative: even if a pct/reset value is
        // (incorrectly) populated without the available flag, absence wins.
        let data = UsageLimitsData {
            five_hour_pct: 1.0,
            five_hour_available: false,
            ..Default::default()
        };
        assert!(lacks_standard_signal(&data));
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
    fn test_full_data_eligible_requires_transcript_path() {
        let ctx = Context {
            transcript_path: None,
            ..Default::default()
        };
        assert!(!full_data_eligible(&ctx));

        let ctx = Context {
            transcript_path: Some("/tmp/t.jsonl".to_string()),
            ..Default::default()
        };
        assert!(full_data_eligible(&ctx));
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

    // ── {bar} placeholder + per-period token tests ───────────────────────────

    #[test]
    fn test_render_bar_fills_proportionally() {
        let cfg = UsageLimitsConfig::default();
        assert_eq!(render_bar(0.0, &cfg), "░░░░░░░░░░");
        assert_eq!(render_bar(50.0, &cfg), "█████░░░░░");
        assert_eq!(render_bar(100.0, &cfg), "██████████");
    }

    #[test]
    fn test_render_bar_clamps_and_floors() {
        let cfg = UsageLimitsConfig::default();
        // Over 100 must not overflow the width.
        assert_eq!(render_bar(150.0, &cfg), "██████████");
        // 99.5% floors to 9 filled (visual approximation), not 10.
        assert_eq!(render_bar(99.5, &cfg), "█████████░");
    }

    #[test]
    fn test_render_bar_custom_width_and_chars() {
        let cfg = UsageLimitsConfig {
            bar_width: Some(4),
            bar_filled_char: Some("#".to_string()),
            bar_empty_char: Some("-".to_string()),
            ..Default::default()
        };
        assert_eq!(render_bar(50.0, &cfg), "##--");
    }

    #[test]
    fn test_format_period_part_substitutes_bar() {
        let cfg = UsageLimitsConfig {
            bar_width: Some(10),
            ..Default::default()
        };
        let out = format_period_part("5h {bar} {pct}%", 40.0, None, FIVE_HOUR_SECS, 0, &cfg);
        assert_eq!(out, "5h ████░░░░░░ 40%");
    }

    /// Builds a Context whose stdin rate_limits carry the given 5h/7d percentages.
    fn ctx_with_pcts(five: f64, seven: f64) -> (tempfile::TempDir, Context) {
        use crate::context::{RateLimitPeriod, RateLimits};
        let dir = tempfile::tempdir().unwrap();
        let transcript = dir.path().join("test.jsonl");
        let ctx = Context {
            transcript_path: Some(transcript.to_str().unwrap().to_string()),
            rate_limits: Some(RateLimits {
                five_hour: Some(RateLimitPeriod {
                    used_percentage: Some(five),
                    resets_at: Some(9_999_999_999),
                }),
                seven_day: Some(RateLimitPeriod {
                    used_percentage: Some(seven),
                    resets_at: Some(9_999_999_999),
                }),
            }),
            ..Default::default()
        };
        (dir, ctx)
    }

    /// A per-period token is colored by its OWN percentage, not `max(5h, 7d)`:
    /// with 5h high and 7d low, the 5h token escalates while the 7d token stays base.
    #[test]
    fn test_per_period_tokens_color_independently() {
        let (_dir, ctx) = ctx_with_pcts(85.0, 20.0);
        let cfg = CshipConfig {
            usage_limits: Some(UsageLimitsConfig {
                // base style unset → below-threshold periods render without ANSI
                warn_threshold: Some(50.0),
                warn_style: Some("bold".to_string()),
                ..Default::default()
            }),
            ..Default::default()
        };
        let five = render_five_hour(&ctx, &cfg).unwrap();
        let seven = render_seven_day(&ctx, &cfg).unwrap();
        assert!(
            five.contains('\x1b'),
            "5h at 85% should be styled (warn): {five:?}"
        );
        assert!(
            !seven.contains('\x1b'),
            "7d at 20% should be unstyled despite high 5h: {seven:?}"
        );
    }

    /// `session`/`weekly` are aliases of `five_hour`/`seven_day`.
    #[test]
    fn test_period_token_aliases_match() {
        let (_dir, ctx) = ctx_with_pcts(42.0, 38.0);
        let cfg = CshipConfig {
            usage_limits: Some(UsageLimitsConfig::default()),
            ..Default::default()
        };
        let five = crate::modules::render_module("cship.usage_limits.five_hour", &ctx, &cfg);
        let session = crate::modules::render_module("cship.usage_limits.session", &ctx, &cfg);
        let seven = crate::modules::render_module("cship.usage_limits.seven_day", &ctx, &cfg);
        let weekly = crate::modules::render_module("cship.usage_limits.weekly", &ctx, &cfg);
        assert_eq!(five, session);
        assert_eq!(seven, weekly);
        assert!(five.is_some() && seven.is_some());
    }

    /// `{bar}` is substituted inside the combined `$cship.usage_limits` output too,
    /// not only the standalone per-window tokens.
    #[test]
    fn test_bar_substituted_in_combined_render() {
        let (_dir, ctx) = ctx_with_pcts(50.0, 0.0);
        let cfg = CshipConfig {
            usage_limits: Some(UsageLimitsConfig {
                five_hour_format: Some("5h {bar}".to_string()),
                seven_day_format: Some("7d {bar}".to_string()),
                bar_width: Some(10),
                ..Default::default()
            }),
            ..Default::default()
        };
        let out = render(&ctx, &cfg).unwrap();
        // 5h at 50% → half-filled; 7d at 0% → empty. Pins each window's own pct.
        assert!(
            out.contains("5h █████░░░░░"),
            "5h bar must reflect 50%: {out:?}"
        );
        assert!(
            out.contains("7d ░░░░░░░░░░"),
            "7d bar must reflect 0%: {out:?}"
        );
    }

    /// `{bar}` is a per-window placeholder only: the per-model and extra-usage
    /// formatters leave it literal (docs make this exact negative claim).
    #[test]
    fn test_bar_not_substituted_in_per_model_or_extra() {
        use crate::usage_limits::UsageLimitsData;
        // Per-model formatter.
        let opus = format_single_model("opus", Some(40.0), &None, Some("opus {bar} {pct}%"))
            .expect("opus pct present");
        assert!(
            opus.contains("{bar}") && opus.contains("40%"),
            "opus_format must not expand {{bar}}: {opus:?}"
        );
        // Extra-usage formatter.
        let data = UsageLimitsData {
            extra_usage_enabled: Some(true),
            extra_usage_utilization: Some(25.0),
            extra_usage_used_credits: Some(500.0),
            ..Default::default()
        };
        let cfg = UsageLimitsConfig {
            extra_usage_format: Some("extra {bar} {pct}%".to_string()),
            ..Default::default()
        };
        let extra = format_extra_usage(&data, &cfg).expect("extra usage enabled");
        assert!(
            extra.contains("{bar}"),
            "extra_usage_format must not expand {{bar}}: {extra:?}"
        );
    }

    #[test]
    fn test_render_bar_zero_width_is_empty() {
        let cfg = UsageLimitsConfig {
            bar_width: Some(0),
            ..Default::default()
        };
        assert_eq!(render_bar(50.0, &cfg), "");
    }

    #[test]
    fn test_render_bar_clamps_oversized_width() {
        let cfg = UsageLimitsConfig {
            bar_width: Some(1_000_000),
            ..Default::default()
        };
        // Width is capped at MAX_BAR_WIDTH regardless of the configured value.
        assert_eq!(render_bar(100.0, &cfg).chars().count(), MAX_BAR_WIDTH);
    }

    /// Enterprise-style plans (no standard 5h/7d signal) yield no per-period token.
    #[test]
    fn test_per_period_tokens_none_without_standard_signal() {
        let dir = tempfile::tempdir().unwrap();
        let transcript = dir.path().join("test.jsonl");
        let ctx = Context {
            transcript_path: Some(transcript.to_str().unwrap().to_string()),
            rate_limits: None,
            ..Default::default()
        };
        let cfg = CshipConfig::default();
        assert!(render_five_hour(&ctx, &cfg).is_none());
        assert!(render_seven_day(&ctx, &cfg).is_none());
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
            five_hour_available: true,
            seven_day_available: true,
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

    // --- merge_window: Phase 1 window-merge correctness ---

    fn snap(pct: f64, epoch: Option<u64>) -> UsageWindowSnapshot {
        UsageWindowSnapshot::new(pct, epoch, "")
    }

    #[test]
    fn test_merge_window_stdin_expired_api_live() {
        let now = 1_000_000;
        let stdin = snap(85.0, Some(now - 10)); // expired
        let api = snap(12.0, Some(now + 10)); // live
        let merged = merge_window(Some(stdin), Some(api), now);
        assert_eq!(merged.pct, 12.0);
        assert_eq!(merged.resets_at_epoch, Some(now + 10));
    }

    #[test]
    fn test_merge_window_api_expired_stdin_live() {
        let now = 1_000_000;
        let stdin = snap(30.0, Some(now + 10));
        let api = snap(70.0, Some(now - 10));
        let merged = merge_window(Some(stdin), Some(api), now);
        assert_eq!(merged.pct, 30.0);
        assert_eq!(merged.resets_at_epoch, Some(now + 10));
    }

    #[test]
    fn test_merge_window_both_live_same_reset_api_higher() {
        let now = 1_000_000;
        let reset = now + 500;
        let stdin = snap(23.0, Some(reset));
        let api = snap(60.0, Some(reset));
        let merged = merge_window(Some(stdin), Some(api), now);
        assert_eq!(merged.pct, 60.0);
        assert_eq!(merged.resets_at_epoch, Some(reset));
    }

    #[test]
    fn test_merge_window_both_live_same_reset_stdin_higher() {
        let now = 1_000_000;
        let reset = now + 500;
        let stdin = snap(60.0, Some(reset));
        let api = snap(23.0, Some(reset));
        let merged = merge_window(Some(stdin), Some(api), now);
        assert_eq!(merged.pct, 60.0);
        assert_eq!(merged.resets_at_epoch, Some(reset));
    }

    #[test]
    fn test_merge_window_both_live_different_reset_uses_later_reset_window() {
        let now = 1_000_000;
        let stdin = snap(85.0, Some(now + 100)); // earlier reset, older window
        let api = snap(12.0, Some(now + 10_000)); // later reset, newer window
        let merged = merge_window(Some(stdin), Some(api), now);
        assert_eq!(merged.pct, 12.0);
        assert_eq!(merged.resets_at_epoch, Some(now + 10_000));
    }

    #[test]
    fn test_merge_window_both_expired_returns_zero_with_no_reset() {
        let now = 1_000_000;
        let stdin = snap(85.0, Some(now - 100));
        let api = snap(70.0, Some(now - 50));
        let merged = merge_window(Some(stdin), Some(api), now);
        assert_eq!(merged.pct, 0.0);
        assert_eq!(merged.resets_at_epoch, None);
        assert!(merged.resets_at_iso.is_empty());
    }

    #[test]
    fn test_merge_window_api_unknown_reset_stdin_expired_prefers_api() {
        let now = 1_000_000;
        let stdin = snap(85.0, Some(now - 100)); // expired
        let api = snap(40.0, None); // unknown reset
        let merged = merge_window(Some(stdin), Some(api), now);
        assert_eq!(merged.pct, 40.0);
        assert_eq!(merged.resets_at_epoch, None);
    }

    #[test]
    fn test_merge_window_stdin_unknown_reset_api_live_prefers_api() {
        let now = 1_000_000;
        let stdin = snap(90.0, None); // unknown reset
        let api = snap(15.0, Some(now + 10)); // live
        let merged = merge_window(Some(stdin), Some(api), now);
        assert_eq!(merged.pct, 15.0);
        assert_eq!(merged.resets_at_epoch, Some(now + 10));
    }

    #[test]
    fn test_merge_window_only_stdin_present_unknown_reset() {
        let now = 1_000_000;
        let stdin = snap(50.0, None);
        let merged = merge_window(Some(stdin), None, now);
        assert_eq!(merged.pct, 50.0);
        assert_eq!(merged.resets_at_epoch, None);
    }

    #[test]
    fn test_merge_window_percentage_and_reset_always_paired() {
        // Regression guard for the exact bug in the deleted branch: selecting
        // API's pct must never pair with stdin's reset epoch or vice versa.
        let now = 1_000_000;
        let stdin = UsageWindowSnapshot::new(85.0, Some(now - 10), "2020-01-01T00:00:00+00:00");
        let api = UsageWindowSnapshot::new(12.0, Some(now + 10), "2099-01-01T00:00:00+00:00");
        let merged = merge_window(Some(stdin), Some(api), now);
        assert_eq!(merged.pct, 12.0);
        assert_eq!(merged.resets_at_iso, "2099-01-01T00:00:00+00:00");
    }

    #[test]
    fn test_resolve_data_stdin_only_still_works_after_coordinator_rewire() {
        let ctx = Context {
            rate_limits: Some(crate::context::RateLimits {
                five_hour: Some(crate::context::RateLimitPeriod {
                    used_percentage: Some(10.0),
                    resets_at: Some(now_epoch() + 3600),
                }),
                seven_day: Some(crate::context::RateLimitPeriod {
                    used_percentage: Some(5.0),
                    resets_at: Some(now_epoch() + 3600),
                }),
            }),
            ..Default::default()
        };
        let cfg = CshipConfig::default();
        let out = render(&ctx, &cfg).unwrap();
        assert!(out.contains("10%"));
    }

    // --- Fix 2 regression: single-source (stdin-only) resolve must also route
    // through merge_window, not bypass it and pass a raw expired snapshot
    // straight to the formatter. ---

    #[test]
    fn test_resolve_data_stdin_only_expired_window_never_renders_resets_in_now() {
        // full_data is always None in test builds (resolve_full_data stub),
        // so this exercises exactly the (Some(stdin), None) arm.
        let now = now_epoch();
        let ctx = Context {
            rate_limits: Some(crate::context::RateLimits {
                five_hour: Some(crate::context::RateLimitPeriod {
                    used_percentage: Some(85.0),
                    resets_at: Some(now.saturating_sub(100)), // expired
                }),
                seven_day: Some(crate::context::RateLimitPeriod {
                    used_percentage: Some(70.0),
                    resets_at: Some(now.saturating_sub(50)), // expired
                }),
            }),
            ..Default::default()
        };
        let data = resolve_data(&ctx, &CshipConfig::default())
            .expect("stdin-only data should still resolve to Some");
        assert_eq!(
            data.five_hour_pct, 0.0,
            "expired window must merge down to 0%, not raw 85%"
        );
        assert_eq!(data.seven_day_pct, 0.0);
        assert_eq!(data.five_hour_resets_at_epoch, None);
        assert_eq!(data.seven_day_resets_at_epoch, None);

        // format_output must never say "resets in now" for an expired window.
        let out = format_output(&data, &UsageLimitsConfig::default());
        assert!(
            !out.contains("resets in now"),
            "expired single-source window must not render 'resets in now': {out:?}"
        );
    }

    #[test]
    fn test_merge_window_one_side_expired_other_side_absent_is_empty() {
        // Underlying invariant both single-source arms now depend on:
        // one side present-and-expired, other side entirely absent → empty
        // result (0 pct, no reset), never the raw expired snapshot.
        let now = 1_000_000;
        let stdin = snap(85.0, Some(now - 100)); // expired
        let merged = merge_window(Some(stdin), None, now);
        assert_eq!(merged.pct, 0.0);
        assert_eq!(merged.resets_at_epoch, None);
        assert!(merged.resets_at_iso.is_empty());
    }

    #[test]
    fn test_format_output_zero_pct_never_renders_resets_in_now() {
        let data = UsageLimitsData {
            five_hour_pct: 0.0,
            seven_day_pct: 0.0,
            five_hour_resets_at: String::new(),
            seven_day_resets_at: String::new(),
            five_hour_resets_at_epoch: None,
            seven_day_resets_at_epoch: None,
            ..Default::default()
        };
        let cfg = UsageLimitsConfig::default();
        let out = format_output(&data, &cfg);
        assert!(!out.contains("resets in now"), "got: {out}");
        assert!(out.contains("0%"), "got: {out}");
    }

    // ── stdin + API data assembly (resolve_data_uncached with injected API data) ──

    /// Makes the test-build `resolve_full_data` return `data` until dropped.
    struct FullDataGuard;
    impl FullDataGuard {
        fn set(data: UsageLimitsData) -> Self {
            TEST_FULL_DATA.with(|d| *d.borrow_mut() = Some(data));
            FullDataGuard
        }
    }
    impl Drop for FullDataGuard {
        fn drop(&mut self) {
            TEST_FULL_DATA.with(|d| *d.borrow_mut() = None);
        }
    }

    fn ctx_with_transcript() -> (tempfile::TempDir, Context) {
        let dir = tempfile::tempdir().unwrap();
        let transcript = dir.path().join("test.jsonl");
        let ctx = Context {
            transcript_path: Some(transcript.to_str().unwrap().to_string()),
            ..Default::default()
        };
        (dir, ctx)
    }

    fn api_window_data() -> UsageLimitsData {
        UsageLimitsData {
            five_hour_pct: 30.0,
            seven_day_pct: 60.0,
            five_hour_resets_at_epoch: Some(9_999_999_999),
            seven_day_resets_at_epoch: Some(9_999_999_999),
            five_hour_available: true,
            seven_day_available: true,
            ..Default::default()
        }
    }

    #[test]
    fn test_render_enterprise_extra_usage_only() {
        let _full = FullDataGuard::set(UsageLimitsData {
            extra_usage_enabled: Some(true),
            extra_usage_monthly_limit: Some(30000.0),
            extra_usage_used_credits: Some(23076.0),
            extra_usage_utilization: Some(76.92),
            ..Default::default()
        });
        let (_dir, ctx) = ctx_with_transcript();
        let out = render(&ctx, &CshipConfig::default())
            .expect("Enterprise: extra_usage_only must render");
        assert!(!out.contains("5h:"), "must not include 5h section: {out}");
        assert!(!out.contains("7d:"), "must not include 7d section: {out}");
        assert!(out.contains("230.76"), "must include used dollars: {out}");
        assert!(out.contains("300.00"), "must include limit dollars: {out}");
    }

    #[test]
    fn test_render_enterprise_extra_disabled_returns_none() {
        let _full = FullDataGuard::set(UsageLimitsData {
            extra_usage_enabled: Some(false),
            ..Default::default()
        });
        let (_dir, ctx) = ctx_with_transcript();
        assert!(render(&ctx, &CshipConfig::default()).is_none());
    }

    /// No stdin `rate_limits`: the standard windows come from the API alone.
    #[test]
    fn test_render_standard_windows_from_api_only() {
        let _full = FullDataGuard::set(api_window_data());
        let (_dir, ctx) = ctx_with_transcript();
        let out = render(&ctx, &CshipConfig::default()).expect("API-only windows must render");
        assert!(out.contains("5h: 30%"), "5h from API: {out}");
        assert!(out.contains("7d: 60%"), "7d from API: {out}");
    }

    /// Stdin sends `rate_limits` with neither window (e.g. at session start)
    /// while the API has them: availability is OR-ed across sources, so the
    /// plan is not mistaken for one without standard windows.
    #[test]
    fn test_availability_flags_are_combined_across_sources() {
        use crate::context::RateLimits;
        for (five_api, seven_api) in [(true, false), (false, true)] {
            let _full = FullDataGuard::set(UsageLimitsData {
                five_hour_pct: 30.0,
                seven_day_pct: 60.0,
                five_hour_resets_at_epoch: Some(9_999_999_999),
                seven_day_resets_at_epoch: Some(9_999_999_999),
                five_hour_available: five_api,
                seven_day_available: seven_api,
                ..Default::default()
            });
            let (_dir, mut ctx) = ctx_with_transcript();
            ctx.rate_limits = Some(RateLimits {
                five_hour: None,
                seven_day: None,
            });
            let out = render(&ctx, &CshipConfig::default()).unwrap_or_else(|| {
                panic!("API window (5h={five_api}, 7d={seven_api}) must keep the module visible")
            });
            assert!(out.contains("5h:") && out.contains("7d:"), "{out}");
        }
    }

    /// Non-window fields (extra usage) come from the API even when stdin
    /// supplies the standard windows.
    #[test]
    fn test_extra_usage_comes_from_api_when_windows_come_from_stdin() {
        let (_dir, ctx) = ctx_with_pcts(20.0, 10.0);
        let _full = FullDataGuard::set(UsageLimitsData {
            extra_usage_enabled: Some(true),
            extra_usage_monthly_limit: Some(10000.0),
            extra_usage_used_credits: Some(2500.0),
            extra_usage_utilization: Some(25.0),
            ..Default::default()
        });
        let out = render(&ctx, &CshipConfig::default()).unwrap();
        assert!(out.contains("5h: 20%"), "stdin window: {out}");
        assert!(
            out.contains("25.00") || out.contains("25%"),
            "API extra usage: {out}"
        );
    }

    /// No API data and no stdin: nothing to show.
    #[test]
    fn test_render_none_without_any_source() {
        let (_dir, ctx) = ctx_with_transcript();
        assert!(render(&ctx, &CshipConfig::default()).is_none());
    }
}
