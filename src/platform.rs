/// Retrieve the Claude Code OAuth token from the OS credential store.
/// Returns Err with a descriptive message if the token is not found or the
/// credential tool is not installed.
///
/// Token is held only as a local String for the duration of the API call —
/// never written to disk, cache, stdout, or stderr (NFR-S1).
///
/// The credential store holds a JSON blob of the form:
/// `{"claudeAiOauth":{"accessToken":"sk-ant-oat01-...","refreshToken":"...","expiresAt":...}}`
/// This function extracts `accessToken` from that blob.
///
/// Service name verified against live Claude Code credential store: "Claude Code-credentials"
/// Reference: https://codelynx.dev/posts/claude-code-usage-limits-statusline
#[cfg(target_os = "macos")]
pub fn get_oauth_token() -> Result<String, String> {
    // CI gap: macOS path only compiled on target_os = "macos".
    // Validated by code review only — cannot be tested in WSL2/ubuntu CI.
    //
    // `security find-generic-password -s <service> -w` prints the stored password
    // (the JSON blob) to stdout and exits 0 on success, non-zero if not found.
    // No `-a <account>` flag is needed — the service name alone is unique.
    get_oauth_token_with_cmd(
        "security",
        &[
            "find-generic-password",
            "-s",
            "Claude Code-credentials",
            "-w",
        ],
    )
}

#[cfg(target_os = "linux")]
pub fn get_oauth_token() -> Result<String, String> {
    // `secret-tool lookup <attr> <value>` prints the stored secret to stdout.
    // The service attribute key for Claude Code is "service", value "Claude Code-credentials".
    get_oauth_token_with_cmd(
        "secret-tool",
        &["lookup", "service", "Claude Code-credentials"],
    )
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
compile_error!("cship: get_oauth_token() is only supported on macOS and Linux");

/// Inner implementation with injectable command name for testability.
/// `tool` is the binary; `args` are the arguments passed to it.
fn get_oauth_token_with_cmd(tool: &str, args: &[&str]) -> Result<String, String> {
    use std::process::Command;

    let mut cmd = Command::new(tool);
    cmd.args(args)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null());

    let child = match cmd.spawn() {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err(install_hint(tool));
        }
        Err(e) => return Err(format!("failed to invoke {tool}: {e}")),
        Ok(child) => child,
    };

    let output = match child.wait_with_output() {
        Err(e) => return Err(format!("failed to wait for {tool}: {e}")),
        Ok(o) => o,
    };

    if !output.status.success() {
        return Err("Claude Code credentials not found — authenticate in Claude Code first".into());
    }

    let raw = String::from_utf8_lossy(&output.stdout);
    let raw = raw.trim();

    // The credential store holds a JSON blob; extract the access token.
    extract_access_token(raw).ok_or_else(|| {
        "Claude Code credentials found but access token could not be parsed — credential may be malformed".into()
    })
}

/// Parse `{"claudeAiOauth":{"accessToken":"...","refreshToken":"...",...}}` and return the token.
fn extract_access_token(json: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(json).ok()?;
    let token = v
        .get("claudeAiOauth")?
        .get("accessToken")?
        .as_str()?
        .to_string();
    if token.is_empty() { None } else { Some(token) }
}

/// Return the platform-specific install hint for a missing credential tool.
fn install_hint(tool: &str) -> String {
    match tool {
        "secret-tool" => {
            "secret-tool not found — install with: sudo apt install libsecret-tools".into()
        }
        "security" => {
            // Rare on macOS; `security` ships with the OS.
            "security command not found — reinstall macOS command line tools: xcode-select --install"
                .into()
        }
        other => format!("{other} not found"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // These tests exercise the Linux path only; macOS path is validated by code review.

    #[test]
    fn test_tool_not_found_returns_install_hint() {
        // A non-existent binary triggers io::ErrorKind::NotFound on spawn.
        let result =
            get_oauth_token_with_cmd("cship_nonexistent_tool_xyz", &["lookup", "service", "test"]);
        assert!(result.is_err());
        let msg = result.unwrap_err();
        assert!(
            msg.contains("not found"),
            "expected 'not found' in error: {msg}"
        );
    }

    #[test]
    fn test_nonzero_exit_returns_credential_not_found_error() {
        // `/bin/sh -c "exit 1"` always exits with code 1 — simulates "credential not found".
        // Uses absolute path so the test is independent of PATH on any CI runner.
        let result = get_oauth_token_with_cmd("/bin/sh", &["-c", "exit 1"]);
        assert!(result.is_err());
        let msg = result.unwrap_err();
        assert!(
            msg.contains("authenticate in Claude Code first"),
            "expected credential-not-found hint in error: {msg}"
        );
    }

    #[test]
    fn test_successful_token_extraction() {
        // Use `/bin/sh` (absolute path, present on both macOS and Linux) to emit a JSON blob.
        let json = r#"{"claudeAiOauth":{"accessToken":"sk-ant-test-token","refreshToken":"rt","expiresAt":9999}}"#;
        let script = format!("printf '%s' '{json}'");
        let result = get_oauth_token_with_cmd("/bin/sh", &["-c", &script]);
        assert!(result.is_ok(), "expected Ok, got: {:?}", result);
        assert_eq!(result.unwrap(), "sk-ant-test-token");
    }

    #[test]
    fn test_extract_access_token_valid_json() {
        let json = r#"{"claudeAiOauth":{"accessToken":"sk-ant-oat01-abc","refreshToken":"rt","expiresAt":1234567890,"scopes":["read"]}}"#;
        assert_eq!(
            extract_access_token(json),
            Some("sk-ant-oat01-abc".to_string())
        );
    }

    #[test]
    fn test_extract_access_token_missing_field() {
        let json = r#"{"claudeAiOauth":{"refreshToken":"rt"}}"#;
        assert_eq!(extract_access_token(json), None);
    }

    #[test]
    fn test_extract_access_token_invalid_json() {
        assert_eq!(extract_access_token("not json"), None);
    }

    #[test]
    fn test_extract_access_token_empty_token() {
        let json = r#"{"claudeAiOauth":{"accessToken":""}}"#;
        assert_eq!(extract_access_token(json), None);
    }

    #[test]
    fn test_install_hint_secret_tool() {
        let hint = install_hint("secret-tool");
        assert!(hint.contains("sudo apt install libsecret-tools"), "{hint}");
    }

    #[test]
    fn test_install_hint_security() {
        let hint = install_hint("security");
        assert!(hint.contains("xcode-select"), "{hint}");
    }
}
