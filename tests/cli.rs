use assert_cmd::Command;
use assert_cmd::cargo_bin_cmd;
use predicates::prelude::*;

fn cship() -> Command {
    cargo_bin_cmd!("cship")
}

#[test]
fn test_valid_full_json_exits_zero_with_no_stdout() {
    let json = std::fs::read_to_string("tests/fixtures/sample_input_full.json").unwrap();
    cargo_bin_cmd!("cship")
        .write_stdin(json)
        .assert()
        .success()
        .stdout("");
}

#[test]
fn test_valid_minimal_json_exits_zero() {
    let json = std::fs::read_to_string("tests/fixtures/sample_input_minimal.json").unwrap();
    cargo_bin_cmd!("cship")
        .write_stdin(json)
        .assert()
        .success()
        .stdout("");
}

#[test]
fn test_empty_stdin_exits_nonzero_with_no_stdout() {
    cargo_bin_cmd!("cship")
        .write_stdin("")
        .assert()
        .failure()
        .stdout("")
        .stderr(predicate::str::contains(
            "failed to parse Claude Code session JSON",
        ));
}

#[test]
fn test_malformed_json_exits_nonzero_with_no_stdout() {
    cargo_bin_cmd!("cship")
        .write_stdin("not valid json{{{")
        .assert()
        .failure()
        .stdout("")
        .stderr(predicate::str::contains(
            "failed to parse Claude Code session JSON",
        ));
}

#[test]
fn test_unknown_fields_silently_ignored() {
    let json = r#"{"session_id":"abc","cwd":"/tmp","transcript_path":"/tmp/t.jsonl","version":"1.0","exceeds_200k_tokens":false,"model":{"id":"claude-test","display_name":"Test"},"workspace":{"current_dir":"/tmp","project_dir":"/tmp"},"output_style":{"name":"default"},"cost":{"total_cost_usd":0.0},"unknown_future_field":true,"nested_unknown":{"key":"value"}}"#;
    cargo_bin_cmd!("cship")
        .write_stdin(json)
        .assert()
        .success()
        .stdout("");
}

#[test]
fn test_config_flag_with_valid_toml_exits_zero() {
    let json = std::fs::read_to_string("tests/fixtures/sample_input_full.json").unwrap();
    cship()
        .args(["--config", "tests/fixtures/sample_starship.toml"])
        .write_stdin(json)
        .assert()
        .success()
        // sample_starship.toml has lines = ["$cship.model $git_branch", "$cship.cost"]
        // model renders "Opus"; git_branch and cost are None → final output contains "Opus"
        .stdout(predicate::str::contains("Opus"));
}

#[test]
fn test_config_flag_with_nonexistent_file_exits_nonzero() {
    let json = std::fs::read_to_string("tests/fixtures/sample_input_full.json").unwrap();
    cship()
        .args(["--config", "/nonexistent/starship.toml"])
        .write_stdin(json)
        .assert()
        .failure()
        .stdout("")
        .stderr(predicate::str::contains("failed to load config"));
}

#[test]
fn test_config_flag_with_malformed_toml_exits_nonzero() {
    let json = std::fs::read_to_string("tests/fixtures/sample_input_full.json").unwrap();
    cship()
        .args(["--config", "tests/fixtures/malformed.toml"])
        .write_stdin(json)
        .assert()
        .failure()
        .stdout("")
        .stderr(predicate::str::contains("failed to load config"));
}

#[test]
fn test_no_local_config_falls_through_to_global_or_default() {
    // sample_input_minimal.json has workspace.current_dir = "/home/user/projects/myapp"
    // which has no starship.toml above it in the test environment.
    // Depending on the machine, this may exercise:
    //   - Step 3: global fallback (~/.config/starship.toml) if it exists, OR
    //   - Step 4: CshipConfig::default() if no global config exists either.
    // Both paths produce exit 0 with empty stdout — the test validates that
    // the discovery chain completes without error when no local config is found.
    let json = std::fs::read_to_string("tests/fixtures/sample_input_minimal.json").unwrap();
    cship().write_stdin(json).assert().success().stdout("");
}

// ── Story 1.4: Rendering pipeline integration tests ──────────────────────

