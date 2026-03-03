use crate::config::CshipConfig;
use crate::context::Context;

const DEFAULT_EXCEEDS_SYMBOL: &str = ">200k";

fn is_disabled(cfg: &CshipConfig) -> bool {
    cfg.context_window
        .as_ref()
        .and_then(|c| c.disabled)
        .unwrap_or(false)
}

fn apply_cw_style(content: &str, cfg: &CshipConfig) -> String {
    crate::ansi::apply_style(
        content,
        cfg.context_window.as_ref().and_then(|c| c.style.as_deref()),
    )
}

/// Renders `$cship.context_window.used_percentage` — integer percentage, no `%` sign.
pub fn render_used_percentage(ctx: &Context, cfg: &CshipConfig) -> Option<String> {
    if is_disabled(cfg) {
        return None;
    }
    let val = match ctx
        .context_window
        .as_ref()
        .and_then(|cw| cw.used_percentage)
    {
        Some(v) => v,
        None => {
            tracing::warn!("cship.context_window.used_percentage: value absent from context");
            return None;
        }
    };
    let val_str = format!("{:.0}", val);
    let cw_cfg = cfg.context_window.as_ref();
    if let Some(fmt) = cw_cfg.and_then(|c| c.format.as_deref()) {
        let symbol = cw_cfg.and_then(|c| c.symbol.as_deref());
        let style = cw_cfg.and_then(|c| c.style.as_deref());
        return crate::format::apply_module_format(fmt, Some(&val_str), symbol, style);
    }
    Some(apply_cw_style(&val_str, cfg))
}

/// Renders `$cship.context_window.remaining_percentage` — integer percentage, no `%` sign.
pub fn render_remaining_percentage(ctx: &Context, cfg: &CshipConfig) -> Option<String> {
    if is_disabled(cfg) {
        return None;
    }
    let val = match ctx
        .context_window
        .as_ref()
        .and_then(|cw| cw.remaining_percentage)
    {
        Some(v) => v,
        None => {
            tracing::warn!("cship.context_window.remaining_percentage: value absent from context");
            return None;
        }
    };
    let val_str = format!("{:.0}", val);
    let cw_cfg = cfg.context_window.as_ref();
    if let Some(fmt) = cw_cfg.and_then(|c| c.format.as_deref()) {
        let symbol = cw_cfg.and_then(|c| c.symbol.as_deref());
        let style = cw_cfg.and_then(|c| c.style.as_deref());
        return crate::format::apply_module_format(fmt, Some(&val_str), symbol, style);
    }
    Some(apply_cw_style(&val_str, cfg))
}

/// Renders `$cship.context_window.size` — reads `context_window_size` field (not `size`).
pub fn render_size(ctx: &Context, cfg: &CshipConfig) -> Option<String> {
    if is_disabled(cfg) {
        return None;
    }
    let val = match ctx
        .context_window
        .as_ref()
        .and_then(|cw| cw.context_window_size)
    {
        Some(v) => v,
        None => {
            tracing::warn!("cship.context_window.size: context_window_size absent from context");
            return None;
        }
    };
    let val_str = val.to_string();
    let cw_cfg = cfg.context_window.as_ref();
    if let Some(fmt) = cw_cfg.and_then(|c| c.format.as_deref()) {
        let symbol = cw_cfg.and_then(|c| c.symbol.as_deref());
        let style = cw_cfg.and_then(|c| c.style.as_deref());
        return crate::format::apply_module_format(fmt, Some(&val_str), symbol, style);
    }
    Some(apply_cw_style(&val_str, cfg))
}

/// Renders `$cship.context_window.total_input_tokens`.
pub fn render_total_input_tokens(ctx: &Context, cfg: &CshipConfig) -> Option<String> {
    if is_disabled(cfg) {
        return None;
    }
    let val = match ctx
        .context_window
        .as_ref()
        .and_then(|cw| cw.total_input_tokens)
    {
        Some(v) => v,
        None => {
            tracing::warn!("cship.context_window.total_input_tokens: value absent from context");
            return None;
        }
    };
    let val_str = val.to_string();
    let cw_cfg = cfg.context_window.as_ref();
    if let Some(fmt) = cw_cfg.and_then(|c| c.format.as_deref()) {
        let symbol = cw_cfg.and_then(|c| c.symbol.as_deref());
        let style = cw_cfg.and_then(|c| c.style.as_deref());
        return crate::format::apply_module_format(fmt, Some(&val_str), symbol, style);
    }
    Some(apply_cw_style(&val_str, cfg))
}

