//! Starship passthrough module renderer.
//!
//! Invokes `starship module <name>` as a subprocess and returns captured stdout.
//! Story 4.1: subprocess only, no cache.
//! Story 4.2: adds 5s file cache (`cache.rs`) and CSHIP_* environment variable injection.

use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};

static TEMP_ID: AtomicUsize = AtomicUsize::new(0);

/// RAII guard that deletes a temporary file when dropped.
struct TempConfig(std::path::PathBuf);

impl Drop for TempConfig {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// Builds a temporary STARSHIP_CONFIG that disables the `line_break` and `character` modules.
///
/// Inherits from the user's existing `STARSHIP_CONFIG` env var or `~/.config/starship.toml`.
/// Returns `None` if the temp file cannot be written (non-fatal; caller falls back to default).
fn build_stripped_starship_config() -> Option<TempConfig> {
    let base = std::env::var("STARSHIP_CONFIG")
        .ok()
        .map(std::path::PathBuf::from)
        .or_else(|| {
            std::env::var("HOME")
                .ok()
                .map(|h| std::path::PathBuf::from(h).join(".config/starship.toml"))
        })
        .filter(|p| p.exists())
        .and_then(|p| std::fs::read_to_string(p).ok())
        .unwrap_or_default();

    let mut table: toml::Table = toml::from_str::<toml::Value>(&base)
        .ok()
        .and_then(|v| match v {
            toml::Value::Table(t) => Some(t),
            _ => None,
        })
        .unwrap_or_default();

    // Disable interactive shell-only modules — they produce output that never
    // belongs in a status bar (newlines, prompt symbols).
    disable_starship_module(&mut table, "line_break");
    disable_starship_module(&mut table, "character");

    let content = toml::to_string(&toml::Value::Table(table)).ok()?;
    let seq = TEMP_ID.fetch_add(1, Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!(
        "cship_starship_{}_{}.toml",
        std::process::id(),
        seq
    ));
    std::fs::write(&path, content).ok()?;
    Some(TempConfig(path))
}

fn disable_starship_module(table: &mut toml::Table, name: &str) {
    let section = table
        .entry(name.to_string())
        .or_insert_with(|| toml::Value::Table(toml::Table::new()));
    if let toml::Value::Table(t) = section {
        t.insert("disabled".to_string(), toml::Value::Boolean(true));
    }
}

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

    // Override STARSHIP_CONFIG so that line_break and character modules are disabled.
    // These interactive shell elements produce newlines/symbols that corrupt status bar output.
    let _stripped_cfg = build_stripped_starship_config();
    if let Some(ref tc) = _stripped_cfg {
        cmd.env("STARSHIP_CONFIG", &tc.0);
    }

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

    // Serializes all tests that mutate or read process-global env vars (PATH,
    // STARSHIP_CONFIG, etc). Required because unit tests run in parallel threads
    // within the same process and `std::env::set_var` is unsafe across threads.
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
    fn test_build_stripped_starship_config_disables_line_break_and_character() {
        // Reads STARSHIP_CONFIG; lock ENV mutex to serialize against tests that mutate it.
        let _guard = PATH_MUTEX.lock().unwrap();
        let result = build_stripped_starship_config();
        assert!(result.is_some(), "should produce a temp config file");
        let tc = result.unwrap();
        assert!(tc.0.exists(), "temp config file should exist on disk");

        let content = std::fs::read_to_string(&tc.0).expect("temp config should be readable");
        let value: toml::Value =
            toml::from_str(&content).expect("temp config should be valid TOML");
        let table = value.as_table().unwrap();

        for module in &["line_break", "character"] {
            let disabled = table
                .get(*module)
                .and_then(|v| v.get("disabled"))
                .and_then(|v| v.as_bool());
            assert_eq!(
                disabled,
                Some(true),
                "[{module}].disabled should be true in stripped config"
            );
        }
    }

