# jujutsu-mcp

Let Claude Code, Codex and Antigravity use [jj](https://jj-vcs.github.io/jj/)
the way a person does, without fighting the shell.

Coding agents are trained on git. Put them in a jj repository and they reach
for `git add`, open an editor that never closes, or misread jj's output.
jujutsu-mcp gives them jj as tools instead: one tool per operation, typed
parameters, structured results, and nothing that can hang waiting for input.

- **Nothing blocks.** No shell, no pager, stdin on `/dev/null`, and an editor
  that fails, so `describe` without a message is an error, not a frozen agent.
- **Errors explain themselves.** A failing call returns the exact command, the
  exit code and jj's own message.
- **Signed pushes are announced.** `git_push` with `dry_run` says how many
  commits will be signed before any security-key touch, and reports progress
  while it waits.
- **Concurrent agents do not trip over each other.** Writes to the same
  repository are queued; reads never wait.
- **Permissions stay precise.** Reads, local writes and network operations are
  separate tools with MCP hints, so a client can allow `log` and still ask
  before `abandon` or `git_push`.

## Install

Pick whatever you already use, then run `jujutsu-mcp setup`.

```sh
# Homebrew (macOS or Linux)
brew install LLawli/tap/jujutsu-mcp

# cargo
cargo install jujutsu-mcp

# mise
mise use -g github:LLawli/jujutsu-mcp

# Prebuilt binary (Linux x86_64; see the release page for other platforms)
curl -fsSL https://github.com/LLawli/jujutsu-mcp/releases/latest/download/jujutsu-mcp-x86_64-unknown-linux-musl.tar.gz | tar xz
```

Release binaries exist for Linux (x86_64, aarch64), macOS (Apple Silicon,
Intel) and Windows (x86_64). The test suite runs on Linux and Windows in CI;
the Windows binary is not code-signed, so SmartScreen warns on first run.

### Register it in your agents

```sh
jujutsu-mcp setup
```

`setup` registers the server as `jj` in every agent whose CLI is on your
`PATH`: Claude Code (`claude mcp add -s user`), Codex (`codex mcp add`, plus
`tool_timeout_sec = 600` so signed pushes are not cut at 60 s) and Antigravity
(`agy mcp add`). If the `jujutsu-mcp` on your `PATH` is the binary you ran, it
is registered where it is; otherwise (a downloaded tarball, a build in
`target/`) it is copied to `~/.cargo/bin` first. Running it again is safe.

```sh
jujutsu-mcp setup --dry-run          # print the plan, change nothing
jujutsu-mcp setup --agent claude     # only some agents
```

Requirements: jj 0.45.1 or newer on `PATH`. The server warns at startup when
it finds an older one.

## What the agent gets

| Kind | Tools |
|---|---|
| Read | `status`, `log`, `show`, `diff`, `bookmark_list`, `op_log` |
| Local write (undoable) | `describe`, `new`, `commit`, `squash`, `split`, `edit`, `rebase`, `restore`, `abandon`, `undo`, `file_untrack`, `bookmark_set` |
| Network | `git_fetch`, `git_push` |
| Anything else | `run`, with the jj arguments as a list |

Every tool takes `repo`, the absolute path of the repository or any directory
inside it. `log`, `bookmark_list` and `op_log` return JSON. The server also
ships a short guide to jj's model (no staging area, change ids, bookmarks,
undo) as MCP instructions, so the agent reads it once per session. See
[docs/tools.md](docs/tools.md) for every parameter.

## Documentation

- [docs/tools.md](docs/tools.md): the tools and their parameters.
- [docs/decisions.md](docs/decisions.md): why it is built this way, with the
  evidence behind each decision and what would reopen it.
- [docs/development.md](docs/development.md): building, testing, and the layout
  of the code.
- [docs/releasing.md](docs/releasing.md): how a version is cut and published.
- [CHANGELOG.md](CHANGELOG.md).

## License

Licensed under either of [Apache License 2.0](LICENSE-APACHE) or
[MIT license](LICENSE-MIT), at your option.
