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
//! Only [`run`] and [`setup`] (`cship codex-setup`) are public: the snapshot
//! and UI-tree shapes follow an experimental protocol and are not part of
//! cship's API.

mod log;
mod mcp;
mod setup;
mod snapshot;
mod ui_tree;

pub use setup::run as setup;

use std::path::{Path, PathBuf};
use std::time::Duration;

/// Per-thread cache files untouched for this long are removed at startup.
const CACHE_MAX_AGE: Duration = Duration::from_secs(7 * 24 * 60 * 60);

/// Run the MCP server on stdin/stdout until Codex closes the pipe.
///
/// This is a CLI-action subcommand: it owns stdout for the JSON-RPC channel,
/// handed to it by `main.rs`. Diagnostics go to stderr via `tracing`, through
/// a subscriber that suppresses the per-poll repeats (see [`log`]).
pub fn run(config_path: Option<&Path>) -> std::io::Result<()> {
    tracing::subscriber::with_default(log::subscriber(), || {
        prune_cache(CACHE_MAX_AGE);
        let server = mcp::Server::new(config_path.map(Path::to_path_buf));
        server.run(std::io::stdin().lock(), std::io::stdout().lock())
    })
}

/// Root of cship's Codex state: `$XDG_CACHE_HOME/cship/codex` or
/// `~/.cache/cship/codex`. Codex passes `HOME` to MCP servers but not
/// `XDG_CACHE_HOME` unless it is listed in the server's `env_vars`.
fn cache_root() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CACHE_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| crate::platform::home_dir().map(|h| h.join(".cache")))?;
    Some(base.join("cship").join("codex"))
}

/// Best-effort removal of cache files older than `max_age`. Each Codex thread
/// gets its own cache files, so without this they would accumulate forever.
fn prune_cache(max_age: Duration) {
    let Some(root) = cache_root() else {
        return;
    };
    prune_dir(&root.join("cship"), max_age);
}

fn prune_dir(dir: &Path, max_age: Duration) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let Ok(meta) = entry.metadata() else {
            continue;
        };
        let stale = meta
            .modified()
            .ok()
            .and_then(|m| m.elapsed().ok())
            .is_some_and(|age| age > max_age);
        if meta.is_file() && stale {
            match std::fs::remove_file(entry.path()) {
                Ok(()) => {
                    tracing::debug!("cship codex: pruned stale cache file {:?}", entry.path())
                }
                Err(e) => tracing::debug!("cship codex: could not prune {:?}: {e}", entry.path()),
            }
        }
    }
}

/// Stable stand-in for Claude Code's `transcript_path`, keyed by Codex thread id.
///
/// cship's caches (Starship passthrough, usage limits) live next to the
/// transcript. Codex has no transcript path, so this points at a per-thread
/// file under the user cache directory; the file itself is never created.
/// The caches themselves are written to `<root>/cship/`, which [`prune_cache`]
/// cleans up.
pub(crate) fn cache_key_path(thread_id: &str) -> Option<PathBuf> {
    let safe: String = thread_id
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .collect();
    if safe.is_empty() {
        return None;
    }
    Some(cache_root()?.join(format!("{safe}.jsonl")))
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

    #[test]
    fn test_prune_dir_removes_only_stale_files() {
        let dir = tempfile::tempdir().unwrap();
        let old = dir.path().join("thread-old-starship-git_branch");
        let fresh = dir.path().join("thread-new-starship-git_branch");
        std::fs::write(&old, "x").unwrap();
        std::fs::write(&fresh, "y").unwrap();
        let week_ago = filetime::FileTime::from_system_time(
            std::time::SystemTime::now() - Duration::from_secs(8 * 24 * 60 * 60),
        );
        filetime::set_file_mtime(&old, week_ago).unwrap();

        prune_dir(dir.path(), CACHE_MAX_AGE);

        assert!(!old.exists(), "stale cache file removed");
        assert!(fresh.exists(), "recent cache file kept");
    }
}
