# Validation gates: formatting, lints, tests.
check:
    cargo fmt --check
    cargo clippy --all-targets -- -D warnings
    cargo test

test *args:
    cargo test {{args}}

# Cut a release: bin/release X.Y.Z (see docs/releasing.md).
release version:
    bin/release {{version}}
