//! Runs the jj CLI: hardened environment, output capture, typed errors.

use std::ffi::OsString;
use std::io;
use std::process::Stdio;

use tokio::process::Command;

use crate::repo::RepoPath;

/// Flags prepended to every invocation, before the caller's arguments.
pub const GLOBAL_ARGS: [&str; 3] = ["--color", "never", "--no-pager"];

/// Editor command given to jj so that any path that would open an editor
/// fails immediately instead of blocking the server.
pub const FAILING_EDITOR: &str = "false";

/// Captured output of a successful jj call.
///
/// jj reports what it did ("Working copy now at", "Rebased 3 commits") and
/// its warnings on stderr, so both streams are kept.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JjOutput {
    pub stdout: String,
    pub stderr: String,
}

#[derive(Debug, thiserror::Error)]
pub enum JjError {
    /// The process could not be started (jj missing from PATH, not executable).
    #[error("could not run `{}`: {source}", argv.join(" "))]
    Spawn {
        argv: Vec<String>,
        #[source]
        source: io::Error,
    },
    /// jj ran and exited unsuccessfully. `code` is `None` when it was killed
    /// by a signal.
    #[error("`{}` exited with {}: {stderr}", argv.join(" "), exit_label(*code))]
    Failed {
        argv: Vec<String>,
        code: Option<i32>,
        stderr: String,
    },
}

fn exit_label(code: Option<i32>) -> String {
    match code {
        Some(code) => format!("code {code}"),
        None => "a signal".to_owned(),
    }
}

/// Builds and runs jj processes.
///
/// The program and extra environment are injectable so tests can point jj
/// at an isolated config (`JJ_CONFIG`) without touching the process
/// environment, which parallel tests share.
#[derive(Debug, Clone)]
pub struct JjRunner {
    program: OsString,
    env: Vec<(OsString, OsString)>,
}

impl Default for JjRunner {
    fn default() -> Self {
        Self::new()
    }
}

impl JjRunner {
    /// A runner for the `jj` found on `PATH`.
    pub fn new() -> Self {
        Self {
            program: OsString::from("jj"),
            env: Vec::new(),
        }
    }

    pub fn with_program(mut self, program: impl Into<OsString>) -> Self {
        self.program = program.into();
        self
    }

    /// Adds an environment variable to every process this runner starts.
    pub fn with_env(mut self, key: impl Into<OsString>, value: impl Into<OsString>) -> Self {
        self.env.push((key.into(), value.into()));
        self
    }

    /// The full argv for `args`: program, [`GLOBAL_ARGS`], then `args`.
    pub fn argv(&self, args: &[String]) -> Vec<String> {
        let mut argv = vec![self.program.to_string_lossy().into_owned()];
        argv.extend(GLOBAL_ARGS.iter().map(|arg| (*arg).to_owned()));
        argv.extend(args.iter().cloned());
        argv
    }

    /// Runs jj in `repo` without a shell, stdin bound to `/dev/null` and the
    /// editor set to [`FAILING_EDITOR`]. Dropping the returned future kills
    /// the process, which is how client cancellation reaches jj.
    pub async fn run(&self, repo: &RepoPath, args: &[String]) -> Result<JjOutput, JjError> {
        let mut command = Command::new(&self.program);
        command
            .args(GLOBAL_ARGS)
            .args(args)
            .current_dir(repo.as_path())
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .env("JJ_EDITOR", FAILING_EDITOR)
            .env("EDITOR", FAILING_EDITOR)
            // Applied last so callers can override the hardened defaults.
            .envs(self.env.iter().map(|(key, value)| (key, value)))
            // Dropping the future (client cancellation) must kill jj.
            .kill_on_drop(true);

        let output = command.output().await.map_err(|source| JjError::Spawn {
            argv: self.argv(args),
            source,
        })?;

        let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
        let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
        if output.status.success() {
            Ok(JjOutput { stdout, stderr })
        } else {
            Err(JjError::Failed {
                argv: self.argv(args),
                code: output.status.code(),
                stderr,
            })
        }
    }
}

