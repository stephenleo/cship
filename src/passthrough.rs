//! Starship passthrough module renderer.
//!
//! Invokes `starship module <name>` as a subprocess and returns captured stdout.
//! Story 4.1: subprocess only, no cache. Story 4.2 adds `cache.rs` with 5s TTL.

use std::process::{Command, Stdio};

/// Render a Starship passthrough module by invoking `starship module <name>`.
///
/// - Returns `None` silently if `starship` binary is not found (FR30 minimal install path).
/// - Returns `None` with `tracing::warn!` if the subprocess exits non-zero.
/// - Returns `None` if stdout is empty (Starship convention: module has nothing to show).
/// - Changes working directory to `workspace.current_dir` before invocation (AC2).
pub fn render_passthrough(name: &str, ctx: &crate::context::Context) -> Option<String> {
    let cwd = ctx
        .workspace
        .as_ref()
        .and_then(|w| w.current_dir.as_deref());

    let mut cmd = Command::new("starship");
    cmd.args(["module", name]);
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::null());

    if let Some(dir) = cwd {
        cmd.current_dir(dir);
    }

    let output = match cmd.output() {
        Ok(o) => o,
        Err(_) => return None, // starship not found — silent (FR30)
    };

    if !output.status.success() {
        tracing::warn!("passthrough: `starship module {name}` exited with non-zero status");
        return None;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let trimmed = stdout.trim_end_matches(&['\r', '\n'][..]);
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::Context;

    #[test]
    fn test_render_passthrough_returns_none_for_nonexistent_module() {
        // starship exits non-zero for unknown module names → None
        let result = render_passthrough("__cship_nonexistent_xyz__", &Context::default());
        assert!(result.is_none());
    }

    #[test]
    fn test_render_passthrough_returns_none_on_nonzero_exit() {
        // Create a fake starship script that exits non-zero to exercise the warn path (AC4).
        // Real starship exits 0 even for unknown modules, so we need a mock.
        use std::fs;
        #[cfg(unix)]
        use std::os::unix::fs::PermissionsExt;

        let dir = std::env::temp_dir().join("cship_test_nonzero");
        fs::create_dir_all(&dir).unwrap();
        let script = dir.join("starship");
        fs::write(&script, "#!/bin/sh\nexit 1\n").unwrap();
        #[cfg(unix)]
        fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();

        let original = std::env::var("PATH").unwrap_or_default();
        unsafe { std::env::set_var("PATH", dir.to_str().unwrap()) };
        let result = render_passthrough("directory", &Context::default());
        unsafe { std::env::set_var("PATH", &original) };
        let _ = fs::remove_dir_all(&dir);

        assert!(result.is_none());
    }

    #[test]
    fn test_render_passthrough_returns_none_silently_when_starship_missing() {
        // Override PATH so starship binary cannot be found, exercising the Err(_) → None path (AC5).
        // SAFETY: No other unit tests in this module depend on PATH; integration tests run in a
        // separate process. set_var is unsafe in edition 2024 due to process-global mutation.
        let original = std::env::var("PATH").unwrap_or_default();
        unsafe { std::env::set_var("PATH", "/nonexistent_cship_test_dir") };
        let result = render_passthrough("directory", &Context::default());
        unsafe { std::env::set_var("PATH", &original) };
        assert!(result.is_none());
    }
}
