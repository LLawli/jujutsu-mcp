//! The binary warns when the jj on PATH is older than the tested release.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::LazyLock;
use std::time::Duration;

use rmcp::ServiceExt;
use tokio::process::Command;

const OLD_JJ: &str = "#!/bin/sh\necho 'jj 0.30.0'\n";

/// A directory holding a fake `jj`, written before any process is spawned
/// (writing an executable while another thread forks causes ETXTBSY).
static OLD_JJ_DIR: LazyLock<PathBuf> = LazyLock::new(|| {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("old-jj");
    fs::create_dir_all(&dir).expect("dir");
    let path = dir.join("jj");
    if !fs::read_to_string(&path).is_ok_and(|current| current == OLD_JJ) {
        let staging = dir.join(format!("jj.{}", std::process::id()));
        fs::write(&staging, OLD_JJ).expect("write");
        fs::set_permissions(&staging, fs::Permissions::from_mode(0o755)).expect("chmod");
        fs::rename(&staging, &path).expect("install");
    }
    dir
});

#[tokio::test]
async fn warns_about_an_older_jj() {
    let fake_dir = LazyLock::force(&OLD_JJ_DIR);
    let temp = tempfile::tempdir().expect("temp");
    let log = temp.path().join("stderr.log");
    let path = format!(
        "{}:{}",
        fake_dir.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let mut child = Command::new(env!("CARGO_BIN_EXE_jujutsu-mcp"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(fs::File::create(&log).expect("log"))
        .env("PATH", path)
        .env("RUST_LOG", "warn")
        .kill_on_drop(true)
        .spawn()
        .expect("binary starts");
    let stdout = child.stdout.take().expect("stdout");
    let stdin = child.stdin.take().expect("stdin");
    let client = tokio::time::timeout(Duration::from_secs(20), ().serve((stdout, stdin)))
        .await
        .expect("handshake in time")
        .expect("handshake");
    client.cancel().await.expect("close");
    let _ = tokio::time::timeout(Duration::from_secs(10), child.wait()).await;

    let stderr = fs::read_to_string(&log).expect("stderr");
    assert!(stderr.contains("0.30.0"), "{stderr}");
    assert!(stderr.contains("0.45.1"), "{stderr}");
    assert!(stderr.contains("older than"), "{stderr}");
}
