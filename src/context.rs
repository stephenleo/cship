use serde::Deserialize;
use std::io::Read;

/// Typed representation of the complete Claude Code session JSON payload.
/// All fields are `Option<T>` because Claude Code may omit any field depending on
/// session state, mode flags, and version. `deny_unknown_fields` is intentionally
/// NOT used — future Claude Code versions may add fields.
#[derive(Debug, Deserialize, Default)]
pub struct Context {
    pub cwd: Option<String>,
    pub session_id: Option<String>,
    pub transcript_path: Option<String>,
    pub version: Option<String>,
    /// Top-level boolean (NOT inside context_window).
    pub exceeds_200k_tokens: Option<bool>,
    pub model: Option<Model>,
    pub workspace: Option<Workspace>,
    pub output_style: Option<OutputStyle>,
    pub cost: Option<Cost>,
    /// May be entirely absent in some Claude Code versions (confirmed absent in v2.0.31).
    pub context_window: Option<ContextWindow>,
    /// Absent unless vim mode is enabled.
    pub vim: Option<Vim>,
    /// Absent unless --agent flag or agent settings are active.
    pub agent: Option<Agent>,
}

#[derive(Debug, Deserialize, Default)]
pub struct Model {
    pub id: Option<String>,
    pub display_name: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
pub struct Workspace {
    pub current_dir: Option<String>,
    pub project_dir: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
pub struct OutputStyle {
    pub name: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
pub struct Cost {
    pub total_cost_usd: Option<f64>,
    pub total_duration_ms: Option<u64>,
    pub total_api_duration_ms: Option<u64>,
    pub total_lines_added: Option<i64>,
    pub total_lines_removed: Option<i64>,
}

#[derive(Debug, Deserialize, Default)]
pub struct ContextWindow {
    pub total_input_tokens: Option<u64>,
    pub total_output_tokens: Option<u64>,
    pub context_window_size: Option<u64>,
    /// May be null early in a session (before first API response).
    pub used_percentage: Option<f64>,
    /// May be null early in a session (before first API response).
    pub remaining_percentage: Option<f64>,
    /// Null before the first API call in a session.
    pub current_usage: Option<CurrentUsage>,
}

#[derive(Debug, Deserialize, Default)]
pub struct CurrentUsage {
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub cache_creation_input_tokens: Option<u64>,
    pub cache_read_input_tokens: Option<u64>,
}

#[derive(Debug, Deserialize, Default)]
pub struct Vim {
    /// "NORMAL" or "INSERT"
    pub mode: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
pub struct Agent {
    pub name: Option<String>,
}

/// Deserialize Claude Code session JSON from any reader.
/// Uses exactly one `serde_json::from_str` call — no per-field parsing.
///
/// Returns an error if input is empty or JSON is malformed.
pub fn from_reader(mut reader: impl Read) -> anyhow::Result<Context> {
    let mut input = String::new();
    reader.read_to_string(&mut input)?;
    if input.trim().is_empty() {
        anyhow::bail!("empty stdin: no Claude Code session JSON received");
    }
    let ctx = serde_json::from_str(&input)?;
    Ok(ctx)
}

/// Read stdin to end, then deserialize as Claude Code session JSON.
/// This is the ONLY place in the entire codebase that reads from stdin.
///
/// Returns an error if stdin is empty or JSON is malformed.
pub fn from_stdin() -> anyhow::Result<Context> {
    from_reader(std::io::stdin())
}

#[cfg(test)]
mod tests {
    use super::*;

    const FULL_JSON: &str = include_str!("../tests/fixtures/sample_input_full.json");
    const MINIMAL_JSON: &str = include_str!("../tests/fixtures/sample_input_minimal.json");