/// Nearest directory at or above `repo` that holds a `.jj` directory, or
/// `repo` itself when there is none. Two paths inside one workspace map to
/// the same root, so they share a write queue.
pub fn workspace_root(repo: &RepoPath) -> std::path::PathBuf {
    repo.as_path()
        .ancestors()
        .find(|dir| dir.join(".jj").is_dir())
        .unwrap_or_else(|| repo.as_path())
        .to_path_buf()
}

/// One writer per workspace: writes to the same workspace run one at a
/// time, in arrival order; reads never wait. Clones share the queue.
///
/// jj tolerates concurrent operations by recording divergent operations and
/// merging them, but that shows up as noise in `op_log` for an agent.
#[derive(Debug, Clone, Default)]
pub struct WriteQueue {
    locks: std::sync::Arc<
        std::sync::Mutex<
            std::collections::HashMap<std::path::PathBuf, std::sync::Arc<tokio::sync::Mutex<()>>>,
        >,
    >,
}

/// Exclusive write access to one workspace, released on drop.
#[derive(Debug)]
pub struct WriteGuard {
    _guard: tokio::sync::OwnedMutexGuard<()>,
}

impl WriteQueue {
    pub fn new() -> Self {
        Self::default()
    }

    /// Waits until no other write to `repo`'s workspace is running.
    pub async fn lock(&self, repo: &RepoPath) -> WriteGuard {
        let mutex = {
            // A poisoned map is still a valid map: the critical section only
            // inserts, so recover it instead of failing the call.
            let mut locks = self
                .locks
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            locks.entry(workspace_root(repo)).or_default().clone()
        };
        // The map's mutex is released above: it must not be held across this await.
        WriteGuard {
            _guard: mutex.lock_owned().await,
        }
    }
}

/// A jj release, compared by (major, minor, patch).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct JjVersion {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
}

impl std::fmt::Display for JjVersion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

/// The jj release the test suite last ran against. An older jj on `PATH`
/// may lack flags or template keywords the tools use.
pub const TESTED_JJ_VERSION: JjVersion = JjVersion {
    major: 0,
    minor: 45,
    patch: 1,
};

/// Parses `jj --version` output such as `jj 0.45.1` or
/// `jj 0.46.0-3fe1a2b`; build suffixes after the patch number are ignored.
pub fn parse_jj_version(output: &str) -> Option<JjVersion> {
    let token = output
        .trim()
        .strip_prefix("jj ")?
        .split_whitespace()
        .next()?;
    let release = token.split(['-', '+']).next()?;
    let mut parts = release.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    let patch = parts.next()?.parse().ok()?;
    if parts.next().is_some() {
        return None;
    }
    Some(JjVersion {
        major,
        minor,
        patch,
    })
}

impl JjRunner {
    /// Version of the jj this runner starts, from `jj --version`; `None`
    /// when the output cannot be parsed. Runs outside any repository.
    pub async fn version(&self) -> Result<Option<JjVersion>, JjError> {
        let argv = || {
            vec![
                self.program.to_string_lossy().into_owned(),
                "--version".to_owned(),
            ]
        };
        // No GLOBAL_ARGS: `--version` needs none and has no color or pager
        // output to suppress. The temp dir keeps jj (and the git-only
        // colocation shim) away from whatever repository the server was
        // launched in.
        let output = Command::new(&self.program)
            .arg("--version")
            .current_dir(std::env::temp_dir())
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .env("JJ_EDITOR", FAILING_EDITOR)
            .env("EDITOR", FAILING_EDITOR)
            .envs(self.env.iter().map(|(key, value)| (key, value)))
            .kill_on_drop(true)
            .output()
            .await
            .map_err(|source| JjError::Spawn {
                argv: argv(),
                source,
            })?;
        if !output.status.success() {
            return Err(JjError::Failed {
                argv: argv(),
                code: output.status.code(),
                stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
            });
        }
        Ok(parse_jj_version(&String::from_utf8_lossy(&output.stdout)))
    }
}
