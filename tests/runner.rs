//! Runner behavior checked against a fake `jj` script that reports what it
//! received: argv, working directory, environment, stdin.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use std::time::Duration;

use jujutsu_mcp::jj::{FAILING_EDITOR, GLOBAL_ARGS, JjError, JjRunner};
use jujutsu_mcp::repo::RepoPath;
use tempfile::TempDir;

const REPORTER: &str = r#"#!/bin/sh
printf 'cwd=%s\n' "$(pwd -P)"
for arg in "$@"; do printf 'arg=%s\n' "$arg"; done
printf 'JJ_EDITOR=%s\n' "$JJ_EDITOR"
printf 'EDITOR=%s\n' "$EDITOR"
printf 'EXTRA=%s\n' "$EXTRA"
printf 'stdin=%s\n' "$(readlink /proc/self/fd/0)"
printf 'reported\n' >&2
"#;

const FAILER: &str = r#"#!/bin/sh
printf 'partial output\n'
printf 'boom: something broke\n' >&2
exit 3
"#;

const SLEEPER: &str = r#"#!/bin/sh
echo $$ > "$PID_FILE"
exec sleep 30
"#;

const SCRIPTS: [(&str, &str); 3] = [
    ("reporter", REPORTER),
    ("failer", FAILER),
    ("sleeper", SLEEPER),
];

/// Every fake jj is written once, before any test spawns a process.
///
/// Writing a script while another test thread forks lets the child inherit
/// the open write descriptor, and exec of that script then fails with
/// ETXTBSY. `Fixture::new` forces this initialization, and every spawn in
/// this file happens after a `Fixture::new`.
static SCRIPT_DIR: LazyLock<PathBuf> = LazyLock::new(|| {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("runner-scripts");
    fs::create_dir_all(&dir).expect("script dir");
    for (name, body) in SCRIPTS {
        let path = dir.join(name);
        if fs::read_to_string(&path).is_ok_and(|current| current == body) {
            continue;
        }
        // Rename keeps a concurrent `cargo test` from exec-ing a half-written file.
        let staging = dir.join(format!("{name}.{}", std::process::id()));
        fs::write(&staging, body).expect("write script");
        fs::set_permissions(&staging, fs::Permissions::from_mode(0o755)).expect("chmod");
        fs::rename(&staging, &path).expect("install script");
    }
    dir
});

struct Fixture {
    _dir: TempDir,
    root: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        LazyLock::force(&SCRIPT_DIR);
        let dir = tempfile::tempdir().expect("temp dir");
        let root = dir.path().canonicalize().expect("canonicalize");
        Self { _dir: dir, root }
    }

    fn script(&self, body: &str) -> PathBuf {
        let (name, _) = SCRIPTS
            .iter()
            .find(|(_, known)| *known == body)
            .expect("script registered in SCRIPTS");
        SCRIPT_DIR.join(name)
    }

    fn repo(&self) -> RepoPath {
        let dir = self.root.join("work");
        fs::create_dir_all(&dir).expect("mkdir");
        RepoPath::new(&dir).expect("repo path")
    }
}

fn args(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| (*s).to_owned()).collect()
}

fn field<'a>(stdout: &'a str, key: &str) -> Vec<&'a str> {
    let prefix = format!("{key}=");
    stdout
        .lines()
        .filter_map(|line| line.strip_prefix(prefix.as_str()))
        .collect()
}

#[test]
fn argv_prepends_global_flags() {
    let runner = JjRunner::new();
    let argv = runner.argv(&args(&["log", "-r", "@"]));
    assert_eq!(
        argv,
        ["jj", "--color", "never", "--no-pager", "log", "-r", "@"]
    );
    assert_eq!(GLOBAL_ARGS, ["--color", "never", "--no-pager"]);
}

#[test]
fn argv_uses_configured_program() {
    let runner = JjRunner::new().with_program("/opt/jj");
    assert_eq!(runner.argv(&args(&["status"]))[0], "/opt/jj");
}

#[tokio::test]
async fn passes_arguments_verbatim_without_a_shell() {
    let fx = Fixture::new();
    let runner = JjRunner::new().with_program(fx.script(REPORTER));
    let hostile = "a b; echo $HOME `id` > /tmp/x";
    let out = runner
        .run(&fx.repo(), &args(&["describe", "-m", hostile]))
        .await
        .expect("success");
    assert_eq!(
        field(&out.stdout, "arg"),
        ["--color", "never", "--no-pager", "describe", "-m", hostile]
    );
}

#[tokio::test]
async fn runs_in_the_repo_directory() {
    let fx = Fixture::new();
    let runner = JjRunner::new().with_program(fx.script(REPORTER));
    let repo = fx.repo();
    let out = runner
        .run(&repo, &args(&["status"]))
        .await
        .expect("success");
    assert_eq!(
        field(&out.stdout, "cwd"),
        [repo.as_path().to_str().expect("utf-8")]
    );
}

