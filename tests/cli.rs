use assert_cmd::Command;
use assert_cmd::cargo_bin_cmd;
use predicates::prelude::*;

fn cship() -> Command {
    cargo_bin_cmd!("cship")
}

#[test]
fn test_valid_full_json_exits_zero_with_no_stdout() {
    let json = std::fs::read_to_string("tests/fixtures/sample_input_full.json").unwrap();
    cargo_bin_cmd!("cship").write_stdin(json).assert().success();
}

#[test]
fn test_valid_minimal_json_exits_zero() {
    let json = std::fs::read_to_string("tests/fixtures/sample_input_minimal.json").unwrap();
    cargo_bin_cmd!("cship").write_stdin(json).assert().success();
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
    cargo_bin_cmd!("cship").write_stdin(json).assert().success();
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
        // model renders "Opus"; git_branch is passthrough (None); cost renders "$0.01"
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
    // Both paths produce exit 0 — the test validates that the discovery
    // chain completes without error when no local config is found.
    // stdout content varies by machine (depends on global starship.toml).
    let json = std::fs::read_to_string("tests/fixtures/sample_input_minimal.json").unwrap();
    cship().write_stdin(json).assert().success();
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
    // Use inline JSON with a guaranteed-nonexistent workspace path so that
    // starship subprocess spawn fails silently (no WARN) regardless of whether
    // starship is installed on the test machine.
    let json = r#"{"session_id":"test","cwd":"/tmp","transcript_path":"/tmp/t.jsonl","version":"1.0","exceeds_200k_tokens":false,"model":{"id":"test","display_name":"Test"},"workspace":{"current_dir":"/nonexistent_cship_test_path_12345","project_dir":"/tmp"},"output_style":{"name":"default"},"cost":{"total_cost_usd":0.0}}"#;
    // passthrough_only.toml: lines = ["$git_branch"] → None → empty stdout
    cship()
        .args(["--config", "tests/fixtures/passthrough_only.toml"])
        .env("RUST_LOG", "debug")
        .write_stdin(json)
        .assert()
        .success()
        .stdout("")
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

// ── Story 2.1: Cost module integration tests ──────────────────────────────

#[test]
fn test_cost_renders_dollar_formatted_value() {
    let json = std::fs::read_to_string("tests/fixtures/sample_input_full.json").unwrap();
    // sample_input_full.json: cost.total_cost_usd = 0.01234 → "$0.01"
    cship()
        .args(["--config", "tests/fixtures/cost_basic.toml"])
        .write_stdin(json)
        .assert()
        .success()
        .stdout(predicate::str::contains("$0.01"));
}

#[test]
fn test_cost_warn_threshold_applies_ansi_style() {
    let json = std::fs::read_to_string("tests/fixtures/cost_warn_value.json").unwrap();
    // cost_warn_value.json: total_cost_usd = 6.0 > warn_threshold 5.0 → ANSI codes
    cship()
        .args(["--config", "tests/fixtures/cost_warn.toml"])
        .write_stdin(json)
        .assert()
        .success()
        .stdout(predicate::str::contains("\x1b["));
}

#[test]
fn test_cost_critical_threshold_applies_critical_style() {
    let json = std::fs::read_to_string("tests/fixtures/cost_high.json").unwrap();
    // cost_high.json: total_cost_usd = 12.0 > critical_threshold 10.0 → ANSI codes
    cship()
        .args(["--config", "tests/fixtures/cost_critical.toml"])
        .write_stdin(json)
        .assert()
        .success()
        .stdout(predicate::str::contains("\x1b["));
}

#[test]
fn test_cost_disabled_produces_no_output() {
    let json = std::fs::read_to_string("tests/fixtures/sample_input_full.json").unwrap();
    cship()
        .args(["--config", "tests/fixtures/cost_disabled.toml"])
        .write_stdin(json)
        .assert()
        .success()
        .stdout("");
}

#[test]
fn test_cost_subfields_render_numeric_values() {
    let json = std::fs::read_to_string("tests/fixtures/sample_input_full.json").unwrap();
    // sample_input_full.json: total_cost_usd=0.01234, total_duration_ms=45000,
    // total_api_duration_ms=2300, total_lines_added=156, total_lines_removed=23
    let output = cship()
        .args(["--config", "tests/fixtures/cost_subfields.toml"])
        .write_stdin(json)
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("0.0123"),
        "expected '0.0123' in: {stdout:?}"
    );
    assert!(stdout.contains("45000"), "expected '45000' in: {stdout:?}");
    assert!(stdout.contains("2300"), "expected '2300' in: {stdout:?}");
    assert!(stdout.contains("156"), "expected '156' in: {stdout:?}");
    assert!(stdout.contains("23"), "expected '23' in: {stdout:?}");
}

