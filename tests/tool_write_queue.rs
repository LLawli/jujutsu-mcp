//! Tools and the write queue, observed through a fake `jj` that logs when
//! each call starts and ends.

// Uses a fake jj shell script.
#![cfg(unix)]

mod common;

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use common::{Client, call, connect};
use jujutsu_mcp::jj::JjRunner;
use serde_json::{Value, json};
use tempfile::TempDir;

/// argv is `--color never --no-pager <subcommand> ...`, so `$4` names the
/// call.
const LOGGING_JJ: &str = r#"#!/bin/sh
echo "start $4" >> "$LOG"
sleep 0.3
echo "end $4" >> "$LOG"
"#;

/// Written once before any test spawns a process: writing an executable
/// while another thread forks makes its exec fail with ETXTBSY.
static LOGGING_JJ_PATH: LazyLock<PathBuf> = LazyLock::new(|| {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("write-queue-scripts");
    fs::create_dir_all(&dir).expect("script dir");
    let path = dir.join("logging-jj");
    if !fs::read_to_string(&path).is_ok_and(|current| current == LOGGING_JJ) {
        let staging = dir.join(format!("logging-jj.{}", std::process::id()));
        fs::write(&staging, LOGGING_JJ).expect("write script");
        fs::set_permissions(&staging, fs::Permissions::from_mode(0o755)).expect("chmod");
        fs::rename(&staging, &path).expect("install script");
    }
    path
});

struct Fixture {
    _dir: TempDir,
    root: PathBuf,
    log: PathBuf,
}

impl Fixture {
    /// A fake workspace (`.jj` plus `sub/`) and a log file.
    fn new() -> Self {
        LazyLock::force(&LOGGING_JJ_PATH);
        let dir = tempfile::tempdir().expect("temp dir");
        let root = dir.path().canonicalize().expect("canonicalize");
        fs::create_dir_all(root.join("ws/.jj")).expect(".jj");
        fs::create_dir_all(root.join("ws/sub")).expect("sub");
        let log = root.join("calls.log");
        Self {
            _dir: dir,
            root,
            log,
        }
    }

    async fn client(&self) -> Client {
        let runner = JjRunner::new()
            .with_program(LOGGING_JJ_PATH.as_path())
            .with_env("LOG", &self.log);
        connect(runner).await
    }

    fn path(&self, relative: &str) -> Value {
        Value::String(self.root.join(relative).to_str().expect("utf-8").to_owned())
    }

    fn events(&self) -> Vec<String> {
        fs::read_to_string(&self.log)
            .expect("log written")
            .lines()
            .map(str::to_owned)
            .collect()
    }
}

/// Whether any call started while another was still running.
fn overlapped(events: &[String]) -> bool {
    let mut running = 0usize;
    for event in events {
        if event.starts_with("start") {
            running += 1;
            if running > 1 {
                return true;
            }
        } else {
            running -= 1;
        }
    }
    false
}

fn assert_ok(result: Result<rmcp::model::CallToolResult, rmcp::service::ServiceError>) {
    let result = result.expect("protocol");
    assert_ne!(result.is_error, Some(true), "{result:?}");
}

#[tokio::test]
async fn writes_to_one_workspace_never_overlap() {
    let fx = Fixture::new();
    let client = fx.client().await;
    let (r0, r1, r2) = tokio::join!(
        call(
            &client,
            "describe",
            json!({ "repo": fx.path("ws"), "message": "a" })
        ),
        call(
            &client,
            "describe",
            json!({ "repo": fx.path("ws/sub"), "message": "b" })
        ),
        call(&client, "new", json!({ "repo": fx.path("ws") }))
    );
    assert_ok(r0);
    assert_ok(r1);
    assert_ok(r2);
    let events = fx.events();
    assert_eq!(events.len(), 6, "{events:?}");
    assert!(!overlapped(&events), "{events:?}");
}

#[tokio::test]
async fn reads_run_alongside_writes() {
    let fx = Fixture::new();
    let client = fx.client().await;
    let (r0, r1, r2) = tokio::join!(
        call(
            &client,
            "describe",
            json!({ "repo": fx.path("ws"), "message": "a" })
        ),
        call(&client, "status", json!({ "repo": fx.path("ws") })),
        call(&client, "diff", json!({ "repo": fx.path("ws/sub") }))
    );
    assert_ok(r0);
    assert_ok(r1);
    assert_ok(r2);
    let events = fx.events();
    assert_eq!(events.len(), 6, "{events:?}");
    assert!(overlapped(&events), "reads waited: {events:?}");
}

#[tokio::test]
async fn fetch_waits_for_writes() {
    let fx = Fixture::new();
    let client = fx.client().await;
    let (r0, r1) = tokio::join!(
        call(&client, "git_fetch", json!({ "repo": fx.path("ws") })),
        call(
            &client,
            "describe",
            json!({ "repo": fx.path("ws/sub"), "message": "a" })
        )
    );
    assert_ok(r0);
    assert_ok(r1);
    let events = fx.events();
    assert_eq!(events.len(), 4, "{events:?}");
    assert!(!overlapped(&events), "{events:?}");
}

#[tokio::test]
async fn push_holds_the_queue_from_dry_run_to_push() {
    let fx = Fixture::new();
    let client = fx.client().await;
    let (r0, r1) = tokio::join!(
        call(
            &client,
            "git_push",
            json!({ "repo": fx.path("ws"), "bookmarks": ["feat"] })
        ),
        call(
            &client,
            "describe",
            json!({ "repo": fx.path("ws"), "message": "a" })
        )
    );
    assert_ok(r0);
    assert_ok(r1);
    let events = fx.events();
    // Dry run and push (the fake prints no targets, so no count query),
    // plus describe.
    assert_eq!(events.len(), 6, "{events:?}");
    assert!(!overlapped(&events), "{events:?}");
    let describe: Vec<usize> = events
        .iter()
        .enumerate()
        .filter(|(_, e)| e.ends_with("describe"))
        .map(|(i, _)| i)
        .collect();
    assert!(
        describe == [0, 1] || describe == [4, 5],
        "describe ran between the dry run and the push: {events:?}"
    );
}

#[tokio::test]
async fn run_waits_for_writes() {
    let fx = Fixture::new();
    let client = fx.client().await;
    let (r0, r1) = tokio::join!(
        call(
            &client,
            "run",
            json!({ "repo": fx.path("ws"), "args": ["bookmark", "delete", "x"] })
        ),
        call(
            &client,
            "describe",
            json!({ "repo": fx.path("ws/sub"), "message": "a" })
        )
    );
    assert_ok(r0);
    assert_ok(r1);
    let events = fx.events();
    assert_eq!(events.len(), 4, "{events:?}");
    assert!(!overlapped(&events), "{events:?}");
}
