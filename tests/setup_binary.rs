//! The `setup` subcommand of the binary.

use std::fs;
use std::process::Command;

#[test]
fn setup_dry_run_prints_the_plan_and_writes_nothing() {
    let dir = tempfile::tempdir().expect("temp");
    let root = dir.path().canonicalize().expect("canonicalize");
    let empty_path = root.join("empty-bin");
    fs::create_dir(&empty_path).expect("mkdir");
    let output = Command::new(env!("CARGO_BIN_EXE_jujutsu-mcp"))
        .args(["setup", "--dry-run"])
        .env("PATH", &empty_path)
        .env("HOME", root.join("home"))
        .env("CARGO_HOME", root.join("cargo"))
        .env("CODEX_HOME", root.join("codex"))
        .output()
        .expect("binary runs");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "{stdout}\n{stderr}");
    let installed = root.join("cargo/bin/jujutsu-mcp");
    assert!(
        stdout.contains(&installed.display().to_string()),
        "{stdout}"
    );
    assert!(stdout.contains("not on PATH"), "{stdout}");
    assert!(!installed.exists());
}

#[test]
fn setup_rejects_unknown_options() {
    let output = Command::new(env!("CARGO_BIN_EXE_jujutsu-mcp"))
        .args(["setup", "--bogus"])
        .output()
        .expect("binary runs");
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("--bogus"), "{stderr}");
    assert!(stderr.contains("usage:"), "{stderr}");
}

#[test]
fn unknown_subcommands_are_rejected() {
    let output = Command::new(env!("CARGO_BIN_EXE_jujutsu-mcp"))
        .arg("bogus")
        .output()
        .expect("binary runs");
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("bogus"), "{stderr}");
}
