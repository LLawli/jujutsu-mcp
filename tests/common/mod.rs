//! Shared fixtures for integration tests.
//!
//! Every repository uses its own `JJ_CONFIG`, so tests never read the
//! user's jj configuration (signing with a hardware key, aliases) and never
//! share state through the process environment.

#![allow(dead_code)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use jujutsu_mcp::jj::JjRunner;
use jujutsu_mcp::repo::RepoPath;
use tempfile::TempDir;

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

    pub fn temp_root(&self) -> &Path {
        self.dir.path()
    }
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
