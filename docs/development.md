# Development

## Requirements

- Rust stable (edition 2024).
- jj 0.45.1 or newer, git and `ssh-keygen` on `PATH`. The integration tests
  drive the real binaries and fail, never skip, when one is missing.
- Linux for the full test suite. Suites built on shell-script fixtures are
  `cfg(unix)`; Windows runs the rest plus `tests/setup_windows.rs` (fake
  `.cmd` agent CLIs), with `ssh-keygen` from Git for Windows on `PATH`. macOS
  builds in CI without running the tests.
- [just](https://github.com/casey/just) for the gates.

## Gates

```sh
just check   # cargo fmt --check, cargo clippy --all-targets -D warnings, cargo test
```

CI runs the same gates on Linux and on Windows, `cargo audit`, and a clippy
plus release build on macOS. Everything must pass before a change lands.

On Windows, canonical paths are verbatim (`\\?\C:\...`). jj accepts them as a
working directory, but git and hand-built paths with `/` or `..` do not, so
test fixtures that hand paths to git do not canonicalize them, and jj's output
uses `\` as separator.

## Layout

```
src/
  main.rs         stdio server, `setup` subcommand, logs on stderr
  jj.rs           runner: argv without a shell, hardened environment, typed
                  errors, version check, per-workspace write queue
  repo.rs         RepoPath: absolute, existing, canonicalized
  templates.rs    jj templates and the serde types they produce
  server.rs       JjServer: tool routing and server info
  instructions.md the jj primer sent to clients as MCP instructions
  setup.rs        install and register in Claude Code, Codex and agy
  tools/
    read.rs       status, log, show, diff, bookmark_list, op_log
    write.rs      describe, new, commit, squash, split, edit, rebase,
                  restore, abandon, undo, file_untrack, bookmark_set
    remote.rs     git_fetch, git_push (dry run, signing count, progress,
                  cancellation)
    free.rs       run
tests/            integration tests, one file per area
```

Each tool separates a pure `params -> argv` function from execution, so argv
construction is tested without running jj (`tests/argv_*.rs`), and behaviour is
tested end to end against the real jj (`tests/tool_*.rs`).

## How the tests are built

- `tests/common/mod.rs` creates a colocated repository per test with its own
  `JJ_CONFIG`, so the developer's jj configuration (aliases, hardware signing
  keys) never leaks in, and tests share nothing through the process
  environment. The server runs in process behind an rmcp client over an
  in-memory duplex channel.
- `SigningRemote` adds a bare git remote and a software SSH key, so push tests
  sign commits and check the signature on the remote.
- Tests that need a slow or misbehaving jj use a fake `jj` shell script. Every
  such script is written once per test binary, before any test spawns a
  process: writing an executable while another thread forks lets the child
  inherit the open file and makes the exec fail with ETXTBSY.
- A tool call in tests has a 30 s deadline, so a panicking handler fails the
  test instead of hanging it.

## Adding a tool

1. Parameters struct with a doc comment on every field (it becomes the JSON
   Schema the agent reads) and `repo: String` first.
2. A pure `*_args` function: free-form values as `--flag=value`, positional
   values after `--`, paths through `literal_path_fileset`, invalid input as
   `ToolError::InvalidParams` naming the field.
3. The `#[tool]` method with honest annotations (`read_only_hint`,
   `destructive_hint`, `open_world_hint`). Writes take the write queue.
4. Tests for the argv and an end-to-end test against the real jj.
5. Update `docs/tools.md`, `src/instructions.md` if agents need to know, and
   `CHANGELOG.md` under `[Unreleased]`.

Design decisions, and what would reopen them, are in
[decisions.md](decisions.md).