    #[test]
    fn test_deserialize_full_payload() {
        let ctx: Context = serde_json::from_str(FULL_JSON).unwrap();
        // Top-level fields
        assert_eq!(ctx.cwd.as_deref(), Some("/home/user/projects/myapp"));
        assert_eq!(ctx.session_id.as_deref(), Some("test-session-id"));
        assert_eq!(
            ctx.transcript_path.as_deref(),
            Some("/home/user/.claude/projects/myapp/transcript.jsonl")
        );
        assert_eq!(ctx.version.as_deref(), Some("1.0.80"));
        assert_eq!(ctx.exceeds_200k_tokens, Some(false));
        // Model
        let model = ctx.model.as_ref().unwrap();
        assert_eq!(model.id.as_deref(), Some("claude-opus-4-6"));
        assert_eq!(model.display_name.as_deref(), Some("Opus"));
        // Workspace
        let ws = ctx.workspace.as_ref().unwrap();
        assert_eq!(ws.current_dir.as_deref(), Some("/home/user/projects/myapp"));
        assert_eq!(ws.project_dir.as_deref(), Some("/home/user/projects/myapp"));
        // OutputStyle
        assert_eq!(
            ctx.output_style.as_ref().unwrap().name.as_deref(),
            Some("default")
        );
        // Cost — all sub-fields
        let cost = ctx.cost.as_ref().unwrap();
        assert_eq!(cost.total_cost_usd, Some(0.01234));
        assert_eq!(cost.total_duration_ms, Some(45000));
        assert_eq!(cost.total_api_duration_ms, Some(2300));
        assert_eq!(cost.total_lines_added, Some(156));
        assert_eq!(cost.total_lines_removed, Some(23));
        // ContextWindow — all sub-fields
        let cw = ctx.context_window.as_ref().unwrap();
        assert_eq!(cw.total_input_tokens, Some(15234));
        assert_eq!(cw.total_output_tokens, Some(4521));
        assert_eq!(cw.context_window_size, Some(200000));
        assert_eq!(cw.used_percentage, Some(8.0));
        assert_eq!(cw.remaining_percentage, Some(92.0));
        let cu = cw.current_usage.as_ref().unwrap();
        assert_eq!(cu.input_tokens, Some(8500));
        assert_eq!(cu.output_tokens, Some(1200));
        assert_eq!(cu.cache_creation_input_tokens, Some(5000));
        assert_eq!(cu.cache_read_input_tokens, Some(2000));
        // Vim
        assert_eq!(ctx.vim.as_ref().unwrap().mode.as_deref(), Some("NORMAL"));
        // Agent
        assert_eq!(
            ctx.agent.as_ref().unwrap().name.as_deref(),
            Some("security-reviewer")
        );
    }

    #[test]
    fn test_deserialize_minimal_payload() {
        let ctx: Context = serde_json::from_str(MINIMAL_JSON).unwrap();
        assert!(ctx.vim.is_none());
        assert!(ctx.agent.is_none());
        assert!(ctx.context_window.is_none());
        assert_eq!(ctx.cost.as_ref().unwrap().total_cost_usd, Some(0.53));
    }

    #[test]
    fn test_unknown_fields_ignored() {
        let json = r#"{"session_id":"abc","cwd":"/","transcript_path":"/t","version":"99.0","exceeds_200k_tokens":false,"unknown_future_field":true,"nested_unknown":{"key":"value"},"model":{"id":"test","display_name":"Test"},"workspace":{"current_dir":"/","project_dir":"/"},"output_style":{"name":"default"},"cost":{"total_cost_usd":0.0}}"#;
        let ctx: Context = serde_json::from_str(json).unwrap();
        assert_eq!(ctx.session_id.as_deref(), Some("abc"));
    }

    #[test]
    fn test_malformed_json_returns_error() {
        let result: Result<Context, _> = serde_json::from_str("not valid json {{{");
        assert!(result.is_err());
    }

    #[test]
    fn test_empty_reader_returns_error() {
        let result = from_reader("".as_bytes());
        assert!(result.is_err());
    }

    #[test]
    fn test_whitespace_only_reader_returns_error() {
        let result = from_reader("   \n\t  ".as_bytes());
        assert!(result.is_err());
    }
}
