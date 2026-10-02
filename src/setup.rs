//! `jujutsu-mcp setup`: install the binary and register it as the `jj` MCP
//! server in Claude Code, Codex and Antigravity (agy).
//!
//! Registration goes through each agent's own CLI, which owns its config
//! format and file locking; Claude Code rewrites `~/.claude.json` while it
//! runs. The one exception is Codex's tool timeout, which its CLI cannot set:
//! it is written into `config.toml` with `toml_edit`, keeping the rest of the
//! file as it was.

use std::ffi::OsString;
use std::fs;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use toml_edit::{DocumentMut, Item, value};

/// Name the server is registered under in every agent.
pub const SERVER_NAME: &str = "jj";

/// Codex cuts MCP tools at 60 s by default; a signed push waits for one
/// security-key touch per commit.
pub const CODEX_TOOL_TIMEOUT_SEC: i64 = 600;

pub const USAGE: &str = "usage: jujutsu-mcp setup [--dry-run] [--agent claude|codex|agy]...";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Agent {
    Claude,
    Codex,
    Agy,
}

impl Agent {
    pub const ALL: [Agent; 3] = [Agent::Claude, Agent::Codex, Agent::Agy];

    /// The agent's CLI program name.
    pub fn program(self) -> &'static str {
        match self {
            Agent::Claude => "claude",
            Agent::Codex => "codex",
            Agent::Agy => "agy",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SetupOptions {
    /// Print what would be done without doing it.
    pub dry_run: bool,
    /// Agents to configure; empty means every agent whose CLI is on `PATH`.
    pub agents: Vec<Agent>,
}

/// One external command. `may_fail` marks cleanup steps, like removing a
/// registration that may not exist.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Step {
    pub argv: Vec<String>,
    pub may_fail: bool,
}

/// Where setup reads and writes. Built from the process environment in
/// production and from temporary directories in tests.
#[derive(Debug, Clone)]
pub struct Environment {
    /// `PATH` used to find the agents' CLIs and passed to them.
    pub path: OsString,
    /// `HOME` passed to the agents' CLIs.
    pub home: PathBuf,
    /// `CARGO_HOME`, when set; the binary goes to `<cargo home>/bin`.
    pub cargo_home: Option<PathBuf>,
    /// `CODEX_HOME`, when set; Codex's config is `<codex home>/config.toml`.
    pub codex_home: Option<PathBuf>,
    /// The running executable, copied into place by setup.
    pub current_exe: PathBuf,
}

#[derive(Debug, thiserror::Error)]
pub enum SetupError {
    #[error("{0}\n{USAGE}")]
    Usage(String),
    #[error("{0} is not on PATH")]
    AgentNotFound(&'static str),
    #[error("could not determine {0}")]
    Environment(&'static str),
    #[error("could not install {path:?}: {source}")]
    Install {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("`{}` failed: {detail}", argv.join(" "))]
    Command { argv: Vec<String>, detail: String },
    #[error("codex config {path:?}: {detail}")]
    CodexConfig { path: PathBuf, detail: String },
    #[error("could not write output: {0}")]
    Output(#[source] std::io::Error),
}

impl Environment {
    pub fn from_process() -> Result<Self, SetupError> {
        let path = std::env::var_os("PATH").unwrap_or_default();
        let home = non_empty_var("HOME").ok_or(SetupError::Environment("HOME"))?;
        let current_exe = std::env::current_exe()
            .map_err(|_| SetupError::Environment("the path of the running executable"))?;
        Ok(Self {
            path,
            home: PathBuf::from(home),
            cargo_home: non_empty_var("CARGO_HOME").map(PathBuf::from),
            codex_home: non_empty_var("CODEX_HOME").map(PathBuf::from),
            current_exe,
        })
    }

    /// `<cargo home>/bin/jujutsu-mcp`, with cargo home defaulting to
    /// `~/.cargo`.
    pub fn install_path(&self) -> PathBuf {
        let cargo_home = self
            .cargo_home
            .clone()
            .unwrap_or_else(|| self.home.join(".cargo"));
        cargo_home.join("bin").join("jujutsu-mcp")
    }

    /// `<codex home>/config.toml`, with codex home defaulting to `~/.codex`.
    pub fn codex_config(&self) -> PathBuf {
        let codex_home = self
            .codex_home
            .clone()
            .unwrap_or_else(|| self.home.join(".codex"));
        codex_home.join("config.toml")
    }
}

/// An environment variable, treating an empty value as unset.
fn non_empty_var(name: &str) -> Option<OsString> {
    std::env::var_os(name).filter(|v| !v.is_empty())
}

/// Parses the arguments after `setup`.
pub fn parse_setup_args(args: &[String]) -> Result<SetupOptions, SetupError> {
    let mut options = SetupOptions::default();
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--dry-run" => options.dry_run = true,
            "--agent" => {
                let name = iter
                    .next()
                    .ok_or_else(|| SetupError::Usage("--agent needs a value".to_owned()))?;
                let agent = Agent::ALL
                    .into_iter()
                    .find(|agent| agent.program() == name)
                    .ok_or_else(|| SetupError::Usage(format!("unknown agent: {name}")))?;
                if !options.agents.contains(&agent) {
                    options.agents.push(agent);
                }
            }
            other => return Err(SetupError::Usage(format!("unknown option: {other}"))),
        }
    }
    Ok(options)
}

/// First executable file named `program` in the directories of `path`.
pub fn find_in_path(program: &str, path: &std::ffi::OsStr) -> Option<PathBuf> {
    std::env::split_paths(path)
        .filter(|dir| !dir.as_os_str().is_empty())
        .map(|dir| dir.join(program))
        .find(|candidate| {
            fs::metadata(candidate)
                .is_ok_and(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
        })
}

/// Commands that register `binary` as [`SERVER_NAME`] in `agent`,
/// replacing any earlier registration.
pub fn registration_steps(agent: Agent, binary: &Path) -> Vec<Step> {
    let binary = binary.to_string_lossy();
    let step = |args: &[&str], may_fail: bool| Step {
        argv: std::iter::once(agent.program())
            .chain(args.iter().copied())
            .map(str::to_owned)
            .collect(),
        may_fail,
    };
    let add = |args: &[&str]| step(&[args, &["--", &binary]].concat(), false);
    match agent {
        Agent::Claude => vec![
            step(&["mcp", "remove", "-s", "user", SERVER_NAME], true),
            add(&["mcp", "add", "-s", "user", SERVER_NAME]),
        ],
        Agent::Codex => vec![
            step(&["mcp", "remove", SERVER_NAME], true),
            add(&["mcp", "add", SERVER_NAME]),
        ],
        // `agy mcp add` already replaces an existing registration.
        Agent::Agy => vec![add(&["mcp", "add", SERVER_NAME])],
    }
}

/// `config` with `tool_timeout_sec` set on `[mcp_servers.<SERVER_NAME>]`,
/// the rest of the document untouched. Fails when the table is missing,
/// which means `codex mcp add` did not register the server.
pub fn codex_with_timeout(config: &str) -> Result<String, String> {
    let mut doc: DocumentMut = config
        .parse()
        .map_err(|err| format!("invalid TOML: {err}"))?;
    let table = doc
        .get_mut("mcp_servers")
        .and_then(Item::as_table_like_mut)
        .and_then(|servers| servers.get_mut(SERVER_NAME))
        .and_then(Item::as_table_like_mut)
        .ok_or_else(|| {
            format!("no [mcp_servers.{SERVER_NAME}] table; `codex mcp add` did not register it")
        })?;
    let mut timeout = value(CODEX_TOOL_TIMEOUT_SEC);
    match table.get_mut("tool_timeout_sec") {
        Some(existing) => {
            // Keep the old value's comments and spacing around the new number.
            if let (Some(old), Some(new)) = (existing.as_value(), timeout.as_value_mut()) {
                *new.decor_mut() = old.decor().clone();
            }
            *existing = timeout;
        }
        None => {
            table.insert("tool_timeout_sec", timeout);
        }
    }
    Ok(doc.to_string())
}

/// Installs the binary and registers it, reporting each action on `out`.
pub fn run_setup(
    env: &Environment,
    options: &SetupOptions,
    out: &mut dyn Write,
) -> Result<(), SetupError> {
    let requested = !options.agents.is_empty();
    let candidates: &[Agent] = if requested {
        &options.agents
    } else {
        &Agent::ALL
    };

    // Resolve every CLI first, so a missing one fails before any effect.
    let mut found = Vec::new();
    for &agent in candidates {
        match find_in_path(agent.program(), &env.path) {
            Some(path) => found.push((agent, path)),
            None if requested => return Err(SetupError::AgentNotFound(agent.program())),
            None => say(
                out,
                format_args!("skipped {0}: {0} is not on PATH", agent.program()),
            )?,
        }
    }

    let installed = env.install_path();
    install_binary(env, &installed, options.dry_run, out)?;

    for (agent, program_path) in &found {
        for step in registration_steps(*agent, &installed) {
            let line = step.argv.join(" ");
            if options.dry_run {
                say(out, format_args!("would run: {line}"))?;
                continue;
            }
            say(out, format_args!("running: {line}"))?;
            match run_step(env, &step, program_path) {
                Ok(()) => {}
                Err(_) if step.may_fail => {}
                Err(detail) => {
                    return Err(SetupError::Command {
                        argv: step.argv,
                        detail,
                    });
                }
            }
        }
        if *agent == Agent::Codex {
            set_codex_timeout(env, options.dry_run, out)?;
        }
    }
    Ok(())
}

fn say(out: &mut dyn Write, line: std::fmt::Arguments<'_>) -> Result<(), SetupError> {
    writeln!(out, "{line}").map_err(SetupError::Output)
}

/// Copies the running binary to `installed` unless it already is that file.
fn install_binary(
    env: &Environment,
    installed: &Path,
    dry_run: bool,
    out: &mut dyn Write,
) -> Result<(), SetupError> {
    let same_file = match (installed.canonicalize(), env.current_exe.canonicalize()) {
        (Ok(target), Ok(current)) => target == current,
        _ => false,
    };
    if same_file {
        return say(
            out,
            format_args!("binary already installed at {}", installed.display()),
        );
    }
    if dry_run {
        return say(
            out,
            format_args!(
                "would install {} to {}",
                env.current_exe.display(),
                installed.display()
            ),
        );
    }
    let install_error = |source| SetupError::Install {
        path: installed.to_path_buf(),
        source,
    };
    let dir = installed
        .parent()
        .ok_or_else(|| install_error(std::io::Error::other("no parent directory")))?;
    fs::create_dir_all(dir).map_err(install_error)?;
    // Copy next to the destination and rename over it: the running server may
    // be this very file, and writing into it would fail with ETXTBSY.
    let staging = dir.join(format!(".jujutsu-mcp.{}.tmp", std::process::id()));
    let result = fs::copy(&env.current_exe, &staging)
        .and_then(|_| fs::set_permissions(&staging, fs::Permissions::from_mode(0o755)))
        .and_then(|_| fs::rename(&staging, installed));
    if let Err(source) = result {
        let _ = fs::remove_file(&staging);
        return Err(install_error(source));
    }
    say(out, format_args!("installed {}", installed.display()))
}

/// `path` followed by the process's own `PATH`. The agents' CLIs are often
/// scripts that need `env`, `node` or coreutils, which a caller-supplied `PATH`
/// may not list; in production the two are the same and the tail is redundant.
fn child_path(path: &std::ffi::OsStr) -> OsString {
    let inherited = std::env::var_os("PATH").unwrap_or_default();
    std::env::join_paths(std::env::split_paths(path).chain(std::env::split_paths(&inherited)))
        .unwrap_or_else(|_| path.to_owned())
}

/// Runs one step with `program_path` in place of `argv[0]`. The error is the
/// stderr and exit code, ready for [`SetupError::Command`].
fn run_step(env: &Environment, step: &Step, program_path: &Path) -> Result<(), String> {
    let mut command = Command::new(program_path);
    command
        .args(&step.argv[1..])
        .stdin(Stdio::null())
        .env("PATH", child_path(&env.path))
        .env("HOME", &env.home);
    match &env.codex_home {
        Some(codex_home) => command.env("CODEX_HOME", codex_home),
        None => command.env_remove("CODEX_HOME"),
    };
    let output = command.output().map_err(|err| err.to_string())?;
    if output.status.success() {
        return Ok(());
    }
    let code = output.status.code().map_or_else(
        || "terminated by a signal".to_owned(),
        |c| format!("exit code {c}"),
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    Err(format!("{code}: {}", stderr.trim()))
}

/// Raises Codex's tool timeout, which `codex mcp add` cannot set.
fn set_codex_timeout(
    env: &Environment,
    dry_run: bool,
    out: &mut dyn Write,
) -> Result<(), SetupError> {
    let path = env.codex_config();
    if dry_run {
        return say(
            out,
            format_args!(
                "would set tool_timeout_sec = {CODEX_TOOL_TIMEOUT_SEC} in {}",
                path.display()
            ),
        );
    }
    let config_error = |detail: String| SetupError::CodexConfig {
        path: path.clone(),
        detail,
    };
    let config = fs::read_to_string(&path).map_err(|err| config_error(err.to_string()))?;
    let updated = codex_with_timeout(&config).map_err(&config_error)?;
    fs::write(&path, updated).map_err(|err| config_error(err.to_string()))?;
    say(
        out,
        format_args!(
            "set tool_timeout_sec = {CODEX_TOOL_TIMEOUT_SEC} in {}",
            path.display()
        ),
    )
}
