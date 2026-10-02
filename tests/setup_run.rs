//! `run_setup` against fake `claude`, `codex` and `agy` CLIs that log their
//! arguments to `$HOME/calls.log`.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use jujutsu_mcp::setup::{Agent, Environment, SetupError, SetupOptions, run_setup};
use tempfile::TempDir;

/// `remove` fails, as it does when nothing is registered yet. A `fail` file
/// in `$HOME` makes every call fail.
const FAKE_CLI: &str = r#"#!/bin/sh
name=$(basename "$0")
echo "$name $*" >> "$HOME/calls.log"
if [ -e "$HOME/fail" ]; then echo "boom from $name" >&2; exit 3; fi
if [ "$2" = remove ]; then exit 1; fi
if [ "$name" = codex ] && [ "$2" = add ]; then
  config="$CODEX_HOME/config.toml"
  if ! grep -q '^\[mcp_servers\.jj\]' "$config" 2>/dev/null; then
    printf '\n[mcp_servers.jj]\ncommand = "%s"\n' "$5" >> "$config"
  fi
fi
exit 0
"#;

/// The fake CLIs, written before any test spawns a process (writing an
/// executable while another thread forks causes ETXTBSY).
static FAKE_DIR: LazyLock<PathBuf> = LazyLock::new(|| {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("setup-fakes");
    fs::create_dir_all(&dir).expect("dir");
    for name in ["claude", "codex", "agy"] {
        let path = dir.join(name);
        if fs::read_to_string(&path).is_ok_and(|current| current == FAKE_CLI) {
            continue;
        }
        let staging = dir.join(format!("{name}.{}", std::process::id()));
        fs::write(&staging, FAKE_CLI).expect("write");
        fs::set_permissions(&staging, fs::Permissions::from_mode(0o755)).expect("chmod");
        fs::rename(&staging, &path).expect("install");
    }
    dir
});

const CODEX_BEFORE: &str = "# mine\nmodel = \"o3\"\n";

struct Fixture {
    _dir: TempDir,
    root: PathBuf,
    env: Environment,
}

