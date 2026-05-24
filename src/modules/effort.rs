//! Render the `[cship.effort]` module.
//!
//! `$cship.effort` — convenience alias for `$cship.effort.level`.
//! `$cship.effort.level` — raw reasoning-effort string (`low`, `medium`, `high`,
//! `xhigh`, or `max`), reflecting mid-session `/effort` changes.
//!
//! The effort field is absent whenever the current model does not support the
//! reasoning-effort parameter — a routine state, not an error. The module
//! therefore returns `None` and logs at `tracing::debug!` (mirroring
//! `context_bar`'s treatment of normally-absent data) rather than warning on
//! every render for users on models without effort support.
use crate::config::{CshipConfig, EffortConfig};
use crate::context::Context;

/// Resolve the effective style for the current effort level, preferring the
/// matching per-level style over the base `style`. Matching is case-insensitive
/// so that an unexpected-cased level from a future Claude Code version still maps.
fn resolve_effort_style<'a>(level: &str, cfg: Option<&'a EffortConfig>) -> Option<&'a str> {
    let cfg = cfg?;
    let per_level = match level.to_lowercase().as_str() {
        "low" => cfg.low_style.as_deref(),
        "medium" => cfg.medium_style.as_deref(),
        "high" => cfg.high_style.as_deref(),
        "xhigh" => cfg.xhigh_style.as_deref(),
        "max" => cfg.max_style.as_deref(),
        _ => None,
    };
    per_level.or(cfg.style.as_deref())
}

/// Renders `$cship.effort` — convenience alias for the effort level.
pub fn render(ctx: &Context, cfg: &CshipConfig) -> Option<String> {
    render_level(ctx, cfg)
}

