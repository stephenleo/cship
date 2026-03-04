//! `cship explain` subcommand — shows each native module's rendered value and config source.

const SAMPLE_CONTEXT: &str = include_str!("sample_context.json");
const SAMPLE_CONTEXT_PATH: &str = ".config/cship/sample-context.json";

/// Run the explain subcommand and return the formatted output as a String.
/// `main.rs` is the sole stdout writer — this function only builds the string.
pub fn run(config_override: Option<&std::path::Path>) -> String {
    let ctx = load_context();
    let workspace_dir = ctx
        .workspace
        .as_ref()
        .and_then(|w| w.current_dir.as_deref());
    let result = crate::config::load_with_source(config_override, workspace_dir);
    let cfg = result.config;
    let source = result.source;

    // Pre-compute module column width from actual names so long names never overflow.
    let mod_w = crate::modules::ALL_NATIVE_MODULES
        .iter()
        .map(|s| s.len())
        .max()
        .unwrap_or(40)
        + 1;
    const VAL_W: usize = 30;
    const CFG_W: usize = 22; // "[cship.context_window]" = 22 chars

    let mut lines = Vec::new();
    lines.push(format!("cship explain — using config: {source}"));
    lines.push(String::new());
    lines.push(format!(
        "{:<mod_w$} {:<VAL_W$} {}",
        "Module", "Value", "Config"
    ));
    lines.push("─".repeat(mod_w + 1 + VAL_W + 1 + CFG_W));

    for &module_name in crate::modules::ALL_NATIVE_MODULES {
        let value = crate::modules::render_module(module_name, &ctx, &cfg);
        let display_value = match &value {
            Some(s) => crate::ansi::strip_ansi(s),
            None => "(empty)".to_string(),
        };
        let config_col = config_section_for(module_name, &cfg);
        // Truncate display_value to VAL_W chars so long path values don't push Config column right.
        // Use char-aware counting to avoid splitting multi-byte characters (e.g. ░, █).
        let display_value = if display_value.chars().count() > VAL_W {
            let truncated: String = display_value.chars().take(VAL_W - 1).collect();
            format!("{truncated}…")
        } else {
            display_value
        };
        lines.push(format!(
            "{:<mod_w$} {:<VAL_W$} {}",
            module_name, display_value, config_col
        ));
    }

    lines.join("\n")
}

fn load_context() -> crate::context::Context {
    use std::io::IsTerminal;

    // 1. If stdin is not a TTY, read from stdin (same path as main render pipeline)
    if !std::io::stdin().is_terminal() {
        match crate::context::from_reader(std::io::stdin()) {
            Ok(ctx) => return ctx,
            Err(e) => {
                tracing::warn!(
                    "cship explain: failed to parse stdin JSON: {e} — falling back to sample context"
                );
            }
        }
    }

    // 2. Try ~/.config/cship/sample-context.json
    if let Ok(home) = std::env::var("HOME") {
        let sample_path = std::path::Path::new(&home).join(SAMPLE_CONTEXT_PATH);
        if let Ok(content) = std::fs::read_to_string(&sample_path)
            && let Ok(ctx) = serde_json::from_str(&content)
        {
            return ctx;
        }
    }

    // 3. Use embedded template (always succeeds — compile-time guarantee)
    serde_json::from_str(SAMPLE_CONTEXT)
        .expect("embedded sample_context.json must be valid — this is a compile-time guarantee")
}

