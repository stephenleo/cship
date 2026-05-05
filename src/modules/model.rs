/// Resolve the effective style for the current model, preferring per-family styles over the
/// base `style`. Family is detected from `model.id` (fallback: `display_name`) via substring.
fn resolve_model_style<'a>(
    ctx: &'a crate::context::Context,
    model_cfg: Option<&'a crate::config::ModelConfig>,
) -> Option<&'a str> {
    let cfg = model_cfg?;
    let key = ctx
        .model
        .as_ref()
        .and_then(|m| m.id.as_deref().or(m.display_name.as_deref()))
        .map(|s| s.to_lowercase())
        .unwrap_or_default();
    let family = if key.contains("haiku") {
        cfg.haiku_style.as_deref()
    } else if key.contains("sonnet") {
        cfg.sonnet_style.as_deref()
    } else if key.contains("opus") {
        cfg.opus_style.as_deref()
    } else {
        None
    };
    family.or(cfg.style.as_deref())
}

/// Render the `[cship.model]` module.
///
/// Output format: `{symbol}{display_name}` with optional ANSI style applied.
/// Returns `None` if `disabled = true` or if `model.display_name` is absent.
///
/// Source: epics.md#Story 1.4, architecture.md#Module System Architecture
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

    let symbol = model_cfg.and_then(|m| m.symbol.as_deref());
    let style = resolve_model_style(ctx, model_cfg);

    // Format string takes priority if configured (AC1–4)
    if let Some(fmt) = model_cfg.and_then(|m| m.format.as_deref()) {
        return crate::format::apply_module_format(fmt, Some(display_name), symbol, style);
    }

    // Default behavior — unchanged (AC5)
    let symbol_str = symbol.unwrap_or("");
    let content = format!("{symbol_str}{display_name}");
    Some(crate::ansi::apply_style(&content, style))
}

/// Renders `$cship.model.display_name` — explicit sub-field alias for `render`.
pub fn render_display_name(
    ctx: &crate::context::Context,
    cfg: &crate::config::CshipConfig,
) -> Option<String> {
    render(ctx, cfg)
}

