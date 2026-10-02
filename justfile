# Validation gates: formatting, lints, tests.
check:
    cargo fmt --check
    cargo clippy --all-targets -- -D warnings
    cargo test

test *args:
    cargo test {{args}}
