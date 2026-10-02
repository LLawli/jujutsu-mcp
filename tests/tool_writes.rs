//! Local write tools end to end against the real jj.

mod common;

use std::fs;

use common::{TestRepo, call_text, call_tool_error};
use serde_json::json;

/// base (a.txt) <- @ (b.txt, undescribed).
fn repo_with_base() -> TestRepo {
    let repo = TestRepo::new();
    repo.write("a.txt", "a\n");
    repo.jj(&["describe", "-m", "base"]);
    repo.jj(&["bookmark", "create", "base", "-r", "@"]);
    repo.jj(&["new"]);
    repo.write("b.txt", "b\n");
    repo
}

#[tokio::test]
async fn describe_sets_the_message_verbatim() {
    let repo = repo_with_base();
    let client = repo.client().await;
    let message = "-n looks like a flag\n\nbody with \"quotes\" and $HOME";
    call_text(
        &client,
        "describe",
        json!({ "repo": repo.repo_arg(), "message": message }),
    )
    .await;
    assert_eq!(repo.description("@"), format!("{message}\n"));

    call_text(
        &client,
        "describe",
        json!({ "repo": repo.repo_arg(), "message": "renamed", "revision": "base" }),
    )
    .await;
    assert_eq!(repo.description("base"), "renamed\n");
}

#[tokio::test]
async fn new_creates_a_child_and_reports_what_jj_did() {
    let repo = repo_with_base();
    let before = repo.change_id("@");
    let client = repo.client().await;
    let text = call_text(
        &client,
        "new",
        json!({ "repo": repo.repo_arg(), "message": "next" }),
    )
    .await;
    assert!(text.contains("Working copy"), "{text}");
    assert_eq!(repo.description("@"), "next\n");
    assert_eq!(repo.change_id("@-"), before);
}

#[tokio::test]
async fn new_with_two_parents_makes_a_merge() {
    let repo = repo_with_base();
    repo.jj(&["describe", "-m", "side"]);
    repo.jj(&["bookmark", "create", "side", "-r", "@"]);
    let client = repo.client().await;
    call_text(
        &client,
        "new",
        json!({ "repo": repo.repo_arg(), "parents": ["base", "side"] }),
    )
    .await;
    let parents = repo.jj(&["log", "--no-graph", "-r", "@-", "-T", "description"]);
    assert!(
        parents.contains("base") && parents.contains("side"),
        "{parents}"
    );
}

#[tokio::test]
async fn commit_with_paths_commits_only_those() {
    let repo = repo_with_base();
    repo.write("c.txt", "c\n");
    let client = repo.client().await;
    call_text(
        &client,
        "commit",
        json!({ "repo": repo.repo_arg(), "message": "only b", "paths": ["b.txt"] }),
    )
    .await;
    assert_eq!(repo.description("@-"), "only b\n");
    assert_eq!(repo.changed_files("@-"), ["b.txt"]);
    assert_eq!(repo.changed_files("@"), ["c.txt"]);
}

#[tokio::test]
async fn squash_moves_the_working_copy_into_its_parent() {
    let repo = repo_with_base();
    let client = repo.client().await;
    call_text(&client, "squash", json!({ "repo": repo.repo_arg() })).await;
    assert_eq!(repo.changed_files("base"), ["a.txt", "b.txt"]);
    assert!(repo.changed_files("@").is_empty());
}

#[tokio::test]
async fn squash_with_two_descriptions_needs_a_message() {
    let repo = repo_with_base();
    repo.jj(&["describe", "-m", "top"]);
    let client = repo.client().await;

    let text = call_tool_error(&client, "squash", json!({ "repo": repo.repo_arg() })).await;
    assert!(text.contains("squash"), "{text}");

    call_text(
        &client,
        "squash",
        json!({ "repo": repo.repo_arg(), "use_destination_message": true }),
    )
    .await;
    assert_eq!(repo.description("base"), "base\n");
    assert_eq!(repo.changed_files("base"), ["a.txt", "b.txt"]);
}

#[tokio::test]
async fn split_by_paths() {
    let repo = repo_with_base();
    repo.write("c.txt", "c\n");
    repo.jj(&["describe", "-m", "both"]);
    let client = repo.client().await;
    call_text(
        &client,
        "split",
        json!({ "repo": repo.repo_arg(), "paths": ["b.txt"], "message": "just b" }),
    )
    .await;
    assert_eq!(repo.description("@-"), "just b\n");
    assert_eq!(repo.changed_files("@-"), ["b.txt"]);
    assert_eq!(repo.description("@"), "both\n");
    assert_eq!(repo.changed_files("@"), ["c.txt"]);
}

#[tokio::test]
async fn edit_moves_the_working_copy() {
    let repo = repo_with_base();
    let base = repo.change_id("base");
    let client = repo.client().await;
    call_text(
        &client,
        "edit",
        json!({ "repo": repo.repo_arg(), "revision": "base" }),
    )
    .await;
    assert_eq!(repo.change_id("@"), base);
}

