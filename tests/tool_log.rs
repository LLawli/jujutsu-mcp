//! The `log` tool end to end: MCP client, in-process server, real jj.

mod common;

use common::{TestRepo, call_invalid_params, call_ok, call_tool_error};
use serde_json::{Value, json};

/// first <- second (bookmark `feat`) <- @ (empty, undescribed)
fn three_commit_repo() -> TestRepo {
    let repo = TestRepo::new();
    repo.write("a.txt", "a\n");
    repo.jj(&["describe", "-m", "first"]);
    repo.jj(&["new"]);
    repo.write("b.txt", "b\n");
    repo.jj(&["describe", "-m", "second"]);
    repo.jj(&["bookmark", "create", "feat", "-r", "@"]);
    repo.jj(&["new"]);
    repo
}

fn template_of(repo: &TestRepo, revision: &str, template: &str) -> String {
    repo.jj(&["log", "--no-graph", "-r", revision, "-T", template])
}

fn commits(output: &Value) -> &Vec<Value> {
    output["commits"].as_array().expect("commits array")
}

#[tokio::test]
async fn log_returns_commits_newest_first_with_flags() {
    let repo = three_commit_repo();
    let client = repo.client().await;
    let out = call_ok(
        &client,
        "log",
        json!({ "repo": repo.repo_arg(), "revisions": "root()..@" }),
    )
    .await;

    let list = commits(&out);
    assert_eq!(list.len(), 3, "{out:#}");
    assert_eq!(out["truncated"], json!(false));

    let wc = &list[0];
    assert_eq!(wc["working_copy"], json!(true));
    assert_eq!(wc["empty"], json!(true));
    assert_eq!(wc["description"], json!(""));
    assert_eq!(wc["bookmarks"], json!([]));

    let second = &list[1];
    assert_eq!(second["description"], json!("second\n"));
    assert_eq!(second["bookmarks"], json!(["feat"]));
    assert_eq!(second["remote_bookmarks"], json!([]));
    assert_eq!(second["working_copy"], json!(false));
    assert_eq!(second["empty"], json!(false));
    assert_eq!(second["immutable"], json!(false));
    assert_eq!(second["conflict"], json!(false));
    assert_eq!(second["divergent"], json!(false));
    assert_eq!(
        second["change_id"],
        json!(template_of(&repo, "feat", "change_id"))
    );
    assert_eq!(
        second["commit_id"],
        json!(template_of(&repo, "feat", "commit_id"))
    );
    assert_eq!(
        second["parents"],
        json!([template_of(&repo, "feat-", "commit_id")])
    );
    assert_eq!(second["author"]["name"], json!("Test User"));
    assert_eq!(second["author"]["email"], json!("test@example.com"));
    assert!(
        second["author"]["timestamp"]
            .as_str()
            .is_some_and(|t| !t.is_empty()),
        "{second:#}"
    );
    assert_eq!(second["committer"]["email"], json!("test@example.com"));

    assert_eq!(list[2]["description"], json!("first\n"));
}

#[tokio::test]
async fn log_without_revisions_uses_the_default_revset() {
    let repo = three_commit_repo();
    let client = repo.client().await;
    let out = call_ok(&client, "log", json!({ "repo": repo.repo_arg() })).await;
    let list = commits(&out);
    assert!(
        list.iter().any(|c| c["working_copy"] == json!(true)),
        "{out:#}"
    );
}

#[tokio::test]
async fn log_marks_the_root_commit_immutable() {
    let repo = TestRepo::new();
    let client = repo.client().await;
    let out = call_ok(
        &client,
        "log",
        json!({ "repo": repo.repo_arg(), "revisions": "root()" }),
    )
    .await;
    let list = commits(&out);
    assert_eq!(list.len(), 1, "{out:#}");
    assert_eq!(list[0]["immutable"], json!(true));
}