// ── Story 2.2: Context window modules integration tests ───────────────────

#[test]
fn test_context_bar_renders_block_chars_and_percentage() {
    let json = std::fs::read_to_string("tests/fixtures/context_high.json").unwrap();
    // context_high.json: used_percentage = 90.0 → "█████████░90%"
    let output = cship()
        .args(["--config", "tests/fixtures/context_bar_basic.toml"])
        .write_stdin(json)
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains('█'),
        "expected filled bar chars: {stdout:?}"
    );
    assert!(stdout.contains("90%"), "expected '90%': {stdout:?}");
}

#[test]
fn test_context_bar_warn_threshold_applies_ansi_style() {
    let json = std::fs::read_to_string("tests/fixtures/context_warn.json").unwrap();
    // context_warn.json: used_percentage = 75.0 > warn_threshold 70.0 → ANSI codes
    cship()
        .args(["--config", "tests/fixtures/context_bar_warn.toml"])
        .write_stdin(json)
        .assert()
        .success()
        .stdout(predicate::str::contains("\x1b["));
}

#[test]
fn test_context_bar_critical_threshold_applies_critical_style() {
    let json = std::fs::read_to_string("tests/fixtures/context_high.json").unwrap();
    // context_high.json: used_percentage = 90.0 > critical_threshold 85.0 → ANSI codes
    cship()
        .args(["--config", "tests/fixtures/context_bar_warn.toml"])
        .write_stdin(json)
        .assert()
        .success()
        .stdout(predicate::str::contains("\x1b["));
}

#[test]
fn test_context_bar_disabled_produces_no_output() {
    let json = std::fs::read_to_string("tests/fixtures/sample_input_full.json").unwrap();
    cship()
        .args(["--config", "tests/fixtures/context_bar_disabled.toml"])
        .write_stdin(json)
        .assert()
        .success()
        .stdout("");
}

#[test]
fn test_context_bar_custom_width_5() {
    let json = std::fs::read_to_string("tests/fixtures/sample_input_full.json").unwrap();
    // sample_input_full.json: used_percentage = 8.0; width 5 → 0 filled, 5 empty
    let output = cship()
        .args(["--config", "tests/fixtures/context_bar_width.toml"])
        .write_stdin(json)
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let total_bar: usize = stdout.chars().filter(|&c| c == '█' || c == '░').count();
    assert_eq!(total_bar, 5, "expected bar width 5 in: {stdout:?}");
    // sample_input_full.json: used_percentage = 8.0; floor(8% of 5) = 0 filled, 5 empty
    let filled: usize = stdout.chars().filter(|&c| c == '█').count();
    let empty: usize = stdout.chars().filter(|&c| c == '░').count();
    assert_eq!(
        filled, 0,
        "expected 0 filled for 8% with width 5: {stdout:?}"
    );
    assert_eq!(empty, 5, "expected 5 empty for 8% with width 5: {stdout:?}");
}

#[test]
fn test_context_window_subfields_render_correctly() {
    let json = std::fs::read_to_string("tests/fixtures/sample_input_full.json").unwrap();
    // sample_input_full.json: used_percentage=8, remaining_percentage=92, context_window_size=200000
    let output = cship()
        .args(["--config", "tests/fixtures/context_window_basic.toml"])
        .write_stdin(json)
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    // Output format: "8 92 200000\n" — verify each value appears as a space-delimited token
    let tokens: Vec<&str> = stdout.trim().split_whitespace().collect();
    assert_eq!(
        tokens.len(),
        3,
        "expected 3 space-separated tokens: {stdout:?}"
    );
    assert_eq!(tokens[0], "8", "expected used_pct '8': {stdout:?}");
    assert_eq!(tokens[1], "92", "expected remaining_pct '92': {stdout:?}");
    assert_eq!(
        tokens[2], "200000",
        "expected window size '200000': {stdout:?}"
    );
}

