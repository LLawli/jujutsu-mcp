//! The installed binary: MCP over stdio, logs on stderr.

mod common;

use std::fs;
use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

use common::TestRepo;
use rmcp::model::CallToolRequestParams;
use rmcp::service::RunningService;
use rmcp::{RoleClient, ServiceExt};
use serde_json::{Value, json};
use tokio::process::{Child, Command};

struct Server {
    client: RunningService<RoleClient, ()>,
    child: Child,
}

/// Starts the binary with `env` added, stderr going to `stderr_log`.
async fn start(env: &[(&str, &Path)], stderr_log: &Path) -> Server {
    let mut command = Command::new(env!("CARGO_BIN_EXE_jujutsu-mcp"));
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(fs::File::create(stderr_log).expect("stderr log"))
        .env("RUST_LOG", "info")
        .kill_on_drop(true);
    for (key, value) in env {
        command.env(key, value);
    }
    let mut child = command.spawn().expect("binary starts");
    let stdout = child.stdout.take().expect("stdout");
    let stdin = child.stdin.take().expect("stdin");
    let client = tokio::time::timeout(Duration::from_secs(20), ().serve((stdout, stdin)))
        .await
        .expect("handshake in time")
        .expect("handshake");
    Server { client, child }
}

const ALL_TOOLS: [&str; 20] = [
    "abandon",
    "bookmark_list",
    "bookmark_set",
    "commit",
    "describe",
    "diff",
    "edit",
    "file_untrack",
    "git_fetch",
    "git_push",
    "log",
    "new",
    "op_log",
    "rebase",
    "restore",
    "show",
    "split",
    "squash",
    "status",
    "undo",
];

#[tokio::test]
async fn serves_every_tool_over_stdio() {
    let repo = TestRepo::new();
    let log = repo.temp_root().join("stderr.log");
    let server = start(&[("JJ_CONFIG", &repo.config)], &log).await;

    let tools = server.client.list_all_tools().await.expect("tools/list");
    let mut names: Vec<&str> = tools.iter().map(|tool| tool.name.as_ref()).collect();
    names.sort_unstable();
    assert_eq!(names, ALL_TOOLS);

    let Value::Object(arguments) = json!({ "repo": repo.repo_arg(), "revisions": "@" }) else {
        unreachable!()
    };
    let result = server
        .client
        .call_tool(CallToolRequestParams::new("log").with_arguments(arguments))
        .await
        .expect("log over stdio");
    assert_ne!(result.is_error, Some(true), "{result:?}");
    let commits = &result.structured_content.expect("structured")["commits"];
    assert_eq!(commits.as_array().map(Vec::len), Some(1));
}

#[tokio::test]
async fn ships_the_jj_mental_model_as_instructions() {
    let repo = TestRepo::new();
    let log = repo.temp_root().join("stderr.log");
    let server = start(&[("JJ_CONFIG", &repo.config)], &log).await;
    let info = server.client.peer_info().expect("server info");
    let instructions = info.instructions.as_deref().expect("instructions");
    assert!(instructions.contains("`repo`"), "{instructions}");
    assert!(instructions.contains("change id"), "{instructions}");
    assert!(instructions.contains("dry_run"), "{instructions}");
    let server_info = info.server_info.as_ref().expect("server_info");
    assert_eq!(server_info.name, "jujutsu-mcp");
}

#[tokio::test]
async fn logs_to_stderr_and_exits_when_stdin_closes() {
    let repo = TestRepo::new();
    let log = repo.temp_root().join("stderr.log");
    let Server { client, mut child } = start(&[("JJ_CONFIG", &repo.config)], &log).await;
    client.cancel().await.expect("client closes");

    let status = tokio::time::timeout(Duration::from_secs(10), child.wait())
        .await
        .expect("server exits after stdin closes")
        .expect("wait");
    assert!(status.success(), "{status:?}");

    let stderr = fs::read_to_string(&log).expect("stderr log");
    assert!(stderr.contains("jujutsu-mcp"), "{stderr}");
    assert!(
        !stderr.contains("older than"),
        "the jj on PATH is the tested one: {stderr}"
    );
}
