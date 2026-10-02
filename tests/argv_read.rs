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

mod slice4 {
    use jujutsu_mcp::templates::{BOOKMARK_TEMPLATE, OPERATION_TEMPLATE};
    use jujutsu_mcp::tools::ToolError;
    use jujutsu_mcp::tools::read::{
        BookmarkListParams, DEFAULT_OP_LOG_LIMIT, DiffFormat, DiffParams, OpLogParams, ShowParams,
        StatusParams, bookmark_list_args, diff_args, literal_path_fileset, op_log_args, show_args,
        status_args,
    };

    fn diff(revisions: Option<&str>, from: Option<&str>, to: Option<&str>) -> DiffParams {
        DiffParams {
            repo: "/repo".to_owned(),
            revisions: revisions.map(str::to_owned),
            from: from.map(str::to_owned),
            to: to.map(str::to_owned),
            paths: Vec::new(),
            format: None,
        }
    }

    fn assert_invalid(result: Result<Vec<String>, ToolError>, mentions: &str) {
        match result {
            Err(err @ ToolError::InvalidParams(_)) => {
                assert!(err.to_string().contains(mentions), "{err}")
            }
            other => panic!("expected InvalidParams mentioning {mentions}, got {other:?}"),
        }
    }

    #[test]
    fn status_is_plain() {
        let params = StatusParams {
            repo: "/repo".to_owned(),
        };
        assert_eq!(status_args(&params), ["status"]);
    }

    #[test]
    fn literal_paths_escape_quotes_and_backslashes() {
        assert_eq!(
            literal_path_fileset("src/a b.rs").unwrap(),
            r#"cwd:"src/a b.rs""#
        );
        assert_eq!(literal_path_fileset("a(1)|b").unwrap(), r#"cwd:"a(1)|b""#);
        assert_eq!(
            literal_path_fileset(r#"say "hi"\now"#).unwrap(),
            r#"cwd:"say \"hi\"\\now""#
        );
    }

    #[test]
    fn literal_paths_reject_empty() {
        let err = literal_path_fileset("").expect_err("empty path");
        assert!(matches!(err, ToolError::InvalidParams(_)), "{err:?}");
    }

    #[test]
    fn show_defaults_to_the_working_copy_after_a_separator() {
        let params = ShowParams {
            repo: "/repo".to_owned(),
            revision: None,
            format: None,
        };
        assert_eq!(show_args(&params).unwrap(), ["show", "--", "@"]);
    }

    #[test]
    fn show_with_format_and_dash_revision() {
        let params = ShowParams {
            repo: "/repo".to_owned(),
            revision: Some("-x".to_owned()),
            format: Some(DiffFormat::Git),
        };
        assert_eq!(show_args(&params).unwrap(), ["show", "--git", "--", "-x"]);
    }

    #[test]
    fn show_rejects_an_empty_revision() {
        let params = ShowParams {
            repo: "/repo".to_owned(),
            revision: Some(" ".to_owned()),
            format: None,
        };
        assert_invalid(show_args(&params), "revision");
    }

    #[test]
    fn diff_defaults_to_jj() {
        assert_eq!(diff_args(&diff(None, None, None)).unwrap(), ["diff"]);
    }

    #[test]
    fn diff_formats_map_to_flags() {
        for (format, flag) in [
            (DiffFormat::Git, "--git"),
            (DiffFormat::Stat, "--stat"),
            (DiffFormat::Summary, "--summary"),
            (DiffFormat::NameOnly, "--name-only"),
        ] {
            let mut params = diff(None, None, None);
            params.format = Some(format);
            assert_eq!(diff_args(&params).unwrap(), ["diff", flag]);
        }
    }

    #[test]
    fn diff_revisions_and_paths() {
        let mut params = diff(Some("@-"), None, None);
        params.paths = vec!["a b.txt".to_owned(), "dir".to_owned()];
        params.format = Some(DiffFormat::Stat);
        assert_eq!(
            diff_args(&params).unwrap(),
            [
                "diff",
                "--stat",
                "--revisions=@-",
                "--",
                r#"cwd:"a b.txt""#,
                r#"cwd:"dir""#
            ]
        );
    }

    #[test]
    fn diff_from_to() {
        assert_eq!(
            diff_args(&diff(None, Some("main"), Some("@"))).unwrap(),
            ["diff", "--from=main", "--to=@"]
        );
        assert_eq!(
            diff_args(&diff(None, Some("main"), None)).unwrap(),
            ["diff", "--from=main"]
        );
    }

    #[test]
    fn diff_rejects_revisions_with_from_or_to() {
        assert_invalid(diff_args(&diff(Some("@"), Some("main"), None)), "revisions");
        assert_invalid(diff_args(&diff(Some("@"), None, Some("main"))), "revisions");
    }

    #[test]
    fn diff_rejects_empty_revsets_and_paths() {
        assert_invalid(diff_args(&diff(Some(""), None, None)), "revisions");
        assert_invalid(diff_args(&diff(None, Some(" "), None)), "from");
        assert_invalid(diff_args(&diff(None, None, Some(""))), "to");
        let mut params = diff(None, None, None);
        params.paths = vec![String::new()];
        assert!(diff_args(&params).is_err());
    }

    #[test]
    fn bookmark_list_args_shape() {
        let params = BookmarkListParams {
            repo: "/repo".to_owned(),
            all_remotes: false,
            names: Vec::new(),
        };
        assert_eq!(
            bookmark_list_args(&params).unwrap(),
            ["bookmark", "list", "--template", BOOKMARK_TEMPLATE]
        );
        let params = BookmarkListParams {
            repo: "/repo".to_owned(),
            all_remotes: true,
            names: vec!["feat*".to_owned(), "-x".to_owned()],
        };
        assert_eq!(
            bookmark_list_args(&params).unwrap(),
            [
                "bookmark",
                "list",
                "--template",
                BOOKMARK_TEMPLATE,
                "--all-remotes",
                "--",
                "feat*",
                "-x"
            ]
        );
    }

    #[test]
    fn op_log_args_shape() {
        let limit = (DEFAULT_OP_LOG_LIMIT + 1).to_string();
        let params = OpLogParams {
            repo: "/repo".to_owned(),
            limit: None,
        };
        assert_eq!(
            op_log_args(&params).unwrap(),
            [
                "operation",
                "log",
                "--no-graph",
                "--template",
                OPERATION_TEMPLATE,
                "--limit",
                limit.as_str()
            ]
        );
        assert_eq!(DEFAULT_OP_LOG_LIMIT, 20);
        let zero = OpLogParams {
            repo: "/repo".to_owned(),
            limit: Some(0),
        };
        assert_invalid(op_log_args(&zero), "limit");
    }
}