#[test]
fn test_context_window_exceeds_200k_false_produces_no_output() {
    let json = std::fs::read_to_string("tests/fixtures/sample_input_full.json").unwrap();
    // sample_input_full.json: exceeds_200k_tokens = false → empty output
    cship()
        .args(["--config", "tests/fixtures/context_window_exceeds.toml"])
        .write_stdin(json)
        .assert()
        .success()
        .stdout("");
}

#[test]
fn test_context_window_exceeds_200k_true_renders_marker() {
    let json = std::fs::read_to_string("tests/fixtures/context_high.json").unwrap();
    // context_high.json: exceeds_200k_tokens = true → ">200k"
    cship()
        .args(["--config", "tests/fixtures/context_window_exceeds.toml"])
        .write_stdin(json)
        .assert()
        .success()
        .stdout(predicate::str::contains(">200k"));
}

#[test]
fn test_context_window_current_usage_tokens() {
    let json = std::fs::read_to_string("tests/fixtures/sample_input_full.json").unwrap();
    // sample_input_full.json: current_usage.input_tokens=8500, output_tokens=1200
    let output = cship()
        .args([
            "--config",
            "tests/fixtures/context_window_current_usage.toml",
        ])
        .write_stdin(json)
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("8500"),
        "expected input_tokens in: {stdout:?}"
    );
    assert!(
        stdout.contains("1200"),
        "expected output_tokens in: {stdout:?}"
    );
}

#[test]
fn test_context_window_total_tokens_render_correctly() {
    let json = std::fs::read_to_string("tests/fixtures/sample_input_full.json").unwrap();
    // sample_input_full.json: total_input_tokens=15234, total_output_tokens=4521
    let output = cship()
        .args(["--config", "tests/fixtures/context_window_totals.toml"])
        .write_stdin(json)
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let tokens: Vec<&str> = stdout.trim().split_whitespace().collect();
    assert_eq!(tokens.len(), 2, "expected 2 tokens: {stdout:?}");
    assert_eq!(
        tokens[0], "15234",
        "expected total_input_tokens: {stdout:?}"
    );
    assert_eq!(
        tokens[1], "4521",
        "expected total_output_tokens: {stdout:?}"
    );
}

// ── Story 2.3: Vim and Agent modules integration tests ────────────────────

#[test]
fn test_vim_renders_mode_string() {
    let json = std::fs::read_to_string("tests/fixtures/sample_input_full.json").unwrap();
    // sample_input_full.json: vim.mode = "NORMAL"
    cship()
        .args(["--config", "tests/fixtures/vim_basic.toml"])
        .write_stdin(json)
        .assert()
        .success()
        .stdout(predicate::str::contains("NORMAL"));
}

#[test]
fn test_vim_applies_symbol_and_style() {
    let json = std::fs::read_to_string("tests/fixtures/sample_input_full.json").unwrap();
    // vim_styled.toml: symbol = "✏ ", style = "bold yellow" → ANSI codes present
    cship()
        .args(["--config", "tests/fixtures/vim_styled.toml"])
        .write_stdin(json)
        .assert()
        .success()
        .stdout(predicate::str::contains("\x1b["))
        .stdout(predicate::str::contains("✏ "));
}

#[test]
fn test_vim_disabled_produces_no_output() {
    let json = std::fs::read_to_string("tests/fixtures/sample_input_full.json").unwrap();
    cship()
        .args(["--config", "tests/fixtures/vim_disabled.toml"])
        .write_stdin(json)
        .assert()
        .success()
        .stdout("");
}

#[test]
fn test_vim_absent_produces_no_output() {
    let json = std::fs::read_to_string("tests/fixtures/sample_input_minimal.json").unwrap();
    // sample_input_minimal.json has no vim field → empty render
    cship()
        .args(["--config", "tests/fixtures/vim_basic.toml"])
        .write_stdin(json)
        .assert()
        .success()
        .stdout("");
}