/// Renders `$cship.context_window.total_output_tokens`.
pub fn render_total_output_tokens(ctx: &Context, cfg: &CshipConfig) -> Option<String> {
    if is_disabled(cfg) {
        return None;
    }
    let val = match ctx
        .context_window
        .as_ref()
        .and_then(|cw| cw.total_output_tokens)
    {
        Some(v) => v,
        None => {
            tracing::warn!("cship.context_window.total_output_tokens: value absent from context");
            return None;
        }
    };
    let val_str = val.to_string();
    let cw_cfg = cfg.context_window.as_ref();
    if let Some(fmt) = cw_cfg.and_then(|c| c.format.as_deref()) {
        let symbol = cw_cfg.and_then(|c| c.symbol.as_deref());
        let style = cw_cfg.and_then(|c| c.style.as_deref());
        return crate::format::apply_module_format(fmt, Some(&val_str), symbol, style);
    }
    Some(apply_cw_style(&val_str, cfg))
}

/// Renders `$cship.context_window.exceeds_200k`.
///
/// CRITICAL: `exceeds_200k_tokens` is a TOP-LEVEL field on `Context`, NOT inside `context_window`.
/// Returns None when false or absent (no tracing::warn! — false is a valid expected state).
/// When true, renders configurable symbol (default: ">200k").
pub fn render_exceeds_200k(ctx: &Context, cfg: &CshipConfig) -> Option<String> {
    if is_disabled(cfg) {
        return None;
    }
    let exceeds = ctx.exceeds_200k_tokens.unwrap_or(false);
    if !exceeds {
        return None; // false is normal — no warn
    }
    let cw_cfg = cfg.context_window.as_ref();
    let symbol_str = cw_cfg
        .and_then(|c| c.symbol.as_deref())
        .unwrap_or(DEFAULT_EXCEEDS_SYMBOL);
    if let Some(fmt) = cw_cfg.and_then(|c| c.format.as_deref()) {
        let style = cw_cfg.and_then(|c| c.style.as_deref());
        return crate::format::apply_module_format(fmt, Some(symbol_str), Some(symbol_str), style);
    }
    Some(apply_cw_style(symbol_str, cfg))
}

/// Renders `$cship.context_window.current_usage.input_tokens`.
pub fn render_current_usage_input_tokens(ctx: &Context, cfg: &CshipConfig) -> Option<String> {
    if is_disabled(cfg) {
        return None;
    }
    let val = match ctx
        .context_window
        .as_ref()
        .and_then(|cw| cw.current_usage.as_ref())
        .and_then(|cu| cu.input_tokens)
    {
        Some(v) => v,
        None => {
            tracing::warn!(
                "cship.context_window.current_usage.input_tokens: value absent from context"
            );
            return None;
        }
    };
    let val_str = val.to_string();
    let cw_cfg = cfg.context_window.as_ref();
    if let Some(fmt) = cw_cfg.and_then(|c| c.format.as_deref()) {
        let symbol = cw_cfg.and_then(|c| c.symbol.as_deref());
        let style = cw_cfg.and_then(|c| c.style.as_deref());
        return crate::format::apply_module_format(fmt, Some(&val_str), symbol, style);
    }
    Some(apply_cw_style(&val_str, cfg))
}