impl Fixture {
    /// PATH holds only the given agents' CLIs (symlinks to the fakes).
    fn with_agents(agents: &[&str]) -> Self {
        let fakes = LazyLock::force(&FAKE_DIR);
        let dir = tempfile::tempdir().expect("temp");
        let root = dir.path().canonicalize().expect("canonicalize");
        let bin = root.join("bin");
        for sub in ["bin", "home", "cargo", "codex", "build"] {
            fs::create_dir(root.join(sub)).expect("mkdir");
        }
        for agent in agents {
            std::os::unix::fs::symlink(fakes.join(agent), bin.join(agent)).expect("symlink");
        }
        fs::write(root.join("codex/config.toml"), CODEX_BEFORE).expect("codex config");
        let exe = root.join("build/jujutsu-mcp");
        fs::write(&exe, "binary-v1").expect("exe");
        fs::set_permissions(&exe, fs::Permissions::from_mode(0o755)).expect("chmod");
        let env = Environment {
            path: bin.into_os_string(),
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

    fn all() -> Self {
        Self::with_agents(&["claude", "codex", "agy"])
    }

    fn installed(&self) -> PathBuf {
        self.root.join("cargo/bin/jujutsu-mcp")
    }

    fn calls(&self) -> Vec<String> {
        fs::read_to_string(self.root.join("home/calls.log"))
            .unwrap_or_default()
            .lines()
            .map(str::to_owned)
            .collect()
    }

    fn codex_config(&self) -> String {
        fs::read_to_string(self.root.join("codex/config.toml")).expect("codex config")
    }

    fn run(&self, options: &SetupOptions) -> (Result<(), SetupError>, String) {
        let mut out = Vec::new();
        let result = run_setup(&self.env, options, &mut out);
        (result, String::from_utf8(out).expect("utf-8 output"))
    }
}

fn expected_calls(installed: &Path) -> Vec<String> {
    let bin = installed.display();
    vec![
        "claude mcp remove -s user jj".to_owned(),
        format!("claude mcp add -s user jj -- {bin}"),
        "codex mcp remove jj".to_owned(),
        format!("codex mcp add jj -- {bin}"),
        format!("agy mcp add jj -- {bin}"),
    ]
}

#[test]
fn installs_and_registers_in_every_agent() {
    let fx = Fixture::all();
    let (result, out) = fx.run(&SetupOptions::default());
    result.unwrap_or_else(|err| panic!("{err}\n{out}"));

    let installed = fx.installed();
    assert_eq!(
        fs::read_to_string(&installed).expect("installed"),
        "binary-v1"
    );
    let mode = fs::metadata(&installed).expect("meta").permissions().mode();
    assert_eq!(mode & 0o111, 0o111, "not executable: {mode:o}");

    assert_eq!(fx.calls(), expected_calls(&installed));

    let codex = fx.codex_config();
    assert!(codex.starts_with(CODEX_BEFORE), "{codex}");
    let doc: toml_edit::DocumentMut = codex.parse().expect("toml");
    assert_eq!(
        doc["mcp_servers"]["jj"]["tool_timeout_sec"].as_integer(),
        Some(600)
    );

    for agent in ["claude", "codex", "agy"] {
        assert!(out.contains(agent), "{out}");
    }
    assert!(out.contains(&installed.display().to_string()), "{out}");
}

#[test]
fn running_twice_is_harmless() {
    let fx = Fixture::all();
    fx.run(&SetupOptions::default()).0.expect("first");
    fx.run(&SetupOptions::default()).0.expect("second");
    let codex = fx.codex_config();
    assert_eq!(codex.matches("[mcp_servers.jj]").count(), 1, "{codex}");
    assert_eq!(codex.matches("tool_timeout_sec").count(), 1, "{codex}");
    let mut twice = expected_calls(&fx.installed());
    twice.extend(expected_calls(&fx.installed()));
    assert_eq!(fx.calls(), twice);
}

#[test]
fn dry_run_changes_nothing() {
    let fx = Fixture::all();
    let options = SetupOptions {
        dry_run: true,
        agents: Vec::new(),
    };
    let (result, out) = fx.run(&options);
    result.expect("dry run");
    assert!(!fx.installed().exists());
    assert!(fx.calls().is_empty(), "{:?}", fx.calls());
    assert_eq!(fx.codex_config(), CODEX_BEFORE);
    let bin = fx.installed().display().to_string();
    assert!(
        out.contains(&format!("claude mcp add -s user jj -- {bin}")),
        "{out}"
    );
    assert!(out.contains(&format!("agy mcp add jj -- {bin}")), "{out}");
    assert!(out.contains("tool_timeout_sec"), "{out}");
}

#[test]
fn only_the_requested_agents() {
    let fx = Fixture::all();
    let options = SetupOptions {
        dry_run: false,
        agents: vec![Agent::Codex],
    };
    fx.run(&options).0.expect("setup");
    assert!(
        fx.calls().iter().all(|c| c.starts_with("codex ")),
        "{:?}",
        fx.calls()
    );
}

#[test]
fn missing_clis_are_skipped_by_default() {
    let fx = Fixture::with_agents(&["claude"]);
    let (result, out) = fx.run(&SetupOptions::default());
    result.expect("setup");
    assert!(
        fx.calls().iter().all(|c| c.starts_with("claude ")),
        "{:?}",
        fx.calls()
    );
    assert!(out.contains("not on PATH"), "{out}");
    assert!(out.contains("agy"), "{out}");
}

#[test]
fn a_requested_but_missing_cli_fails_before_installing() {
    let fx = Fixture::with_agents(&["claude"]);
    let options = SetupOptions {
        dry_run: false,
        agents: vec![Agent::Claude, Agent::Agy],
    };
    let (result, _) = fx.run(&options);
    assert!(
        matches!(result, Err(SetupError::AgentNotFound("agy"))),
        "{result:?}"
    );
    assert!(!fx.installed().exists());
    assert!(fx.calls().is_empty());
}

#[test]
fn a_failing_registration_reports_the_command() {
    let fx = Fixture::with_agents(&["agy"]);
    fs::write(fx.root.join("home/fail"), "").expect("fail marker");
    let (result, _) = fx.run(&SetupOptions::default());
    match result {
        Err(SetupError::Command { argv, detail }) => {
            assert_eq!(argv[0], "agy");
            assert!(detail.contains("boom from agy"), "{detail}");
        }
        other => panic!("expected Command error, got {other:?}"),
    }
}

#[test]
fn already_installed_binary_is_left_in_place() {
    let mut fx = Fixture::with_agents(&["agy"]);
    let installed = fx.installed();
    fs::create_dir_all(installed.parent().expect("parent")).expect("mkdir");
    fs::write(&installed, "binary-v0").expect("write");
    fs::set_permissions(&installed, fs::Permissions::from_mode(0o755)).expect("chmod");
    fx.env.current_exe = installed.clone();
    fx.run(&SetupOptions::default()).0.expect("setup");
    assert_eq!(fs::read_to_string(&installed).expect("read"), "binary-v0");
    assert_eq!(
        fx.calls(),
        [format!("agy mcp add jj -- {}", installed.display())]
    );
}