#[test]
fn test_vim_mode_subfield_renders_identically() {
    let json = std::fs::read_to_string("tests/fixtures/sample_input_full.json").unwrap();
    // vim_mode_subfield.toml: lines = ["$cship.vim.mode"] → same as $cship.vim
    cship()
        .args(["--config", "tests/fixtures/vim_mode_subfield.toml"])
        .write_stdin(json)
        .assert()
        .success()
        .stdout(predicate::str::contains("NORMAL"));
}

#[test]
fn test_agent_renders_name_string() {
    let json = std::fs::read_to_string("tests/fixtures/sample_input_full.json").unwrap();
    // sample_input_full.json: agent.name = "security-reviewer"
    cship()
        .args(["--config", "tests/fixtures/agent_basic.toml"])
        .write_stdin(json)
        .assert()
        .success()
        .stdout(predicate::str::contains("security-reviewer"));
}

#[test]
fn test_agent_name_subfield_renders_identically() {
    let json = std::fs::read_to_string("tests/fixtures/sample_input_full.json").unwrap();
    // agent_name_subfield.toml: lines = ["$cship.agent.name"] → same as $cship.agent
    cship()
        .args(["--config", "tests/fixtures/agent_name_subfield.toml"])
        .write_stdin(json)
        .assert()
        .success()
        .stdout(predicate::str::contains("security-reviewer"));
}

#[test]
fn test_agent_disabled_produces_no_output() {
    let json = std::fs::read_to_string("tests/fixtures/sample_input_full.json").unwrap();
    cship()
        .args(["--config", "tests/fixtures/agent_disabled.toml"])
        .write_stdin(json)
        .assert()
        .success()
        .stdout("");
}

#[test]
fn test_agent_absent_produces_no_output() {
    let json = std::fs::read_to_string("tests/fixtures/sample_input_minimal.json").unwrap();
    // sample_input_minimal.json has no agent field → empty render
    cship()
        .args(["--config", "tests/fixtures/agent_basic.toml"])
        .write_stdin(json)
        .assert()
        .success()
        .stdout("");
}

// ── Story 2.4: Session identity and workspace modules integration tests ───

#[test]
fn test_session_cwd_renders_path() {
    let json = std::fs::read_to_string("tests/fixtures/sample_input_full.json").unwrap();
    // sample_input_full.json: cwd = "/home/user/projects/myapp"
    cship()
        .args(["--config", "tests/fixtures/session_cwd.toml"])
        .write_stdin(json)
        .assert()
        .success()
        .stdout(predicate::str::contains("/home/user/projects/myapp"));
}

#[test]
fn test_session_id_renders_string() {
    let json = std::fs::read_to_string("tests/fixtures/sample_input_full.json").unwrap();
    // sample_input_full.json: session_id = "test-session-id"
    cship()
        .args(["--config", "tests/fixtures/session_id.toml"])
        .write_stdin(json)
        .assert()
        .success()
        .stdout(predicate::str::contains("test-session-id"));
}

#[test]
fn test_session_transcript_path_renders_string() {
    let json = std::fs::read_to_string("tests/fixtures/sample_input_full.json").unwrap();
    // sample_input_full.json: transcript_path = "/home/user/.claude/projects/myapp/transcript.jsonl"
    cship()
        .args(["--config", "tests/fixtures/session_transcript_path.toml"])
        .write_stdin(json)
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "/home/user/.claude/projects/myapp/transcript.jsonl",
        ));
}

#[test]
fn test_session_version_renders_string() {
    let json = std::fs::read_to_string("tests/fixtures/sample_input_full.json").unwrap();
    // sample_input_full.json: version = "1.0.80"
    cship()
        .args(["--config", "tests/fixtures/session_version.toml"])
        .write_stdin(json)
        .assert()
        .success()
        .stdout(predicate::str::contains("1.0.80"));
}

#[test]
fn test_session_output_style_renders_name() {
    let json = std::fs::read_to_string("tests/fixtures/sample_input_full.json").unwrap();
    // sample_input_full.json: output_style.name = "default"
    cship()
        .args(["--config", "tests/fixtures/session_output_style.toml"])
        .write_stdin(json)
        .assert()
        .success()
        .stdout(predicate::str::contains("default"));
}