/// Renders `$cship.context_window.current_usage.output_tokens`.
pub fn render_current_usage_output_tokens(ctx: &Context, cfg: &CshipConfig) -> Option<String> {
    if is_disabled(cfg) {
        return None;
    }
    let val = match ctx
        .context_window
        .as_ref()
        .and_then(|cw| cw.current_usage.as_ref())
        .and_then(|cu| cu.output_tokens)
    {
        Some(v) => v,
        None => {
            tracing::warn!(
                "cship.context_window.current_usage.output_tokens: value absent from context"
            );
            return None;
        }
    };
    let val_str = val.to_string();
    let cw_cfg = cfg.context_window.as_ref();
    if let Some(fmt) = cw_cfg.and_then(|c| c.format.as_deref()) {
        let symbol = cw_cfg.and_then(|c| c.symbol.as_deref());
        let style = cw_cfg.and_then(|c| c.style.as_deref());
        return crate::format::apply_module_format(fmt, Some(&val_str), symbol, style);
    }
    Some(apply_cw_style(&val_str, cfg))
}

/// Renders `$cship.context_window.current_usage.cache_creation_input_tokens`.
pub fn render_current_usage_cache_creation_input_tokens(
    ctx: &Context,
    cfg: &CshipConfig,
) -> Option<String> {
    if is_disabled(cfg) {
        return None;
    }
    let val = match ctx
        .context_window
        .as_ref()
        .and_then(|cw| cw.current_usage.as_ref())
        .and_then(|cu| cu.cache_creation_input_tokens)
    {
        Some(v) => v,
        None => {
            tracing::warn!(
                "cship.context_window.current_usage.cache_creation_input_tokens: value absent from context"
            );
            return None;
        }
    };
    let val_str = val.to_string();
    let cw_cfg = cfg.context_window.as_ref();
    if let Some(fmt) = cw_cfg.and_then(|c| c.format.as_deref()) {
        let symbol = cw_cfg.and_then(|c| c.symbol.as_deref());
        let style = cw_cfg.and_then(|c| c.style.as_deref());
        return crate::format::apply_module_format(fmt, Some(&val_str), symbol, style);
    }
    Some(apply_cw_style(&val_str, cfg))
}

