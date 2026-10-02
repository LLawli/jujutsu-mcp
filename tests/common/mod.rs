//! Shared fixtures for integration tests.
//!
//! Every repository uses its own `JJ_CONFIG`, so tests never read the
//! user's jj configuration (signing with a hardware key, aliases) and never
//! share state through the process environment.

#![allow(dead_code)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use jujutsu_mcp::jj::JjRunner;
use jujutsu_mcp::repo::RepoPath;
use jujutsu_mcp::server::JjServer;
use rmcp::model::{CallToolRequestParams, CallToolResult};
use rmcp::service::{RunningService, ServiceError};
use rmcp::{RoleClient, ServiceExt};
use serde_json::Value;
use tempfile::TempDir;

pub type Client = RunningService<RoleClient, ()>;

/// Deadline for one tool call in tests.
pub const CALL_TIMEOUT: Duration = Duration::from_secs(30);

const BASE_CONFIG: &str = r#"
[user]
name = "Test User"
email = "test@example.com"
"#;

/// A colocated jj repository in a temporary directory with an isolated
/// config.
pub struct TestRepo {
    dir: TempDir,
    pub path: PathBuf,
    pub config: PathBuf,
}

impl TestRepo {
    pub fn new() -> Self {
        Self::with_config("")
    }

    /// Like [`TestRepo::new`], with `extra` appended to the jj config.
    pub fn with_config(extra: &str) -> Self {
        let dir = tempfile::tempdir().expect("create temp dir");
        let root = dir.path().canonicalize().expect("canonicalize temp dir");
        let config = root.join("config.toml");
        fs::write(&config, format!("{BASE_CONFIG}\n{extra}\n")).expect("write jj config");
        let path = root.join("repo");
        fs::create_dir(&path).expect("create repo dir");
        let repo = Self { dir, path, config };
        repo.jj(&["git", "init", "--colocate"]);
        repo
    }

    pub fn repo_path(&self) -> RepoPath {
        RepoPath::new(&self.path).expect("valid repo path")
    }

    /// The runner under test, pointed at this repository's config.
    pub fn runner(&self) -> JjRunner {
        JjRunner::new().with_env("JJ_CONFIG", &self.config)
    }

    /// Runs jj synchronously for setup and inspection, independent of the
    /// runner under test. Panics when jj is missing or fails: integration
    /// tests must fail, never skip, without jj on PATH.
    pub fn jj(&self, args: &[&str]) -> String {
        jj_in(&self.path, &self.config, args)
    }

    pub fn write(&self, relative: &str, contents: &str) {
        let file = self.path.join(relative);
        if let Some(parent) = file.parent() {
            fs::create_dir_all(parent).expect("create parent dirs");
        }
        fs::write(file, contents).expect("write file");
    }

    /// Description of `revision`.
    pub fn description(&self, revision: &str) -> String {
        self.jj(&["log", "--no-graph", "-r", revision, "-T", "description"])
    }

    /// Change id of `revision`.
    pub fn change_id(&self, revision: &str) -> String {
        self.jj(&["log", "--no-graph", "-r", revision, "-T", "change_id"])
    }

    /// Paths changed in `revision`, sorted.
    pub fn changed_files(&self, revision: &str) -> Vec<String> {
        let mut files: Vec<String> = self
            .jj(&["diff", "--name-only", "-r", revision])
            .lines()
            .map(str::to_owned)
            .collect();
        files.sort_unstable();
        files
    }

    pub fn temp_root(&self) -> &Path {
        self.dir.path()
    }

    /// An MCP client connected to an in-process server using this
    /// repository's runner.
    pub async fn client(&self) -> Client {
        connect(self.runner()).await
    }

    /// The repo argument every tool takes.
    pub fn repo_arg(&self) -> Value {
        Value::String(self.path.to_str().expect("utf-8 path").to_owned())
    }
}

/// Serves a [`JjServer`] over an in-memory duplex channel and connects a
/// client to it.
pub async fn connect(runner: JjRunner) -> Client {
    let (server_io, client_io) = tokio::io::duplex(1 << 20);
    tokio::spawn(async move {
        let server = JjServer::new(runner)
            .serve(server_io)
            .await
            .expect("server starts");
        let _ = server.waiting().await;
    });
    ().serve(client_io).await.expect("client connects")
}

/// Calls `tool` with `arguments`, which must be a JSON object.
pub async fn call(
    client: &Client,
    tool: &'static str,
    arguments: Value,
) -> Result<CallToolResult, ServiceError> {
    let Value::Object(arguments) = arguments else {
        panic!("tool arguments must be a JSON object");
    };
    let request = client.call_tool(CallToolRequestParams::new(tool).with_arguments(arguments));
    // A panic inside a tool handler drops the response instead of failing
    // the call; without a deadline the test would hang.
    tokio::time::timeout(CALL_TIMEOUT, request)
        .await
        .unwrap_or_else(|_| panic!("{tool} did not answer within {CALL_TIMEOUT:?}"))
}

/// Calls `tool` and expects a successful structured result, returned as
/// JSON. Also checks the text copy carries the same JSON.
pub async fn call_ok(client: &Client, tool: &'static str, arguments: Value) -> Value {
    let result = call(client, tool, arguments)
        .await
        .unwrap_or_else(|err| panic!("{tool} protocol error: {err:?}"));
    assert_ne!(result.is_error, Some(true), "{tool} failed: {result:?}");
    let structured = result
        .structured_content
        .clone()
        .unwrap_or_else(|| panic!("{tool} returned no structured content: {result:?}"));
    let text = text_of(&result);
    let copy: Value = serde_json::from_str(&text)
        .unwrap_or_else(|err| panic!("{tool} text copy is not JSON ({err}): {text}"));
    assert_eq!(
        copy, structured,
        "text copy differs from structured content"
    );
    structured
}

/// Calls `tool` and expects a successful text result; returns all its text
/// blocks joined by newlines.
pub async fn call_text(client: &Client, tool: &'static str, arguments: Value) -> String {
    let result = call(client, tool, arguments)
        .await
        .unwrap_or_else(|err| panic!("{tool} protocol error: {err:?}"));
    assert_ne!(result.is_error, Some(true), "{tool} failed: {result:?}");
    text_of(&result)
}

/// Calls `tool` and expects a tool result with `isError: true`; returns its
/// text.
pub async fn call_tool_error(client: &Client, tool: &'static str, arguments: Value) -> String {
    let result = call(client, tool, arguments)
        .await
        .unwrap_or_else(|err| panic!("{tool} protocol error: {err:?}"));
    assert_eq!(result.is_error, Some(true), "{tool} succeeded: {result:?}");
    text_of(&result)
}

/// Concatenated text content of a tool result.
pub fn text_of(result: &CallToolResult) -> String {
    result
        .content
        .iter()
        .filter_map(|block| block.as_text().map(|text| text.text.clone()))
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn jj_in(cwd: &Path, config: &Path, args: &[&str]) -> String {
    let output = Command::new("jj")
        .args(["--color", "never", "--no-pager"])
        .args(args)
        .current_dir(cwd)
        .env("JJ_CONFIG", config)
        .output()
        .unwrap_or_else(|err| panic!("jj must be on PATH for integration tests: {err}"));
    assert!(
        output.status.success(),
        "jj {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).expect("utf-8 stdout")
}
