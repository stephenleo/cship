//! `cship explain` subcommand — shows each native module's rendered value and config source.

const SAMPLE_CONTEXT: &str = include_str!("sample_context.json");
const SAMPLE_CONTEXT_PATH: &str = ".config/cship/sample-context.json";

/// Run the explain subcommand and return the formatted output as a String.
/// `main.rs` is the sole stdout writer — this function only builds the string.
pub fn run(config_override: Option<&std::path::Path>) -> String {
    let (ctx, creation_notes) = load_context();
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

    let mut none_modules: Vec<(&str, String, String)> = Vec::new();

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

        let display_name = if value.is_none() {
            let (error, remediation) = error_hint_for(module_name, &ctx, &cfg);
            none_modules.push((module_name, error, remediation));
            format!("⚠ {module_name}")
        } else {
            module_name.to_string()
        };

        lines.push(format!(
            "{:<mod_w$} {:<VAL_W$} {}",
            display_name, display_value, config_col
        ));
    }

    // Hints section for modules that returned None
    if !none_modules.is_empty() {
        lines.push(String::new());
        lines.push(format!(
            "─── Hints for modules showing (empty) {}",
            "─".repeat(34)
        ));
        for (name, error, remediation) in &none_modules {
            lines.push(String::new());
            lines.push(format!("⚠ {name}"));
            lines.push(format!("  Error: {error}"));
            lines.push(format!("  Hint:  {remediation}"));
        }
    }

    // Sample file creation notes
    if !creation_notes.is_empty() {
        lines.push(String::new());
        lines.push(format!("─── Note {}", "─".repeat(59)));
        lines.push(String::new());
        for note in &creation_notes {
            lines.push(format!("i  {note}"));
        }
    }

    lines.join("\n")
}

