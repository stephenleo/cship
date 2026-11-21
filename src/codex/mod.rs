//! Codex CLI support: `cship codex-mcp`.
//!
//! Codex's experimental custom status line talks to a stdio MCP server instead
//! of piping JSON to a command. This module adapts cship to that contract
//! without touching the module interface:
//!
//! 1. [`snapshot`] maps the Codex thread snapshot onto cship's `Context`.
//! 2. The regular renderer produces ANSI-styled pieces.
//! 3. [`ui_tree`] converts those pieces into Codex's JSON UI tree.
//! 4. [`mcp`] serves it over newline-delimited JSON-RPC.
//!
//! Everything Codex-specific lives here so protocol changes stay contained.

pub mod mcp;
pub mod snapshot;
pub mod ui_tree;

use std::path::{Path, PathBuf};

/// Run the MCP server on stdin/stdout until Codex closes the pipe.
///
/// This is a CLI-action subcommand: it owns stdout for the JSON-RPC channel,
/// handed to it by `main.rs`. Diagnostics still go to stderr via `tracing`.
pub fn run(config_path: Option<&Path>) -> std::io::Result<()> {
    let server = mcp::Server::new(config_path.map(Path::to_path_buf));
    server.run(std::io::stdin().lock(), std::io::stdout().lock())
}

/// Stable stand-in for Claude Code's `transcript_path`, keyed by Codex thread id.
///
/// cship's caches (Starship passthrough, usage limits) live next to the
/// transcript. Codex has no transcript path, so this points at a per-thread
/// file under the user cache directory; the file itself is never created.
pub(crate) fn cache_key_path(thread_id: &str) -> Option<PathBuf> {
    let safe: String = thread_id
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .collect();
    if safe.is_empty() {
        return None;
    }
    let base = std::env::var_os("XDG_CACHE_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| crate::platform::home_dir().map(|h| h.join(".cache")))?;
    Some(
        base.join("cship")
            .join("codex")
            .join(format!("{safe}.jsonl")),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cache_key_path_strips_unsafe_characters() {
        let p = cache_key_path("../../etc/passwd").unwrap();
        assert_eq!(p.file_name().unwrap(), "etcpasswd.jsonl");
        assert!(p.parent().unwrap().ends_with("cship/codex"));
    }

    #[test]
    fn test_cache_key_path_empty_id_is_none() {
        assert!(cache_key_path("///").is_none());
    }
}