fn config_section_for(module_name: &str, cfg: &crate::config::CshipConfig) -> &'static str {
    let top = module_name.strip_prefix("cship.").unwrap_or(module_name);
    let segment = top.split('.').next().unwrap_or(top);
    match segment {
        "model" if cfg.model.is_some() => "[cship.model]",
        "cost" if cfg.cost.is_some() => "[cship.cost]",
        "context_bar" if cfg.context_bar.is_some() => "[cship.context_bar]",
        "context_window" if cfg.context_window.is_some() => "[cship.context_window]",
        "vim" if cfg.vim.is_some() => "[cship.vim]",
        "agent" if cfg.agent.is_some() => "[cship.agent]",
        "cwd" | "session_id" | "transcript_path" | "version" | "output_style"
            if cfg.session.is_some() =>
        {
            "[cship.session]"
        }
        "workspace" if cfg.workspace.is_some() => "[cship.workspace]",
        _ => "(default)",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::CshipConfig;
    use crate::context::{Context, Model};

    #[test]
    fn test_run_returns_header_with_using_config() {
        let output = run(None);
        assert!(
            output.contains("using config:"),
            "expected 'using config:' in output: {output}"
        );
    }

    #[test]
    fn test_run_contains_all_module_names() {
        let output = run(None);
        assert!(
            output.contains("cship.model"),
            "expected 'cship.model' in output"
        );
        assert!(
            output.contains("cship.cost"),
            "expected 'cship.cost' in output"
        );
        assert!(
            output.contains("cship.context_bar"),
            "expected 'cship.context_bar' in output"
        );
        assert!(
            output.contains("cship.vim"),
            "expected 'cship.vim' in output"
        );
    }

    #[test]
    fn test_strip_ansi_removes_escape_codes() {
        let styled = "\x1b[1;32mSonnet\x1b[0m";
        assert_eq!(crate::ansi::strip_ansi(styled), "Sonnet");
    }

    #[test]
    fn test_strip_ansi_leaves_plain_text_unchanged() {
        assert_eq!(crate::ansi::strip_ansi("plain text"), "plain text");
    }

    #[test]
    fn test_config_section_for_model_with_config() {
        let mut cfg = CshipConfig::default();
        cfg.model = Some(crate::config::ModelConfig::default());
        assert_eq!(config_section_for("cship.model", &cfg), "[cship.model]");
    }

    #[test]
    fn test_config_section_for_model_without_config() {
        let cfg = CshipConfig::default();
        assert_eq!(config_section_for("cship.model", &cfg), "(default)");
    }

    #[test]
    fn test_load_context_embedded_fallback_is_valid() {
        let ctx: Result<Context, _> = serde_json::from_str(SAMPLE_CONTEXT);
        assert!(
            ctx.is_ok(),
            "embedded sample_context.json must parse as Context"
        );
    }

    #[test]
    fn test_run_with_config_override_does_not_panic() {
        let bad_path = Some(std::path::PathBuf::from("/nonexistent/path.toml"));
        let output = run(bad_path.as_deref());
        assert!(output.contains("using config:"));
    }

    #[test]
    fn test_load_with_source_respects_workspace_dir() {
        // Verify that load_with_source accepts workspace_dir parameter (H1 fix)
        let result = crate::config::load_with_source(None, Some("/nonexistent/dir"));
        // Should fall through to global or default without panicking
        assert!(
            matches!(
                result.source,
                crate::config::ConfigSource::Global(_) | crate::config::ConfigSource::Default
            ),
            "expected Global or Default source for nonexistent workspace dir"
        );
    }

    #[test]
    fn test_run_output_shows_sample_model_value() {
        // The embedded sample_context.json has model.display_name = "Sonnet"
        let ctx: Context = serde_json::from_str(SAMPLE_CONTEXT).unwrap();
        let cfg = CshipConfig::default();
        let value = crate::modules::render_module("cship.model", &ctx, &cfg);
        assert!(value.is_some());
        let stripped = crate::ansi::strip_ansi(&value.unwrap());
        assert!(
            stripped.contains("Sonnet"),
            "expected Sonnet in: {stripped}"
        );
    }

    #[test]
    fn test_run_with_valid_context_shows_model_in_explain_column() {
        let model_ctx = Context {
            model: Some(Model {
                display_name: Some("TestModel".to_string()),
                ..Default::default()
            }),
            ..Default::default()
        };
        let cfg = CshipConfig::default();
        let value = crate::modules::render_module("cship.model", &model_ctx, &cfg);
        let stripped = crate::ansi::strip_ansi(&value.unwrap_or_default());
        assert!(stripped.contains("TestModel"));
    }
}
