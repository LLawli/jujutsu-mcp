# Releasing

A release is one command:

```sh
bin/release 0.2.0     # or: just release 0.2.0
```

It:

1. Refuses to run with changes in the working-copy commit, with nothing under
   `[Unreleased]` in `CHANGELOG.md`, or when the tag already exists.
2. Sets the version in `Cargo.toml` and `Cargo.lock`.
3. Turns `[Unreleased]` into `[0.2.0] - <today>` and updates the compare links.
4. Runs `just check`.
5. Commits `chore: release v0.2.0` with jj and points `main` at it.
6. Pushes `main` with `jj git push`. Commits are signed on push, so a hardware
   key asks for one touch per outgoing commit.
7. Creates a signed tag `v0.2.0` on the pushed commit (signing on push rewrote
   it, so the tag is made afterwards) and pushes the tag.

`--no-push` stops after the local commit; `--no-check` skips the gates. Both
exist for trying the script out.

## What the tag triggers

`.github/workflows/release.yml`, on any `v*.*.*` tag (or by hand with an
existing tag):

1. Checks that the tag matches `Cargo.toml` and that `CHANGELOG.md` has a
   section for it.
2. Runs the gates on Linux against jj 0.45.1.
3. Builds once per target, natively: Linux x86_64 and aarch64 (musl, static),
   macOS aarch64 and x86_64, Windows x86_64. Each archive holds the binary,
   README, CHANGELOG and licenses, with a `.sha256` next to it.
4. Creates the GitHub Release with the changelog section, install
   instructions and the checksums.
5. Publishes to crates.io, when enabled. A version that is already there is
   not an error, so a failed release can be rerun.
6. Renders `packaging/homebrew/jujutsu-mcp.rb.in` with the version and the
   checksums of the archives built in step 3 and pushes it to the tap over SSH,
   when enabled.

The archives are named `jujutsu-mcp-<target>.tar.gz` (`.zip` on Windows), the
convention mise's GitHub backend recognizes, so `mise use
github:LLawli/jujutsu-mcp` works without any mise-specific setup.

## One-time setup

Publishing to crates.io and Homebrew is opt-in per repository, through
repository variables, so a release never fails on a channel that is not set
up yet:

- crates.io: variable `CRATES_IO=true` and secret `CARGO_REGISTRY_TOKEN` (a
  crates.io API token with publish scope).
- Homebrew: variable `HOMEBREW_TAP=true` and secret `HOMEBREW_TAP_DEPLOY_KEY`,
  the private half of an SSH deploy key with write access on
  `LLawli/homebrew-tap` (the same scheme the other projects in the tap use).

The Windows binary is not code-signed; SmartScreen warns on first run. macOS
binaries are not notarized: archives fetched with curl, Homebrew or mise are
not quarantined, so Gatekeeper does not block them.
