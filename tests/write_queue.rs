//! WriteQueue and workspace_root, without running jj.

use std::fs;
use std::path::PathBuf;
use std::time::Duration;

use jujutsu_mcp::jj::{WriteQueue, workspace_root};
use jujutsu_mcp::repo::RepoPath;
use tempfile::TempDir;

/// Two workspaces `one` and `two`, each with a `.jj` directory and a
/// nested `sub/dir`.
fn workspaces() -> (TempDir, PathBuf, PathBuf) {
    let dir = tempfile::tempdir().expect("temp dir");
    let root = dir.path().canonicalize().expect("canonicalize");
    for name in ["one", "two"] {
        fs::create_dir_all(root.join(name).join(".jj")).expect(".jj");
        fs::create_dir_all(root.join(name).join("sub/dir")).expect("sub");
    }
    (dir, root.join("one"), root.join("two"))
}

fn repo(path: PathBuf) -> RepoPath {
    RepoPath::new(path).expect("repo path")
}

const SHORT: Duration = Duration::from_millis(150);

#[test]
fn root_is_the_nearest_directory_with_dot_jj() {
    let (_dir, one, _) = workspaces();
    assert_eq!(workspace_root(&repo(one.clone())), one);
    assert_eq!(workspace_root(&repo(one.join("sub/dir"))), one);
}

#[test]
fn without_dot_jj_the_path_is_its_own_root() {
    let dir = tempfile::tempdir().expect("temp dir");
    let plain = dir
        .path()
        .canonicalize()
        .expect("canonicalize")
        .join("plain");
    fs::create_dir(&plain).expect("mkdir");
    assert_eq!(workspace_root(&repo(plain.clone())), plain);
}

#[tokio::test]
async fn writes_to_one_workspace_wait_for_each_other() {
    let (_dir, one, _) = workspaces();
    let queue = WriteQueue::new();
    let first = queue.lock(&repo(one.clone())).await;

    let sub = repo(one.join("sub/dir"));
    let blocked = tokio::time::timeout(SHORT, queue.lock(&sub)).await;
    assert!(blocked.is_err(), "second writer did not wait");

    drop(first);
    tokio::time::timeout(Duration::from_secs(2), queue.lock(&sub))
        .await
        .expect("second writer runs once the first is done");
}

#[tokio::test]
async fn other_workspaces_do_not_wait() {
    let (_dir, one, two) = workspaces();
    let queue = WriteQueue::new();
    let _held = queue.lock(&repo(one)).await;
    tokio::time::timeout(SHORT, queue.lock(&repo(two)))
        .await
        .expect("another workspace is independent");
}

#[tokio::test]
async fn clones_share_the_queue() {
    let (_dir, one, _) = workspaces();
    let queue = WriteQueue::new();
    let other = queue.clone();
    let _held = queue.lock(&repo(one.clone())).await;
    let blocked = tokio::time::timeout(SHORT, other.lock(&repo(one))).await;
    assert!(blocked.is_err(), "clone did not share the lock");
}
