//! Starship passthrough module renderer.
//!
//! Invokes `starship module <name>` as a subprocess and returns captured stdout.
//! Story 4.1: subprocess only, no cache.
//! Story 4.2: adds 5s file cache (`cache.rs`) and CSHIP_* environment variable injection.

use std::path::Path;
use std::process::{Command, Stdio};

/// Render a Starship passthrough module by invoking `starship module <name>`.
///
/// - Returns cached stdout immediately if cache hit (< 5s old) — no subprocess spawned.
/// - Returns `None` silently if `starship` binary is not found (FR30 minimal install path).
/// - Returns `None` with `tracing::warn!` if the subprocess exits non-zero.
/// - Returns `None` if stdout is empty (Starship convention: module has nothing to show).
/// - Changes working directory to `workspace.current_dir` (fallback: `ctx.cwd`) before invocation.
/// - Injects all 9 CSHIP_* environment variables into the subprocess environment.
pub fn render_passthrough(name: &str, ctx: &crate::context::Context) -> Option<String> {
    // Derive transcript_path once — used for both cache read and write
    let transcript_path = ctx.transcript_path.as_deref().map(Path::new);

    // Cache hit check (before any subprocess)
    if let Some(tp) = transcript_path
        && let Some(cached) = crate::cache::read_passthrough(name, tp)
    {
        return Some(cached);
    }

    // CWD resolution: workspace.current_dir → ctx.cwd → None (inherit, warn)
    let cwd = ctx
        .workspace
        .as_ref()
        .and_then(|w| w.current_dir.as_deref())
        .or(ctx.cwd.as_deref());
    if cwd.is_none() {
        tracing::warn!(
            "passthrough: no CWD available for `{name}` — subprocess inherits cship's cwd"
        );
    }

    let mut cmd = Command::new("starship");
    cmd.args(["module", name]);
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::null());

    if let Some(dir) = cwd {
        cmd.current_dir(dir);
    }

    // Strip null bytes before passing strings to cmd.env(): environment variable values are
    // null-terminated at the OS level, so Command::env panics if a value contains '\0'.
    // serde_json faithfully decodes JSON \u0000 escapes into Rust Strings, making this possible.
    let san = |s: &str| s.replace('\0', "");

    // CSHIP_* environment variable injection (all 9 — empty string for None fields)
    cmd.env(
        "CSHIP_MODEL",
        san(ctx
            .model
            .as_ref()
            .and_then(|m| m.display_name.as_deref())
            .unwrap_or("")),
    );
    cmd.env(
        "CSHIP_MODEL_ID",
        san(ctx
            .model
            .as_ref()
            .and_then(|m| m.id.as_deref())
            .unwrap_or("")),
    );
    cmd.env(
        "CSHIP_COST_USD",
        ctx.cost
            .as_ref()
            .and_then(|c| c.total_cost_usd)
            .map(|v| v.to_string())
            .unwrap_or_default(),
    );
    cmd.env(
        "CSHIP_CONTEXT_PCT",
        ctx.context_window
            .as_ref()
            .and_then(|cw| cw.used_percentage)
            .map(|v| v.to_string())
            .unwrap_or_default(),
    );
    cmd.env(
        "CSHIP_CONTEXT_REMAINING_PCT",
        ctx.context_window
            .as_ref()
            .and_then(|cw| cw.remaining_percentage)
            .map(|v| v.to_string())
            .unwrap_or_default(),
    );
    cmd.env(
        "CSHIP_VIM_MODE",
        san(ctx
            .vim
            .as_ref()
            .and_then(|v| v.mode.as_deref())
            .unwrap_or("")),
    );
    cmd.env(
        "CSHIP_AGENT_NAME",
        san(ctx
            .agent
            .as_ref()
            .and_then(|a| a.name.as_deref())
            .unwrap_or("")),
    );
    cmd.env(
        "CSHIP_SESSION_ID",
        san(ctx.session_id.as_deref().unwrap_or("")),
    );
    cmd.env("CSHIP_CWD", san(cwd.unwrap_or("")));

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
        return None;
    }

    let result = trimmed.to_string();

    // Write to cache for future hits
    if let Some(tp) = transcript_path {
        crate::cache::write_passthrough(name, tp, &result);
    }

    Some(result)
}

