# Working on jujutsu-mcp

- Gates: `just check` (fmt, clippy with `-D warnings`, tests). Run it before
  every commit; the integration tests need jj, git and `ssh-keygen` on `PATH`.
- Release: `bin/release X.Y.Z` (see docs/releasing.md). It bumps the version,
  dates the changelog, commits, pushes `main` and the signed tag.
- Every user-visible change gets a line under `[Unreleased]` in CHANGELOG.md.
- Design decisions live in docs/decisions.md, each with the evidence and what
  would reopen it. Read it before changing behaviour; update it when a
  decision changes.
- Adding a tool: follow docs/development.md ("Adding a tool").
- Code, comments and docs in English. Errors with `thiserror`; no `unwrap` or
  `expect` outside tests.
- Version control is jj (colocated with git). Conventional commit messages.
