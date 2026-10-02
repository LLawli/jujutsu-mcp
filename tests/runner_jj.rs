//! Runner against the real jj on PATH.

mod common;

use std::fs;
use std::time::Duration;

use common::TestRepo;
use jujutsu_mcp::jj::JjError;
use jujutsu_mcp::repo::RepoPath;

fn args(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| (*s).to_owned()).collect()
}

#[tokio::test]
async fn reads_the_working_copy_commit() {
    let repo = TestRepo::new();
    let expected = repo.jj(&["log", "-r", "@", "--no-graph", "-T", "change_id"]);
    let out = repo
        .runner()
        .run(
            &repo.repo_path(),
            &args(&["log", "-r", "@", "--no-graph", "-T", "change_id"]),
        )
        .await
        .expect("log succeeds");
    assert!(!expected.is_empty());
    assert_eq!(out.stdout, expected);
}

#[tokio::test]
async fn reports_what_jj_did_on_stderr() {
    let repo = TestRepo::new();
    let out = repo
        .runner()
        .run(&repo.repo_path(), &args(&["new"]))
        .await
        .expect("new succeeds");
    assert!(out.stderr.contains("Working copy"), "{out:?}");
}

#[tokio::test]
async fn failing_call_carries_jj_stderr() {
    let repo = TestRepo::new();
    let err = repo
        .runner()
        .run(
            &repo.repo_path(),
            &args(&["log", "-r", "no_such_revision_xyz"]),
        )
        .await
        .expect_err("unknown revision fails");
    match err {
        JjError::Failed { code, stderr, .. } => {
            assert!(matches!(code, Some(c) if c != 0), "{code:?}");
            assert!(stderr.contains("no_such_revision_xyz"), "{stderr}");
        }
        other => panic!("expected Failed, got {other:?}"),
    }
}

#[tokio::test]
async fn describe_without_message_fails_instead_of_opening_an_editor() {
    let repo = TestRepo::new();
    let run = repo.runner();
    let path = repo.repo_path();
    let call = args(&["describe"]);
    let result = tokio::time::timeout(Duration::from_secs(20), run.run(&path, &call))
        .await
        .expect("jj must not block waiting for an editor");
    assert!(matches!(result, Err(JjError::Failed { .. })), "{result:?}");
}

#[tokio::test]
async fn works_from_a_subdirectory() {
    let repo = TestRepo::new();
    repo.write("nested/dir/file.txt", "hello\n");
    let sub = RepoPath::new(repo.path.join("nested/dir")).expect("subdir");
    let out = repo
        .runner()
        .run(&sub, &args(&["root"]))
        .await
        .expect("root succeeds");
    // Compared canonicalized: on Windows, canonical paths carry the `\\?\` prefix.
    let root = std::path::Path::new(out.stdout.trim_end())
        .canonicalize()
        .expect("jj root exists");
    assert_eq!(root, repo.path);
}

#[tokio::test]
async fn overrides_user_color_and_pager_settings() {
    let repo = TestRepo::with_config(
        r#"
[ui]
color = "always"
paginate = "auto"
pager = "false"
"#,
    );
    repo.write("file.txt", "content\n");
    let out = repo
        .runner()
        .run(&repo.repo_path(), &args(&["diff"]))
        .await
        .expect("diff succeeds");
    assert!(out.stdout.contains("file.txt"), "{out:?}");
    assert!(!out.stdout.contains('\u{1b}'), "ANSI escapes in {out:?}");
}

#[test]
fn fixture_config_is_isolated() {
    let repo = TestRepo::new();
    let config = fs::read_to_string(&repo.config).expect("config");
    assert!(config.contains("test@example.com"));
    let email = repo.jj(&["config", "get", "user.email"]);
    assert_eq!(email.trim(), "test@example.com");
}