/// Renders `$cship.context_window.current_usage.cache_read_input_tokens`.
pub fn render_current_usage_cache_read_input_tokens(
    ctx: &Context,
    cfg: &CshipConfig,
) -> Option<String> {
    if is_disabled(cfg) {
        return None;
    }
    let val = match ctx
        .context_window
        .as_ref()
        .and_then(|cw| cw.current_usage.as_ref())
        .and_then(|cu| cu.cache_read_input_tokens)
    {
        Some(v) => v,
        None => {
            tracing::warn!(
                "cship.context_window.current_usage.cache_read_input_tokens: value absent from context"
            );
            return None;
        }
    };
    let val_str = val.to_string();
    let cw_cfg = cfg.context_window.as_ref();
    if let Some(fmt) = cw_cfg.and_then(|c| c.format.as_deref()) {
        let symbol = cw_cfg.and_then(|c| c.symbol.as_deref());
        let style = cw_cfg.and_then(|c| c.style.as_deref());
        return crate::format::apply_module_format(fmt, Some(&val_str), symbol, style);
    }
    Some(apply_cw_style(&val_str, cfg))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{ContextWindowConfig, CshipConfig};
    use crate::context::{Context, ContextWindow, CurrentUsage};

    fn ctx_full() -> Context {
        Context {
            exceeds_200k_tokens: Some(false),
            context_window: Some(ContextWindow {
                used_percentage: Some(35.0),
                remaining_percentage: Some(65.0),
                context_window_size: Some(200000),
                total_input_tokens: Some(15234),
                total_output_tokens: Some(4521),
                current_usage: Some(CurrentUsage {
                    input_tokens: Some(8500),
                    output_tokens: Some(1200),
                    cache_creation_input_tokens: Some(5000),
                    cache_read_input_tokens: Some(2000),
                }),
            }),
            ..Default::default()
        }
    }

    #[test]
    fn test_used_percentage_renders_as_integer_no_percent_sign() {
        let ctx = ctx_full();
        assert_eq!(
            render_used_percentage(&ctx, &CshipConfig::default()),
            Some("35".to_string())
        );
    }

    #[test]
    fn test_remaining_percentage_renders_as_integer() {
        let ctx = ctx_full();
        assert_eq!(
            render_remaining_percentage(&ctx, &CshipConfig::default()),
            Some("65".to_string())
        );
    }

    #[test]
    fn test_size_reads_context_window_size_field() {
        let ctx = ctx_full();
        assert_eq!(
            render_size(&ctx, &CshipConfig::default()),
            Some("200000".to_string())
        );
    }

    #[test]
    fn test_total_input_tokens() {
        let ctx = ctx_full();
        assert_eq!(
            render_total_input_tokens(&ctx, &CshipConfig::default()),
            Some("15234".to_string())
        );
    }

    #[test]
    fn test_total_output_tokens() {
        let ctx = ctx_full();
        assert_eq!(
            render_total_output_tokens(&ctx, &CshipConfig::default()),
            Some("4521".to_string())
        );
    }

    #[test]
    fn test_exceeds_200k_false_returns_none_no_warn() {
        let ctx = ctx_full(); // exceeds_200k_tokens = false
        assert_eq!(render_exceeds_200k(&ctx, &CshipConfig::default()), None);
    }

    #[test]
    fn test_exceeds_200k_absent_treated_as_false() {
        let ctx = Context::default(); // exceeds_200k_tokens = None
        assert_eq!(render_exceeds_200k(&ctx, &CshipConfig::default()), None);
    }

    #[test]
    fn test_exceeds_200k_true_renders_default_symbol() {
        let ctx = Context {
            exceeds_200k_tokens: Some(true),
            ..Default::default()
        };
        let result = render_exceeds_200k(&ctx, &CshipConfig::default());
        assert_eq!(result, Some(">200k".to_string()));
    }

    #[test]
    fn test_current_usage_input_tokens() {
        let ctx = ctx_full();
        assert_eq!(
            render_current_usage_input_tokens(&ctx, &CshipConfig::default()),
            Some("8500".to_string())
        );
    }

    #[test]
    fn test_current_usage_output_tokens() {
        let ctx = ctx_full();
        assert_eq!(
            render_current_usage_output_tokens(&ctx, &CshipConfig::default()),
            Some("1200".to_string())
        );
    }

    #[test]
    fn test_current_usage_cache_creation_tokens() {
        let ctx = ctx_full();
        assert_eq!(
            render_current_usage_cache_creation_input_tokens(&ctx, &CshipConfig::default()),
            Some("5000".to_string())
        );
    }

    #[test]
    fn test_current_usage_cache_read_tokens() {
        let ctx = ctx_full();
        assert_eq!(
            render_current_usage_cache_read_input_tokens(&ctx, &CshipConfig::default()),
            Some("2000".to_string())
        );
    }

    #[test]
    fn test_exceeds_200k_true_renders_custom_symbol() {
        let ctx = Context {
            exceeds_200k_tokens: Some(true),
            ..Default::default()
        };
        let cfg = CshipConfig {
            context_window: Some(ContextWindowConfig {
                symbol: Some("⚠".to_string()),
                ..Default::default()
            }),
            ..Default::default()
        };
        let result = render_exceeds_200k(&ctx, &cfg);
        assert_eq!(result, Some("⚠".to_string()));
    }

    #[test]
    fn test_disabled_flag_suppresses_all_renders() {
        let ctx = ctx_full();
        let cfg = CshipConfig {
            context_window: Some(ContextWindowConfig {
                disabled: Some(true),
                ..Default::default()
            }),
            ..Default::default()
        };
        assert_eq!(render_used_percentage(&ctx, &cfg), None);
        assert_eq!(render_size(&ctx, &cfg), None);
        assert_eq!(render_exceeds_200k(&ctx, &cfg), None);
    }

    #[test]
    fn test_absent_context_window_returns_none() {
        let ctx = Context::default(); // no context_window
        assert_eq!(render_used_percentage(&ctx, &CshipConfig::default()), None);
        assert_eq!(render_size(&ctx, &CshipConfig::default()), None);
        assert_eq!(
            render_total_input_tokens(&ctx, &CshipConfig::default()),
            None
        );
    }
}
