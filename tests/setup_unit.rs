//! Pure parts of `jujutsu-mcp setup`.

use std::ffi::OsString;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use jujutsu_mcp::setup::{
    Agent, Environment, SetupError, SetupOptions, Step, codex_with_timeout, parse_setup_args,
    registration_steps,
};

fn s(list: &[&str]) -> Vec<String> {
    list.iter().map(|x| (*x).to_owned()).collect()
}

#[test]
fn agent_programs() {
    assert_eq!(Agent::Claude.program(), "claude");
    assert_eq!(Agent::Codex.program(), "codex");
    assert_eq!(Agent::Agy.program(), "agy");
}

#[test]
fn parses_options() {
    assert_eq!(parse_setup_args(&[]).unwrap(), SetupOptions::default());
    assert_eq!(
        parse_setup_args(&s(&["--dry-run"])).unwrap(),
        SetupOptions {
            dry_run: true,
            agents: Vec::new()
        }
    );
    assert_eq!(
        parse_setup_args(&s(&[
            "--agent", "codex", "--agent", "claude", "--agent", "codex"
        ]))
        .unwrap()
        .agents,
        [Agent::Codex, Agent::Claude]
    );
}

#[test]
fn rejects_bad_options() {
    for (args, mentions) in [
        (s(&["--agent"]), "--agent"),
        (s(&["--agent", "vim"]), "vim"),
        (s(&["--bogus"]), "--bogus"),
    ] {
        match parse_setup_args(&args) {
            Err(err @ SetupError::Usage(_)) => {
                let message = err.to_string();
                assert!(message.contains(mentions), "{message}");
                assert!(message.contains("usage:"), "{message}");
            }
            other => panic!("{args:?}: expected Usage, got {other:?}"),
        }
    }
}

#[cfg(unix)]
#[test]
fn finds_executables_on_path() {
    use jujutsu_mcp::setup::find_in_path;
    use std::fs;

    let dir = tempfile::tempdir().expect("temp");
    let root = dir.path().canonicalize().expect("canonicalize");
    let (empty, plain, exec) = (root.join("a"), root.join("b"), root.join("c"));
    for d in [&empty, &plain, &exec] {
        fs::create_dir(d).expect("mkdir");
    }
    fs::write(plain.join("tool"), "x").expect("write");
    fs::write(exec.join("tool"), "x").expect("write");
    fs::set_permissions(exec.join("tool"), fs::Permissions::from_mode(0o755)).expect("chmod");

    let path = std::env::join_paths([&empty, &plain, &exec]).expect("join");
    assert_eq!(find_in_path("tool", &path), Some(exec.join("tool")));
    assert_eq!(find_in_path("missing", &path), None);
    assert_eq!(find_in_path("tool", &OsString::new()), None);
}

#[test]
fn registration_commands_per_agent() {
    let binary = Path::new("/opt/bin/jujutsu-mcp");
    assert_eq!(
        registration_steps(Agent::Claude, binary),
        [
            Step {
                argv: s(&["claude", "mcp", "remove", "-s", "user", "jj"]),
                may_fail: true
            },
            Step {
                argv: s(&[
                    "claude",
                    "mcp",
                    "add",
                    "-s",
                    "user",
                    "jj",
                    "--",
                    "/opt/bin/jujutsu-mcp"
                ]),
                may_fail: false
            },
        ]
    );
    assert_eq!(
        registration_steps(Agent::Codex, binary),
        [
            Step {
                argv: s(&["codex", "mcp", "remove", "jj"]),
                may_fail: true
            },
            Step {
                argv: s(&["codex", "mcp", "add", "jj", "--", "/opt/bin/jujutsu-mcp"]),
                may_fail: false
            },
        ]
    );
    assert_eq!(
        registration_steps(Agent::Agy, binary),
        [Step {
            argv: s(&["agy", "mcp", "add", "jj", "--", "/opt/bin/jujutsu-mcp"]),
            may_fail: false
        }]
    );
}

const CODEX_CONFIG: &str = r#"# my settings
model = "o3"

[features]
web_search = true

[mcp_servers.jj]
command = "/opt/bin/jujutsu-mcp"
"#;

#[test]
fn codex_timeout_is_added_and_the_rest_kept() {
    let updated = codex_with_timeout(CODEX_CONFIG).expect("valid");
    let doc: toml_edit::DocumentMut = updated.parse().expect("toml");
    assert_eq!(
        doc["mcp_servers"]["jj"]["tool_timeout_sec"].as_integer(),
        Some(600)
    );
    assert_eq!(
        doc["mcp_servers"]["jj"]["command"].as_str(),
        Some("/opt/bin/jujutsu-mcp")
    );
    assert!(
        updated.starts_with("# my settings\nmodel = \"o3\"\n"),
        "{updated}"
    );
    assert!(
        updated.contains("[features]\nweb_search = true\n"),
        "{updated}"
    );
    assert_eq!(codex_with_timeout(&updated).expect("idempotent"), updated);
}

#[test]
fn codex_timeout_replaces_an_older_value() {
    let config = format!("{CODEX_CONFIG}tool_timeout_sec = 60\n");
    let updated = codex_with_timeout(&config).expect("valid");
    assert!(updated.contains("tool_timeout_sec = 600"), "{updated}");
    assert!(!updated.contains("tool_timeout_sec = 60\n"), "{updated}");
}

#[test]
fn codex_timeout_needs_the_registration() {
    let err = codex_with_timeout("model = \"o3\"\n").expect_err("no table");
    assert!(err.contains("mcp_servers.jj"), "{err}");
    assert!(codex_with_timeout("not = [valid").is_err());
}

fn env(cargo_home: Option<&str>, codex_home: Option<&str>) -> Environment {
    Environment {
        path: OsString::new(),
        home: PathBuf::from("/home/u"),
        cargo_home: cargo_home.map(PathBuf::from),
        codex_home: codex_home.map(PathBuf::from),
        current_exe: PathBuf::from("/tmp/build/jujutsu-mcp"),
    }
}

#[test]
fn install_and_codex_paths() {
    let binary = format!("jujutsu-mcp{}", std::env::consts::EXE_SUFFIX);
    assert_eq!(
        env(None, None).install_path(),
        Path::new("/home/u/.cargo/bin").join(&binary)
    );
    assert_eq!(
        env(Some("/c"), None).install_path(),
        Path::new("/c/bin").join(&binary)
    );
    assert_eq!(
        env(None, None).codex_config(),
        Path::new("/home/u/.codex/config.toml")
    );
    assert_eq!(
        env(None, Some("/x")).codex_config(),
        Path::new("/x/config.toml")
    );
}

#[test]
fn executable_candidates_per_platform() {
    use jujutsu_mcp::setup::executable_candidates;
    use std::ffi::OsStr;
    assert_eq!(executable_candidates("claude", None), ["claude"]);
    assert_eq!(
        executable_candidates("claude", Some(OsStr::new(".COM;.EXE;;.CMD"))),
        ["claude.com", "claude.exe", "claude.cmd"]
    );
    assert!(executable_candidates("claude", Some(OsStr::new(""))).is_empty());
}
