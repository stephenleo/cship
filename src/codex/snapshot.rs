//! Codex statusline snapshot and its mapping onto cship's [`Context`].
//!
//! Codex sends this snapshot with every `resources/read` in
//! `params._meta["com.openai/codex-statusline-state"]`. The shape follows the
//! `statusline.d.ts` reference shipped with Codex's `create-statusline` skill.
//!
//! The feature is experimental, so parsing is lenient field by field: every
//! field is optional, numbers are read as `f64`, and a field with an unexpected
//! type is dropped (with a warning) instead of failing the whole snapshot. One
//! odd field therefore blanks only the modules that read it.

use serde::Deserialize;
use serde::de::{DeserializeOwned, Deserializer};
use serde_json::Value;

use crate::context::{
    Context, ContextWindow, Cost, CurrentUsage, Effort, Host, Model, RateLimitPeriod, RateLimits,
    Workspace,
};

/// Window lengths (minutes) that correspond to cship's two usage periods.
const FIVE_HOUR_MINUTES: f64 = 300.0;
const SEVEN_DAY_MINUTES: f64 = 10_080.0;
/// The rate-limit bucket for Codex usage itself, preferred over any others.
const CODEX_LIMIT_ID: &str = "codex";

/// Deserialize a field, mapping a value of the wrong type to `None`.
fn lenient<'de, D, T>(d: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: DeserializeOwned,
{
    let value = Value::deserialize(d)?;
    if value.is_null() {
        return Ok(None);
    }
    match T::deserialize(value) {
        Ok(v) => Ok(Some(v)),
        Err(e) => {
            tracing::warn!("cship codex: ignoring snapshot field with unexpected type: {e}");
            Ok(None)
        }
    }
}

/// Deserialize a list, dropping elements of the wrong shape individually.
fn lenient_vec<'de, D, T>(d: D) -> Result<Option<Vec<T>>, D::Error>
where
    D: Deserializer<'de>,
    T: DeserializeOwned,
{
    let Some(items) = lenient::<D, Vec<Value>>(d)? else {
        return Ok(None);
    };
    Ok(Some(
        items
            .into_iter()
            .filter_map(|item| match T::deserialize(item) {
                Ok(v) => Some(v),
                Err(e) => {
                    tracing::warn!("cship codex: ignoring malformed snapshot list entry: {e}");
                    None
                }
            })
            .collect(),
    ))
}

#[derive(Debug, Deserialize, Default)]
pub(crate) struct StatuslineState {
    #[serde(default, deserialize_with = "lenient")]
    account: Option<Account>,
    #[serde(default, deserialize_with = "lenient")]
    app: Option<App>,
    #[serde(default, deserialize_with = "lenient")]
    thread: Option<Thread>,
}

#[derive(Debug, Deserialize, Default)]
struct Account {
    #[serde(default, deserialize_with = "lenient_vec")]
    rate_limits: Option<Vec<RateLimitSnapshot>>,
}

#[derive(Debug, Deserialize, Default)]
struct RateLimitSnapshot {
    #[serde(default, deserialize_with = "lenient")]
    limit_id: Option<String>,
    #[serde(default, deserialize_with = "lenient")]
    primary_limit: Option<RateLimitWindow>,
    #[serde(default, deserialize_with = "lenient")]
    secondary_limit: Option<RateLimitWindow>,
}

#[derive(Debug, Deserialize, Default)]
struct RateLimitWindow {
    #[serde(default, deserialize_with = "lenient")]
    used_percent: Option<f64>,
    #[serde(default, deserialize_with = "lenient")]
    window_minutes: Option<f64>,
    #[serde(default, deserialize_with = "lenient")]
    resets_at: Option<f64>,
}

