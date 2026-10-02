//! jj version parsing and lookup.

use jujutsu_mcp::jj::{JjError, JjRunner, JjVersion, TESTED_JJ_VERSION, parse_jj_version};

fn v(major: u32, minor: u32, patch: u32) -> JjVersion {
    JjVersion {
        major,
        minor,
        patch,
    }
}

#[test]
fn parses_release_and_build_suffixes() {
    assert_eq!(parse_jj_version("jj 0.45.1\n"), Some(v(0, 45, 1)));
    assert_eq!(
        parse_jj_version("jj 0.46.0-3fe1a2b4c5d6\n"),
        Some(v(0, 46, 0))
    );
    assert_eq!(parse_jj_version("jj 1.2.3+git.abc"), Some(v(1, 2, 3)));
}

#[test]
fn rejects_unrecognized_output() {
    for output in ["", "jj", "jj version unknown", "git 2.45.1", "jj 0.45"] {
        assert_eq!(parse_jj_version(output), None, "{output:?}");
    }
}

#[test]
fn versions_order_numerically() {
    assert!(v(0, 9, 0) < v(0, 45, 1));
    assert!(v(0, 45, 1) < v(0, 45, 10));
    assert!(v(0, 46, 0) > v(0, 45, 9));
    assert_eq!(v(0, 45, 1).to_string(), "0.45.1");
    assert_eq!(TESTED_JJ_VERSION, v(0, 45, 1));
}

#[tokio::test]
async fn reads_the_installed_version() {
    let version = JjRunner::new()
        .version()
        .await
        .expect("jj must be on PATH")
        .expect("parsable version");
    assert!(version >= TESTED_JJ_VERSION, "{version}");
}

#[tokio::test]
async fn missing_program_is_an_error() {
    let err = JjRunner::new()
        .with_program("/nonexistent/bin/jj")
        .version()
        .await
        .expect_err("missing jj");
    assert!(matches!(err, JjError::Spawn { .. }), "{err:?}");
}