#[test]
fn test_workspace_current_dir_renders_path() {
    let json = std::fs::read_to_string("tests/fixtures/sample_input_full.json").unwrap();
    // sample_input_full.json: workspace.current_dir = "/home/user/projects/myapp"
    cship()
        .args(["--config", "tests/fixtures/workspace_current_dir.toml"])
        .write_stdin(json)
        .assert()
        .success()
        .stdout(predicate::str::contains("/home/user/projects/myapp"));
}

#[test]
fn test_workspace_project_dir_renders_path() {
    let json = std::fs::read_to_string("tests/fixtures/sample_input_full.json").unwrap();
    // sample_input_full.json: workspace.project_dir = "/home/user/projects/myapp"
    cship()
        .args(["--config", "tests/fixtures/workspace_project_dir.toml"])
        .write_stdin(json)
        .assert()
        .success()
        .stdout(predicate::str::contains("/home/user/projects/myapp"));
}

#[test]
fn test_workspace_absent_produces_no_output() {
    // Inline JSON without workspace field → empty render (AC13)
    let json = r#"{"session_id":"test","cwd":"/tmp","transcript_path":"/t","version":"1.0","exceeds_200k_tokens":false,"model":{"id":"test","display_name":"Test"},"output_style":{"name":"default"},"cost":{"total_cost_usd":0.0}}"#;
    cship()
        .args(["--config", "tests/fixtures/workspace_current_dir.toml"])
        .write_stdin(json)
        .assert()
        .success()
        .stdout("");
}

#[test]
fn test_session_cwd_absent_produces_no_output() {
    // Inline JSON without cwd field → empty render
    let json = r#"{"session_id":"test","transcript_path":"/t","version":"1.0","exceeds_200k_tokens":false,"model":{"id":"test","display_name":"Test"},"workspace":{"current_dir":"/tmp","project_dir":"/tmp"},"output_style":{"name":"default"},"cost":{"total_cost_usd":0.0}}"#;
    cship()
        .args(["--config", "tests/fixtures/session_cwd.toml"])
        .write_stdin(json)
        .assert()
        .success()
        .stdout("");
}

// ── Story 3.1: cship explain integration tests ────────────────────────────

#[test]
fn test_explain_with_stdin_json_shows_module_names() {
    let json = std::fs::read_to_string("tests/fixtures/sample_input_full.json").unwrap();
    let output = cship()
        .args(["explain"])
        .write_stdin(json)
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("cship.model"),
        "expected 'cship.model' in explain output: {stdout}"
    );
}

#[test]
fn test_explain_with_stdin_json_shows_config_line() {
    let json = std::fs::read_to_string("tests/fixtures/sample_input_full.json").unwrap();
    let output = cship()
        .args(["explain"])
        .write_stdin(json)
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("using config:"),
        "expected 'using config:' in explain output: {stdout}"
    );
}

#[test]
fn test_explain_with_config_flag() {
    let json = std::fs::read_to_string("tests/fixtures/sample_input_full.json").unwrap();
    let output = cship()
        .args(["explain", "--config", "tests/fixtures/minimal.toml"])
        .write_stdin(json)
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("cship.model"),
        "expected module names in explain output: {stdout}"
    );
}

#[test]
fn test_explain_no_stdin_uses_embedded_fallback() {
    // Invoke without piped stdin — process spawned without write_stdin uses TTY detection
    // which triggers the embedded fallback path in load_context()
    let output = cargo_bin_cmd!("cship").args(["explain"]).output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("cship.model"),
        "expected module names from embedded fallback: {stdout}"
    );
    assert!(
        stdout.contains("using config:"),
        "expected config line from embedded fallback: {stdout}"
    );
    // Verify embedded sample_context.json values are actually rendered (model = "Sonnet")
    assert!(
        stdout.contains("Sonnet"),
        "expected 'Sonnet' from embedded sample context: {stdout}"
    );
}

// ── Story 3.2: per-module error hints integration tests ───────────────────