#[derive(Debug, Deserialize, Default)]
struct App {
    #[serde(default, deserialize_with = "lenient")]
    cli_version: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct Thread {
    #[serde(default, deserialize_with = "lenient")]
    id: Option<String>,
    #[serde(default, deserialize_with = "lenient")]
    model: Option<ThreadModel>,
    #[serde(default, deserialize_with = "lenient")]
    workspace: Option<ThreadWorkspace>,
    #[serde(default, deserialize_with = "lenient")]
    usage: Option<Usage>,
}

#[derive(Debug, Deserialize, Default)]
struct ThreadModel {
    #[serde(default, deserialize_with = "lenient")]
    display_name: Option<String>,
    #[serde(default, deserialize_with = "lenient")]
    id: Option<String>,
    #[serde(default, deserialize_with = "lenient")]
    reasoning_level: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct ThreadWorkspace {
    #[serde(default, deserialize_with = "lenient")]
    cwd: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct Usage {
    #[serde(default, deserialize_with = "lenient")]
    context: Option<UsageContext>,
    #[serde(default, deserialize_with = "lenient")]
    total: Option<TokenUsage>,
    #[serde(default, deserialize_with = "lenient")]
    cost: Option<UsageCost>,
}

#[derive(Debug, Deserialize, Default)]
struct UsageContext {
    #[serde(default, deserialize_with = "lenient")]
    context_window_size: Option<f64>,
    #[serde(default, deserialize_with = "lenient")]
    used_tokens: Option<f64>,
    #[serde(default, deserialize_with = "lenient")]
    used_percentage: Option<f64>,
}

#[derive(Debug, Deserialize, Default)]
struct TokenUsage {
    #[serde(default, deserialize_with = "lenient")]
    total_input_tokens: Option<f64>,
    #[serde(default, deserialize_with = "lenient")]
    output_tokens: Option<f64>,
}

#[derive(Debug, Deserialize, Default)]
struct UsageCost {
    #[serde(default, deserialize_with = "lenient")]
    total_cost_usd: Option<f64>,
}

/// Convert a non-negative JSON number to `u64`, dropping negatives and NaN.
fn to_u64(v: Option<f64>) -> Option<u64> {
    v.filter(|n| n.is_finite() && *n >= 0.0).map(|n| n as u64)
}

impl StatuslineState {
    /// Parse a raw snapshot. Only a snapshot that is not a JSON object at all
    /// falls back to an empty state; malformed fields are dropped one by one.
    pub(crate) fn from_value(raw: &Value) -> Self {
        match Self::deserialize(raw) {
            Ok(state) => state,
            Err(e) => {
                tracing::warn!("cship codex: could not parse statusline snapshot: {e}");
                Self::default()
            }
        }
    }

    /// Build the cship [`Context`] that the regular renderer consumes.
    pub(crate) fn into_context(self) -> Context {
        let thread = self.thread.unwrap_or_default();
        let usage = thread.usage.unwrap_or_default();

        let cwd = thread.workspace.and_then(|w| w.cwd);
        let transcript_path = thread
            .id
            .as_deref()
            .and_then(super::cache_key_path)
            .map(|p| p.to_string_lossy().into_owned());

        let (model, effort) = match thread.model {
            Some(m) => (
                Some(Model {
                    id: m.id,
                    display_name: m.display_name,
                }),
                m.reasoning_level.map(|level| Effort { level: Some(level) }),
            ),
            None => (None, None),
        };

        let mut ctx = Context {
            cwd: cwd.clone(),
            session_id: thread.id,
            transcript_path,
            version: self.app.and_then(|a| a.cli_version),
            model,
            workspace: cwd.map(|dir| Workspace {
                current_dir: Some(dir),
                project_dir: None,
            }),
            cost: usage.cost.map(|c| Cost {
                total_cost_usd: c.total_cost_usd,
                ..Default::default()
            }),
            context_window: map_context_window(usage.context, usage.total),
            effort,
            rate_limits: self
                .account
                .and_then(|a| a.rate_limits)
                .and_then(|snapshots| map_rate_limits(&snapshots)),
            host: Host::Codex,
            ..Default::default()
        };
        ctx.sanitize();
        ctx
    }
}

fn map_context_window(
    context: Option<UsageContext>,
    total: Option<TokenUsage>,
) -> Option<ContextWindow> {
    if context.is_none() && total.is_none() {
        return None;
    }
    let context = context.unwrap_or_default();
    let total = total.unwrap_or_default();

    let size = to_u64(context.context_window_size);
    let used_percentage = context.used_percentage.or_else(|| {
        match (context.used_tokens, context.context_window_size) {
            (Some(used), Some(size)) if size > 0.0 => Some(used / size * 100.0),
            _ => None,
        }
    });

    Some(ContextWindow {
        total_input_tokens: to_u64(total.total_input_tokens),
        total_output_tokens: to_u64(total.output_tokens),
        context_window_size: size,
        used_percentage,
        remaining_percentage: used_percentage.map(|u| (100.0 - u).max(0.0)),
        // Codex reports one total for the tokens in context, without Claude's
        // input/cache split. Carrying it as input tokens makes
        // `$cship.context_window.used_tokens` (which sums the split) correct.
        current_usage: to_u64(context.used_tokens).map(|used| CurrentUsage {
            input_tokens: Some(used),
            ..Default::default()
        }),
    })
}

/// Pick the 5-hour and 7-day windows out of Codex's rate-limit snapshots.
///
/// Only the `codex` bucket is considered when present, so windows from other
/// buckets never mix into the result. Windows are matched by `window_minutes`.
/// If no window reports a length, the first snapshot's primary/secondary windows
/// are treated as 5h/7d, which is how Codex lays them out today. A window of some
/// other known length is never relabelled as 5h or 7d.
fn map_rate_limits(snapshots: &[RateLimitSnapshot]) -> Option<RateLimits> {
    let codex: Vec<&RateLimitSnapshot> = snapshots
        .iter()
        .filter(|s| s.limit_id.as_deref() == Some(CODEX_LIMIT_ID))
        .collect();
    let snapshots: Vec<&RateLimitSnapshot> = if codex.is_empty() {
        snapshots.iter().collect()
    } else {
        codex
    };

    let windows: Vec<&RateLimitWindow> = snapshots
        .iter()
        .flat_map(|s| [s.primary_limit.as_ref(), s.secondary_limit.as_ref()])
        .flatten()
        .collect();

    let by_length = |minutes: f64| {
        windows
            .iter()
            .find(|w| w.window_minutes == Some(minutes))
            .copied()
    };

    let (five, seven) = if windows.iter().any(|w| w.window_minutes.is_some()) {
        (by_length(FIVE_HOUR_MINUTES), by_length(SEVEN_DAY_MINUTES))
    } else {
        let first = snapshots.first();
        (
            first.and_then(|s| s.primary_limit.as_ref()),
            first.and_then(|s| s.secondary_limit.as_ref()),
        )
    };

    if five.is_none() && seven.is_none() {
        tracing::debug!("cship codex: no 5h/7d rate-limit window in snapshot");
        return None;
    }

    let period = |w: &RateLimitWindow| RateLimitPeriod {
        used_percentage: w.used_percent,
        resets_at: to_u64(w.resets_at),
    };
    Some(RateLimits {
        five_hour: five.map(period),
        seven_day: seven.map(period),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const FULL: &str = include_str!("../../tests/fixtures/codex_snapshot_full.json");

    fn parse(json: &str) -> Context {
        serde_json::from_str::<StatuslineState>(json)
            .unwrap()
            .into_context()
    }

    #[test]
    fn test_full_snapshot_maps_core_fields() {
        let ctx = parse(FULL);
        assert_eq!(ctx.host, Host::Codex);
        let model = ctx.model.as_ref().unwrap();
        assert_eq!(model.display_name.as_deref(), Some("gpt-5.5-codex"));
        assert_eq!(model.id.as_deref(), Some("gpt-5.5-codex"));
        assert_eq!(ctx.effort.as_ref().unwrap().level.as_deref(), Some("high"));
        assert_eq!(ctx.cwd.as_deref(), Some("/home/user/projects/myapp"));
        assert_eq!(
            ctx.workspace.as_ref().unwrap().current_dir.as_deref(),
            Some("/home/user/projects/myapp")
        );
        assert_eq!(ctx.session_id.as_deref(), Some("thread-123"));
        assert_eq!(ctx.version.as_deref(), Some("0.171.0"));
        assert_eq!(ctx.cost.as_ref().unwrap().total_cost_usd, Some(1.25));
    }

    #[test]
    fn test_full_snapshot_maps_context_window() {
        let cw = parse(FULL).context_window.unwrap();
        assert_eq!(cw.context_window_size, Some(272_000));
        assert_eq!(cw.used_percentage, Some(25.0));
        assert_eq!(cw.remaining_percentage, Some(75.0));
        assert_eq!(cw.total_input_tokens, Some(120_000));
        assert_eq!(cw.total_output_tokens, Some(8_000));
    }

    #[test]
    fn test_full_snapshot_maps_rate_limits_by_window_length() {
        let rl = parse(FULL).rate_limits.unwrap();
        let five = rl.five_hour.unwrap();
        let seven = rl.seven_day.unwrap();
        assert_eq!(five.used_percentage, Some(42.0));
        assert_eq!(five.resets_at, Some(1_900_000_000));
        assert_eq!(seven.used_percentage, Some(13.5));
        assert_eq!(seven.resets_at, Some(1_900_500_000));
    }

    #[test]
    fn test_rate_limits_out_of_order_windows_matched_by_length() {
        let json = r#"{"account":{"rate_limits":[{"limit_id":"codex","limit_name":null,
            "primary_limit":{"used_percent":7,"window_minutes":10080,"resets_at":null},
            "secondary_limit":{"used_percent":3,"window_minutes":300,"resets_at":null}}]}}"#;
        let rl = parse(json).rate_limits.unwrap();
        assert_eq!(rl.five_hour.unwrap().used_percentage, Some(3.0));
        assert_eq!(rl.seven_day.unwrap().used_percentage, Some(7.0));
    }