/// Renders `$cship.model.id` — raw model ID string (e.g., "claude-opus-4-6").
pub fn render_id(
    ctx: &crate::context::Context,
    cfg: &crate::config::CshipConfig,
) -> Option<String> {
    let model_cfg = cfg.model.as_ref();
    if model_cfg.and_then(|m| m.disabled).unwrap_or(false) {
        return None;
    }
    let id = match ctx.model.as_ref().and_then(|m| m.id.as_deref()) {
        Some(id) => id,
        None => {
            tracing::warn!("cship.model: model.id is absent — skipping");
            return None;
        }
    };
    let symbol = model_cfg.and_then(|m| m.symbol.as_deref());
    let style = resolve_model_style(ctx, model_cfg);

    // Format string takes priority if configured (AC1–4)
    if let Some(fmt) = model_cfg.and_then(|m| m.format.as_deref()) {
        return crate::format::apply_module_format(fmt, Some(id), symbol, style);
    }

    // Default behavior — unchanged (AC5)
    let symbol_str = symbol.unwrap_or("");
    let content = format!("{symbol_str}{id}");
    Some(crate::ansi::apply_style(&content, style))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ansi;
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

    #[test]
    fn test_render_id_returns_model_id() {
        let ctx = Context {
            model: Some(Model {
                id: Some("claude-opus-4-6".to_string()),
                ..Default::default()
            }),
            ..Default::default()
        };
        assert_eq!(
            render_id(&ctx, &CshipConfig::default()),
            Some("claude-opus-4-6".to_string())
        );
    }

    #[test]
    fn test_render_id_absent_returns_none() {
        let ctx = Context::default();
        assert_eq!(render_id(&ctx, &CshipConfig::default()), None);
    }

    #[test]
    fn test_render_id_disabled_returns_none() {
        let ctx = Context {
            model: Some(Model {
                id: Some("claude-opus-4-6".to_string()),
                ..Default::default()
            }),
            ..Default::default()
        };
        let cfg = CshipConfig {
            model: Some(ModelConfig {
                disabled: Some(true),
                ..Default::default()
            }),
            ..Default::default()
        };
        assert_eq!(render_id(&ctx, &cfg), None);
    }

    fn ctx_with_id(id: &str, display_name: &str) -> Context {
        Context {
            model: Some(Model {
                id: Some(id.to_string()),
                display_name: Some(display_name.to_string()),
            }),
            ..Default::default()
        }
    }

    fn all_family_cfg(haiku: &str, sonnet: &str, opus: &str, base: &str) -> CshipConfig {
        CshipConfig {
            model: Some(ModelConfig {
                haiku_style: Some(haiku.to_string()),
                sonnet_style: Some(sonnet.to_string()),
                opus_style: Some(opus.to_string()),
                style: Some(base.to_string()),
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    #[test]
    fn test_haiku_style_applied() {
        let ctx = ctx_with_id("claude-haiku-4-5", "Haiku");
        let cfg = all_family_cfg("dim cyan", "bold blue", "bold magenta", "bold red");
        assert_eq!(
            render(&ctx, &cfg).unwrap(),
            ansi::apply_style("Haiku", Some("dim cyan")),
        );
    }

    #[test]
    fn test_sonnet_style_applied() {
        let ctx = ctx_with_id("claude-sonnet-4-6", "Sonnet");
        let cfg = all_family_cfg("dim cyan", "bold blue", "bold magenta", "bold red");
        assert_eq!(
            render(&ctx, &cfg).unwrap(),
            ansi::apply_style("Sonnet", Some("bold blue")),
        );
    }

    #[test]
    fn test_opus_style_applied() {
        let ctx = ctx_with_id("claude-opus-4-7", "Opus");
        let cfg = all_family_cfg("dim cyan", "bold blue", "bold magenta", "bold red");
        assert_eq!(
            render(&ctx, &cfg).unwrap(),
            ansi::apply_style("Opus", Some("bold magenta")),
        );
    }

    #[test]
    fn test_family_style_falls_back_to_base_style() {
        // haiku_style not set — should use base style, not any other family style
        let ctx = ctx_with_id("claude-haiku-4-5", "Haiku");
        let cfg = CshipConfig {
            model: Some(ModelConfig {
                style: Some("bold green".to_string()),
                sonnet_style: Some("bold blue".to_string()),
                opus_style: Some("bold magenta".to_string()),
                ..Default::default()
            }),
            ..Default::default()
        };
        assert_eq!(
            render(&ctx, &cfg).unwrap(),
            ansi::apply_style("Haiku", Some("bold green")),
        );
    }

    #[test]
    fn test_unknown_model_id_uses_base_style() {
        let ctx = Context {
            model: Some(Model {
                id: Some("claude-future-99".to_string()),
                display_name: Some("Future".to_string()),
            }),
            ..Default::default()
        };
        let cfg = all_family_cfg("dim cyan", "bold blue", "bold magenta", "bold red");
        assert_eq!(
            render(&ctx, &cfg).unwrap(),
            ansi::apply_style("Future", Some("bold red")),
        );
    }

    #[test]
    fn test_family_match_via_display_name_when_id_absent() {
        let ctx = Context {
            model: Some(Model {
                id: None,
                display_name: Some("Opus".to_string()),
            }),
            ..Default::default()
        };
        let cfg = all_family_cfg("dim cyan", "bold blue", "bold magenta", "bold red");
        assert_eq!(
            render(&ctx, &cfg).unwrap(),
            ansi::apply_style("Opus", Some("bold magenta")),
        );
    }

    #[test]
    fn test_render_id_honors_per_model_style() {
        let ctx = ctx_with_id("claude-opus-4-7", "Opus");
        let cfg = all_family_cfg("dim cyan", "bold blue", "bold magenta", "bold red");
        assert_eq!(
            render_id(&ctx, &cfg).unwrap(),
            ansi::apply_style("claude-opus-4-7", Some("bold magenta")),
        );
    }
}