#[test]
fn test_explain_shows_warning_for_disabled_module() {
    // Pipe a minimal JSON with no model data + use config that disables model
    let json = r#"{"model":null}"#;
    let output = cargo_bin_cmd!("cship")
        .args(["explain", "--config", "tests/fixtures/disabled-model.toml"])
        .write_stdin(json)
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains('⚠'),
        "expected '⚠' warning indicator in explain output: {stdout}"
    );
    // AC5: verify disabled-specific hint text appears with the actual section name
    assert!(
        stdout.contains("disabled"),
        "expected 'disabled' in hint section: {stdout}"
    );
    assert!(
        stdout.contains("[cship.model]"),
        "expected specific section '[cship.model]' in remediation hint: {stdout}"
    );
}

// ── Story 4.1: Starship passthrough integration tests ─────────────────────

fn starship_available() -> bool {
    std::process::Command::new("starship")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

#[test]
fn test_passthrough_directory_renders_when_starship_installed() {
    if !starship_available() {
        return; // skip silently in environments without starship
    }
    // Use a real workspace.current_dir (/tmp) so starship can spawn correctly (AC2).
    let json = r#"{"session_id":"test","cwd":"/tmp","transcript_path":"/tmp/t.jsonl","version":"1.0","exceeds_200k_tokens":false,"model":{"id":"claude-opus-4-6","display_name":"Opus"},"workspace":{"current_dir":"/tmp","project_dir":"/tmp"},"output_style":{"name":"default"},"cost":{"total_cost_usd":0.0}}"#;
    let output = cship()
        .args(["--config", "tests/fixtures/passthrough_directory.toml"])
        .write_stdin(json)
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        !stdout.trim().is_empty(),
        "expected non-empty output from $directory passthrough: {stdout:?}"
    );
}

#[test]
fn test_native_renders_alongside_passthrough_not_installed() {
    // sample_starship.toml: lines = ["$cship.model $git_branch", "$cship.cost"]
    // Native $cship.model renders "Opus" regardless of whether starship is present.
    // Passthrough $git_branch renders via starship (or None silently). No panic, exit 0.
    let json = std::fs::read_to_string("tests/fixtures/sample_input_full.json").unwrap();
    cship()
        .args(["--config", "tests/fixtures/sample_starship.toml"])
        .write_stdin(json)
        .assert()
        .success()
        .stdout(predicate::str::contains("Opus"));
}

// ── Story 4.2: Cache and CSHIP_* env var integration tests ────────────────

#[test]
fn test_passthrough_env_vars_injected_via_cship_model() {
    // Create a fake starship script that echoes $CSHIP_MODEL to stdout.
    // Uses .env("PATH", ...) on the cship subprocess rather than mutating
    // the test process's global PATH — safe for parallel test execution.
    use std::fs;
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;

    let dir = std::env::temp_dir().join("cship_inttest_cship_env");
    fs::create_dir_all(&dir).unwrap();
    let script = dir.join("starship");
    fs::write(&script, "#!/bin/sh\nprintf '%s' \"$CSHIP_MODEL\"\n").unwrap();
    #[cfg(unix)]
    fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();

    let json = r#"{"session_id":"test","cwd":"/tmp","transcript_path":"/tmp/cship_inttest_tp.jsonl","version":"1.0","exceeds_200k_tokens":false,"model":{"id":"claude-opus-4-6","display_name":"IntTestModel"},"workspace":{"current_dir":"/tmp","project_dir":"/tmp"},"output_style":{"name":"default"},"cost":{"total_cost_usd":0.0}}"#;

    let result = cship()
        .args(["--config", "tests/fixtures/passthrough_only.toml"])
        .env("PATH", dir.to_str().unwrap())
        .write_stdin(json)
        .output()
        .unwrap();

    let _ = fs::remove_dir_all(&dir);
    // Clean up cache file and directory written during this test
    let _ = fs::remove_file("/tmp/cship/cship_inttest_tp-starship-git_branch");
    let _ = fs::remove_dir("/tmp/cship");

    assert!(result.status.success());
    let stdout = String::from_utf8_lossy(&result.stdout);
    assert!(
        stdout.contains("IntTestModel"),
        "expected CSHIP_MODEL value 'IntTestModel' in passthrough output: {stdout:?}"
    );
}

