#[cfg(test)]
use crate::config::CostSubfieldConfig;
/// Render the `[cship.cost]` family of modules.
///
/// `$cship.cost` — convenience alias: formats total_cost_usd as "$X.XX" with threshold styling.
/// `$cship.cost.total_cost_usd` — raw USD value, 4 decimal places.
/// `$cship.cost.total_duration_ms` / `total_api_duration_ms` — integer milliseconds.
/// `$cship.cost.total_lines_added` / `total_lines_removed` — integer counts.
///
/// [Source: epics.md#Story 2.1, architecture.md#Structure Patterns]
use crate::config::{CostConfig, CshipConfig};
use crate::context::Context;

/// Renders `$cship.cost` — total cost as `$X.XX` with threshold color escalation.
pub fn render(ctx: &Context, cfg: &CshipConfig) -> Option<String> {
    let cost_cfg = cfg.cost.as_ref();

    // Respect disabled flag — return None silently
    if cost_cfg.and_then(|c| c.disabled).unwrap_or(false) {
        return None;
    }

    // total_cost_usd absent → warn and return None (AC9 requires tracing::warn!)
    let val = match ctx.cost.as_ref().and_then(|c| c.total_cost_usd) {
        Some(v) => v,
        None => {
            tracing::warn!("cship.cost: total_cost_usd absent from context");
            return None;
        }
    };

    let symbol = cost_cfg.and_then(|c| c.symbol.as_deref());
    let style = cost_cfg.and_then(|c| c.style.as_deref());
    let formatted = format!("${:.2}", val);

    // Extract threshold variables FIRST (before format check)
    let warn_threshold = cost_cfg.and_then(|c| c.warn_threshold);
    let warn_style = cost_cfg.and_then(|c| c.warn_style.as_deref());
    let critical_threshold = cost_cfg.and_then(|c| c.critical_threshold);
    let critical_style = cost_cfg.and_then(|c| c.critical_style.as_deref());

    // Format string takes priority if configured (AC1)
    if let Some(fmt) = cost_cfg.and_then(|c| c.format.as_deref()) {
        let effective_style = crate::ansi::resolve_threshold_style(
            Some(val),
            style,
            warn_threshold,
            warn_style,
            critical_threshold,
            critical_style,
        );
        return crate::format::apply_module_format(fmt, Some(&formatted), symbol, effective_style);
    }

    // Default behavior — unchanged (AC5): threshold-style logic
    let symbol_str = symbol.unwrap_or("");
    let content = format!("{symbol_str}{formatted}");

    Some(crate::ansi::apply_style_with_threshold(
        &content,
        Some(val),
        style,
        warn_threshold,
        warn_style,
        critical_threshold,
        critical_style,
    ))
}

/// Renders `$cship.cost.total_cost_usd` — raw USD value to 4 decimal places.
pub fn render_total_cost_usd(ctx: &Context, cfg: &CshipConfig) -> Option<String> {
    let cost_cfg = cfg.cost.as_ref();
    let sub_cfg = cost_cfg.and_then(|c| c.total_cost_usd.as_ref());
    if is_subfield_disabled(sub_cfg, cost_cfg) {
        return None;
    }
    let val = match ctx.cost.as_ref().and_then(|c| c.total_cost_usd) {
        Some(v) => v,
        None => {
            tracing::warn!("cship.cost.total_cost_usd: value absent from context");
            return None;
        }
    };
    let val_str = format!("{:.4}", val);
    crate::format::render_styled_value(&val_str, Some(val), sub_cfg, None)
}

/// Renders `$cship.cost.total_duration_ms` — total wall time in milliseconds.
pub fn render_total_duration_ms(ctx: &Context, cfg: &CshipConfig) -> Option<String> {
    let cost_cfg = cfg.cost.as_ref();
    let sub_cfg = cost_cfg.and_then(|c| c.total_duration_ms.as_ref());
    if is_subfield_disabled(sub_cfg, cost_cfg) {
        return None;
    }
    let val = match ctx.cost.as_ref().and_then(|c| c.total_duration_ms) {
        Some(v) => v,
        None => {
            tracing::warn!("cship.cost.total_duration_ms: value absent from context");
            return None;
        }
    };
    let val_str = val.to_string();
    crate::format::render_styled_value(&val_str, Some(val as f64), sub_cfg, None)
}

