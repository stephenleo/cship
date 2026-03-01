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
        .stdout("");
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
