//! status, show, diff, bookmark_list and op_log end to end.

mod common;

use common::{TestRepo, call_ok, call_text, call_tool_error};
use serde_json::{Value, json};

/// first (a.txt) <- second (b.txt, bookmark `feat`) <- @ (edits a.txt,
/// adds "dir with space/f (1).txt").
fn repo_with_history() -> TestRepo {
    let repo = TestRepo::new();
    repo.write("a.txt", "a\n");
    repo.jj(&["describe", "-m", "first"]);
    repo.jj(&["new"]);
    repo.write("b.txt", "b\n");
    repo.jj(&["describe", "-m", "second"]);
    repo.jj(&["bookmark", "create", "feat", "-r", "@"]);
    repo.jj(&["new"]);
    repo.write("a.txt", "a changed\n");
    repo.write("dir with space/f (1).txt", "f\n");
    repo
}

/// Non-empty lines, sorted, with `/` as separator (jj prints `\` on Windows).
fn lines(text: &str) -> Vec<String> {
    let mut lines: Vec<String> = text
        .lines()
        .filter(|l| !l.is_empty())
        .map(|l| l.replace('\\', "/"))
        .collect();
    lines.sort_unstable();
    lines
}

#[tokio::test]
async fn status_lists_working_copy_changes() {
    let repo = repo_with_history();
    let client = repo.client().await;
    let text = call_text(&client, "status", json!({ "repo": repo.repo_arg() }))
        .await
        .replace('\\', "/");
    assert!(text.contains("a.txt"), "{text}");
    assert!(text.contains("dir with space/f (1).txt"), "{text}");
    assert!(text.contains("Working copy"), "{text}");
}

#[tokio::test]
async fn show_renders_a_revision() {
    let repo = repo_with_history();
    let client = repo.client().await;
    let text = call_text(
        &client,
        "show",
        json!({ "repo": repo.repo_arg(), "revision": "feat", "format": "git" }),
    )
    .await;
    assert!(text.contains("second"), "{text}");
    assert!(text.contains("diff --git a/b.txt b/b.txt"), "{text}");
    assert!(!text.contains("a changed"), "{text}");
}

#[tokio::test]
async fn show_defaults_to_the_working_copy() {
    let repo = repo_with_history();
    let client = repo.client().await;
    let text = call_text(&client, "show", json!({ "repo": repo.repo_arg() })).await;
    assert!(text.contains("a.txt"), "{text}");
    assert!(!text.contains("b.txt"), "{text}");
}

#[tokio::test]
async fn diff_of_the_working_copy_by_name() {
    let repo = repo_with_history();
    let client = repo.client().await;
    let text = call_text(
        &client,
        "diff",
        json!({ "repo": repo.repo_arg(), "format": "name_only" }),
    )
    .await;
    assert_eq!(lines(&text), ["a.txt", "dir with space/f (1).txt"]);
}

#[tokio::test]
async fn diff_paths_are_literal() {
    let repo = repo_with_history();
    let client = repo.client().await;
    let text = call_text(
        &client,
        "diff",
        json!({
            "repo": repo.repo_arg(),
            "format": "name_only",
            "paths": ["dir with space/f (1).txt"]
        }),
    )
    .await;
    assert_eq!(lines(&text), ["dir with space/f (1).txt"]);
}

#[tokio::test]
async fn diff_paths_are_relative_to_repo() {
    let repo = repo_with_history();
    let client = repo.client().await;
    let sub = repo.path.join("dir with space");
    let text = call_text(
        &client,
        "diff",
        json!({
            "repo": sub.to_str().expect("utf-8"),
            "format": "name_only",
            "paths": ["f (1).txt"]
        }),
    )
    .await;
    assert_eq!(lines(&text).len(), 1, "{text}");
    assert!(text.contains("f (1).txt"), "{text}");
}

#[tokio::test]
async fn diff_between_revisions_and_of_a_revision() {
    let repo = repo_with_history();
    let client = repo.client().await;
    let between = call_text(
        &client,
        "diff",
        json!({ "repo": repo.repo_arg(), "from": "feat-", "to": "feat", "format": "name_only" }),
    )
    .await;
    assert_eq!(lines(&between), ["b.txt"]);

    let of = call_text(
        &client,
        "diff",
        json!({ "repo": repo.repo_arg(), "revisions": "feat", "format": "stat" }),
    )
    .await;
    assert!(of.contains("b.txt"), "{of}");
    assert!(of.contains("1 file changed"), "{of}");
}

