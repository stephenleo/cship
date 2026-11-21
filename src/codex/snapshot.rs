//! Codex statusline snapshot and its mapping onto cship's [`Context`].
//!
//! Codex sends this snapshot with every `resources/read` in
//! `params._meta["com.openai/codex-statusline-state"]`. The shape follows the
//! `statusline.d.ts` reference shipped with Codex's `create-statusline` skill.
//!
//! Every field is optional and numbers are read as `f64`: the feature is
//! experimental, and a single unexpected field must not break the whole footer.

use serde::Deserialize;

use crate::context::{
    Context, ContextWindow, Cost, Effort, Host, Model, RateLimitPeriod, RateLimits, Workspace,
};

/// Window lengths (minutes) that correspond to cship's two usage periods.
const FIVE_HOUR_MINUTES: f64 = 300.0;
const SEVEN_DAY_MINUTES: f64 = 10_080.0;

#[derive(Debug, Deserialize, Default)]
pub struct StatuslineState {
    pub account: Option<Account>,
    pub app: Option<App>,
    pub thread: Option<Thread>,
}

#[derive(Debug, Deserialize, Default)]
pub struct Account {
    pub rate_limits: Option<Vec<RateLimitSnapshot>>,
}

#[derive(Debug, Deserialize, Default)]
pub struct RateLimitSnapshot {
    pub limit_id: Option<String>,
    pub primary_limit: Option<RateLimitWindow>,
    pub secondary_limit: Option<RateLimitWindow>,
}

#[derive(Debug, Deserialize, Default)]
pub struct RateLimitWindow {
    pub used_percent: Option<f64>,
    pub window_minutes: Option<f64>,
    pub resets_at: Option<f64>,
}

#[derive(Debug, Deserialize, Default)]
pub struct App {
    pub cli_version: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
pub struct Thread {
    pub id: Option<String>,
    pub model: Option<ThreadModel>,
    pub workspace: Option<ThreadWorkspace>,
    pub usage: Option<Usage>,
}

#[derive(Debug, Deserialize, Default)]
pub struct ThreadModel {
    pub display_name: Option<String>,
    pub id: Option<String>,
    pub reasoning_level: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
pub struct ThreadWorkspace {
    pub cwd: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
pub struct Usage {
    pub context: Option<UsageContext>,
    pub total: Option<TokenUsage>,
    pub cost: Option<UsageCost>,
}

#[derive(Debug, Deserialize, Default)]
pub struct UsageContext {
    pub context_window_size: Option<f64>,
    pub used_tokens: Option<f64>,
    pub used_percentage: Option<f64>,
}

#[derive(Debug, Deserialize, Default)]
pub struct TokenUsage {
    pub total_input_tokens: Option<f64>,
    pub output_tokens: Option<f64>,
}

#[derive(Debug, Deserialize, Default)]
pub struct UsageCost {
    pub total_cost_usd: Option<f64>,
}

/// Convert a non-negative JSON number to `u64`, dropping negatives and NaN.
fn to_u64(v: Option<f64>) -> Option<u64> {
    v.filter(|n| n.is_finite() && *n >= 0.0).map(|n| n as u64)
}

impl StatuslineState {
    /// Build the cship [`Context`] that the regular renderer consumes.
    pub fn into_context(self) -> Context {
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
        current_usage: None,
    })
}

/// Pick the 5-hour and 7-day windows out of Codex's rate-limit snapshots.
///
/// Windows are matched by `window_minutes`. If no window reports a length, the
/// first snapshot's primary/secondary windows are treated as 5h/7d, which is how
/// Codex lays them out today. A window of some other known length is never
/// relabelled as 5h or 7d.
fn map_rate_limits(snapshots: &[RateLimitSnapshot]) -> Option<RateLimits> {
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
    fn test_control_characters_are_sanitized() {
        let json = r#"{"thread":{"model":{"display_name":"evil\u001b]52;c;x\u0007","provider":"openai"}}}"#;
        let ctx = parse(json);
        assert_eq!(
            ctx.model.unwrap().display_name.as_deref(),
            Some("evil]52;c;x")
        );
    }
}
