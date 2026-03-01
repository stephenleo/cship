pub mod model;

/// Static dispatch registry — the ONLY file modified when adding a new native module.
/// [Source: architecture.md#Module System Architecture]
pub fn render_module(
    name: &str,
    ctx: &crate::context::Context,
    cfg: &crate::config::CshipConfig,
) -> Option<String> {
    match name {
        "cship.model" => model::render(ctx, cfg),
        other => {
            tracing::warn!("cship: unknown native module '{other}' — skipping");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::CshipConfig;
    use crate::context::{Context, Model};

    #[test]
    fn test_dispatch_to_model_module_with_display_name_returns_some() {
        let ctx = Context {
            model: Some(Model {
                display_name: Some("Sonnet".to_string()),
                ..Default::default()
            }),
            ..Default::default()
        };
        let cfg = CshipConfig::default();
        let result = render_module("cship.model", &ctx, &cfg);
        assert!(result.is_some());
        assert!(result.unwrap().contains("Sonnet"));
    }

    #[test]
    fn test_unknown_module_name_returns_none() {
        let ctx = Context::default();
        let cfg = CshipConfig::default();
        assert!(render_module("cship.unknown_future_module", &ctx, &cfg).is_none());
    }
}
