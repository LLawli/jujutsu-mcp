//! `run`, the escape hatch for jj commands without a dedicated tool.

mod common;

use common::{TestRepo, call_text, call_tool_error};
use jujutsu_mcp::tools::ToolError;
use jujutsu_mcp::tools::free::{RunParams, run_args};
use serde_json::json;

fn params(args: &[&str]) -> RunParams {
    RunParams {
        repo: "/repo".to_owned(),
        args: args.iter().map(|a| (*a).to_owned()).collect(),
    }
}

#[test]
fn args_pass_through_unchanged() {
    assert_eq!(
        run_args(&params(&["bookmark", "delete", "a b", "$X"])).unwrap(),
        ["bookmark", "delete", "a b", "$X"]
    );
    assert_eq!(
        run_args(&params(&["--ignore-working-copy", "log"])).unwrap(),
        ["--ignore-working-copy", "log"]
    );
}

#[test]
fn needs_a_subcommand() {
    for args in [&[][..], &[""][..], &["  ", "log"][..]] {
        match run_args(&params(args)) {
            Err(err @ ToolError::InvalidParams(_)) => {
                assert!(err.to_string().contains("args"), "{err}")
            }
            other => panic!("{args:?}: expected InvalidParams, got {other:?}"),
        }
    }
}

#[tokio::test]
async fn runs_a_command_without_a_dedicated_tool() {
    let repo = TestRepo::new();
    repo.jj(&["bookmark", "create", "old", "-r", "@"]);
    let client = repo.client().await;
    let text = call_text(
        &client,
        "run",
        json!({ "repo": repo.repo_arg(), "args": ["bookmark", "delete", "old"] }),
    )
    .await;
    assert!(text.contains("Deleted 1 bookmark"), "{text}");
    let listed = repo.jj(&["bookmark", "list"]);
    assert!(!listed.contains("old:"), "{listed}");
}

#[tokio::test]
async fn arguments_are_not_interpreted_by_a_shell() {
    let repo = TestRepo::new();
    let client = repo.client().await;
    let message = "$HOME `id` ; echo hi";
    call_text(
        &client,
        "run",
        json!({ "repo": repo.repo_arg(), "args": ["describe", "-m", message] }),
    )
    .await;
    assert_eq!(repo.description("@"), format!("{message}\n"));
}

#[tokio::test]
async fn interactive_commands_fail_instead_of_blocking() {
    let repo = TestRepo::new();
    repo.write("a.txt", "a\n");
    let client = repo.client().await;
    let text = call_tool_error(
        &client,
        "run",
        json!({ "repo": repo.repo_arg(), "args": ["describe"] }),
    )
    .await;
    assert!(text.contains("exited with code"), "{text}");
}

#[tokio::test]
async fn failure_is_a_tool_error_with_stderr() {
    let repo = TestRepo::new();
    let client = repo.client().await;
    let text = call_tool_error(
        &client,
        "run",
        json!({ "repo": repo.repo_arg(), "args": ["no-such-subcommand"] }),
    )
    .await;
    assert!(text.contains("no-such-subcommand"), "{text}");
}

#[tokio::test]
async fn run_is_annotated_as_destructive_and_open_world() {
    let repo = TestRepo::new();
    let client = repo.client().await;
    let tools = client.list_all_tools().await.expect("tools/list");
    let run = tools
        .iter()
        .find(|tool| tool.name == "run")
        .expect("run listed");
    let annotations = run.annotations.as_ref().expect("annotations");
    assert_eq!(annotations.read_only_hint, Some(false));
    assert_eq!(annotations.destructive_hint, Some(true));
    assert_eq!(annotations.open_world_hint, Some(true));
}
