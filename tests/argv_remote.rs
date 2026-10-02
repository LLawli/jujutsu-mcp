//! argv construction and dry-run parsing for network tools.

use jujutsu_mcp::tools::ToolError;
use jujutsu_mcp::tools::remote::{
    GitFetchParams, GitPushParams, commits_to_sign_line, git_fetch_args, git_push_args,
    push_targets,
};

fn s(list: &[&str]) -> Vec<String> {
    list.iter().map(|x| (*x).to_owned()).collect()
}

fn assert_invalid(result: Result<Vec<String>, ToolError>, mentions: &str) {
    match result {
        Err(err @ ToolError::InvalidParams(_)) => {
            assert!(err.to_string().contains(mentions), "{err}")
        }
        other => panic!("expected InvalidParams mentioning {mentions}, got {other:?}"),
    }
}

fn fetch() -> GitFetchParams {
    GitFetchParams {
        repo: "/repo".to_owned(),
        remote: None,
        all_remotes: false,
        branches: Vec::new(),
    }
}

fn push() -> GitPushParams {
    GitPushParams {
        repo: "/repo".to_owned(),
        bookmarks: Vec::new(),
        changes: Vec::new(),
        remote: None,
        dry_run: false,
    }
}

#[test]
fn fetch_shapes() {
    assert_eq!(git_fetch_args(&fetch()).unwrap(), s(&["git", "fetch"]));

    let mut params = fetch();
    params.remote = Some("origin".to_owned());
    params.branches = s(&["main", "feat*"]);
    assert_eq!(
        git_fetch_args(&params).unwrap(),
        s(&[
            "git",
            "fetch",
            "--remote=origin",
            "--branch=main",
            "--branch=feat*"
        ])
    );

    let mut params = fetch();
    params.all_remotes = true;
    assert_eq!(
        git_fetch_args(&params).unwrap(),
        s(&["git", "fetch", "--all-remotes"])
    );
}

#[test]
fn fetch_rejects_invalid_combinations() {
    let mut params = fetch();
    params.remote = Some("origin".to_owned());
    params.all_remotes = true;
    assert_invalid(git_fetch_args(&params), "remote");

    let mut params = fetch();
    params.branches = s(&[" "]);
    assert_invalid(git_fetch_args(&params), "branches");
}

#[test]
fn push_shapes() {
    assert_eq!(git_push_args(&push(), false).unwrap(), s(&["git", "push"]));

    let mut params = push();
    params.remote = Some("origin".to_owned());
    params.bookmarks = s(&["a", "-b"]);
    params.changes = s(&["@-"]);
    assert_eq!(
        git_push_args(&params, false).unwrap(),
        s(&[
            "git",
            "push",
            "--remote=origin",
            "--bookmark=a",
            "--bookmark=-b",
            "--change=@-"
        ])
    );
    assert_eq!(
        git_push_args(&params, true)
            .unwrap()
            .last()
            .map(String::as_str),
        Some("--dry-run")
    );
}

#[test]
fn push_rejects_empty_names() {
    let mut params = push();
    params.bookmarks = s(&[""]);
    assert_invalid(git_push_args(&params, false), "bookmarks");

    let mut params = push();
    params.changes = s(&[" "]);
    assert_invalid(git_push_args(&params, false), "changes");

    let mut params = push();
    params.remote = Some(String::new());
    assert_invalid(git_push_args(&params, false), "remote");
}

#[test]
fn dry_run_targets_skip_deletions() {
    let output = "\
Changes to push to origin:
  bookmark: feat [add to 37710df6fd4a]
  bookmark: main [move forward from 369af8f34fe5 to 7548f12d8d54]
  bookmark: side [move sideways from aaaaaaaaaaaa to bbbbbbbbbbbb]
  bookmark: back [move backward from dddddddddddd to eeeeeeeeeeee]
  bookmark: old [delete from cccccccccccc]
Dry-run requested, not pushing.
";
    assert_eq!(
        push_targets(output),
        s(&[
            "37710df6fd4a",
            "7548f12d8d54",
            "bbbbbbbbbbbb",
            "eeeeeeeeeeee"
        ])
    );
}

#[test]
fn dry_run_with_nothing_to_push() {
    assert!(push_targets("Nothing changed.\n").is_empty());
    assert!(push_targets("").is_empty());
}

#[test]
fn summary_line() {
    assert_eq!(commits_to_sign_line(3), "commits to sign: 3");
}