/// Renders `$cship.cost.total_api_duration_ms` — API-only duration in milliseconds.
pub fn render_total_api_duration_ms(ctx: &Context, cfg: &CshipConfig) -> Option<String> {
    let cost_cfg = cfg.cost.as_ref();
    let sub_cfg = cost_cfg.and_then(|c| c.total_api_duration_ms.as_ref());
    if is_subfield_disabled(sub_cfg, cost_cfg) {
        return None;
    }
    let val = match ctx.cost.as_ref().and_then(|c| c.total_api_duration_ms) {
        Some(v) => v,
        None => {
            tracing::warn!("cship.cost.total_api_duration_ms: value absent from context");
            return None;
        }
    };
    let val_str = val.to_string();
    crate::format::render_styled_value(&val_str, Some(val as f64), sub_cfg, None)
}

/// Renders `$cship.cost.total_lines_added` — cumulative lines added this session.
pub fn render_total_lines_added(ctx: &Context, cfg: &CshipConfig) -> Option<String> {
    let cost_cfg = cfg.cost.as_ref();
    let sub_cfg = cost_cfg.and_then(|c| c.total_lines_added.as_ref());
    if is_subfield_disabled(sub_cfg, cost_cfg) {
        return None;
    }
    let val = match ctx.cost.as_ref().and_then(|c| c.total_lines_added) {
        Some(v) => v,
        None => {
            tracing::warn!("cship.cost.total_lines_added: value absent from context");
            return None;
        }
    };
    let val_str = val.to_string();
    crate::format::render_styled_value(&val_str, Some(val as f64), sub_cfg, None)
}

/// Renders `$cship.cost.total_lines_removed` — cumulative lines removed this session.
pub fn render_total_lines_removed(ctx: &Context, cfg: &CshipConfig) -> Option<String> {
    let cost_cfg = cfg.cost.as_ref();
    let sub_cfg = cost_cfg.and_then(|c| c.total_lines_removed.as_ref());
    if is_subfield_disabled(sub_cfg, cost_cfg) {
        return None;
    }
    let val = match ctx.cost.as_ref().and_then(|c| c.total_lines_removed) {
        Some(v) => v,
        None => {
            tracing::warn!("cship.cost.total_lines_removed: value absent from context");
            return None;
        }
    };
    let val_str = val.to_string();
    crate::format::render_styled_value(&val_str, Some(val as f64), sub_cfg, None)
}