#[test]
fn test_model_renders_display_name_to_stdout() {
    let json = std::fs::read_to_string("tests/fixtures/sample_input_full.json").unwrap();
    // model_only.toml: lines = ["$cship.model"], no style → plain text "Opus"
    cship()
        .args(["--config", "tests/fixtures/model_only.toml"])
        .write_stdin(json)
        .assert()
        .success()
        .stdout(predicate::str::contains("Opus"));
}

#[test]
fn test_model_with_symbol_renders_symbol_and_name() {
    let json = std::fs::read_to_string("tests/fixtures/sample_input_full.json").unwrap();
    // model_styled.toml: symbol = "★ "
    cship()
        .args(["--config", "tests/fixtures/model_styled.toml"])
        .write_stdin(json)
        .assert()
        .success()
        .stdout(predicate::str::contains("★ Opus"));
}

#[test]
fn test_model_with_style_renders_ansi_codes() {
    let json = std::fs::read_to_string("tests/fixtures/sample_input_full.json").unwrap();
    // model_styled.toml: style = "bold green" → ANSI escape codes in stdout
    cship()
        .args(["--config", "tests/fixtures/model_styled.toml"])
        .write_stdin(json)
        .assert()
        .success()
        .stdout(predicate::str::contains("\x1b["));
}

#[test]
fn test_disabled_model_produces_empty_stdout() {
    let json = std::fs::read_to_string("tests/fixtures/sample_input_full.json").unwrap();
    // model_disabled.toml: disabled = true → no output
    cship()
        .args(["--config", "tests/fixtures/model_disabled.toml"])
        .write_stdin(json)
        .assert()
        .success()
        .stdout("");
}

#[test]
fn test_two_row_layout_produces_newline_separated_output() {
    let json = std::fs::read_to_string("tests/fixtures/sample_input_full.json").unwrap();
    // two_rows.toml: lines = ["$cship.model", "$cship.model"]
    let output = cship()
        .args(["--config", "tests/fixtures/two_rows.toml"])
        .write_stdin(json)
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    // stdout should contain two lines, each with "Opus"
    let lines: Vec<&str> = stdout.trim_end_matches('\n').split('\n').collect();
    assert_eq!(lines.len(), 2, "expected 2 lines; got: {stdout:?}");
    assert!(lines[0].contains("Opus"), "line 0: {}", lines[0]);
    assert!(lines[1].contains("Opus"), "line 1: {}", lines[1]);
}

#[test]
fn test_passthrough_tokens_skipped_silently() {
    let json = std::fs::read_to_string("tests/fixtures/sample_input_full.json").unwrap();
    // passthrough_only.toml: lines = ["$git_branch"] → Story 1.4 returns None → empty stdout
    cship()
        .args(["--config", "tests/fixtures/passthrough_only.toml"])
        .env("RUST_LOG", "debug")
        .write_stdin(json)
        .assert()
        .success()
        .stdout("")
        // Passthrough logs at debug level — no error or warn level output
        .stderr(predicate::str::contains("error").not())
        .stderr(predicate::str::contains("WARN").not());
}

#[test]
fn test_missing_model_logs_warning_to_stderr() {
    // JSON with no model field — triggers tracing::warn! per AC8
    let json = r#"{"session_id":"test","cwd":"/tmp","transcript_path":"/tmp/t.jsonl","version":"1.0","exceeds_200k_tokens":false,"workspace":{"current_dir":"/tmp"},"output_style":{"name":"default"},"cost":{"total_cost_usd":0.0}}"#;
    cship()
        .args(["--config", "tests/fixtures/model_only.toml"])
        .env("RUST_LOG", "warn")
        .write_stdin(json)
        .assert()
        .success()
        .stdout("")
        .stderr(predicate::str::contains("cship.model"));
}

#[test]
fn test_no_lines_config_produces_empty_stdout() {
    let json = std::fs::read_to_string("tests/fixtures/sample_input_full.json").unwrap();
    // empty_cship.toml: [cship] with no lines key → cfg.lines is None → no output
    cship()
        .args(["--config", "tests/fixtures/empty_cship.toml"])
        .write_stdin(json)
        .assert()
        .success()
        .stdout("");
}
