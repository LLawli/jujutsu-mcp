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
`split` (by paths or by contents, never interactive), `edit`, `rebase`, `restore`
(destructive), `abandon` (destructive), `undo`, `file_untrack`,
`bookmark_set`.

Network (`openWorldHint`): `git_fetch`, `git_push`.

Left to the shell on purpose: `init` and `clone` (the shim handles init),
`resolve` (interactive merge tool; conflicts are listed by `status`),
`file_track`, `bookmark_delete`, `bookmark_track`. The jj mental model ships
in the server `instructions`, not as a tool.

Reopen if: agents keep falling back to the shell for one of the excluded
operations.

`split` by contents, added in 0.1.4: jj splits inside a file only through a
diff editor, so `split` runs this binary (`jujutsu-mcp split-editor`) as a
non-interactive one that writes the requested contents into `$right`.
Measured on a real project (python-humanize, two options added to one
function, sharing lines) against two alternatives:

- Writing the intermediate file by hand, committing, then writing the final
  file back: works, but took 6 calls on `@` and 7 or more on an older
  revision (one through `run`, for `new --insert-after`), rewrote the
  working copy three times, left descendants without the second change
  while in progress, and tripped over jj abandoning the empty working-copy
  commit on `edit`.
- Selecting hunks by number: all 4 hunks mixed both changes, and two single
  lines held both, so it could not express the split at all.
- `contents`: one call on `@` and on an older revision, results identical
  byte for byte, no conflicts, descendants untouched.

The editor is the server's own binary rather than a script so it works on
Windows. Reopen if: jj gains a non-interactive way to select changes inside
a file.

Reopened on 2026-10-02, at the user's request, before the first rollout:
`run` takes a jj argv (`["bookmark", "delete", "old"]`) for anything the
dedicated tools do not cover, so the shell hook can block jj without
leaving agents stuck. It keeps the hardening (no shell, stdin on
`/dev/null`, failing editor), always takes the repository's write queue
because it cannot tell reads from writes, and is annotated destructive and
open-world so clients ask before running it. Its description steers agents
to the dedicated tools first.

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
  the exit code, and the verbatim stderr.
- Invalid parameters (missing field, relative `repo`, `limit` 0) are also
  tool results with `isError: true`, raised before anything runs. The MCP
  spec (2025-11-25, SEP-1303) classifies input validation as a tool
  execution error so the model reads the message and corrects the call;
  clients may render protocol errors opaquely. rmcp 3.5 does the same for
  deserialization failures. Protocol errors are left for an unknown tool.
- Text blocks the server adds (the `commits to sign: N` summary) end with a
  newline. Claude Code was seen joining a result's content blocks with
  nothing in between, which glued the summary to jj's first line in 2 of 5
  real pushes; `summary_stays_on_its_own_line_when_blocks_are_joined`
  reproduces it.
- stdout carries JSON-RPC only. Logs go to stderr, filtered by `RUST_LOG`.
- The runner is a value (`JjRunner`) holding the program and extra
  environment, handed to the server at construction. Tests point it at an
  isolated `JJ_CONFIG` through it. Setting variables on the process instead
  would need `unsafe` `set_var` under edition 2024 and would leak between
  tests running in parallel threads.

## Long operations: synchronous, prepared

Each commit signed on push asks for a YubiKey touch. Codex cuts MCP tools at
60 s by default (`tool_timeout_sec`).

- `git_push` runs a `--dry-run` first and reports how many commits will be
  signed.
- When the client sends a `progressToken`, the server emits progress
  notifications while it waits.
- The count mirrors `sign_commits_before_push` in jj's
  `cli/src/commands/git/push.rs` (0.45.1):
  `((::targets ~ ::remote_bookmarks(remote=exact:"<dest>")) ~ immutable()) & mine() & ~signed()`,
  with the destination remote read from the dry run's `Changes to push to
  <remote>:` line. Each filter matches one of jj's: it only excludes what the
  destination remote already has, skips immutable commits (they are pushed
  unsigned, with a warning), and signs only unsigned commits authored by the
  user. Without a remote line the remote exclusion is dropped: counting too
  many touches is better than announcing none and then waiting on the key.
- Evidence: an end-to-end run against the released binary compared the
  announced count with jj's `Updated signatures of K commits` in 11
  scenarios. The v0.1.1 revset (all remotes but `git`, no other filter) was
  wrong in 5 of them: a coworker's rebased commits (3 announced, 1 signed),
  a branch re-pushed after the remote lost it (2, 0), `signing.behavior =
  "own"` (2, 0), a commit under a tag (2, 1) and commits that reached another
  remote unsigned (0, 2). The new revset matched all 11. v0.1.0 had also
  subtracted the `git` remote's commits from every remote's, which dropped a
  synced trunk from the exclusion and counted all of it (23 announced, 16
  signed).
- `git_push` does not track bookmarks for the agent. jj 0.45.1 refuses to
  create a bookmark on a remote that does not track it ("Refusing to create
  new remote bookmark", with a `jj bookmark track NAME@REMOTE` hint) and has
  no push flag to skip that (`--allow-new` is gone). Tracking inside the tool
  would make `dry_run` change state, and a tracked bookmark follows the
  remote on later fetches, which the user did not ask for. The parameter
  description says so, and the agent tracks with `run` (measured in an
  end-to-end run: one failed dry run, one `run`, then the push).
- Reopen if: jj changes which commits it signs on push (watch
  `sign_commits_before_push` across releases), or `mine()` / `signed()`
  stop matching its author and signature checks.
- jj 0.45.1 exits 0 when asked to push a bookmark that does not exist, with
  only a `No matching bookmarks for names` warning. `git_push` turns that
  warning from the dry run into an error before anything is pushed. It
  depends on the warning's wording; if jj rewords it, the push falls back to
  jj's own behavior and `push_failure_is_a_tool_error` catches the change.
- No short server-side timeout; client cancellation kills the jj process.
  rmcp 3.5 does not drop a handler on cancellation, it only cancels the
  request's token, so the tool selects on that token and drops the jj
  future itself.
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

`target/` lives on the data disk via `offload`. On startup the server logs a
warning when the jj on `PATH` is older than the version the tests ran
against.

## Installation: `jujutsu-mcp setup`

`setup` registers the binary as the `jj` server in every agent whose CLI is
on `PATH`. When the `jujutsu-mcp` found on `PATH` is the running binary,
directly or through a symlink (Homebrew's `bin/` entry, `~/.cargo/bin`), that
path is registered as is, so package-manager upgrades reach the agents.
Otherwise (a downloaded tarball, a build in `target/`) the binary is copied to
`<cargo home>/bin/jujutsu-mcp` first, so nothing is ever registered at a path
that `cargo clean` deletes. mise installs under a versioned directory; after a
mise upgrade, rerun `setup`.

- Registration goes through each agent's CLI (`claude mcp add -s user`,
  `codex mcp add`, `agy mcp add`), never by editing their files: Claude Code
  rewrites `~/.claude.json` while it runs, and the CLIs own their formats.
- Codex's CLI cannot set `tool_timeout_sec`, so setup writes
  `tool_timeout_sec = 600` into `config.toml` with `toml_edit`, which keeps
  comments and ordering. That is the reason for the dependency.
- The copy goes to a temporary file in the target directory and is renamed
  over the old binary, which may be running.
- Rerunning is safe: earlier registrations are removed or updated first.
- `--dry-run` prints the plan; `--agent` limits it to some agents.
- The user's shell hook (`jj-guarda`) and instruction files are not part of
  setup: they belong to one machine's environment, not to the server.
