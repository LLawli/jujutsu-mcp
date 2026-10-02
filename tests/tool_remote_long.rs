//! Long network calls: progress notifications and client cancellation,
//! driven by a fake `jj` that sleeps.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::Duration;

use jujutsu_mcp::jj::JjRunner;
use jujutsu_mcp::server::JjServer;
use rmcp::model::{
    CallToolRequestParams, ClientRequest, ProgressNotificationParam, Request, ServerResult,
};
use rmcp::service::{NotificationContext, PeerRequestOptions, RunningService};
use rmcp::{ClientHandler, RoleClient, ServiceExt};
use serde_json::{Value, json};
use tempfile::TempDir;

const SLOW_JJ: &str = r#"#!/bin/sh
if [ -n "$PID_FILE" ]; then echo $$ > "$PID_FILE"; fi
exec sleep "$SLEEP"
"#;

/// Written once before any test spawns a process: writing an executable
/// while another thread forks makes its exec fail with ETXTBSY.
static SLOW_JJ_PATH: LazyLock<PathBuf> = LazyLock::new(|| {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("remote-long-scripts");
    fs::create_dir_all(&dir).expect("script dir");
    let path = dir.join("slow-jj");
    if !fs::read_to_string(&path).is_ok_and(|current| current == SLOW_JJ) {
        let staging = dir.join(format!("slow-jj.{}", std::process::id()));
        fs::write(&staging, SLOW_JJ).expect("write script");
        fs::set_permissions(&staging, fs::Permissions::from_mode(0o755)).expect("chmod");
        fs::rename(&staging, &path).expect("install script");
    }
    path
});

#[derive(Clone, Default)]
struct Recorder {
    progress: Arc<Mutex<Vec<ProgressNotificationParam>>>,
}

impl ClientHandler for Recorder {
    async fn on_progress(
        &self,
        params: ProgressNotificationParam,
        _context: NotificationContext<RoleClient>,
    ) {
        self.progress.lock().expect("lock").push(params);
    }
}

struct Fixture {
    _dir: TempDir,
    root: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        LazyLock::force(&SLOW_JJ_PATH);
        let dir = tempfile::tempdir().expect("temp dir");
        let root = dir.path().canonicalize().expect("canonicalize");
        Self { _dir: dir, root }
    }

    async fn client(
        &self,
        sleep: &str,
        recorder: Recorder,
    ) -> RunningService<RoleClient, Recorder> {
        let runner = JjRunner::new()
            .with_program(SLOW_JJ_PATH.as_path())
            .with_env("SLEEP", sleep)
            .with_env("PID_FILE", self.root.join("pid"));
        let server = JjServer::new(runner).with_progress_interval(Duration::from_millis(100));
        let (server_io, client_io) = tokio::io::duplex(1 << 20);
        tokio::spawn(async move {
            let running = server.serve(server_io).await.expect("server starts");
            let _ = running.waiting().await;
        });
        recorder.serve(client_io).await.expect("client connects")
    }

    fn repo_arg(&self) -> Value {
        Value::String(self.root.to_str().expect("utf-8").to_owned())
    }
}

fn fetch_request(arguments: Value) -> ClientRequest {
    tool_request("git_fetch", arguments)
}

fn tool_request(tool: &'static str, arguments: Value) -> ClientRequest {
    let Value::Object(arguments) = arguments else {
        panic!("object");
    };
    ClientRequest::CallToolRequest(Request::new(
        CallToolRequestParams::new(tool).with_arguments(arguments),
    ))
}

#[tokio::test]
async fn long_fetch_reports_progress() {
    let fx = Fixture::new();
    let recorder = Recorder::default();
    let client = fx.client("1", recorder.clone()).await;
    let handle = client
        .send_cancellable_request(
            fetch_request(json!({ "repo": fx.repo_arg() })),
            PeerRequestOptions::no_options(),
        )
        .await
        .expect("request sent");
    let response = tokio::time::timeout(Duration::from_secs(20), handle.await_response())
        .await
        .expect("fetch answered")
        .expect("fetch response");
    match response {
        ServerResult::CallToolResult(result) => {
            assert_ne!(result.is_error, Some(true), "{result:?}")
        }
        other => panic!("unexpected response {other:?}"),
    }

    let progress = recorder.progress.lock().expect("lock").clone();
    assert!(progress.len() >= 2, "{progress:?}");
    assert!(
        progress.windows(2).all(|w| w[1].progress > w[0].progress),
        "progress must increase: {progress:?}"
    );
    assert!(
        progress.iter().all(|p| p
            .message
            .as_deref()
            .is_some_and(|m| m.contains("git fetch"))),
        "{progress:?}"
    );
}

fn is_dead(pid: &str) -> bool {
    match fs::read_to_string(format!("/proc/{pid}/stat")) {
        Err(_) => true,
        Ok(stat) => stat
            .rsplit(") ")
            .next()
            .is_some_and(|rest| rest.starts_with('Z')),
    }
}

#[tokio::test]
async fn cancelling_a_fetch_kills_jj() {
    let fx = Fixture::new();
    let client = fx.client("30", Recorder::default()).await;
    let handle = client
        .send_cancellable_request(
            fetch_request(json!({ "repo": fx.repo_arg() })),
            PeerRequestOptions::no_options(),
        )
        .await
        .expect("request sent");

    let pid_file = fx.root.join("pid");
    let mut pid = String::new();
    for _ in 0..500 {
        if let Ok(found) = fs::read_to_string(&pid_file) {
            pid = found.trim().to_owned();
            if !pid.is_empty() {
                break;
            }
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(!pid.is_empty(), "fake jj never started");

    handle.cancel(None).await.expect("cancel sent");
    for _ in 0..300 {
        if is_dead(&pid) {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("jj process {pid} still running after cancellation");
}

#[tokio::test]
async fn cancelling_run_kills_jj() {
    let fx = Fixture::new();
    let client = fx.client("30", Recorder::default()).await;
    let handle = client
        .send_cancellable_request(
            tool_request(
                "run",
                json!({ "repo": fx.repo_arg(), "args": ["git", "fetch"] }),
            ),
            PeerRequestOptions::no_options(),
        )
        .await
        .expect("request sent");

    let pid_file = fx.root.join("pid");
    let mut pid = String::new();
    for _ in 0..500 {
        if let Ok(found) = fs::read_to_string(&pid_file) {
            pid = found.trim().to_owned();
            if !pid.is_empty() {
                break;
            }
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(!pid.is_empty(), "fake jj never started");

    handle.cancel(None).await.expect("cancel sent");
    for _ in 0..300 {
        if is_dead(&pid) {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("jj process {pid} still running after cancellation");
}
