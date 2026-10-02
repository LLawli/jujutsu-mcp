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

/// In a colocated repository a synced trunk has `master@git` and
/// `master@origin` on the same commit. Commits already on the remote must
/// not be counted as commits to sign.
#[tokio::test]
async fn dry_run_ignores_commits_already_on_the_remote() {
    let remote = SigningRemote::new();
    let repo = remote.repo();
    for name in ["t1", "t2", "t3"] {
        repo.write(&format!("{name}.txt"), "t\n");
        repo.jj(&["commit", "-m", name]);
    }
    repo.jj(&["bookmark", "create", "master", "-r", "@-"]);
    let client = repo.client().await;
    call_text(
        &client,
        "git_push",
        json!({ "repo": repo.repo_arg(), "bookmarks": ["master"] }),
    )
    .await;

    repo.write("f1.txt", "f\n");
    repo.jj(&["commit", "-m", "f1"]);
    repo.jj(&["bookmark", "create", "feat", "-r", "@-"]);
    let text = call_text(
        &client,
        "git_push",
        json!({ "repo": repo.repo_arg(), "bookmarks": ["feat"], "dry_run": true }),
    )
    .await;
    assert!(text.starts_with("commits to sign: 1"), "{text}");

    call_text(
        &client,
        "git_push",
        json!({ "repo": repo.repo_arg(), "bookmarks": ["feat"] }),
    )
    .await;
    repo.write("f2.txt", "f\n");
    repo.jj(&["commit", "-m", "f2"]);
    repo.jj(&["bookmark", "set", "feat", "-r", "@-"]);
    let text = call_text(
        &client,
        "git_push",
        json!({ "repo": repo.repo_arg(), "bookmarks": ["feat"], "dry_run": true }),
    )
    .await;
    assert!(text.starts_with("commits to sign: 1"), "{text}");
}

/// `commits to sign: N` of a dry run of `args`.
async fn announced(client: &common::Client, args: serde_json::Value) -> String {
    let mut args = args;
    args["dry_run"] = json!(true);
    let text = call_text(client, "git_push", args).await;
    text.lines().next().unwrap_or_default().to_owned()
}

/// A repository with `main` (one commit) published on `origin`.
async fn repo_with_published_main(remote: &SigningRemote) -> (TestRepo, common::Client) {
    let repo = remote.repo();
    repo.write("base.txt", "base\n");
    repo.jj(&["commit", "-m", "base"]);
    repo.jj(&["bookmark", "create", "main", "-r", "@-"]);
    let client = repo.client().await;
    call_text(
        &client,
        "git_push",
        json!({ "repo": repo.repo_arg(), "bookmarks": ["main"] }),
    )
    .await;
    (repo, client)
}

/// jj signs only commits authored by the user: a coworker's commits,
/// rewritten by a rebase, are pushed without a touch.
#[tokio::test]
async fn dry_run_skips_commits_authored_by_others() {
    let remote = SigningRemote::new();
    let (repo, client) = repo_with_published_main(&remote).await;
    for name in ["o1", "o2"] {
        repo.write(&format!("{name}.txt"), "o\n");
        repo.jj(&["metaedit", "--author", "Other <other@example.com>"]);
        repo.jj(&["commit", "-m", name]);
    }
    repo.jj(&["bookmark", "create", "pr", "-r", "@-"]);
    call_text(
        &client,
        "git_push",
        json!({ "repo": repo.repo_arg(), "bookmarks": ["pr"] }),
    )
    .await;

    repo.jj(&["new", "main"]);
    repo.write("main2.txt", "m\n");
    repo.jj(&["commit", "-m", "main2"]);
    repo.jj(&["bookmark", "set", "main", "-r", "@-"]);
    call_text(
        &client,
        "git_push",
        json!({ "repo": repo.repo_arg(), "bookmarks": ["main"] }),
    )
    .await;
    repo.jj(&["rebase", "-b", "pr", "-d", "main"]);
    repo.jj(&["new", "pr"]);
    repo.write("mine.txt", "m\n");
    repo.jj(&["commit", "-m", "mine"]);
    repo.jj(&["bookmark", "set", "pr", "-r", "@-"]);

    let line = announced(
        &client,
        json!({ "repo": repo.repo_arg(), "bookmarks": ["pr"] }),
    )
    .await;
    assert_eq!(line, "commits to sign: 1");
}