/// Render the full starship prompt by invoking `starship prompt`.
///
/// - Returns `None` silently if `disabled` is set in `[cship.starship_prompt]`.
/// - Returns cached result immediately if cache hit (< 5s old).
/// - Returns `None` silently if `starship` binary is not found.
/// - Returns `None` with `tracing::warn!` if the subprocess exits non-zero.
/// - Terminal width is read from `$COLUMNS` env var (fallback: 80).
/// - `STARSHIP_SHELL` is set to "unknown" to force plain ANSI output.
/// - Injects all 9 CSHIP_* environment variables into the subprocess.
/// - Trims trailing newlines from output.
pub fn render_starship_prompt(
    ctx: &crate::context::Context,
    cfg: &crate::config::CshipConfig,
) -> Option<String> {
    // Check disabled flag — return silent None
    if let Some(sp_cfg) = &cfg.starship_prompt
        && sp_cfg.disabled == Some(true)
    {
        return None;
    }

    let transcript_path = ctx.transcript_path.as_deref().map(Path::new);
    let cache_key = "__starship_prompt__";

    // Cache hit check (before any subprocess)
    if let Some(tp) = transcript_path
        && let Some(cached) = crate::cache::read_passthrough(cache_key, tp)
    {
        return Some(cached);
    }

    // CWD resolution: workspace.current_dir → ctx.cwd → None (inherit, warn)
    let cwd = ctx
        .workspace
        .as_ref()
        .and_then(|w| w.current_dir.as_deref())
        .or(ctx.cwd.as_deref());
    if cwd.is_none() {
        tracing::warn!("starship_prompt: no CWD available — subprocess inherits cship's cwd");
    }

    // Derive terminal width from $COLUMNS, fallback to 80
    let width = std::env::var("COLUMNS")
        .ok()
        .and_then(|s| s.parse::<u16>().ok())
        .unwrap_or(80);

    let mut cmd = Command::new("starship");
    cmd.args([
        "prompt",
        "--status",
        "0",
        "--terminal-width",
        &width.to_string(),
    ]);
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::null());

    if let Some(dir) = cwd {
        cmd.current_dir(dir);
    }

    // Force plain ANSI output (no shell-specific wrapping) by setting STARSHIP_SHELL to "unknown".
    // This ensures output is identical to individual passthrough modules and displays correctly
    // when echoed as plain text.
    cmd.env("STARSHIP_SHELL", "unknown");

    // Strip null bytes before passing strings to cmd.env()
    let san = |s: &str| s.replace('\0', "");

    // Inject all 9 CSHIP_* env vars (same as render_passthrough)
    cmd.env(
        "CSHIP_MODEL",
        san(ctx
            .model
            .as_ref()
            .and_then(|m| m.display_name.as_deref())
            .unwrap_or("")),
    );
    cmd.env(
        "CSHIP_MODEL_ID",
        san(ctx
            .model
            .as_ref()
            .and_then(|m| m.id.as_deref())
            .unwrap_or("")),
    );
    cmd.env(
        "CSHIP_COST_USD",
        ctx.cost
            .as_ref()
            .and_then(|c| c.total_cost_usd)
            .map(|v| v.to_string())
            .unwrap_or_default(),
    );
    cmd.env(
        "CSHIP_CONTEXT_PCT",
        ctx.context_window
            .as_ref()
            .and_then(|cw| cw.used_percentage)
            .map(|v| v.to_string())
            .unwrap_or_default(),
    );
    cmd.env(
        "CSHIP_CONTEXT_REMAINING_PCT",
        ctx.context_window
            .as_ref()
            .and_then(|cw| cw.remaining_percentage)
            .map(|v| v.to_string())
            .unwrap_or_default(),
    );
    cmd.env(
        "CSHIP_VIM_MODE",
        san(ctx
            .vim
            .as_ref()
            .and_then(|v| v.mode.as_deref())
            .unwrap_or("")),
    );
    cmd.env(
        "CSHIP_AGENT_NAME",
        san(ctx
            .agent
            .as_ref()
            .and_then(|a| a.name.as_deref())
            .unwrap_or("")),
    );
    cmd.env(
        "CSHIP_SESSION_ID",
        san(ctx.session_id.as_deref().unwrap_or("")),
    );
    cmd.env("CSHIP_CWD", san(cwd.unwrap_or("")));

    let output = match cmd.output() {
        Ok(o) => o,
        Err(_) => return None, // starship not found — silent
    };

    if !output.status.success() {
        tracing::warn!("starship prompt: subprocess exited with non-zero status");
        return None;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let trimmed = stdout.trim_end_matches(&['\r', '\n'][..]);
    if trimmed.is_empty() {
        return None;
    }

    let result = trimmed.to_string();

    // Write to cache for future hits
    if let Some(tp) = transcript_path {
        crate::cache::write_passthrough(cache_key, tp, &result);
    }

    Some(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::Context;

    // Serializes all tests that mutate the process-global PATH environment variable.
    // Required because unit tests run in parallel threads within the same process.
    static PATH_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());

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

        let _guard = PATH_MUTEX.lock().unwrap();
        let original = std::env::var("PATH").unwrap_or_default();
        unsafe { std::env::set_var("PATH", dir.to_str().unwrap()) };
        let result = render_passthrough("directory", &Context::default());
        unsafe { std::env::set_var("PATH", &original) };
        drop(_guard);
        let _ = fs::remove_dir_all(&dir);

        assert!(result.is_none());
    }

    #[test]
    fn test_render_passthrough_returns_none_silently_when_starship_missing() {
        // Override PATH so starship binary cannot be found, exercising the Err(_) → None path (AC5).
        // SAFETY: PATH_MUTEX serializes all PATH-mutating tests within this module.
        // Integration tests run in a separate process and are unaffected.
        let _guard = PATH_MUTEX.lock().unwrap();
        let original = std::env::var("PATH").unwrap_or_default();
        unsafe { std::env::set_var("PATH", "/nonexistent_cship_test_dir") };
        let result = render_passthrough("directory", &Context::default());
        unsafe { std::env::set_var("PATH", &original) };
        assert!(result.is_none());
    }

    #[test]
    fn test_render_passthrough_injects_cship_model_env_var() {
        use std::fs;
        #[cfg(unix)]
        use std::os::unix::fs::PermissionsExt;

        let dir = std::env::temp_dir().join("cship_test_cship_env");
        fs::create_dir_all(&dir).unwrap();
        let script = dir.join("starship");
        // Script: print CSHIP_MODEL env var, exit 0
        fs::write(&script, "#!/bin/sh\nprintf '%s' \"$CSHIP_MODEL\"\n").unwrap();
        #[cfg(unix)]
        fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();

        let _guard = PATH_MUTEX.lock().unwrap();
        let original = std::env::var("PATH").unwrap_or_default();
        unsafe { std::env::set_var("PATH", dir.to_str().unwrap()) };

        let ctx = Context {
            model: Some(crate::context::Model {
                display_name: Some("TestModelXYZ".to_string()),
                id: None,
            }),
            ..Context::default()
        };
        let result = render_passthrough("test_module", &ctx);

        unsafe { std::env::set_var("PATH", &original) };
        drop(_guard);
        let _ = fs::remove_dir_all(&dir);

        assert_eq!(result, Some("TestModelXYZ".to_string()));
    }
}
