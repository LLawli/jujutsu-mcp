//! argv construction for read tools, without running jj.

use jujutsu_mcp::templates::COMMIT_TEMPLATE;
use jujutsu_mcp::tools::ToolError;
use jujutsu_mcp::tools::read::{DEFAULT_LOG_LIMIT, LogParams, log_args};

fn log_params(revisions: Option<&str>, limit: Option<u32>) -> LogParams {
    LogParams {
        repo: "/repo".to_owned(),
        revisions: revisions.map(str::to_owned),
        limit,
    }
}

#[test]
fn log_defaults_ask_for_one_more_than_the_default_limit() {
    let argv = log_args(&log_params(None, None)).expect("valid");
    let limit = (DEFAULT_LOG_LIMIT + 1).to_string();
    assert_eq!(
        argv,
        [
            "log",
            "--no-graph",
            "--template",
            COMMIT_TEMPLATE,
            "--limit",
            limit.as_str()
        ]
    );
    assert_eq!(DEFAULT_LOG_LIMIT, 50);
}

#[test]
fn log_passes_revisions_as_a_single_option_argument() {
    let argv = log_args(&log_params(Some("main::@"), Some(5))).expect("valid");
    assert_eq!(
        argv,
        [
            "log",
            "--no-graph",
            "--template",
            COMMIT_TEMPLATE,
            "--limit",
            "6",
            "--revisions=main::@"
        ]
    );
}

#[test]
fn log_revisions_starting_with_a_dash_cannot_become_flags() {
    let argv = log_args(&log_params(Some("--config=ui.editor=evil"), None)).expect("valid");
    assert_eq!(
        argv.last().map(String::as_str),
        Some("--revisions=--config=ui.editor=evil")
    );
    assert!(!argv.iter().any(|arg| arg == "--config=ui.editor=evil"));
}

#[test]
fn log_limit_at_the_maximum_does_not_overflow() {
    let argv = log_args(&log_params(None, Some(u32::MAX))).expect("valid");
    let expected = (u64::from(u32::MAX) + 1).to_string();
    assert!(
        argv.windows(2)
            .any(|pair| pair[0] == "--limit" && pair[1] == expected),
        "{argv:?}"
    );
}

#[test]
fn log_rejects_a_zero_limit() {
    let err = log_args(&log_params(None, Some(0))).expect_err("zero limit");
    assert!(matches!(err, ToolError::InvalidParams(_)), "{err:?}");
    assert!(err.to_string().contains("limit"), "{err}");
}

#[test]
fn log_rejects_an_empty_revset() {
    let err = log_args(&log_params(Some("  "), None)).expect_err("empty revset");
    assert!(matches!(err, ToolError::InvalidParams(_)), "{err:?}");
    assert!(err.to_string().contains("revisions"), "{err}");
}