fn is_subfield_disabled(
    sub_cfg: Option<&crate::config::SubfieldConfig>,
    cost_cfg: Option<&CostConfig>,
) -> bool {
    // Sub-field explicit disabled takes precedence
    if let Some(d) = sub_cfg.and_then(|c| c.disabled) {
        return d;
    }
    // Fall through to parent disabled
    cost_cfg.and_then(|c| c.disabled).unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{CostConfig, CshipConfig};
    use crate::context::{Context, Cost};

    fn ctx_with_cost(usd: f64) -> Context {
        Context {
            cost: Some(Cost {
                total_cost_usd: Some(usd),
                total_duration_ms: Some(45000),
                total_api_duration_ms: Some(2300),
                total_lines_added: Some(156),
                total_lines_removed: Some(23),
            }),
            ..Default::default()
        }
    }

    #[test]
    fn test_cost_renders_dollar_formatted() {
        let ctx = ctx_with_cost(0.01234);
        let result = render(&ctx, &CshipConfig::default());
        assert_eq!(result, Some("$0.01".to_string()));
    }

    #[test]
    fn test_cost_disabled_returns_none() {
        let ctx = ctx_with_cost(5.0);
        let cfg = CshipConfig {
            cost: Some(CostConfig {
                disabled: Some(true),
                ..Default::default()
            }),
            ..Default::default()
        };
        assert_eq!(render(&ctx, &cfg), None);
    }

    #[test]
    fn test_cost_absent_returns_none_and_warns() {
        let ctx = Context::default(); // no cost field
        let result = render(&ctx, &CshipConfig::default());
        assert_eq!(result, None);
    }

    #[test]
    fn test_cost_below_warn_uses_base_style() {
        let ctx = ctx_with_cost(3.0);
        let cfg = CshipConfig {
            cost: Some(CostConfig {
                warn_threshold: Some(5.0),
                warn_style: Some("yellow".to_string()),
                critical_threshold: Some(10.0),
                critical_style: Some("bold red".to_string()),
                ..Default::default()
            }),
            ..Default::default()
        };
        let result = render(&ctx, &cfg).unwrap();
        // No ANSI codes when base style is None and value is below warn
        assert!(
            !result.contains('\x1b'),
            "should not have ANSI when below warn: {result:?}"
        );
        assert!(result.contains("$3.00"));
    }

    #[test]
    fn test_cost_above_warn_applies_warn_style() {
        let ctx = ctx_with_cost(6.0);
        let cfg = CshipConfig {
            cost: Some(CostConfig {
                warn_threshold: Some(5.0),
                warn_style: Some("yellow".to_string()),
                critical_threshold: Some(10.0),
                critical_style: Some("bold red".to_string()),
                ..Default::default()
            }),
            ..Default::default()
        };
        let result = render(&ctx, &cfg).unwrap();
        assert!(
            result.contains('\x1b'),
            "expected warn ANSI codes: {result:?}"
        );
    }

    #[test]
    fn test_cost_above_critical_applies_critical_style() {
        let ctx = ctx_with_cost(12.0);
        let cfg = CshipConfig {
            cost: Some(CostConfig {
                warn_threshold: Some(5.0),
                warn_style: Some("yellow".to_string()),
                critical_threshold: Some(10.0),
                critical_style: Some("bold red".to_string()),
                ..Default::default()
            }),
            ..Default::default()
        };
        let result = render(&ctx, &cfg).unwrap();
        assert!(
            result.contains('\x1b'),
            "expected critical ANSI codes: {result:?}"
        );
    }

    #[test]
    fn test_subfield_inherits_parent_disabled() {
        let ctx = ctx_with_cost(5.0);
        let cfg = CshipConfig {
            cost: Some(CostConfig {
                disabled: Some(true),
                ..Default::default()
            }),
            ..Default::default()
        };
        // Sub-fields should inherit parent disabled when not explicitly overridden
        assert_eq!(render_total_cost_usd(&ctx, &cfg), None);
        assert_eq!(render_total_duration_ms(&ctx, &cfg), None);
        assert_eq!(render_total_api_duration_ms(&ctx, &cfg), None);
        assert_eq!(render_total_lines_added(&ctx, &cfg), None);
        assert_eq!(render_total_lines_removed(&ctx, &cfg), None);
    }

    #[test]
    fn test_render_total_cost_usd_four_decimal_places() {
        let ctx = ctx_with_cost(0.01234);
        let result = render_total_cost_usd(&ctx, &CshipConfig::default());
        assert_eq!(result, Some("0.0123".to_string()));
    }

    #[test]
    fn test_render_total_duration_ms() {
        let ctx = ctx_with_cost(0.01);
        let result = render_total_duration_ms(&ctx, &CshipConfig::default());
        assert_eq!(result, Some("45000".to_string()));
    }

    #[test]
    fn test_render_total_api_duration_ms() {
        let ctx = ctx_with_cost(0.01);
        let result = render_total_api_duration_ms(&ctx, &CshipConfig::default());
        assert_eq!(result, Some("2300".to_string()));
    }

    #[test]
    fn test_render_total_lines_added() {
        let ctx = ctx_with_cost(0.01);
        let result = render_total_lines_added(&ctx, &CshipConfig::default());
        assert_eq!(result, Some("156".to_string()));
    }

    #[test]
    fn test_render_total_lines_removed() {
        let ctx = ctx_with_cost(0.01);
        let result = render_total_lines_removed(&ctx, &CshipConfig::default());
        assert_eq!(result, Some("23".to_string()));
    }

    #[test]
    fn test_cost_format_below_threshold_uses_base_style() {
        // AC1: format + value below all thresholds → base style (None) used
        let ctx = ctx_with_cost(3.0); // below warn_threshold of 5.0
        let cfg = CshipConfig {
            cost: Some(CostConfig {
                format: Some("[$value]($style)".to_string()),
                warn_threshold: Some(5.0),
                warn_style: Some("yellow".to_string()),
                critical_threshold: Some(10.0),
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
        assert!(
            result.contains("$3.00"),
            "expected formatted value: {result:?}"
        );
    }

    #[test]
    fn test_cost_format_with_warn_threshold_uses_warn_style() {
        // AC1: format + warn_threshold → warn_style flows into format renderer
        let ctx = ctx_with_cost(6.0); // above warn_threshold of 5.0
        let cfg = CshipConfig {
            cost: Some(CostConfig {
                format: Some("[$value]($style)".to_string()),
                warn_threshold: Some(5.0),
                warn_style: Some("yellow".to_string()),
                critical_threshold: Some(10.0),
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
        assert!(
            result.contains("$6.00"),
            "expected formatted value: {result:?}"
        );
    }

    #[test]
    fn test_cost_format_with_critical_threshold_uses_critical_style() {
        // AC1: format + critical_threshold → critical_style flows into format renderer
        let ctx = ctx_with_cost(12.0); // above critical_threshold of 10.0
        let cfg = CshipConfig {
            cost: Some(CostConfig {
                format: Some("[$value]($style)".to_string()),
                warn_threshold: Some(5.0),
                warn_style: Some("yellow".to_string()),
                critical_threshold: Some(10.0),
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
        assert!(
            result.contains("$12.00"),
            "expected formatted value: {result:?}"
        );
    }

    #[test]
    fn test_cost_format_warn_and_critical_produce_different_styles() {
        // M1 fix: verify warn and critical styles are distinguishable
        let warn_cfg = CshipConfig {
            cost: Some(CostConfig {
                format: Some("[$value]($style)".to_string()),
                warn_threshold: Some(5.0),
                warn_style: Some("yellow".to_string()),
                critical_threshold: Some(100.0),
                critical_style: Some("bold red".to_string()),
                ..Default::default()
            }),
            ..Default::default()
        };
        let crit_cfg = CshipConfig {
            cost: Some(CostConfig {
                format: Some("[$value]($style)".to_string()),
                warn_threshold: Some(5.0),
                warn_style: Some("yellow".to_string()),
                critical_threshold: Some(5.0),
                critical_style: Some("bold red".to_string()),
                ..Default::default()
            }),
            ..Default::default()
        };
        let ctx = ctx_with_cost(6.0);
        let warn_result = render(&ctx, &warn_cfg).unwrap();
        let crit_result = render(&ctx, &crit_cfg).unwrap();
        assert_ne!(
            warn_result, crit_result,
            "warn and critical styles must produce different output"
        );
    }

    // --- Story 9.1: Subfield threshold tests ---

    #[test]
    fn test_subfield_total_cost_usd_above_warn_applies_warn_style() {
        // AC2, AC6: value above warn_threshold → warn_style applied in default path
        let ctx = ctx_with_cost(5.0); // total_cost_usd = 5.0
        let cfg = CshipConfig {
            cost: Some(CostConfig {
                total_cost_usd: Some(CostSubfieldConfig {
                    warn_threshold: Some(3.0),
                    warn_style: Some("yellow".to_string()),
                    critical_threshold: Some(10.0),
                    critical_style: Some("bold red".to_string()),
                    ..Default::default()
                }),
                ..Default::default()
            }),
            ..Default::default()
        };
        let result = render_total_cost_usd(&ctx, &cfg).unwrap();
        assert!(
            result.contains('\x1b'),
            "expected ANSI for warn: {result:?}"
        );
        assert!(result.contains("5.0000"), "expected 4dp value: {result:?}");
    }

    #[test]
    fn test_subfield_total_cost_usd_above_critical_applies_critical_style() {
        // AC2, AC6: value above critical_threshold → critical_style applied
        let ctx = ctx_with_cost(12.0); // total_cost_usd = 12.0
        let cfg = CshipConfig {
            cost: Some(CostConfig {
                total_cost_usd: Some(CostSubfieldConfig {
                    warn_threshold: Some(3.0),
                    warn_style: Some("yellow".to_string()),
                    critical_threshold: Some(10.0),
                    critical_style: Some("bold red".to_string()),
                    ..Default::default()
                }),
                ..Default::default()
            }),
            ..Default::default()
        };
        let result = render_total_cost_usd(&ctx, &cfg).unwrap();
        assert!(
            result.contains('\x1b'),
            "expected ANSI for critical: {result:?}"
        );
        assert!(result.contains("12.0000"), "expected 4dp value: {result:?}");
    }

    #[test]
    fn test_subfield_total_cost_usd_below_warn_uses_base_style() {
        // AC5, AC6: value below warn_threshold → no ANSI when base style is None
        let ctx = ctx_with_cost(1.0); // below warn_threshold of 3.0
        let cfg = CshipConfig {
            cost: Some(CostConfig {
                total_cost_usd: Some(CostSubfieldConfig {
                    warn_threshold: Some(3.0),
                    warn_style: Some("yellow".to_string()),
                    ..Default::default()
                }),
                ..Default::default()
            }),
            ..Default::default()
        };
        let result = render_total_cost_usd(&ctx, &cfg).unwrap();
        assert!(
            !result.contains('\x1b'),
            "no ANSI expected below warn: {result:?}"
        );
        assert!(result.contains("1.0000"), "expected value: {result:?}");
    }

    #[test]
    fn test_subfield_total_duration_ms_above_warn_applies_warn_style() {
        // AC4: 45000ms > 30000ms warn threshold → warn_style applied
        let ctx = ctx_with_cost(0.01); // ctx_with_cost sets total_duration_ms = 45000
        let cfg = CshipConfig {
            cost: Some(CostConfig {
                total_duration_ms: Some(CostSubfieldConfig {
                    warn_threshold: Some(30000.0),
                    warn_style: Some("yellow".to_string()),
                    critical_threshold: Some(60000.0),
                    critical_style: Some("bold red".to_string()),
                    ..Default::default()
                }),
                ..Default::default()
            }),
            ..Default::default()
        };
        let result = render_total_duration_ms(&ctx, &cfg).unwrap();
        assert!(result.contains('\x1b'), "expected warn ANSI: {result:?}");
        assert!(result.contains("45000"), "expected value: {result:?}");
    }

    #[test]
    fn test_subfield_format_with_warn_threshold_uses_warn_style() {
        // AC3, AC6: format path + threshold → threshold-resolved style in apply_module_format
        let ctx = ctx_with_cost(0.01); // total_duration_ms = 45000 > 30000
        let cfg = CshipConfig {
            cost: Some(CostConfig {
                total_duration_ms: Some(CostSubfieldConfig {
                    format: Some("[$value ms]($style)".to_string()),
                    warn_threshold: Some(30000.0),
                    warn_style: Some("yellow".to_string()),
                    critical_threshold: Some(60000.0),
                    critical_style: Some("bold red".to_string()),
                    ..Default::default()
                }),
                ..Default::default()
            }),
            ..Default::default()
        };
        let result = render_total_duration_ms(&ctx, &cfg).unwrap();
        assert!(
            result.contains('\x1b'),
            "expected ANSI in format path: {result:?}"
        );
        assert!(
            result.contains("45000"),
            "expected value in format: {result:?}"
        );
    }

    #[test]
    fn test_subfield_total_api_duration_ms_above_warn_applies_warn_style() {
        // AC2: u64 threshold wiring for total_api_duration_ms (2300 > 2000)
        let ctx = ctx_with_cost(0.01); // ctx_with_cost sets total_api_duration_ms = 2300
        let cfg = CshipConfig {
            cost: Some(CostConfig {
                total_api_duration_ms: Some(CostSubfieldConfig {
                    warn_threshold: Some(2000.0),
                    warn_style: Some("yellow".to_string()),
                    ..Default::default()
                }),
                ..Default::default()
            }),
            ..Default::default()
        };
        let result = render_total_api_duration_ms(&ctx, &cfg).unwrap();
        assert!(result.contains('\x1b'), "expected warn ANSI: {result:?}");
        assert!(result.contains("2300"), "expected value: {result:?}");
    }

    #[test]
    fn test_subfield_total_lines_added_above_warn_applies_warn_style() {
        // AC2: i64 threshold wiring for total_lines_added (156 > 100)
        let ctx = ctx_with_cost(0.01); // ctx_with_cost sets total_lines_added = 156
        let cfg = CshipConfig {
            cost: Some(CostConfig {
                total_lines_added: Some(CostSubfieldConfig {
                    warn_threshold: Some(100.0),
                    warn_style: Some("yellow".to_string()),
                    ..Default::default()
                }),
                ..Default::default()
            }),
            ..Default::default()
        };
        let result = render_total_lines_added(&ctx, &cfg).unwrap();
        assert!(result.contains('\x1b'), "expected warn ANSI: {result:?}");
        assert!(result.contains("156"), "expected value: {result:?}");
    }

    #[test]
    fn test_subfield_total_lines_removed_above_warn_applies_warn_style() {
        // AC2: i64 threshold wiring for total_lines_removed (23 > 10)
        let ctx = ctx_with_cost(0.01); // ctx_with_cost sets total_lines_removed = 23
        let cfg = CshipConfig {
            cost: Some(CostConfig {
                total_lines_removed: Some(CostSubfieldConfig {
                    warn_threshold: Some(10.0),
                    warn_style: Some("yellow".to_string()),
                    ..Default::default()
                }),
                ..Default::default()
            }),
            ..Default::default()
        };
        let result = render_total_lines_removed(&ctx, &cfg).unwrap();
        assert!(result.contains('\x1b'), "expected warn ANSI: {result:?}");
        assert!(result.contains("23"), "expected value: {result:?}");
    }

    #[test]
    fn test_subfield_no_threshold_unchanged() {
        // AC5, AC6: no threshold fields → output identical to baseline (no regression)
        let ctx = ctx_with_cost(0.01234);
        let result_default = render_total_cost_usd(&ctx, &CshipConfig::default());
        let cfg_no_thresh = CshipConfig {
            cost: Some(CostConfig {
                total_cost_usd: Some(CostSubfieldConfig {
                    ..Default::default() // all None
                }),
                ..Default::default()
            }),
            ..Default::default()
        };
        let result_explicit = render_total_cost_usd(&ctx, &cfg_no_thresh);
        assert_eq!(
            result_default, result_explicit,
            "no-threshold config should match default"
        );
    }
}