#[tokio::test]
async fn binds_stdin_to_dev_null() {
    let fx = Fixture::new();
    let runner = JjRunner::new().with_program(fx.script(REPORTER));
    let out = runner
        .run(&fx.repo(), &args(&["status"]))
        .await
        .expect("success");
    assert_eq!(field(&out.stdout, "stdin"), ["/dev/null"]);
}

#[tokio::test]
async fn sets_a_failing_editor() {
    let fx = Fixture::new();
    let runner = JjRunner::new().with_program(fx.script(REPORTER));
    let out = runner
        .run(&fx.repo(), &args(&["status"]))
        .await
        .expect("success");
    assert_eq!(field(&out.stdout, "JJ_EDITOR"), [FAILING_EDITOR]);
    assert_eq!(field(&out.stdout, "EDITOR"), [FAILING_EDITOR]);
}

#[tokio::test]
async fn passes_extra_environment() {
    let fx = Fixture::new();
    let runner = JjRunner::new()
        .with_program(fx.script(REPORTER))
        .with_env("EXTRA", "injected value");
    let out = runner
        .run(&fx.repo(), &args(&["status"]))
        .await
        .expect("success");
    assert_eq!(field(&out.stdout, "EXTRA"), ["injected value"]);
}

#[tokio::test]
async fn keeps_stderr_on_success() {
    let fx = Fixture::new();
    let runner = JjRunner::new().with_program(fx.script(REPORTER));
    let out = runner
        .run(&fx.repo(), &args(&["status"]))
        .await
        .expect("success");
    assert_eq!(out.stderr, "reported\n");
}

#[tokio::test]
async fn failure_carries_argv_exit_code_and_stderr() {
    let fx = Fixture::new();
    let program = fx.script(FAILER);
    let runner = JjRunner::new().with_program(&program);
    let err = runner
        .run(&fx.repo(), &args(&["log", "-r", "nope"]))
        .await
        .expect_err("failure");
    let program = program.to_str().expect("utf-8").to_owned();
    match &err {
        JjError::Failed { argv, code, stderr } => {
            assert_eq!(
                argv,
                &[
                    program.as_str(),
                    "--color",
                    "never",
                    "--no-pager",
                    "log",
                    "-r",
                    "nope"
                ]
            );
            assert_eq!(*code, Some(3));
            assert_eq!(stderr, "boom: something broke\n");
        }
        other => panic!("expected Failed, got {other:?}"),
    }
    let message = err.to_string();
    assert!(message.contains("log -r nope"), "{message}");
    assert!(message.contains("code 3"), "{message}");
    assert!(message.contains("boom: something broke"), "{message}");
}

#[tokio::test]
async fn missing_program_is_a_spawn_error() {
    let fx = Fixture::new();
    let runner = JjRunner::new().with_program("/nonexistent/bin/jj");
    let err = runner
        .run(&fx.repo(), &args(&["status"]))
        .await
        .expect_err("spawn failure");
    match &err {
        JjError::Spawn { argv, .. } => assert_eq!(argv[0], "/nonexistent/bin/jj"),
        other => panic!("expected Spawn, got {other:?}"),
    }
    assert!(err.to_string().contains("/nonexistent/bin/jj"), "{err}");
}

fn is_dead(pid: &str) -> bool {
    match fs::read_to_string(format!("/proc/{pid}/stat")) {
        Err(_) => true,
        // Field 3 is the state; a zombie has already exited.
        Ok(stat) => stat
            .rsplit(") ")
            .next()
            .is_some_and(|rest| rest.starts_with('Z')),
    }
}

async fn wait_for(path: &Path) -> String {
    for _ in 0..200 {
        if let Ok(pid) = fs::read_to_string(path) {
            let pid = pid.trim().to_owned();
            if !pid.is_empty() {
                return pid;
            }
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("script never wrote {}", path.display());
}

#[tokio::test]
async fn dropping_the_future_kills_the_process() {
    let fx = Fixture::new();
    let pid_file = fx.root.join("pid");
    let runner = JjRunner::new()
        .with_program(fx.script(SLEEPER))
        .with_env("PID_FILE", &pid_file);
    let repo = fx.repo();
    let call = args(&["git", "push"]);
    let run = runner.run(&repo, &call);
    let pid = tokio::select! {
        result = run => panic!("sleeper returned early: {result:?}"),
        pid = wait_for(&pid_file) => pid,
    };
    for _ in 0..200 {
        if is_dead(&pid) {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("process {pid} still running after the future was dropped");
}
