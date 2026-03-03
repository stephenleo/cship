pub mod agent;
pub mod context_bar;
pub mod context_window;
pub mod cost;
pub mod model;
pub mod session;
pub mod vim;
pub mod workspace;

/// Static dispatch registry — the ONLY file modified when adding a new native module.
/// [Source: architecture.md#Module System Architecture]
pub fn render_module(
    name: &str,
    ctx: &crate::context::Context,
    cfg: &crate::config::CshipConfig,
) -> Option<String> {
    match name {
        "cship.model" => model::render(ctx, cfg),
        "cship.model.display_name" => model::render_display_name(ctx, cfg),
        "cship.model.id" => model::render_id(ctx, cfg),
        // Cost module — main alias and sub-fields
        "cship.cost" => cost::render(ctx, cfg),
        "cship.cost.total_cost_usd" => cost::render_total_cost_usd(ctx, cfg),
        "cship.cost.total_duration_ms" => cost::render_total_duration_ms(ctx, cfg),
        "cship.cost.total_api_duration_ms" => cost::render_total_api_duration_ms(ctx, cfg),
        "cship.cost.total_lines_added" => cost::render_total_lines_added(ctx, cfg),
        "cship.cost.total_lines_removed" => cost::render_total_lines_removed(ctx, cfg),
        // Context bar — progress bar with threshold styling
        "cship.context_bar" => context_bar::render(ctx, cfg),
        // Context window sub-fields
        "cship.context_window.used_percentage" => context_window::render_used_percentage(ctx, cfg),
        "cship.context_window.remaining_percentage" => {
            context_window::render_remaining_percentage(ctx, cfg)
        }
        "cship.context_window.size" => context_window::render_size(ctx, cfg),
        "cship.context_window.total_input_tokens" => {
            context_window::render_total_input_tokens(ctx, cfg)
        }
        "cship.context_window.total_output_tokens" => {
            context_window::render_total_output_tokens(ctx, cfg)
        }
        "cship.context_window.exceeds_200k" => context_window::render_exceeds_200k(ctx, cfg),
        "cship.context_window.current_usage.input_tokens" => {
            context_window::render_current_usage_input_tokens(ctx, cfg)
        }
        "cship.context_window.current_usage.output_tokens" => {
            context_window::render_current_usage_output_tokens(ctx, cfg)
        }
        "cship.context_window.current_usage.cache_creation_input_tokens" => {
            context_window::render_current_usage_cache_creation_input_tokens(ctx, cfg)
        }
        "cship.context_window.current_usage.cache_read_input_tokens" => {
            context_window::render_current_usage_cache_read_input_tokens(ctx, cfg)
        }
        // Vim module — mode display
        "cship.vim" => vim::render(ctx, cfg),
        "cship.vim.mode" => vim::render_mode(ctx, cfg),
        // Agent module — agent name display
        "cship.agent" => agent::render(ctx, cfg),
        "cship.agent.name" => agent::render_name(ctx, cfg),
        // Session identity modules
        "cship.cwd" => session::render_cwd(ctx, cfg),
        "cship.session_id" => session::render_session_id(ctx, cfg),
        "cship.transcript_path" => session::render_transcript_path(ctx, cfg),
        "cship.version" => session::render_version(ctx, cfg),
        "cship.output_style" => session::render_output_style(ctx, cfg),
        // Workspace modules
        "cship.workspace.current_dir" => workspace::render_current_dir(ctx, cfg),
        "cship.workspace.project_dir" => workspace::render_project_dir(ctx, cfg),
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
