/// Render the `[cship.vim]` module.
///
/// `$cship.vim` — convenience alias for `$cship.vim.mode`.
/// `$cship.vim.mode` — raw vim mode string (e.g., "NORMAL", "INSERT").
///
/// [Source: epics.md#Story 2.3, architecture.md#Module System Architecture]
use crate::config::CshipConfig;
use crate::context::Context;

/// Renders `$cship.vim` — convenience alias for vim mode.
pub fn render(ctx: &Context, cfg: &CshipConfig) -> Option<String> {
    render_mode(ctx, cfg)
}

/// Renders `$cship.vim.mode` — raw vim mode string with optional symbol and style.
pub fn render_mode(ctx: &Context, cfg: &CshipConfig) -> Option<String> {
    // Disabled check — SILENT (no warn, no log)
    if cfg.vim.as_ref().and_then(|v| v.disabled).unwrap_or(false) {
        return None;
    }

    // Extract value — WARN before returning None (AC3 requirement)
    let mode = match ctx.vim.as_ref().and_then(|v| v.mode.as_ref()) {
        Some(m) => m,
        None => {
            tracing::warn!("cship.vim: mode absent from context");
            return None;
        }
    };

    let vim_cfg = cfg.vim.as_ref();
    let raw_value: &str = mode;
    let symbol = vim_cfg.and_then(|v| v.symbol.as_deref());
    let style = vim_cfg.and_then(|v| v.style.as_deref());

    // Format string takes priority if configured (AC1–4)
    if let Some(fmt) = vim_cfg.and_then(|v| v.format.as_deref()) {
        return crate::format::apply_module_format(fmt, Some(raw_value), symbol, style);
    }

    // Default behavior — unchanged (AC5)
    let symbol_str = symbol.unwrap_or("");
    let content = format!("{symbol_str}{raw_value}");
    Some(crate::ansi::apply_style(&content, style))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{CshipConfig, VimConfig};
    use crate::context::{Context, Vim};

    fn ctx_with_vim(mode: &str) -> Context {
        Context {
            vim: Some(Vim {
                mode: Some(mode.to_string()),
            }),
            ..Default::default()
        }
    }

    #[test]
    fn test_vim_renders_mode_string() {
        let ctx = ctx_with_vim("NORMAL");
        let result = render(&ctx, &CshipConfig::default());
        assert_eq!(result, Some("NORMAL".to_string()));
    }

    #[test]
    fn test_vim_mode_alias_identical_to_render() {
        let ctx = ctx_with_vim("VISUAL");
        let r1 = render(&ctx, &CshipConfig::default());
        let r2 = render_mode(&ctx, &CshipConfig::default());
        assert_eq!(r1, r2);
    }

    #[test]
    fn test_vim_disabled_returns_none() {
        let ctx = ctx_with_vim("NORMAL");
        let cfg = CshipConfig {
            vim: Some(VimConfig {
                disabled: Some(true),
                ..Default::default()
            }),
            ..Default::default()
        };
        assert_eq!(render(&ctx, &cfg), None);
    }

    #[test]
    fn test_vim_absent_returns_none() {
        let ctx = Context::default(); // no vim field
        assert_eq!(render(&ctx, &CshipConfig::default()), None);
    }

    #[test]
    fn test_vim_applies_symbol_and_style() {
        let ctx = ctx_with_vim("INSERT");
        let cfg = CshipConfig {
            vim: Some(VimConfig {
                symbol: Some("✏ ".to_string()),
                style: Some("bold yellow".to_string()),
                ..Default::default()
            }),
            ..Default::default()
        };
        let result = render(&ctx, &cfg).unwrap();
        assert!(result.contains("INSERT"), "should contain mode: {result:?}");
        assert!(result.contains("✏ "), "should contain symbol: {result:?}");
        assert!(
            result.contains('\x1b'),
            "should contain ANSI codes: {result:?}"
        );
    }
}