#[test]
fn test_cache_hit_does_not_spawn_subprocess() {
    // Pre-write a cache file at the expected path, then verify cship returns it
    // without needing starship installed. Uses .env("PATH", ...) on the cship
    // subprocess to block starship lookup — safe for parallel test execution.
    use std::fs;

    // Cache path: {dirname(transcript_path)}/cship/{stem}-starship-{module}
    // transcript_path = "/tmp/cship_cache_inttest.jsonl"
    // → /tmp/cship/cship_cache_inttest-starship-git_branch
    let cache_dir = std::path::Path::new("/tmp/cship");
    fs::create_dir_all(cache_dir).unwrap();
    let cache_file = cache_dir.join("cship_cache_inttest-starship-git_branch");
    fs::write(&cache_file, "cached-branch-value").unwrap();

    let json = r#"{"session_id":"test","cwd":"/tmp","transcript_path":"/tmp/cship_cache_inttest.jsonl","version":"1.0","exceeds_200k_tokens":false,"model":{"id":"claude-opus-4-6","display_name":"Opus"},"workspace":{"current_dir":"/tmp","project_dir":"/tmp"},"output_style":{"name":"default"},"cost":{"total_cost_usd":0.0}}"#;

    let result = cship()
        .args(["--config", "tests/fixtures/passthrough_only.toml"])
        // Block starship subprocess to prove the cached value is used instead
        .env("PATH", "/nonexistent_cship_test_dir_42")
        .write_stdin(json)
        .output()
        .unwrap();

    let _ = fs::remove_file(&cache_file);
    let _ = fs::remove_dir(cache_dir);

    assert!(result.status.success());
    let stdout = String::from_utf8_lossy(&result.stdout);
    assert!(
        stdout.contains("cached-branch-value"),
        "expected cached value in output (no subprocess needed): {stdout:?}"
    );
}

// ── Story 2.5: Per-module format strings integration tests ────────────────

// AC1 — format style span with context_window.used_percentage
#[test]
fn test_context_window_format_style_span() {
    let json = std::fs::read_to_string("tests/fixtures/sample_input_full.json").unwrap();
    // sample_input_full.json: context_window.used_percentage = 8.0
    // context_window_format.toml: format = "[ ctx: $value% ]($style)", style = "bold green"
    cship()
        .args(["--config", "tests/fixtures/context_window_format.toml"])
        .write_stdin(json)
        .assert()
        .success()
        .stdout(predicate::str::contains("ctx: 8%"));
}

// AC2 — format with symbol in context_bar
#[test]
fn test_context_bar_format_with_symbol() {
    let json = std::fs::read_to_string("tests/fixtures/sample_input_full.json").unwrap();
    // context_bar_format.toml: format = "[$value $symbol]($style)", symbol = "🧠"
    cship()
        .args(["--config", "tests/fixtures/context_bar_format.toml"])
        .write_stdin(json)
        .assert()
        .success()
        .stdout(predicate::str::contains("🧠"));
}

// AC3 — conditional group with absent vim field → empty output
#[test]
fn test_vim_format_conditional_absent_produces_no_output() {
    // Inline JSON with no "vim" field
    let json = r#"{"model":{"id":"claude-opus-4-6","display_name":"Opus"}}"#;
    cship()
        .args(["--config", "tests/fixtures/vim_format_conditional.toml"])
        .write_stdin(json)
        .assert()
        .success()
        .stdout(predicate::str::is_empty());
}

// AC4 — conditional group with present vim field → renders value without parens
#[test]
fn test_vim_format_conditional_present_renders_value() {
    let json = std::fs::read_to_string("tests/fixtures/sample_input_full.json").unwrap();
    // sample_input_full.json: vim.mode = "NORMAL"
    cship()
        .args(["--config", "tests/fixtures/vim_format_conditional.toml"])
        .write_stdin(json)
        .assert()
        .success()
        .stdout(predicate::str::contains("NORMAL"));
}

// AC6 — literal text in lines[] preserved alongside module token
#[test]
fn test_literal_text_in_lines_preserved() {
    let json = std::fs::read_to_string("tests/fixtures/sample_input_full.json").unwrap();
    // literal_text.toml: lines = ["in: $cship.context_window.total_input_tokens"]
    // sample_input_full.json: total_input_tokens = 15234
    cship()
        .args(["--config", "tests/fixtures/literal_text.toml"])
        .write_stdin(json)
        .assert()
        .success()
        .stdout(predicate::str::contains("in: 15234"));
}