#[tokio::test]
async fn diff_rejects_conflicting_selectors() {
    let repo = repo_with_history();
    let client = repo.client().await;
    let text = call_tool_error(
        &client,
        "diff",
        json!({ "repo": repo.repo_arg(), "revisions": "@", "from": "feat" }),
    )
    .await;
    assert!(text.contains("revisions"), "{text}");
}

#[tokio::test]
async fn bookmark_list_returns_local_bookmarks() {
    let repo = repo_with_history();
    repo.jj(&["bookmark", "create", "other", "-r", "@-"]);
    let client = repo.client().await;
    let out = call_ok(&client, "bookmark_list", json!({ "repo": repo.repo_arg() })).await;
    let bookmarks = out["bookmarks"].as_array().expect("bookmarks");
    let feat_commit = repo.jj(&["log", "--no-graph", "-r", "feat", "-T", "commit_id"]);
    let names: Vec<&str> = bookmarks
        .iter()
        .map(|b| b["name"].as_str().expect("name"))
        .collect();
    assert_eq!(names, ["feat", "other"], "{out:#}");
    let feat = &bookmarks[0];
    assert_eq!(feat["remote"], Value::Null);
    assert_eq!(feat["target"], json!([feat_commit]));
    assert_eq!(feat["conflict"], json!(false));
}

#[tokio::test]
async fn bookmark_list_skips_the_git_remote_and_filters_names() {
    let repo = repo_with_history();
    repo.jj(&["bookmark", "create", "other", "-r", "@-"]);
    let client = repo.client().await;
    let out = call_ok(
        &client,
        "bookmark_list",
        json!({ "repo": repo.repo_arg(), "all_remotes": true, "names": ["feat"] }),
    )
    .await;
    let bookmarks = out["bookmarks"].as_array().expect("bookmarks");
    assert_eq!(bookmarks.len(), 1, "{out:#}");
    assert_eq!(bookmarks[0]["name"], json!("feat"));
    assert!(
        bookmarks.iter().all(|b| b["remote"] != json!("git")),
        "{out:#}"
    );
}

#[tokio::test]
async fn op_log_returns_recent_operations() {
    let repo = repo_with_history();
    let client = repo.client().await;
    let out = call_ok(
        &client,
        "op_log",
        json!({ "repo": repo.repo_arg(), "limit": 3 }),
    )
    .await;
    let ops = out["operations"].as_array().expect("operations");
    assert_eq!(ops.len(), 3, "{out:#}");
    assert_eq!(out["truncated"], json!(true));
    for op in ops {
        let id = op["id"].as_str().expect("id");
        assert_eq!(id.len(), 12, "{op}");
        assert!(id.chars().all(|c| c.is_ascii_hexdigit()), "{op}");
        assert!(op["time"].as_str().is_some_and(|t| !t.is_empty()), "{op}");
        assert!(op["snapshot"].is_boolean(), "{op}");
    }

    let all = call_ok(
        &client,
        "op_log",
        json!({ "repo": repo.repo_arg(), "limit": 50 }),
    )
    .await;
    let ops = all["operations"].as_array().expect("operations");
    assert_eq!(all["truncated"], json!(false));
    assert!(
        ops.iter().any(|op| op["args"]
            .as_str()
            .is_some_and(|a| a.contains("bookmark create feat"))),
        "{all:#}"
    );
    assert!(
        ops.iter().any(|op| op["snapshot"] == json!(true)),
        "{all:#}"
    );
}

#[tokio::test]
async fn read_tools_are_annotated() {
    let repo = TestRepo::new();
    let client = repo.client().await;
    let tools = client.list_all_tools().await.expect("tools/list");
    for name in ["status", "show", "diff", "bookmark_list", "op_log"] {
        let tool = tools
            .iter()
            .find(|tool| tool.name == name)
            .unwrap_or_else(|| panic!("{name} not listed"));
        let annotations = tool.annotations.as_ref().expect("annotations");
        assert_eq!(annotations.read_only_hint, Some(true), "{name}");
        let structured = matches!(name, "bookmark_list" | "op_log");
        assert_eq!(tool.output_schema.is_some(), structured, "{name}");
    }
}
