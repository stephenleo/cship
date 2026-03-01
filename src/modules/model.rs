/// Render the `[cship.model]` module.
///
/// Output format: `{symbol}{display_name}` with optional ANSI style applied.
/// Returns `None` if `disabled = true` or if `model.display_name` is absent.
///
/// [Source: epics.md#Story 1.4, architecture.md#Module System Architecture]
pub fn render(ctx: &crate::context::Context, cfg: &crate::config::CshipConfig) -> Option<String> {
    let model_cfg = cfg.model.as_ref();

    // Respect disabled flag — return None silently (no warning needed)
    if model_cfg.and_then(|m| m.disabled).unwrap_or(false) {
        return None;
    }

    // Explicit check with tracing::warn! per AC8 — do NOT use `?` here
    let display_name = match ctx.model.as_ref().and_then(|m| m.display_name.as_deref()) {
        Some(name) => name,
        None => {
            tracing::warn!("cship.model: model.display_name is absent — skipping");
            return None;
        }
    };

    let symbol = model_cfg.and_then(|m| m.symbol.as_deref()).unwrap_or("");

    let content = format!("{symbol}{display_name}");
    let style = model_cfg.and_then(|m| m.style.as_deref());

    Some(crate::ansi::apply_style(&content, style))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{CshipConfig, ModelConfig};
    use crate::context::{Context, Model};

    fn ctx_with_model(display_name: &str) -> Context {
        Context {
            model: Some(Model {
                display_name: Some(display_name.to_string()),
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    #[test]
    fn test_renders_display_name() {
        let ctx = ctx_with_model("Sonnet");
        let result = render(&ctx, &CshipConfig::default());
        assert_eq!(result, Some("Sonnet".to_string()));
    }

    #[test]
    fn test_symbol_prepended() {
        let ctx = ctx_with_model("Sonnet");
        let cfg = CshipConfig {
            model: Some(ModelConfig {
                symbol: Some("★ ".to_string()),
                ..Default::default()
            }),
            ..Default::default()
        };
        let result = render(&ctx, &cfg).unwrap();
        assert!(
            result.starts_with("★ "),
            "expected '★ ' prefix in: {result:?}"
        );
        assert!(result.contains("Sonnet"));
    }

    #[test]
    fn test_disabled_returns_none() {
        let ctx = ctx_with_model("Sonnet");
        let cfg = CshipConfig {
            model: Some(ModelConfig {
                disabled: Some(true),
                ..Default::default()
            }),
            ..Default::default()
        };
        assert_eq!(render(&ctx, &cfg), None);
    }

    #[test]
    fn test_none_display_name_returns_none() {
        let ctx = Context {
            model: Some(Model {
                display_name: None,
                ..Default::default()
            }),
            ..Default::default()
        };
        assert_eq!(render(&ctx, &CshipConfig::default()), None);
    }

    #[test]
    fn test_no_model_in_context_returns_none() {
        let ctx = Context::default();
        assert_eq!(render(&ctx, &CshipConfig::default()), None);
    }

    #[test]
    fn test_style_applied_produces_ansi_codes() {
        let ctx = ctx_with_model("Sonnet");
        let cfg = CshipConfig {
            model: Some(ModelConfig {
                style: Some("bold green".to_string()),
                ..Default::default()
            }),
            ..Default::default()
        };
        let result = render(&ctx, &cfg).unwrap();
        assert!(
            result.contains('\x1b'),
            "expected ANSI codes in: {result:?}"
        );
        assert!(result.contains("Sonnet"));
    }

    #[test]
    fn test_no_style_returns_plain_text() {
        let ctx = ctx_with_model("Sonnet");
        let result = render(&ctx, &CshipConfig::default()).unwrap();
        assert_eq!(result, "Sonnet");
    }
}