/// Commits signed by an earlier push need no touch, even when the remote
/// lost the branch that held them.
#[tokio::test]
async fn dry_run_skips_commits_already_signed() {
    let remote = SigningRemote::new();
    let (repo, client) = repo_with_published_main(&remote).await;
    for name in ["f1", "f2"] {
        repo.write(&format!("{name}.txt"), "f\n");
        repo.jj(&["commit", "-m", name]);
    }
    repo.jj(&["bookmark", "create", "feat", "-r", "@-"]);
    call_text(
        &client,
        "git_push",
        json!({ "repo": repo.repo_arg(), "bookmarks": ["feat"] }),
    )
    .await;
    let signed_tip = repo.jj(&["log", "--no-graph", "-r", "feat", "-T", "commit_id"]);

    let deleted = std::process::Command::new("git")
        .arg("-C")
        .arg(&remote.bare)
        .args(["branch", "-D", "feat"])
        .output()
        .expect("git must be on PATH");
    assert!(deleted.status.success(), "{deleted:?}");
    repo.jj(&["git", "fetch"]);
    repo.jj(&["bookmark", "create", "feat", "-r", &signed_tip]);
    repo.jj(&["bookmark", "track", "feat@origin"]);

    let line = announced(
        &client,
        json!({ "repo": repo.repo_arg(), "bookmarks": ["feat"] }),
    )
    .await;
    assert_eq!(line, "commits to sign: 0");
}

/// jj does not sign immutable commits (here, one under a tag); it pushes
/// them unsigned with a warning.
#[tokio::test]
async fn dry_run_skips_immutable_commits() {
    let remote = SigningRemote::new();
    let (repo, client) = repo_with_published_main(&remote).await;
    repo.write("tagged.txt", "t\n");
    repo.jj(&["commit", "-m", "tagged"]);
    let tagged = repo.jj(&["log", "--no-graph", "-r", "@-", "-T", "commit_id"]);
    let tagged_ok = std::process::Command::new("git")
        .arg("-C")
        .arg(&repo.path)
        .args(["-c", "tag.gpgSign=false", "tag", "v9", &tagged])
        .output()
        .expect("git must be on PATH");
    assert!(tagged_ok.status.success(), "{tagged_ok:?}");
    repo.jj(&["git", "import"]);
    repo.write("f1.txt", "f\n");
    repo.jj(&["commit", "-m", "f1"]);
    repo.jj(&["bookmark", "create", "feat", "-r", "@-"]);

    let line = announced(
        &client,
        json!({ "repo": repo.repo_arg(), "bookmarks": ["feat"] }),
    )
    .await;
    assert_eq!(line, "commits to sign: 1");
}

/// Only the destination remote counts: commits that reached another remote
/// unsigned are still signed when pushed to this one.
#[tokio::test]
async fn dry_run_counts_commits_missing_from_the_destination_remote() {
    let remote = SigningRemote::new();
    let upstream = SigningRemote::new();
    let repo = remote.repo();
    let upstream_path = upstream.bare.to_str().expect("utf-8 path");
    repo.jj(&["git", "remote", "add", "upstream", upstream_path]);
    for name in ["u1", "u2"] {
        repo.write(&format!("{name}.txt"), "u\n");
        repo.jj(&["commit", "-m", name]);
    }
    repo.jj(&["bookmark", "create", "feat", "-r", "@-"]);
    repo.jj(&["bookmark", "track", "feat@upstream"]);
    repo.jj(&["bookmark", "track", "feat@origin"]);
    repo.jj(&[
        "--config",
        "git.sign-on-push=false",
        "git",
        "push",
        "--remote",
        "upstream",
        "--bookmark",
        "feat",
    ]);
    assert!(upstream.branch("feat").is_some(), "feat not on upstream");
    let client = repo.client().await;

    let line = announced(
        &client,
        json!({ "repo": repo.repo_arg(), "bookmarks": ["feat"], "remote": "origin" }),
    )
    .await;
    assert_eq!(line, "commits to sign: 2");
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
