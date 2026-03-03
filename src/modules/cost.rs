/// Render the `[cship.cost]` family of modules.
///
/// `$cship.cost` — convenience alias: formats total_cost_usd as "$X.XX" with threshold styling.
/// `$cship.cost.total_cost_usd` — raw USD value, 4 decimal places.
/// `$cship.cost.total_duration_ms` / `total_api_duration_ms` — integer milliseconds.
/// `$cship.cost.total_lines_added` / `total_lines_removed` — integer counts.
///
/// [Source: epics.md#Story 2.1, architecture.md#Structure Patterns]
use crate::config::{CostConfig, CostSubfieldConfig, CshipConfig};
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

    // Format string takes priority if configured (AC1–4)
    if let Some(fmt) = cost_cfg.and_then(|c| c.format.as_deref()) {
        return crate::format::apply_module_format(fmt, Some(&formatted), symbol, style);
    }

    // Default behavior — unchanged (AC5): threshold-style logic
    let symbol_str = symbol.unwrap_or("");
    let content = format!("{symbol_str}{formatted}");
    let warn_threshold = cost_cfg.and_then(|c| c.warn_threshold);
    let warn_style = cost_cfg.and_then(|c| c.warn_style.as_deref());
    let critical_threshold = cost_cfg.and_then(|c| c.critical_threshold);
    let critical_style = cost_cfg.and_then(|c| c.critical_style.as_deref());

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
    if let Some(fmt) = sub_cfg.and_then(|c| c.format.as_deref()) {
        let symbol = sub_cfg.and_then(|c| c.symbol.as_deref());
        let style = sub_cfg.and_then(|c| c.style.as_deref());
        return crate::format::apply_module_format(fmt, Some(&val_str), symbol, style);
    }
    Some(apply_subfield_style(&val_str, sub_cfg))
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
    if let Some(fmt) = sub_cfg.and_then(|c| c.format.as_deref()) {
        let symbol = sub_cfg.and_then(|c| c.symbol.as_deref());
        let style = sub_cfg.and_then(|c| c.style.as_deref());
        return crate::format::apply_module_format(fmt, Some(&val_str), symbol, style);
    }
    Some(apply_subfield_style(&val_str, sub_cfg))
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
    if let Some(fmt) = sub_cfg.and_then(|c| c.format.as_deref()) {
        let symbol = sub_cfg.and_then(|c| c.symbol.as_deref());
        let style = sub_cfg.and_then(|c| c.style.as_deref());
        return crate::format::apply_module_format(fmt, Some(&val_str), symbol, style);
    }
    Some(apply_subfield_style(&val_str, sub_cfg))
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
    if let Some(fmt) = sub_cfg.and_then(|c| c.format.as_deref()) {
        let symbol = sub_cfg.and_then(|c| c.symbol.as_deref());
        let style = sub_cfg.and_then(|c| c.style.as_deref());
        return crate::format::apply_module_format(fmt, Some(&val_str), symbol, style);
    }
    Some(apply_subfield_style(&val_str, sub_cfg))
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
    if let Some(fmt) = sub_cfg.and_then(|c| c.format.as_deref()) {
        let symbol = sub_cfg.and_then(|c| c.symbol.as_deref());
        let style = sub_cfg.and_then(|c| c.style.as_deref());
        return crate::format::apply_module_format(fmt, Some(&val_str), symbol, style);
    }
    Some(apply_subfield_style(&val_str, sub_cfg))
}

fn is_subfield_disabled(
    sub_cfg: Option<&CostSubfieldConfig>,
    cost_cfg: Option<&CostConfig>,
) -> bool {
    // Sub-field explicit disabled takes precedence
    if let Some(d) = sub_cfg.and_then(|c| c.disabled) {
        return d;
    }
    // Fall through to parent disabled
    cost_cfg.and_then(|c| c.disabled).unwrap_or(false)
}

fn apply_subfield_style(content: &str, cfg: Option<&CostSubfieldConfig>) -> String {
    crate::ansi::apply_style(content, cfg.and_then(|c| c.style.as_deref()))
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
}
