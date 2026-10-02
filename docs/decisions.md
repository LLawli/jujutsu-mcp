# Design decisions

Each entry records what was decided, the evidence behind it, and what would
reopen it. Measurements were taken on 2026-10-02 against jj 0.45.1.

## Why write a new server

Two existing jj MCP servers were tested end to end before starting this one.

- `@cyberistic/jj-mcp-server` 0.1.2 accepts `cwd`/`repoPath` on every tool but
  never passes them to the jj runner, so every call runs in the directory the
  agent was launched from. A failing jj call reaches the agent as an empty
  error (the `CommandFailed` stderr is dropped), and the Effect logger writes
  error reports to stdout, the JSON-RPC channel.
- `keanemind/jj-mcp-server` 1.0.1 honors `cwd` and surfaces stderr, but it was
  written over two days in May 2025 and never updated. Against jj 0.45.1, 3 of
  its 55 tools are broken (`new` with parents, `git-push` with `allowNew`,
  `operation-undo`), two bug reports from March 2026 sit unanswered with fix
  PRs open, and its tool list costs about 9.5k tokens of context.

## Stack: Rust with rmcp

- Single static binary, millisecond startup, no runtime to install. The
  server starts with every session of three agents (Claude Code, Codex, agy).
- rmcp is the official Rust MCP SDK (3.5.0, updated 2026-09-28). Tool
  parameters are typed and their JSON Schema is generated with schemars.
- The rest of the local agent tooling is already maintained in Rust.

Reopen if: rmcp stops tracking the MCP spec.

## Talking to jj: the CLI, with templates

The server runs the `jj` binary from `PATH` instead of embedding `jj-lib`.

| | CLI subprocess | `jj-lib` embedded |
|---|---|---|
| Cost per call | 10 ms process floor, 19 to 22 ms for status, log, diff | none |
| Structured output | `-T 'json(self)'` for commits and bookmarks; `status` and diffs are text | native types |
| Push and sign-on-push | inherited | reimplement: `sign_commits_before_push` lives in `cli/src/commands/git/push.rs` (1,382 lines), not in the lib |
| Config, snapshot, immutable set | inherited | reimplement parts of `cli/src/cli_util.rs` (4,862 lines) |
| jj upgrades | keeps working unless a used flag changes | `jj-lib` shipped 18 releases in 18 months, each semver-breaking; the binary must be rebuilt in lockstep with the system jj |
| Dependency tree | 78 crates | 297 crates |

The CLI also brings the user's configuration for free: signing through
`ssh-keygen-avisando`, `git.sign-on-push`, aliases, and the `~/.local/bin/jj`
shim that colocates jj in git-only repositories on first use.

Reopen if: jj gains a stable library API, or per-call latency starts to matter.

## Tool surface: one tool per operation

The MCP annotations (`readOnlyHint`, `destructiveHint`, `openWorldHint`) and
the agents' permission rules apply per tool. Coarse tools that hide several
operations behind an `op` parameter would mix safe reads with destructive
writes under one permission. A generic `jj(args)` passthrough would be a shell
with extra steps.

Read (`readOnlyHint`): `status`, `log`, `show`, `diff`, `bookmark_list`,
`op_log`.

Local write, undoable with `undo`: `describe`, `new`, `commit`, `squash`,
`split` (by paths, never interactive), `edit`, `rebase`, `restore`
(destructive), `abandon` (destructive), `undo`, `file_untrack`,
`bookmark_set`.

Network (`openWorldHint`): `git_fetch`, `git_push`.

Left to the shell on purpose: `init` and `clone` (the shim handles init),
`resolve` (interactive merge tool; conflicts are listed by `status`),
`file_track`, `bookmark_delete`, `bookmark_track`. The jj mental model ships
in the server `instructions`, not as a tool.

Reopen if: agents keep falling back to the shell for one of the excluded
operations.

## Repository location: `repo` is required

Every tool takes `repo`, an absolute path. The server runs jj with it as the
working directory, so any subdirectory works and jj finds the root. Relative
or missing paths are rejected before anything runs. The server's own working
directory is never used: it is wherever the agent was launched, which stops
matching as soon as the agent changes directory.

## Execution and response contract

- argv is built as a vector and executed without a shell.
- Every call runs with `--color never --no-pager`, stdin bound to `/dev/null`,
  and `JJ_EDITOR`/`EDITOR` set to a command that fails, so no path can block
  waiting for input.
- Success returns stdout and also jj's stderr, where jj reports what it did
  ("Working copy now at", "Rebased 3 commits") and its warnings.
- JSON tools declare an `outputSchema` and return `structuredContent`, always
  with a text copy for clients that do not read structured output.
- A failing jj call is a tool result with `isError: true` carrying the argv,
  the exit code, and the verbatim stderr. Invalid parameters are protocol
  errors (`-32602`) raised before execution.
- stdout carries JSON-RPC only. Logs go to stderr, filtered by `RUST_LOG`.

## Long operations: synchronous, prepared

Each commit signed on push asks for a YubiKey touch. Codex cuts MCP tools at
60 s by default (`tool_timeout_sec`).

- `git_push` runs a `--dry-run` first and reports how many commits will be
  signed.
- When the client sends a `progressToken`, the server emits progress
  notifications while it waits.
- No short server-side timeout; client cancellation kills the jj process.
- The agent configs raise the tool timeout for this server (Codex
  `tool_timeout_sec = 600`, same for agy).

Reopen if: pushes regularly outlive client timeouts even after raising them;
then an asynchronous `git_push` plus `push_status` becomes worth its state.

## Concurrency: one writer per repository

Writes to the same repository are queued; reads run freely. jj tolerates
concurrent operations by recording divergent operations and merging them,
which shows up as noise in `op_log` for an agent.

## Architecture and tests

A single crate with a library and a binary (edition 2024).

```
src/
  main.rs        tokio, stdio transport, tracing on stderr
  jj.rs          runner: command builder, hardened environment, output capture,
                 typed errors, per-repository write queue
  repo.rs        RepoPath newtype: absolute, existing, canonicalized
  templates.rs   jj templates and serde types (Commit, Bookmark, Operation)
  tools/
    read.rs      status, log, show, diff, bookmark_list, op_log
    write.rs     describe, new, commit, squash, split, edit, rebase,
                 restore, abandon, undo, file_untrack, bookmark_set
    remote.rs    git_fetch, git_push
  instructions.md
```

Each tool separates a pure `params -> argv` function from execution.

- Unit tests cover argv construction and `RepoPath` validation.
- Integration tests create a colocated repository and a bare remote, run the
  server in process behind an rmcp client over a duplex channel, and exercise
  all 20 tools against the real jj, including a push signed with a software
  SSH key and a signature check on the remote.
- Integration tests fail, not skip, when `jj` is missing from `PATH`.
- Gates: `cargo fmt --check`, `cargo clippy -- -D warnings`, `cargo test`,
  wrapped in a `justfile`. Errors use `thiserror`; no `unwrap` outside tests.

Installed with `cargo install --path .`; `target/` lives on the data disk via
`offload`. On startup the server logs a warning when the jj on `PATH` is older
than the version the tests ran against.
