//! CLI integration tests.
//!
//! Runs the `meeting-agent` binary as a subprocess with `HOME` set to a temp
//! dir so `~/.meeting-agent` is fully isolated from the real filesystem.

use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::tempdir;

/// Helper: build a Command with HOME pointed at a temp dir.
fn cli() -> (Command, tempfile::TempDir) {
    let tmp = tempdir().unwrap();
    let cmd = cmd_with_home(tmp.path().to_path_buf());
    (cmd, tmp)
}

/// Build a Command reusing an existing temp dir path.
fn cmd_with_home(home: std::path::PathBuf) -> Command {
    let mut cmd = Command::cargo_bin("meeting-agent").unwrap();
    cmd.env("HOME", home.clone());
    // Set cwd to temp dir so the binary's dotenv::dotenv() doesn't find
    // the real .env file in the workspace root.
    cmd.current_dir(home);
    // Clear env vars that could override config values.
    cmd.env_remove("TRANSCRIPTION_PROVIDER");
    cmd.env_remove("TRANSCRIPTION_API_KEY");
    cmd.env_remove("TRANSCRIPTION_BASE_URL");
    cmd.env_remove("TRANSCRIPTION_MODEL");
    cmd.env_remove("SUMMARY_PROVIDER");
    cmd.env_remove("SUMMARY_API_KEY");
    cmd.env_remove("MEETING_AGENT_API_KEY");
    cmd.env_remove("MEETING_AGENT_PORT");
    cmd.env_remove("MEETING_AGENT_HOST");
    cmd
}

#[test]
fn config_show_creates_default_and_succeeds() {
    let (mut cmd, _tmp) = cli();
    cmd.arg("config").arg("show");
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("Current Configuration"))
        .stdout(predicate::str::contains("Transcription"));
}

#[test]
fn config_set_then_show_reflects_value() {
    let (mut cmd, tmp) = cli();
    cmd.arg("config").arg("set").arg("server.port").arg("9999");
    cmd.assert().success();

    let mut cmd = cmd_with_home(tmp.path().to_path_buf());
    cmd.arg("config").arg("show");
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("9999"));
}

#[test]
fn config_set_invalid_key_fails() {
    let (mut cmd, _tmp) = cli();
    cmd.arg("config").arg("set").arg("invalid.key").arg("foo");
    cmd.assert().failure();
}

#[test]
fn config_set_persists_chunk_seconds() {
    let (mut cmd, tmp) = cli();
    cmd.arg("config")
        .arg("set")
        .arg("transcription.chunk_seconds")
        .arg("300");
    cmd.assert().success();

    let mut cmd = cmd_with_home(tmp.path().to_path_buf());
    cmd.arg("config").arg("show");
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("300"));
}

#[test]
fn list_on_empty_succeeds() {
    let (mut cmd, _tmp) = cli();
    cmd.arg("list");
    cmd.assert().success();
}

#[test]
fn import_missing_file_errors() {
    let (mut cmd, _tmp) = cli();
    cmd.arg("import").arg("/nonexistent/audio.mp3");
    cmd.assert().failure();
}

#[test]
fn show_bad_id_errors() {
    let (mut cmd, _tmp) = cli();
    cmd.arg("show").arg("deadbeef");
    cmd.assert().failure();
}

#[test]
fn config_set_diarize_enabled() {
    let (mut cmd, tmp) = cli();
    cmd.arg("config")
        .arg("set")
        .arg("diarize.enabled")
        .arg("true");
    cmd.assert().success();

    let mut cmd = cmd_with_home(tmp.path().to_path_buf());
    cmd.arg("config").arg("show");
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("enabled:        true"));
}