fn load_context() -> (crate::context::Context, Vec<String>) {
    use std::io::IsTerminal;
    let mut notes = Vec::new();

    // 1. If stdin is not a TTY, read from stdin (same path as main render pipeline)
    if !std::io::stdin().is_terminal() {
        match crate::context::from_reader(std::io::stdin()) {
            Ok(ctx) => return (ctx, notes),
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
        if sample_path.exists() {
            if let Ok(content) = std::fs::read_to_string(&sample_path)
                && let Ok(ctx) = serde_json::from_str(&content)
            {
                return (ctx, notes);
            }
        } else {
            // File does not exist — create it from the embedded template
            if let Some(parent) = sample_path.parent()
                && std::fs::create_dir_all(parent).is_ok()
                && std::fs::write(&sample_path, SAMPLE_CONTEXT).is_ok()
            {
                notes.push(format!(
                    "Created sample context at {}. Edit it to test different threshold scenarios.",
                    sample_path.display()
                ));
            }
        }
    }

    // 3. Use embedded template (always succeeds — compile-time guarantee)
    let ctx = serde_json::from_str(SAMPLE_CONTEXT)
        .expect("embedded sample_context.json must be valid — this is a compile-time guarantee");
    (ctx, notes)
}

fn is_disabled(name: &str, cfg: &crate::config::CshipConfig) -> bool {
    let top = name.strip_prefix("cship.").unwrap_or(name);
    let segment = top.split('.').next().unwrap_or(top);
    match segment {
        "model" => cfg.model.as_ref().and_then(|m| m.disabled).unwrap_or(false),
        "cost" => cfg.cost.as_ref().and_then(|m| m.disabled).unwrap_or(false),
        "context_bar" => cfg
            .context_bar
            .as_ref()
            .and_then(|m| m.disabled)
            .unwrap_or(false),
        "context_window" => cfg
            .context_window
            .as_ref()
            .and_then(|m| m.disabled)
            .unwrap_or(false),
        "vim" => cfg.vim.as_ref().and_then(|m| m.disabled).unwrap_or(false),
        "agent" => cfg.agent.as_ref().and_then(|m| m.disabled).unwrap_or(false),
        "cwd" | "session_id" | "transcript_path" | "version" | "output_style" => cfg
            .session
            .as_ref()
            .and_then(|m| m.disabled)
            .unwrap_or(false),
        "workspace" => cfg
            .workspace
            .as_ref()
            .and_then(|m| m.disabled)
            .unwrap_or(false),
        "usage_limits" => cfg
            .usage_limits
            .as_ref()
            .and_then(|m| m.disabled)
            .unwrap_or(false),
        _ => false,
    }
}

fn error_hint_for(
    name: &str,
    _ctx: &crate::context::Context,
    cfg: &crate::config::CshipConfig,
) -> (String, String) {
    let top = name.strip_prefix("cship.").unwrap_or(name);
    let segment = top.split('.').next().unwrap_or(top);
    if is_disabled(name, cfg) {
        return (
            "module explicitly disabled in config".into(),
            format!(
                "Remove `disabled = true` from the [cship.{segment}] section in starship.toml."
            ),
        );
    }
    match segment {
        "model" => (
            "model data absent from Claude Code context".into(),
            "Ensure Claude Code is running and cship is invoked via the \"statusline\" key in ~/.claude/settings.json.".into(),
        ),
        "cost" => (
            "cost data absent from Claude Code context".into(),
            "Ensure Claude Code is running and cship is invoked via the \"statusline\" key in ~/.claude/settings.json.".into(),
        ),
        "context_bar" | "context_window" => (
            "context_window data absent from Claude Code context (may be absent early in a session)".into(),
            "Ensure Claude Code is running. Context window data appears after the first API response.".into(),
        ),
        "vim" => (
            "vim mode data absent — vim mode may not be enabled".into(),
            "Enable vim mode: add \"vim.mode\": \"INSERT\" to ~/.claude/settings.json.".into(),
        ),
        "agent" => (
            "agent data absent — no agent session active".into(),
            "Agent data is only present during agent sessions. Start an agent session or use the --agent flag.".into(),
        ),
        "cwd" | "session_id" | "transcript_path" | "version" | "output_style" => (
            "session field absent from Claude Code context".into(),
            "Ensure Claude Code is running and cship is invoked via the \"statusline\" key in ~/.claude/settings.json.".into(),
        ),
        "workspace" => (
            "workspace data absent from Claude Code context".into(),
            "Ensure Claude Code is running and cship is invoked via the \"statusline\" key in ~/.claude/settings.json.".into(),
        ),
        "usage_limits" => (
            "OAuth token retrieval failed — credential store unavailable or credentials not found".into(),
            "Authenticate in Claude Code first. On Linux, ensure libsecret-tools is installed: sudo apt install libsecret-tools".into(),
        ),
        _ => (
            "module returned no value".into(),
            "Check cship configuration and ensure Claude Code is running.".into(),
        ),
    }
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
        "usage_limits" if cfg.usage_limits.is_some() => "[cship.usage_limits]",
        _ => "(default)",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{CshipConfig, ModelConfig};
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

    #[test]
    fn test_run_shows_warning_indicator_for_none_module() {
        // run() loads the embedded sample context — cship.context_window.exceeds_200k
        // returns None because sample context value is below 200k threshold.
        let output = run(None);
        assert!(
            output.contains("⚠ cship.context_window.exceeds_200k"),
            "expected '⚠ cship.context_window.exceeds_200k' in output: {output}"
        );
    }

    #[test]
    fn test_run_shows_hint_section_for_none_module() {
        let output = run(None);
        assert!(
            output.contains("Hints for modules"),
            "expected hints section in output: {output}"
        );
    }

    #[test]
    fn test_run_shows_error_reason_in_hint() {
        let output = run(None);
        // model data absent hint should appear since sample context has model data,
        // but other modules like vim will be absent
        assert!(
            output.contains("absent"),
            "expected 'absent' in hint output: {output}"
        );
    }

    #[test]
    fn test_error_hint_for_disabled_module_returns_disabled_text() {
        let mut cfg = CshipConfig::default();
        cfg.model = Some(ModelConfig {
            disabled: Some(true),
            ..Default::default()
        });
        let ctx = Context {
            model: Some(Model {
                display_name: Some("Sonnet".to_string()),
                ..Default::default()
            }),
            ..Default::default()
        };
        // Even with model data present, disabled=true makes it return None
        let value = crate::modules::render_module("cship.model", &ctx, &cfg);
        assert!(value.is_none(), "disabled module must return None");
        let (error, remediation) = error_hint_for("cship.model", &ctx, &cfg);
        assert!(
            error.contains("disabled"),
            "expected 'disabled' in error hint: {error}"
        );
        assert!(
            remediation.contains("[cship.model]"),
            "expected specific section '[cship.model]' in remediation: {remediation}"
        );
    }

    #[test]
    fn test_is_disabled_returns_true_for_disabled_model() {
        let mut cfg = CshipConfig::default();
        cfg.model = Some(ModelConfig {
            disabled: Some(true),
            ..Default::default()
        });
        assert!(
            is_disabled("cship.model", &cfg),
            "is_disabled should return true when model.disabled = Some(true)"
        );
    }

    #[test]
    fn test_is_disabled_returns_false_for_enabled_model() {
        let cfg = CshipConfig::default();
        assert!(
            !is_disabled("cship.model", &cfg),
            "is_disabled should return false when model config is absent"
        );
    }
}