#[tokio::test]
async fn rebase_moves_a_commit_onto_another() {
    let repo = repo_with_base();
    repo.jj(&["describe", "-m", "top"]);
    repo.jj(&["new", "base"]);
    repo.write("s.txt", "s\n");
    repo.jj(&["describe", "-m", "sibling"]);
    repo.jj(&["bookmark", "create", "sibling", "-r", "@"]);
    let client = repo.client().await;
    call_text(
        &client,
        "rebase",
        json!({
            "repo": repo.repo_arg(),
            "revisions": "description(exact:\"top\n\")",
            "onto": "sibling"
        }),
    )
    .await;
    let parent = repo.jj(&[
        "log",
        "--no-graph",
        "-r",
        "description(exact:\"top\n\")-",
        "-T",
        "description",
    ]);
    assert_eq!(parent, "sibling\n");
}

#[tokio::test]
async fn restore_discards_working_copy_changes_to_a_path() {
    let repo = repo_with_base();
    repo.write("a.txt", "edited\n");
    let client = repo.client().await;
    call_text(
        &client,
        "restore",
        json!({ "repo": repo.repo_arg(), "paths": ["a.txt"] }),
    )
    .await;
    assert_eq!(fs::read_to_string(repo.path.join("a.txt")).unwrap(), "a\n");
    assert_eq!(repo.changed_files("@"), ["b.txt"]);
}

#[tokio::test]
async fn abandon_drops_a_commit_and_rebases_descendants() {
    let repo = repo_with_base();
    repo.jj(&["describe", "-m", "doomed"]);
    repo.jj(&["new"]);
    repo.write("d.txt", "d\n");
    let client = repo.client().await;
    call_text(
        &client,
        "abandon",
        json!({ "repo": repo.repo_arg(), "revisions": "@-" }),
    )
    .await;
    assert_eq!(repo.description("@-"), "base\n");
    let doomed = repo.jj(&[
        "log",
        "--no-graph",
        "-r",
        "description(exact:\"doomed\n\")",
        "-T",
        "change_id",
    ]);
    assert!(doomed.is_empty(), "{doomed}");
}

#[tokio::test]
async fn undo_reverts_the_last_operation() {
    let repo = repo_with_base();
    repo.jj(&["describe", "-m", "original"]);
    let client = repo.client().await;
    call_text(
        &client,
        "describe",
        json!({ "repo": repo.repo_arg(), "message": "changed" }),
    )
    .await;
    assert_eq!(repo.description("@"), "changed\n");
    call_text(&client, "undo", json!({ "repo": repo.repo_arg() })).await;
    assert_eq!(repo.description("@"), "original\n");
}

#[tokio::test]
async fn file_untrack_stops_tracking_an_ignored_file() {
    let repo = repo_with_base();
    repo.write("x.log", "noise\n");
    repo.jj(&["status"]);
    repo.write(".gitignore", "x.log\n");
    let client = repo.client().await;
    call_text(
        &client,
        "file_untrack",
        json!({ "repo": repo.repo_arg(), "paths": ["x.log"] }),
    )
    .await;
    let files = repo.jj(&["file", "list"]);
    assert!(!files.lines().any(|f| f == "x.log"), "{files}");
    assert!(repo.path.join("x.log").exists());
}

#[tokio::test]
async fn bookmark_set_creates_and_moves() {
    let repo = repo_with_base();
    repo.jj(&["describe", "-m", "top"]);
    let client = repo.client().await;
    call_text(
        &client,
        "bookmark_set",
        json!({ "repo": repo.repo_arg(), "name": "feat" }),
    )
    .await;
    assert_eq!(repo.change_id("feat"), repo.change_id("@"));

    let text = call_tool_error(
        &client,
        "bookmark_set",
        json!({ "repo": repo.repo_arg(), "name": "feat", "revision": "base" }),
    )
    .await;
    assert!(text.contains("backwards"), "{text}");

    call_text(
        &client,
        "bookmark_set",
        json!({
            "repo": repo.repo_arg(),
            "name": "feat",
            "revision": "base",
            "allow_backwards": true
        }),
    )
    .await;
    assert_eq!(repo.change_id("feat"), repo.change_id("base"));
}

#[tokio::test]
async fn write_failure_is_a_tool_error() {
    let repo = repo_with_base();
    let client = repo.client().await;
    let text = call_tool_error(
        &client,
        "edit",
        json!({ "repo": repo.repo_arg(), "revision": "no_such_rev_xyz" }),
    )
    .await;
    assert!(text.contains("no_such_rev_xyz"), "{text}");
    assert!(text.contains("exited with code"), "{text}");
}

#[tokio::test]
async fn write_tools_are_annotated() {
    let repo = TestRepo::new();
    let client = repo.client().await;
    let tools = client.list_all_tools().await.expect("tools/list");
    let writes = [
        "describe",
        "new",
        "commit",
        "squash",
        "split",
        "edit",
        "rebase",
        "restore",
        "abandon",
        "undo",
        "file_untrack",
        "bookmark_set",
    ];
    for name in writes {
        let tool = tools
            .iter()
            .find(|tool| tool.name == name)
            .unwrap_or_else(|| panic!("{name} not listed"));
        let annotations = tool.annotations.as_ref().expect("annotations");
        assert_eq!(annotations.read_only_hint, Some(false), "{name}");
        let destructive = matches!(name, "restore" | "abandon");
        assert_eq!(annotations.destructive_hint, Some(destructive), "{name}");
        assert_eq!(annotations.open_world_hint, Some(false), "{name}");
        assert!(tool.output_schema.is_none(), "{name}");
    }
}
