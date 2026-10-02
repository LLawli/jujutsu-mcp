# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project
adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.2] - 2026-10-02

### Fixed

- `git_push`'s `commits to sign: N` now matches the commits jj actually
  signs. It no longer counts commits authored by someone else, commits that
  are already signed, or immutable commits, and it counts commits that
  another remote has but the destination does not. Before, a push to a
  second remote could announce 0 and then ask for touches.

## [0.1.1] - 2026-10-02

### Fixed

- `git_push` no longer counts commits already on the remote in
  `commits to sign: N`. In a colocated repository with a synced trunk the
  count included the whole trunk history.

## [0.1.0] - 2026-10-02

### Added

- MCP server over stdio exposing jj to coding agents, one tool per operation.
- Read tools: `status`, `log`, `show`, `diff`, `bookmark_list`, `op_log`.
  `log`, `bookmark_list` and `op_log` return structured JSON with a text copy.
- Local write tools: `describe`, `new`, `commit`, `squash`, `split` (by
  paths), `edit`, `rebase`, `restore`, `abandon`, `undo`, `file_untrack`,
  `bookmark_set`.
- Network tools: `git_fetch` and `git_push`. `git_push` reports how many
  commits will be signed (`commits to sign: N`), supports `dry_run`, sends
  progress notifications while it waits and is killed on client cancellation.
- `run`, for any jj command without a dedicated tool.
- Every call runs without a shell, with stdin on `/dev/null` and an editor
  that fails, so nothing can block waiting for input.
- Writes to the same workspace are queued; reads never wait.
- `jujutsu-mcp setup` installs the binary and registers it in Claude Code,
  Codex (with a 600 s tool timeout) and Antigravity.
- Startup warning when the jj on `PATH` is older than the tested release.

[Unreleased]: https://github.com/LLawli/jujutsu-mcp/compare/v0.1.2...HEAD
[0.1.2]: https://github.com/LLawli/jujutsu-mcp/compare/v0.1.1...v0.1.2
[0.1.1]: https://github.com/LLawli/jujutsu-mcp/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/LLawli/jujutsu-mcp/releases/tag/v0.1.0
