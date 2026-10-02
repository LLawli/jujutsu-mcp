//! `run_setup` on Windows against fake `claude.cmd`, `codex.cmd` and
//! `agy.cmd`, the shape npm gives the real CLIs. They log their arguments to
//! `%HOME%\calls.log`; `setup` passes `HOME` to them.
#![cfg(windows)]

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use jujutsu_mcp::setup::{Environment, SetupOptions, run_setup};
use tempfile::TempDir;

const LOG_AND_REMOVE: &str = "@echo off\r\n\
echo %~n0 %*>>\"%HOME%\\calls.log\"\r\n\
if \"%2\"==\"remove\" exit /b 1\r\n";

const CODEX_ADD: &str = "if not \"%2\"==\"add\" exit /b 0\r\n\
findstr /b /c:\"[mcp_servers.jj]\" \"%CODEX_HOME%\\config.toml\" >nul 2>&1 && exit /b 0\r\n\
>>\"%CODEX_HOME%\\config.toml\" echo.\r\n\
>>\"%CODEX_HOME%\\config.toml\" echo [mcp_servers.jj]\r\n\
>>\"%CODEX_HOME%\\config.toml\" echo command = '%~5'\r\n";

/// The fake CLIs, written once before any test runs setup.
static FAKE_DIR: LazyLock<PathBuf> = LazyLock::new(|| {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("setup-fakes-windows");
    fs::create_dir_all(&dir).expect("dir");
    let simple = format!("{LOG_AND_REMOVE}exit /b 0\r\n");
    let codex = format!("{LOG_AND_REMOVE}{CODEX_ADD}exit /b 0\r\n");
    for (name, body) in [("claude", &simple), ("agy", &simple), ("codex", &codex)] {
        fs::write(dir.join(format!("{name}.cmd")), body).expect("write fake");
    }
    dir
});

struct Fixture {
    _dir: TempDir,
    root: PathBuf,
    env: Environment,
}

impl Fixture {
    fn new() -> Self {
        let fakes = LazyLock::force(&FAKE_DIR);
        let dir = tempfile::tempdir().expect("temp");
        let root = dir.path().to_path_buf();
        for sub in ["home", "cargo", "codex", "build"] {
            fs::create_dir(root.join(sub)).expect("mkdir");
        }
        fs::write(
            root.join("codex").join("config.toml"),
            "# mine\nmodel = \"o3\"\n",
        )
        .expect("codex config");
        let exe = root.join("build").join("jujutsu-mcp.exe");
        fs::write(&exe, "binary-v1").expect("exe");
        let env = Environment {
            path: fakes.clone().into_os_string(),
            home: root.join("home"),
            cargo_home: Some(root.join("cargo")),
            codex_home: Some(root.join("codex")),
            current_exe: exe,
        };
        Self {
            _dir: dir,
            root,
            env,
        }
    }

    fn installed(&self) -> PathBuf {
        self.root.join("cargo").join("bin").join("jujutsu-mcp.exe")
    }

    fn calls(&self) -> String {
        fs::read_to_string(self.root.join("home").join("calls.log")).unwrap_or_default()
    }

    fn run(&self) -> String {
        let mut out = Vec::new();
        let result = run_setup(&self.env, &SetupOptions::default(), &mut out);
        let out = String::from_utf8(out).expect("utf-8");
        result.unwrap_or_else(|err| panic!("{err}\n{out}\ncalls:\n{}", self.calls()));
        out
    }
}

#[test]
fn registers_through_cmd_shims() {
    let fx = Fixture::new();
    fx.run();
    let installed = fx.installed();
    assert_eq!(
        fs::read_to_string(&installed).expect("installed"),
        "binary-v1"
    );

    let calls = fx.calls();
    let bin = installed.display().to_string();
    for expected in [
        format!("claude mcp add -s user jj -- {bin}"),
        format!("codex mcp add jj -- {bin}"),
        format!("agy mcp add jj -- {bin}"),
    ] {
        assert!(
            calls.contains(&expected),
            "missing {expected:?} in\n{calls}"
        );
    }

    let config = fs::read_to_string(fx.root.join("codex").join("config.toml")).expect("config");
    let doc: toml_edit::DocumentMut = config.parse().expect("toml");
    assert_eq!(
        doc["mcp_servers"]["jj"]["tool_timeout_sec"].as_integer(),
        Some(600)
    );
    assert!(config.starts_with("# mine\nmodel = \"o3\"\n"), "{config}");
}

#[test]
fn an_existing_binary_is_moved_aside() {
    let fx = Fixture::new();
    let installed = fx.installed();
    fs::create_dir_all(installed.parent().expect("parent")).expect("mkdir");
    fs::write(&installed, "binary-v0").expect("old binary");
    fx.run();
    assert_eq!(fs::read_to_string(&installed).expect("new"), "binary-v1");
    let old = installed.with_file_name("jujutsu-mcp.old.exe");
    assert_eq!(
        fs::read_to_string(old).expect("old kept aside"),
        "binary-v0"
    );
}

#[test]
fn the_binary_on_path_is_registered_in_place() {
    let mut fx = Fixture::new();
    let pkg = fx.root.join("pkg");
    fs::create_dir(&pkg).expect("mkdir");
    let exe = pkg.join("jujutsu-mcp.exe");
    fs::write(&exe, "binary-v1").expect("exe");
    fx.env.current_exe = exe.clone();
    fx.env.path = std::env::join_paths([FAKE_DIR.as_path(), pkg.as_path()]).expect("join");
    fx.run();
    assert!(!fx.installed().exists(), "copied anyway");
    let calls = fx.calls();
    assert!(
        calls.contains(&format!("agy mcp add jj -- {}", exe.display())),
        "{calls}"
    );
}