    #[test]
    fn test_build_stripped_starship_config_preserves_existing_module_settings() {
        use std::fs;

        let base_config = "[character]\nsuccess_symbol = \"X\"\n";
        let base_path = std::env::temp_dir().join("cship_test_base_starship.toml");
        fs::write(&base_path, base_config).unwrap();

        // Mutates STARSHIP_CONFIG; lock ENV mutex to avoid racing parallel tests.
        let _guard = PATH_MUTEX.lock().unwrap();

        // Point build_stripped_starship_config at our test base config
        let original = std::env::var("STARSHIP_CONFIG").ok();
        unsafe { std::env::set_var("STARSHIP_CONFIG", base_path.to_str().unwrap()) };

        let result = build_stripped_starship_config();

        // Restore env before any assert that might panic
        match &original {
            Some(v) => unsafe { std::env::set_var("STARSHIP_CONFIG", v) },
            None => unsafe { std::env::remove_var("STARSHIP_CONFIG") },
        }
        let _ = fs::remove_file(&base_path);

        let tc = result.unwrap();
        let content = std::fs::read_to_string(&tc.0).unwrap();
        let value: toml::Value = toml::from_str(&content).unwrap();
        let char_section = value.as_table().unwrap().get("character").unwrap();

        assert_eq!(
            char_section.get("disabled").and_then(|v| v.as_bool()),
            Some(true)
        );
        assert_eq!(
            char_section.get("success_symbol").and_then(|v| v.as_str()),
            Some("X"),
            "existing character settings should be preserved"
        );
    }

    // Unix-only: faking a `starship` binary requires a +x shell script, which has no
    // simple equivalent on Windows (Command::new resolves only .exe, not .cmd/.bat).
    // The env-injection logic itself (cmd.env) is platform-independent.
    #[cfg(unix)]
    #[test]
    fn test_render_starship_prompt_passes_stripped_config_to_subprocess() {
        use crate::config::CshipConfig;
        use std::fs;
        use std::os::unix::fs::PermissionsExt;

        // Mock starship: outputs "OK" if STARSHIP_CONFIG is set and contains "disabled = true",
        // otherwise outputs "FAIL\n❯ " (simulating an unstripped prompt with $line_break/$character).
        let dir = std::env::temp_dir().join("cship_test_stripped_cfg");
        fs::create_dir_all(&dir).unwrap();
        let script = dir.join("starship");
        fs::write(
            &script,
            "#!/bin/sh\n\
             if [ -n \"$STARSHIP_CONFIG\" ] && [ -f \"$STARSHIP_CONFIG\" ] \
             && grep -q 'disabled = true' \"$STARSHIP_CONFIG\"; then\n\
             printf 'OK'\n\
             else\n\
             printf 'FAIL\\n\\xe2\\x9d\\xaf '\n\
             fi\n",
        )
        .unwrap();
        fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();

        let _guard = PATH_MUTEX.lock().unwrap();
        let original = std::env::var("PATH").unwrap_or_default();
        // Prepend mock dir rather than replacing PATH entirely — system commands
        // (e.g. grep, cat) used inside the mock script must remain resolvable.
        unsafe { std::env::set_var("PATH", format!("{}:{}", dir.to_str().unwrap(), original)) };

        let result = render_starship_prompt(&Context::default(), &CshipConfig::default());

        unsafe { std::env::set_var("PATH", &original) };
        drop(_guard);
        let _ = fs::remove_dir_all(&dir);

        assert_eq!(result, Some("OK".to_string()));
    }

    #[cfg(unix)]
    #[test]
    fn test_render_passthrough_injects_cship_model_env_var() {
        use std::fs;
        use std::os::unix::fs::PermissionsExt;

        let dir = std::env::temp_dir().join("cship_test_cship_env");
        fs::create_dir_all(&dir).unwrap();

        let script = dir.join("starship");
        fs::write(&script, "#!/bin/sh\nprintf '%s' \"$CSHIP_MODEL\"\n").unwrap();
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