#[tokio::test]
async fn log_marks_conflicted_commits() {
    let repo = TestRepo::new();
    repo.write("f.txt", "base\n");
    repo.jj(&["describe", "-m", "base"]);
    repo.jj(&["bookmark", "create", "base", "-r", "@"]);
    repo.jj(&["new"]);
    repo.write("f.txt", "left\n");
    repo.jj(&["describe", "-m", "left"]);
    repo.jj(&["bookmark", "create", "left", "-r", "@"]);
    repo.jj(&["new", "base"]);
    repo.write("f.txt", "right\n");
    repo.jj(&["describe", "-m", "right"]);
    repo.jj(&["bookmark", "create", "right", "-r", "@"]);
    repo.jj(&["new", "left", "right"]);

    let client = repo.client().await;
    let out = call_ok(
        &client,
        "log",
        json!({ "repo": repo.repo_arg(), "revisions": "@" }),
    )
    .await;
    let list = commits(&out);
    assert_eq!(list[0]["conflict"], json!(true), "{out:#}");
    assert_eq!(list[0]["parents"].as_array().map(Vec::len), Some(2));
}

#[tokio::test]
async fn log_reports_truncation() {
    let repo = three_commit_repo();
    let client = repo.client().await;

    let cut = call_ok(
        &client,
        "log",
        json!({ "repo": repo.repo_arg(), "revisions": "root()..@", "limit": 2 }),
    )
    .await;
    assert_eq!(commits(&cut).len(), 2, "{cut:#}");
    assert_eq!(cut["truncated"], json!(true));

    let exact = call_ok(
        &client,
        "log",
        json!({ "repo": repo.repo_arg(), "revisions": "root()..@", "limit": 3 }),
    )
    .await;
    assert_eq!(commits(&exact).len(), 3, "{exact:#}");
    assert_eq!(exact["truncated"], json!(false));
}

#[tokio::test]
async fn log_failure_is_a_tool_error_with_argv_code_and_stderr() {
    let repo = TestRepo::new();
    let client = repo.client().await;
    let text = call_tool_error(
        &client,
        "log",
        json!({ "repo": repo.repo_arg(), "revisions": "no_such_rev_xyz" }),
    )
    .await;
    assert!(text.contains("--revisions=no_such_rev_xyz"), "{text}");
    assert!(text.contains("exited with code"), "{text}");
    assert!(text.contains("doesn't exist"), "{text}");
}

#[tokio::test]
async fn log_rejects_invalid_repo_before_running_jj() {
    let repo = TestRepo::new();
    let client = repo.client().await;

    let message = call_invalid_params(&client, "log", json!({ "repo": "relative/path" })).await;
    assert!(message.contains("relative/path"), "{message}");

    let missing = repo.temp_root().join("missing");
    let message = call_invalid_params(
        &client,
        "log",
        json!({ "repo": missing.to_str().expect("utf-8") }),
    )
    .await;
    assert!(message.contains("missing"), "{message}");

    call_invalid_params(&client, "log", json!({})).await;
}

#[tokio::test]
async fn log_rejects_a_zero_limit() {
    let repo = TestRepo::new();
    let client = repo.client().await;
    let message = call_invalid_params(
        &client,
        "log",
        json!({ "repo": repo.repo_arg(), "limit": 0 }),
    )
    .await;
    assert!(message.contains("limit"), "{message}");
}

#[tokio::test]
async fn log_is_listed_as_read_only_with_schemas() {
    let repo = TestRepo::new();
    let client = repo.client().await;
    let tools = client.list_all_tools().await.expect("tools/list");
    let log = tools
        .iter()
        .find(|tool| tool.name == "log")
        .expect("log tool listed");

    let annotations = log.annotations.as_ref().expect("annotations");
    assert_eq!(annotations.read_only_hint, Some(true));
    assert_eq!(annotations.open_world_hint, Some(false));

    let required = log.input_schema.get("required").expect("required list");
    assert!(
        required
            .as_array()
            .is_some_and(|list| list.contains(&json!("repo"))),
        "{required}"
    );

    let output = log.output_schema.as_ref().expect("output schema");
    let properties = output.get("properties").expect("properties");
    assert!(properties.get("commits").is_some(), "{properties}");
    assert!(properties.get("truncated").is_some(), "{properties}");
}
