use crate::config::CshipConfig;
use crate::context::Context;

const DEFAULT_BAR_WIDTH: u32 = 10;
const DEFAULT_FILLED_CHAR: &str = "█";
const DEFAULT_EMPTY_CHAR: &str = "░";

/// Renders `$cship.context_bar` — visual Unicode progress bar with threshold color escalation.
/// Format: `{bar}{used_percentage:.0}%` e.g. `███░░░░░░░35%`
/// Bar width is configurable via `[cship.context_bar].width` (default 10).
/// Fill characters are configurable via `filled_char` (default `"█"`) and `empty_char` (default `"░"`).
///
/// Source: epics.md#Story 2.2, prd.md#FR8
pub fn render(ctx: &Context, cfg: &CshipConfig) -> Option<String> {
    let bar_cfg = cfg.context_bar.as_ref();

    if bar_cfg.and_then(|c| c.disabled).unwrap_or(false) {
        return None;
    }

    let (used_pct, is_empty) = match ctx.context_window.as_ref() {
        Some(cw) => match cw.used_percentage {
            Some(v) => (v, false),
            None => {
                tracing::debug!(
                    "cship.context_bar: context_window present but used_percentage absent; rendering empty bar"
                );
                (0.0f64, true)
            }
        },
        None => (0.0f64, true),
    };

    let width = bar_cfg.and_then(|c| c.width).unwrap_or(DEFAULT_BAR_WIDTH) as usize;
    // Floor via `as usize` truncation — NOT round. The percentage text uses {:.0} (round).
    // At boundary values like 99.5%, bar shows 9/10 filled while text shows "100%".
    // This is intentional: the bar is a visual approximation, the number is canonical.
    let filled = ((used_pct / 100.0) * width as f64) as usize;
    let filled = filled.min(width); // guard floating-point edge at 100%
    let empty = width - filled;

    let filled_char = bar_cfg
        .and_then(|c| c.filled_char.as_deref())
        .unwrap_or(DEFAULT_FILLED_CHAR);
    let empty_char = bar_cfg
        .and_then(|c| c.empty_char.as_deref())
        .unwrap_or(DEFAULT_EMPTY_CHAR);
    let bar: String = filled_char.repeat(filled) + &empty_char.repeat(empty);
    let bar_content = format!("{bar}{:.0}%", used_pct);

    let symbol = bar_cfg.and_then(|c| c.symbol.as_deref());
    let style = bar_cfg.and_then(|c| c.style.as_deref());

    // Absent context: render 0% bar styled with empty_style (AC: 1, 2, 3)
    if is_empty {
        let empty_style = bar_cfg.and_then(|c| c.empty_style.as_deref());
        if let Some(fmt) = bar_cfg.and_then(|c| c.format.as_deref()) {
            return crate::format::apply_module_format(
                fmt,
                Some(&bar_content),
                symbol,
                empty_style,
            );
        }
        return Some(crate::ansi::apply_style(&bar_content, empty_style));
    }

    // Extract threshold variables FIRST (before format check)
    let warn_threshold = bar_cfg.and_then(|c| c.warn_threshold);
    let warn_style = bar_cfg.and_then(|c| c.warn_style.as_deref());
    let critical_threshold = bar_cfg.and_then(|c| c.critical_threshold);
    let critical_style = bar_cfg.and_then(|c| c.critical_style.as_deref());

    // Format string takes priority if configured (AC2)
    if let Some(fmt) = bar_cfg.and_then(|c| c.format.as_deref()) {
        let effective_style = crate::ansi::resolve_threshold_style(
            Some(used_pct),
            style,
            warn_threshold,
            warn_style,
            critical_threshold,
            critical_style,
        );
        return crate::format::apply_module_format(
            fmt,
            Some(&bar_content),
            symbol,
            effective_style,
        );
    }

    // Default behavior — unchanged (AC5): threshold-style logic

    Some(crate::ansi::apply_style_with_threshold(
        &bar_content,
        Some(used_pct),
        style,
        warn_threshold,
        warn_style,
        critical_threshold,
        critical_style,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{ContextBarConfig, CshipConfig};
    use crate::context::{Context, ContextWindow};

    fn ctx_with_pct(pct: f64) -> Context {
        Context {
            context_window: Some(ContextWindow {
                used_percentage: Some(pct),
                remaining_percentage: Some(100.0 - pct),
                context_window_size: Some(200000),
                total_input_tokens: Some(15234),
                total_output_tokens: Some(4521),
                current_usage: None,
            }),
            ..Default::default()
        }
    }

    #[test]
    fn test_context_bar_35_percent_3_filled_7_empty() {
        let ctx = ctx_with_pct(35.0);
        let result = render(&ctx, &CshipConfig::default()).unwrap();
        // 35% of 10 = 3.5 → floor → 3 filled, 7 empty → "███░░░░░░░35%"
        let filled: usize = result.chars().filter(|&c| c == '█').count();
        let empty: usize = result.chars().filter(|&c| c == '░').count();
        assert_eq!(filled, 3, "expected 3 filled chars: {result:?}");
        assert_eq!(empty, 7, "expected 7 empty chars: {result:?}");
        assert!(result.contains("35%"), "expected '35%' in: {result:?}");
    }

    #[test]
    fn test_context_bar_disabled_returns_none() {
        let ctx = ctx_with_pct(50.0);
        let cfg = CshipConfig {
            context_bar: Some(ContextBarConfig {
                disabled: Some(true),
                ..Default::default()
            }),
            ..Default::default()
        };
        assert_eq!(render(&ctx, &cfg), None);
    }

    #[test]
    fn test_context_bar_absent_context_window_renders_empty_bar() {
        let ctx = Context::default(); // context_window is None
        let result = render(&ctx, &CshipConfig::default()).unwrap();
        let empty_count: usize = result.chars().filter(|&c| c == '░').count();
        assert_eq!(
            empty_count, DEFAULT_BAR_WIDTH as usize,
            "all chars should be empty: {result:?}"
        );
        assert!(result.contains("0%"), "should show 0%: {result:?}");
        assert!(!result.contains('█'), "no filled chars: {result:?}");
    }

    #[test]
    fn test_context_bar_absent_with_empty_style_applies_ansi() {
        let ctx = Context::default(); // context_window is None
        let cfg = CshipConfig {
            context_bar: Some(ContextBarConfig {
                empty_style: Some("dimmed".to_string()),
                ..Default::default()
            }),
            ..Default::default()
        };
        let result = render(&ctx, &cfg).unwrap();
        assert!(
            result.contains('\x1b'),
            "expected ANSI codes for empty_style dimmed: {result:?}"
        );
    }

    #[test]
    fn test_context_bar_absent_no_empty_style_no_ansi() {
        let ctx = Context::default(); // context_window is None
        let result = render(&ctx, &CshipConfig::default()).unwrap();
        assert!(
            !result.contains('\x1b'),
            "expected no ANSI codes when no style configured: {result:?}"
        );
    }

    #[test]
    fn test_context_bar_disabled_absent_returns_none() {
        let ctx = Context::default(); // context_window is None
        let cfg = CshipConfig {
            context_bar: Some(ContextBarConfig {
                disabled: Some(true),
                ..Default::default()
            }),
            ..Default::default()
        };
        assert_eq!(render(&ctx, &cfg), None);
    }

    #[test]
    fn test_context_bar_custom_width_5() {
        let ctx = ctx_with_pct(40.0);
        let cfg = CshipConfig {
            context_bar: Some(ContextBarConfig {
                width: Some(5),
                ..Default::default()
            }),
            ..Default::default()
        };
        let result = render(&ctx, &cfg).unwrap();
        // 40% of 5 = 2.0 → 2 filled, 3 empty
        let total_bar: usize = result.chars().filter(|&c| c == '█' || c == '░').count();
        assert_eq!(total_bar, 5, "expected total bar width 5: {result:?}");
        assert_eq!(
            result.chars().filter(|&c| c == '█').count(),
            2,
            "expected 2 filled: {result:?}"
        );
    }

    #[test]
    fn test_context_bar_warn_threshold_applies_ansi() {
        let ctx = ctx_with_pct(75.0);
        let cfg = CshipConfig {
            context_bar: Some(ContextBarConfig {
                warn_threshold: Some(70.0),
                warn_style: Some("yellow".to_string()),
                critical_threshold: Some(85.0),
                critical_style: Some("bold red".to_string()),
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
    fn test_context_bar_critical_threshold_applies_ansi() {
        let ctx = ctx_with_pct(90.0);
        let cfg = CshipConfig {
            context_bar: Some(ContextBarConfig {
                warn_threshold: Some(70.0),
                warn_style: Some("yellow".to_string()),
                critical_threshold: Some(85.0),
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

    #[test]
    fn test_context_bar_100_percent_all_filled() {
        let ctx = ctx_with_pct(100.0);
        let result = render(&ctx, &CshipConfig::default()).unwrap();
        assert!(
            !result.contains('░'),
            "expected no empty chars at 100%: {result:?}"
        );
        assert!(result.contains("100%"));
    }

    #[test]
    fn test_context_bar_boundary_15_percent_floors_to_1_filled() {
        let ctx = ctx_with_pct(15.0);
        let result = render(&ctx, &CshipConfig::default()).unwrap();
        // 15% of 10 = 1.5 → floor → 1 filled, 9 empty (NOT round → 2)
        let filled: usize = result.chars().filter(|&c| c == '█').count();
        let empty: usize = result.chars().filter(|&c| c == '░').count();
        assert_eq!(filled, 1, "15% should floor to 1 filled: {result:?}");
        assert_eq!(empty, 9, "15% should leave 9 empty: {result:?}");
        assert!(result.contains("15%"), "expected '15%' in: {result:?}");
    }

    #[test]
    fn test_context_bar_boundary_99_5_percent_floors_to_9_filled() {
        let ctx = ctx_with_pct(99.5);
        let result = render(&ctx, &CshipConfig::default()).unwrap();
        // 99.5% of 10 = 9.95 → floor → 9 filled, 1 empty; text rounds to "100%"
        let filled: usize = result.chars().filter(|&c| c == '█').count();
        let empty: usize = result.chars().filter(|&c| c == '░').count();
        assert_eq!(filled, 9, "99.5% should floor to 9 filled: {result:?}");
        assert_eq!(empty, 1, "99.5% should leave 1 empty: {result:?}");
        // {:.0} rounds 99.5 to "100" (banker's rounding) — this is expected
        assert!(
            result.contains("100%") || result.contains("99%"),
            "expected rounded percentage in: {result:?}"
        );
    }

    #[test]
    fn test_context_bar_0_percent_all_empty() {
        let ctx = ctx_with_pct(0.0);
        let result = render(&ctx, &CshipConfig::default()).unwrap();
        assert!(
            !result.contains('█'),
            "expected no filled chars at 0%: {result:?}"
        );
        assert!(result.contains("0%"));
    }

    #[test]
    fn test_context_bar_format_below_threshold_uses_base_style() {
        // AC2: format + value below all thresholds → base style (None) used
        let ctx = ctx_with_pct(50.0); // below warn_threshold of 70.0
        let cfg = CshipConfig {
            context_bar: Some(ContextBarConfig {
                format: Some("[$value]($style)".to_string()),
                warn_threshold: Some(70.0),
                warn_style: Some("yellow".to_string()),
                critical_threshold: Some(85.0),
                critical_style: Some("bold red".to_string()),
                ..Default::default()
            }),
            ..Default::default()
        };
        let result = render(&ctx, &cfg).unwrap();
        assert!(
            !result.contains('\x1b'),
            "expected NO ANSI codes below threshold with no base style: {result:?}"
        );
    }

    #[test]
    fn test_context_bar_format_with_warn_threshold_uses_warn_style() {
        // AC2: format + warn_threshold → warn_style flows into format renderer
        let ctx = ctx_with_pct(75.0); // above warn_threshold of 70.0
        let cfg = CshipConfig {
            context_bar: Some(ContextBarConfig {
                format: Some("[$value]($style)".to_string()),
                warn_threshold: Some(70.0),
                warn_style: Some("yellow".to_string()),
                critical_threshold: Some(85.0),
                critical_style: Some("bold red".to_string()),
                ..Default::default()
            }),
            ..Default::default()
        };
        let result = render(&ctx, &cfg).unwrap();
        assert!(
            result.contains('\x1b'),
            "expected ANSI codes for warn style: {result:?}"
        );
    }

    #[test]
    fn test_context_bar_format_with_critical_threshold_uses_critical_style() {
        // AC2: format + critical_threshold → critical_style flows into format renderer
        let ctx = ctx_with_pct(90.0); // above critical_threshold of 85.0
        let cfg = CshipConfig {
            context_bar: Some(ContextBarConfig {
                format: Some("[$value]($style)".to_string()),
                warn_threshold: Some(70.0),
                warn_style: Some("yellow".to_string()),
                critical_threshold: Some(85.0),
                critical_style: Some("bold red".to_string()),
                ..Default::default()
            }),
            ..Default::default()
        };
        let result = render(&ctx, &cfg).unwrap();
        assert!(
            result.contains('\x1b'),
            "expected ANSI codes for critical style: {result:?}"
        );
    }

    #[test]
    fn test_context_bar_format_warn_and_critical_produce_different_styles() {
        // M1 fix: verify warn and critical styles are distinguishable
        let warn_cfg = CshipConfig {
            context_bar: Some(ContextBarConfig {
                format: Some("[$value]($style)".to_string()),
                warn_threshold: Some(70.0),
                warn_style: Some("yellow".to_string()),
                critical_threshold: Some(100.0),
                critical_style: Some("bold red".to_string()),
                ..Default::default()
            }),
            ..Default::default()
        };
        let crit_cfg = CshipConfig {
            context_bar: Some(ContextBarConfig {
                format: Some("[$value]($style)".to_string()),
                warn_threshold: Some(70.0),
                warn_style: Some("yellow".to_string()),
                critical_threshold: Some(70.0),
                critical_style: Some("bold red".to_string()),
                ..Default::default()
            }),
            ..Default::default()
        };
        let ctx = ctx_with_pct(75.0);
        let warn_result = render(&ctx, &warn_cfg).unwrap();
        let crit_result = render(&ctx, &crit_cfg).unwrap();
        assert_ne!(
            warn_result, crit_result,
            "warn and critical styles must produce different output"
        );
    }

    #[test]
    fn test_context_bar_custom_filled_and_empty_chars() {
        let ctx = ctx_with_pct(40.0);
        let cfg = CshipConfig {
            context_bar: Some(ContextBarConfig {
                filled_char: Some("●".to_string()),
                empty_char: Some("○".to_string()),
                ..Default::default()
            }),
            ..Default::default()
        };
        let result = render(&ctx, &cfg).unwrap();
        // 40% of 10 = 4 filled, 6 empty
        let filled: usize = result.chars().filter(|&c| c == '●').count();
        let empty: usize = result.chars().filter(|&c| c == '○').count();
        assert_eq!(filled, 4, "expected 4 filled circles: {result:?}");
        assert_eq!(empty, 6, "expected 6 empty circles: {result:?}");
        assert!(result.contains("40%"), "expected '40%' in: {result:?}");
    }

    #[test]
    fn test_context_bar_only_filled_char_overridden() {
        let ctx = ctx_with_pct(50.0);
        let cfg = CshipConfig {
            context_bar: Some(ContextBarConfig {
                filled_char: Some("●".to_string()),
                ..Default::default()
            }),
            ..Default::default()
        };
        let result = render(&ctx, &cfg).unwrap();
        let filled: usize = result.chars().filter(|&c| c == '●').count();
        let empty: usize = result.chars().filter(|&c| c == '░').count();
        assert_eq!(filled, 5, "expected 5 filled circles: {result:?}");
        assert_eq!(empty, 5, "expected 5 default empty chars: {result:?}");
    }
}
