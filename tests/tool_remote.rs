//! git_fetch and git_push against a bare remote, signing with a software key.

mod common;

use common::{SigningRemote, TestRepo, call_text, call_tool_error};
use serde_json::json;

/// A repository with `origin`, one commit "one" and bookmark `feat` on it.
fn repo_with_feat(remote: &SigningRemote) -> TestRepo {
    let repo = remote.repo();
    repo.write("a.txt", "a\n");
    repo.jj(&["commit", "-m", "one"]);
    repo.jj(&["bookmark", "create", "feat", "-r", "@-"]);
    repo
}

#[tokio::test]
async fn push_creates_the_bookmark_with_signed_commits() {
    let remote = SigningRemote::new();
    let repo = repo_with_feat(&remote);
    let client = repo.client().await;

    let text = call_text(
        &client,
        "git_push",
        json!({ "repo": repo.repo_arg(), "bookmarks": ["feat"] }),
    )
    .await;
    assert!(text.starts_with("commits to sign: 1"), "{text}");

    let pushed = remote.branch("feat").expect("feat on the remote");
    assert!(remote.signature_is_valid(&pushed), "unsigned: {pushed}");
    let local = repo.jj(&["log", "--no-graph", "-r", "feat", "-T", "commit_id"]);
    assert_eq!(pushed, local);
}

#[tokio::test]
async fn dry_run_counts_commits_without_pushing() {
    let remote = SigningRemote::new();
    let repo = repo_with_feat(&remote);
    let client = repo.client().await;
    call_text(
        &client,
        "git_push",
        json!({ "repo": repo.repo_arg(), "bookmarks": ["feat"] }),
    )
    .await;
    let before = remote.branch("feat").expect("feat pushed");

    repo.write("b.txt", "b\n");
    repo.jj(&["commit", "-m", "two"]);
    repo.write("c.txt", "c\n");
    repo.jj(&["commit", "-m", "three"]);
    repo.jj(&["bookmark", "set", "feat", "-r", "@-"]);

    let text = call_text(
        &client,
        "git_push",
        json!({ "repo": repo.repo_arg(), "bookmarks": ["feat"], "dry_run": true }),
    )
    .await;
    assert!(text.starts_with("commits to sign: 2"), "{text}");
    assert!(text.contains("move forward"), "{text}");
    assert_eq!(remote.branch("feat"), Some(before), "dry run pushed");

    let text = call_text(
        &client,
        "git_push",
        json!({ "repo": repo.repo_arg(), "bookmarks": ["feat"] }),
    )
    .await;
    assert!(text.starts_with("commits to sign: 2"), "{text}");
    let pushed = remote.branch("feat").expect("feat on the remote");
    assert!(remote.signature_is_valid(&pushed), "unsigned: {pushed}");
}

#[tokio::test]
async fn push_with_nothing_new_signs_nothing() {
    let remote = SigningRemote::new();
    let repo = repo_with_feat(&remote);
    let client = repo.client().await;
    call_text(
        &client,
        "git_push",
        json!({ "repo": repo.repo_arg(), "bookmarks": ["feat"] }),
    )
    .await;
    let text = call_text(
        &client,
        "git_push",
        json!({ "repo": repo.repo_arg(), "bookmarks": ["feat"], "dry_run": true }),
    )
    .await;
    assert!(text.starts_with("commits to sign: 0"), "{text}");
}

#[tokio::test]
async fn push_failure_is_a_tool_error() {
    let remote = SigningRemote::new();
    let repo = repo_with_feat(&remote);
    let client = repo.client().await;
    let text = call_tool_error(
        &client,
        "git_push",
        json!({ "repo": repo.repo_arg(), "bookmarks": ["no_such_bookmark"] }),
    )
    .await;
    assert!(text.contains("no_such_bookmark"), "{text}");
}

#[tokio::test]
async fn fetch_brings_remote_bookmarks() {
    let remote = SigningRemote::new();
    let pusher = repo_with_feat(&remote);
    let client = pusher.client().await;
    call_text(
        &client,
        "git_push",
        json!({ "repo": pusher.repo_arg(), "bookmarks": ["feat"] }),
    )
    .await;

    let other = remote.repo();
    let client = other.client().await;
    call_text(
        &client,
        "git_fetch",
        json!({ "repo": other.repo_arg(), "remote": "origin" }),
    )
    .await;
    let fetched = other.jj(&["log", "--no-graph", "-r", "feat@origin", "-T", "commit_id"]);
    assert_eq!(Some(fetched), remote.branch("feat"));
}

#[tokio::test]
async fn network_tools_are_annotated() {
    let repo = TestRepo::new();
    let client = repo.client().await;
    let tools = client.list_all_tools().await.expect("tools/list");
    for (name, destructive) in [("git_fetch", false), ("git_push", true)] {
        let tool = tools
            .iter()
            .find(|tool| tool.name == name)
            .unwrap_or_else(|| panic!("{name} not listed"));
        let annotations = tool.annotations.as_ref().expect("annotations");
        assert_eq!(annotations.read_only_hint, Some(false), "{name}");
        assert_eq!(annotations.open_world_hint, Some(true), "{name}");
        assert_eq!(annotations.destructive_hint, Some(destructive), "{name}");
    }
}