    #[test]
    fn test_rate_limits_without_lengths_fall_back_to_position() {
        let json = r#"{"account":{"rate_limits":[{"limit_id":"codex","limit_name":null,
            "primary_limit":{"used_percent":10,"window_minutes":null,"resets_at":null},
            "secondary_limit":{"used_percent":20,"window_minutes":null,"resets_at":null}}]}}"#;
        let rl = parse(json).rate_limits.unwrap();
        assert_eq!(rl.five_hour.unwrap().used_percentage, Some(10.0));
        assert_eq!(rl.seven_day.unwrap().used_percentage, Some(20.0));
    }

    #[test]
    fn test_rate_limits_other_lengths_are_not_relabelled() {
        let json = r#"{"account":{"rate_limits":[{"limit_id":"x","limit_name":null,
            "primary_limit":{"used_percent":50,"window_minutes":60,"resets_at":null},
            "secondary_limit":null}]}}"#;
        assert!(parse(json).rate_limits.is_none());
    }

    #[test]
    fn test_used_percentage_derived_from_tokens() {
        let json = r#"{"thread":{"usage":{"context":{"context_window_size":200000,"used_tokens":50000}}}}"#;
        let cw = parse(json).context_window.unwrap();
        assert_eq!(cw.used_percentage, Some(25.0));
    }

    #[test]
    fn test_empty_snapshot_yields_empty_codex_context() {
        let ctx = parse("{}");
        assert_eq!(ctx.host, Host::Codex);
        assert!(ctx.model.is_none());
        assert!(ctx.context_window.is_none());
        assert!(ctx.rate_limits.is_none());
        assert!(ctx.transcript_path.is_none());
    }

    #[test]
    fn test_wrong_typed_field_drops_only_that_field() {
        // A numeric thread id and a string window length must not blank the
        // rest of the snapshot.
        let json = r#"{"thread":{"id":42,"model":{"display_name":"gpt-5.5","provider":"openai"},
            "workspace":{"cwd":"/w"}},
            "account":{"rate_limits":[{"limit_id":"codex","limit_name":null,
            "primary_limit":{"used_percent":10,"window_minutes":"300","resets_at":null},
            "secondary_limit":{"used_percent":20,"window_minutes":10080,"resets_at":null}}]}}"#;
        let ctx = parse(json);
        assert!(ctx.session_id.is_none(), "bad id dropped");
        assert_eq!(ctx.model.unwrap().display_name.as_deref(), Some("gpt-5.5"));
        assert_eq!(
            ctx.cwd.as_deref(),
            Some("/w"),
            "config discovery keeps the workspace"
        );
        let rl = ctx.rate_limits.unwrap();
        assert!(
            rl.five_hour.is_none(),
            "window with bad length is not matched"
        );
        assert_eq!(rl.seven_day.unwrap().used_percentage, Some(20.0));
    }

    #[test]
    fn test_malformed_rate_limit_entry_is_skipped() {
        let json = r#"{"account":{"rate_limits":["oops",{"limit_id":"codex","limit_name":null,
            "primary_limit":{"used_percent":5,"window_minutes":300,"resets_at":null},
            "secondary_limit":null}]}}"#;
        let rl = parse(json).rate_limits.unwrap();
        assert_eq!(rl.five_hour.unwrap().used_percentage, Some(5.0));
    }

    #[test]
    fn test_non_object_snapshot_falls_back_to_empty_state() {
        let ctx = StatuslineState::from_value(&serde_json::json!("garbage")).into_context();
        assert!(ctx.model.is_none());
        assert_eq!(ctx.host, Host::Codex);
    }

    #[test]
    fn test_rate_limits_prefer_codex_bucket() {
        let json = r#"{"account":{"rate_limits":[
            {"limit_id":"other","limit_name":null,
             "primary_limit":{"used_percent":99,"window_minutes":300,"resets_at":null},
             "secondary_limit":null},
            {"limit_id":"codex","limit_name":null,
             "primary_limit":{"used_percent":5,"window_minutes":300,"resets_at":null},
             "secondary_limit":{"used_percent":6,"window_minutes":10080,"resets_at":null}}]}}"#;
        let rl = parse(json).rate_limits.unwrap();
        assert_eq!(rl.five_hour.unwrap().used_percentage, Some(5.0));
        assert_eq!(rl.seven_day.unwrap().used_percentage, Some(6.0));
    }

    #[test]
    fn test_used_tokens_maps_to_current_usage() {
        let cu = parse(FULL).context_window.unwrap().current_usage.unwrap();
        assert_eq!(cu.input_tokens, Some(68_000));
        assert!(cu.cache_read_input_tokens.is_none());
    }

    #[test]
    fn test_used_tokens_module_renders_under_codex() {
        let ctx = parse(FULL);
        let out = crate::modules::render_module(
            "cship.context_window.used_tokens",
            &ctx,
            &crate::config::CshipConfig::default(),
        )
        .unwrap();
        assert_eq!(crate::ansi::strip_ansi(&out), "25%(68k/272k)");
    }

    #[test]
    fn test_control_characters_are_sanitized() {
        let json = r#"{"thread":{"model":{"display_name":"evil\u001b]52;c;x\u0007","provider":"openai"}}}"#;
        let ctx = parse(json);
        assert_eq!(
            ctx.model.unwrap().display_name.as_deref(),
            Some("evil]52;c;x")
        );
    }
}