/// Renders `$cship.effort.level` — raw effort string with optional symbol, style,
/// and per-level style escalation.
pub fn render_level(ctx: &Context, cfg: &CshipConfig) -> Option<String> {
    let effort_cfg = cfg.effort.as_ref();

    // Disabled check — SILENT (no warn, no log)
    if effort_cfg.and_then(|e| e.disabled).unwrap_or(false) {
        return None;
    }

    // Extract value — log before returning None (do NOT use `?` here). Absence is
    // the normal state when the active model lacks effort support, so this logs at
    // debug rather than warn (cf. context_bar's normally-absent handling).
    let level = match ctx.effort.as_ref().and_then(|e| e.level.as_deref()) {
        Some(l) => l,
        None => {
            tracing::debug!("cship.effort: effort.level absent from context");
            return None;
        }
    };

    let symbol = effort_cfg.and_then(|e| e.symbol.as_deref());
    let style = resolve_effort_style(level, effort_cfg);

    // Format string takes priority if configured
    if let Some(fmt) = effort_cfg.and_then(|e| e.format.as_deref()) {
        return crate::format::apply_module_format(fmt, Some(level), symbol, style);
    }

    // Default behavior — `{symbol}{level}` with the resolved style applied
    let symbol_str = symbol.unwrap_or("");
    let content = format!("{symbol_str}{level}");
    Some(crate::ansi::apply_style(&content, style))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ansi;
    use crate::config::{CshipConfig, EffortConfig};
    use crate::context::{Context, Effort};

    fn ctx_with_effort(level: &str) -> Context {
        Context {
            effort: Some(Effort {
                level: Some(level.to_string()),
            }),
            ..Default::default()
        }
    }

    fn per_level_cfg() -> CshipConfig {
        CshipConfig {
            effort: Some(EffortConfig {
                style: Some("dim".to_string()),
                low_style: Some("green".to_string()),
                medium_style: Some("cyan".to_string()),
                high_style: Some("yellow".to_string()),
                xhigh_style: Some("bold yellow".to_string()),
                max_style: Some("bold red".to_string()),
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    #[test]
    fn test_effort_renders_level_string() {
        let ctx = ctx_with_effort("high");
        let result = render(&ctx, &CshipConfig::default());
        assert_eq!(result, Some("high".to_string()));
    }

    #[test]
    fn test_effort_alias_identical_to_level() {
        let ctx = ctx_with_effort("xhigh");
        let r1 = render(&ctx, &CshipConfig::default());
        let r2 = render_level(&ctx, &CshipConfig::default());
        assert_eq!(r1, r2);
    }

    #[test]
    fn test_effort_disabled_returns_none() {
        let ctx = ctx_with_effort("high");
        let cfg = CshipConfig {
            effort: Some(EffortConfig {
                disabled: Some(true),
                ..Default::default()
            }),
            ..Default::default()
        };
        assert_eq!(render(&ctx, &cfg), None);
    }

    #[test]
    fn test_effort_absent_returns_none() {
        let ctx = Context::default(); // no effort field (model without effort support)
        assert_eq!(render(&ctx, &CshipConfig::default()), None);
    }

    #[test]
    fn test_effort_applies_symbol_and_style() {
        let ctx = ctx_with_effort("max");
        let cfg = CshipConfig {
            effort: Some(EffortConfig {
                symbol: Some("⚡ ".to_string()),
                style: Some("bold magenta".to_string()),
                ..Default::default()
            }),
            ..Default::default()
        };
        let result = render(&ctx, &cfg).unwrap();
        assert!(result.contains("max"), "should contain level: {result:?}");
        assert!(result.contains("⚡ "), "should contain symbol: {result:?}");
        assert!(
            result.contains('\x1b'),
            "should contain ANSI codes: {result:?}"
        );
    }

    #[test]
    fn test_per_level_style_low() {
        let ctx = ctx_with_effort("low");
        assert_eq!(
            render(&ctx, &per_level_cfg()).unwrap(),
            ansi::apply_style("low", Some("green")),
        );
    }

    #[test]
    fn test_per_level_style_medium() {
        let ctx = ctx_with_effort("medium");
        assert_eq!(
            render(&ctx, &per_level_cfg()).unwrap(),
            ansi::apply_style("medium", Some("cyan")),
        );
    }

    #[test]
    fn test_per_level_style_high() {
        let ctx = ctx_with_effort("high");
        assert_eq!(
            render(&ctx, &per_level_cfg()).unwrap(),
            ansi::apply_style("high", Some("yellow")),
        );
    }

    #[test]
    fn test_per_level_style_xhigh() {
        let ctx = ctx_with_effort("xhigh");
        assert_eq!(
            render(&ctx, &per_level_cfg()).unwrap(),
            ansi::apply_style("xhigh", Some("bold yellow")),
        );
    }

    #[test]
    fn test_per_level_style_max() {
        let ctx = ctx_with_effort("max");
        assert_eq!(
            render(&ctx, &per_level_cfg()).unwrap(),
            ansi::apply_style("max", Some("bold red")),
        );
    }

    #[test]
    fn test_per_level_style_matches_case_insensitively() {
        // A future Claude Code version sending "HIGH" should still map to high_style.
        let ctx = ctx_with_effort("HIGH");
        assert_eq!(
            render(&ctx, &per_level_cfg()).unwrap(),
            ansi::apply_style("HIGH", Some("yellow")),
        );
    }

    #[test]
    fn test_per_level_style_falls_back_to_base_style() {
        // No max_style set — should use base style, not another level's style.
        let ctx = ctx_with_effort("max");
        let cfg = CshipConfig {
            effort: Some(EffortConfig {
                style: Some("bold green".to_string()),
                high_style: Some("yellow".to_string()),
                ..Default::default()
            }),
            ..Default::default()
        };
        assert_eq!(
            render(&ctx, &cfg).unwrap(),
            ansi::apply_style("max", Some("bold green")),
        );
    }

    #[test]
    fn test_unknown_level_uses_base_style() {
        let ctx = ctx_with_effort("ludicrous");
        assert_eq!(
            render(&ctx, &per_level_cfg()).unwrap(),
            ansi::apply_style("ludicrous", Some("dim")),
        );
    }

    #[test]
    fn test_no_style_returns_plain_text() {
        let ctx = ctx_with_effort("medium");
        assert_eq!(render(&ctx, &CshipConfig::default()).unwrap(), "medium");
    }

    #[test]
    fn test_format_string_substitutes_value_and_symbol() {
        let ctx = ctx_with_effort("high");
        let cfg = CshipConfig {
            effort: Some(EffortConfig {
                symbol: Some("⚡".to_string()),
                format: Some("$symbol $value".to_string()),
                ..Default::default()
            }),
            ..Default::default()
        };
        assert_eq!(render(&ctx, &cfg), Some("⚡ high".to_string()));
    }
}
