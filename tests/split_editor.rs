//! The non-interactive diff editor behind `split` with `contents`.

use std::fs;
use std::process::Command;

use jujutsu_mcp::split_editor::{COMMAND, SplitEditorError, fill_right, run};

#[test]
fn copies_staged_files_into_right_overwriting() {
    let staged = tempfile::tempdir().expect("staged dir");
    let right = tempfile::tempdir().expect("right dir");
    fs::create_dir_all(staged.path().join("src/deep")).expect("mkdir");
    fs::write(staged.path().join("a.txt"), "staged a\n").expect("write");
    fs::write(staged.path().join("src/deep/b.txt"), "staged b\n").expect("write");
    fs::write(right.path().join("a.txt"), "old a\n").expect("write");
    fs::write(right.path().join("untouched.txt"), "keep\n").expect("write");

    fill_right(staged.path(), right.path()).expect("fill");

    let read = |path: &str| fs::read_to_string(right.path().join(path)).expect("read");
    assert_eq!(read("a.txt"), "staged a\n");
    assert_eq!(read("src/deep/b.txt"), "staged b\n");
    assert_eq!(read("untouched.txt"), "keep\n");
}

#[test]
fn missing_staged_dir_is_an_error() {
    let right = tempfile::tempdir().expect("right dir");
    let missing = right.path().join("no-such-dir");
    let err = fill_right(&missing, right.path()).expect_err("must fail");
    assert!(matches!(err, SplitEditorError::Io { .. }), "{err:?}");
}

#[test]
fn run_needs_two_arguments() {
    assert!(matches!(run(&[]), Err(SplitEditorError::Usage)));
    assert!(matches!(
        run(&["one".to_owned()]),
        Err(SplitEditorError::Usage)
    ));
    assert!(matches!(
        run(&["a".to_owned(), "b".to_owned(), "c".to_owned()]),
        Err(SplitEditorError::Usage)
    ));
}

#[test]
fn the_binary_runs_as_the_split_editor() {
    let staged = tempfile::tempdir().expect("staged dir");
    let right = tempfile::tempdir().expect("right dir");
    fs::write(staged.path().join("a.txt"), "staged\n").expect("write");
    let output = Command::new(env!("CARGO_BIN_EXE_jujutsu-mcp"))
        .arg(COMMAND)
        .arg(staged.path())
        .arg(right.path())
        .output()
        .expect("binary runs");
    assert!(output.status.success(), "{output:?}");
    assert_eq!(
        fs::read_to_string(right.path().join("a.txt")).expect("read"),
        "staged\n"
    );

    let usage = Command::new(env!("CARGO_BIN_EXE_jujutsu-mcp"))
        .arg(COMMAND)
        .output()
        .expect("binary runs");
    assert_eq!(usage.status.code(), Some(2), "{usage:?}");
}
