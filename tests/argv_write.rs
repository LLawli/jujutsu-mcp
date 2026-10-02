//! argv construction for write tools, without running jj.

use std::collections::BTreeMap;
use std::path::Path;

use jujutsu_mcp::tools::ToolError;
use jujutsu_mcp::tools::write::*;

const REPO: &str = "/repo";

fn s(list: &[&str]) -> Vec<String> {
    list.iter().map(|x| (*x).to_owned()).collect()
}

fn assert_invalid(result: Result<Vec<String>, ToolError>, mentions: &str) {
    match result {
        Err(err @ ToolError::InvalidParams(_)) => {
            assert!(err.to_string().contains(mentions), "{err}")
        }
        other => panic!("expected InvalidParams mentioning {mentions}, got {other:?}"),
    }
}

fn squash() -> SquashParams {
    SquashParams {
        repo: REPO.to_owned(),
        revision: None,
        from: None,
        into: None,
        paths: Vec::new(),
        message: None,
        use_destination_message: false,
        keep_emptied: false,
    }
}

fn rebase() -> RebaseParams {
    RebaseParams {
        repo: REPO.to_owned(),
        revisions: None,
        source: None,
        branch: None,
        onto: Some("main".to_owned()),
        insert_after: None,
        insert_before: None,
        skip_emptied: false,
    }
}

fn restore() -> RestoreParams {
    RestoreParams {
        repo: REPO.to_owned(),
        from: None,
        into: None,
        changes_in: None,
        paths: Vec::new(),
        restore_descendants: false,
    }
}

#[test]
fn describe_message_is_one_argument_and_revision_follows_separator() {
    let params = DescribeParams {
        repo: REPO.to_owned(),
        message: "-n not a flag\n\nbody".to_owned(),
        revision: None,
    };
    assert_eq!(
        describe_args(&params).unwrap(),
        s(&["describe", "--message=-n not a flag\n\nbody", "--", "@"])
    );
    let params = DescribeParams {
        repo: REPO.to_owned(),
        message: String::new(),
        revision: Some("feat".to_owned()),
    };
    assert_eq!(
        describe_args(&params).unwrap(),
        s(&["describe", "--message=", "--", "feat"])
    );
}

#[test]
fn describe_rejects_an_empty_revision() {
    let params = DescribeParams {
        repo: REPO.to_owned(),
        message: "m".to_owned(),
        revision: Some(String::new()),
    };
    assert_invalid(describe_args(&params), "revision");
}

#[test]
fn new_shapes() {
    let bare = NewParams {
        repo: REPO.to_owned(),
        parents: Vec::new(),
        message: None,
    };
    assert_eq!(new_args(&bare).unwrap(), s(&["new"]));
    let merge = NewParams {
        repo: REPO.to_owned(),
        parents: s(&["a", "b"]),
        message: Some("merge".to_owned()),
    };
    assert_eq!(
        new_args(&merge).unwrap(),
        s(&["new", "--message=merge", "--", "a", "b"])
    );
    let empty = NewParams {
        repo: REPO.to_owned(),
        parents: s(&[""]),
        message: None,
    };
    assert_invalid(new_args(&empty), "parents");
}

#[test]
fn commit_shapes() {
    let all = CommitParams {
        repo: REPO.to_owned(),
        message: "feat: x".to_owned(),
        paths: Vec::new(),
    };
    assert_eq!(
        commit_args(&all).unwrap(),
        s(&["commit", "--message=feat: x"])
    );
    let some = CommitParams {
        repo: REPO.to_owned(),
        message: "feat: x".to_owned(),
        paths: s(&["a b.txt"]),
    };
    assert_eq!(
        commit_args(&some).unwrap(),
        s(&["commit", "--message=feat: x", "--", r#"cwd:"a b.txt""#])
    );
}

#[test]
fn squash_default_and_full() {
    assert_eq!(squash_args(&squash()).unwrap(), s(&["squash"]));

    let mut params = squash();
    params.revision = Some("@-".to_owned());
    params.message = Some("joined".to_owned());
    params.keep_emptied = true;
    params.paths = s(&["src"]);
    assert_eq!(
        squash_args(&params).unwrap(),
        s(&[
            "squash",
            "--revision=@-",
            "--message=joined",
            "--keep-emptied",
            "--",
            r#"cwd:"src""#
        ])
    );

    let mut params = squash();
    params.from = Some("a".to_owned());
    params.into = Some("b".to_owned());
    params.use_destination_message = true;
    assert_eq!(
        squash_args(&params).unwrap(),
        s(&[
            "squash",
            "--from=a",
            "--into=b",
            "--use-destination-message"
        ])
    );
}

#[test]
fn squash_rejects_conflicting_options() {
    let mut params = squash();
    params.revision = Some("a".to_owned());
    params.into = Some("b".to_owned());
    assert_invalid(squash_args(&params), "revision");

    let mut params = squash();
    params.message = Some("m".to_owned());
    params.use_destination_message = true;
    assert_invalid(squash_args(&params), "message");
}

#[test]
fn split_shapes() {
    let params = SplitParams {
        repo: REPO.to_owned(),
        revision: None,
        paths: s(&["a.txt"]),
        contents: BTreeMap::new(),
        message: "first part".to_owned(),
        parallel: false,
    };
    assert_eq!(
        split_args(&params, None).unwrap(),
        s(&["split", "--message=first part", "--", r#"cwd:"a.txt""#])
    );
    let params = SplitParams {
        repo: REPO.to_owned(),
        revision: Some("feat".to_owned()),
        paths: s(&["a.txt", "b"]),
        contents: BTreeMap::new(),
        message: "first".to_owned(),
        parallel: true,
    };
    assert_eq!(
        split_args(&params, None).unwrap(),
        s(&[
            "split",
            "--revision=feat",
            "--message=first",
            "--parallel",
            "--",
            r#"cwd:"a.txt""#,
            r#"cwd:"b""#
        ])
    );
}

#[test]
fn split_requires_paths() {
    let params = SplitParams {
        repo: REPO.to_owned(),
        revision: None,
        paths: Vec::new(),
        contents: BTreeMap::new(),
        message: "m".to_owned(),
        parallel: false,
    };
    assert_invalid(split_args(&params, None), "paths");
}

fn contents(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    pairs
        .iter()
        .map(|(path, text)| ((*path).to_owned(), (*text).to_owned()))
        .collect()
}

#[test]
fn split_with_contents_runs_the_split_editor() {
    let params = SplitParams {
        repo: REPO.to_owned(),
        revision: Some("feat".to_owned()),
        paths: s(&["a.txt"]),
        contents: contents(&[("src/b.txt", "b\n"), ("c.txt", "c\n")]),
        message: "first".to_owned(),
        parallel: false,
    };
    let editor = SplitEditor {
        program: Path::new("/opt/bin/jujutsu-mcp"),
        staged: Path::new("/tmp/staged"),
    };
    assert_eq!(
        split_args(&params, Some(editor)).unwrap(),
        s(&[
            "split",
            "--tool=jujutsu-mcp-split",
            r#"--config=merge-tools.jujutsu-mcp-split.program="/opt/bin/jujutsu-mcp""#,
            r#"--config=merge-tools.jujutsu-mcp-split.edit-args=["split-editor", "/tmp/staged", "$right"]"#,
            "--revision=feat",
            "--message=first",
            "--",
            r#"cwd:"a.txt""#,
            r#"cwd:"c.txt""#,
            r#"cwd:"src/b.txt""#,
        ])
    );
}

/// Windows paths carry backslashes; the config values are TOML strings.
#[test]
fn split_editor_paths_are_toml_escaped() {
    let params = SplitParams {
        repo: REPO.to_owned(),
        revision: None,
        paths: Vec::new(),
        contents: contents(&[("a.txt", "a\n")]),
        message: "m".to_owned(),
        parallel: false,
    };
    let editor = SplitEditor {
        program: Path::new(r#"C:\Program Files\jj "mcp"\jujutsu-mcp.exe"#),
        staged: Path::new(r"C:\Temp\staged"),
    };
    let args = split_args(&params, Some(editor)).unwrap();
    assert_eq!(
        args[2],
        r#"--config=merge-tools.jujutsu-mcp-split.program="C:\\Program Files\\jj \"mcp\"\\jujutsu-mcp.exe""#
    );
    assert_eq!(
        args[3],
        r#"--config=merge-tools.jujutsu-mcp-split.edit-args=["split-editor", "C:\\Temp\\staged", "$right"]"#
    );
}

#[test]
fn split_rejects_a_path_in_both_paths_and_contents() {
    let params = SplitParams {
        repo: REPO.to_owned(),
        revision: None,
        paths: s(&["a.txt"]),
        contents: contents(&[("a.txt", "a\n")]),
        message: "m".to_owned(),
        parallel: false,
    };
    let editor = SplitEditor {
        program: Path::new("/bin/jujutsu-mcp"),
        staged: Path::new("/tmp/staged"),
    };
    assert_invalid(split_args(&params, Some(editor)), "contents");
}

#[test]
fn split_with_contents_needs_the_editor() {
    let params = SplitParams {
        repo: REPO.to_owned(),
        revision: None,
        paths: Vec::new(),
        contents: contents(&[("a.txt", "a\n")]),
        message: "m".to_owned(),
        parallel: false,
    };
    assert_invalid(split_args(&params, None), "contents");
}

#[test]
fn edit_shape() {
    let params = EditParams {
        repo: REPO.to_owned(),
        revision: "-x".to_owned(),
    };
    assert_eq!(edit_args(&params).unwrap(), s(&["edit", "--", "-x"]));
    let empty = EditParams {
        repo: REPO.to_owned(),
        revision: " ".to_owned(),
    };
    assert_invalid(edit_args(&empty), "revision");
}

#[test]
fn rebase_shapes() {
    assert_eq!(
        rebase_args(&rebase()).unwrap(),
        s(&["rebase", "--onto=main"])
    );

    let mut params = rebase();
    params.source = Some("feat".to_owned());
    params.onto = None;
    params.insert_after = Some("x".to_owned());
    params.skip_emptied = true;
    assert_eq!(
        rebase_args(&params).unwrap(),
        s(&[
            "rebase",
            "--source=feat",
            "--insert-after=x",
            "--skip-emptied"
        ])
    );

    let mut params = rebase();
    params.revisions = Some("a|b".to_owned());
    params.onto = None;
    params.insert_before = Some("y".to_owned());
    assert_eq!(
        rebase_args(&params).unwrap(),
        s(&["rebase", "--revision=a|b", "--insert-before=y"])
    );

    let mut params = rebase();
    params.branch = Some("@".to_owned());
    assert_eq!(
        rebase_args(&params).unwrap(),
        s(&["rebase", "--branch=@", "--onto=main"])
    );
}

#[test]
fn rebase_needs_exactly_one_destination_and_at_most_one_selector() {
    let mut params = rebase();
    params.onto = None;
    assert_invalid(rebase_args(&params), "onto");

    let mut params = rebase();
    params.insert_after = Some("x".to_owned());
    assert_invalid(rebase_args(&params), "onto");

    let mut params = rebase();
    params.source = Some("a".to_owned());
    params.branch = Some("b".to_owned());
    assert_invalid(rebase_args(&params), "source");
}

#[test]
fn restore_shapes() {
    assert_eq!(restore_args(&restore()).unwrap(), s(&["restore"]));

    let mut params = restore();
    params.from = Some("a".to_owned());
    params.into = Some("b".to_owned());
    params.restore_descendants = true;
    params.paths = s(&["f.txt"]);
    assert_eq!(
        restore_args(&params).unwrap(),
        s(&[
            "restore",
            "--from=a",
            "--into=b",
            "--restore-descendants",
            "--",
            r#"cwd:"f.txt""#
        ])
    );

    let mut params = restore();
    params.changes_in = Some("x".to_owned());
    assert_eq!(
        restore_args(&params).unwrap(),
        s(&["restore", "--changes-in=x"])
    );

    let mut params = restore();
    params.changes_in = Some("x".to_owned());
    params.from = Some("a".to_owned());
    assert_invalid(restore_args(&params), "changes_in");
}

#[test]
fn abandon_shapes() {
    let params = AbandonParams {
        repo: REPO.to_owned(),
        revisions: "a::b".to_owned(),
        retain_bookmarks: true,
        restore_descendants: true,
    };
    assert_eq!(
        abandon_args(&params).unwrap(),
        s(&[
            "abandon",
            "--retain-bookmarks",
            "--restore-descendants",
            "--",
            "a::b"
        ])
    );
    let empty = AbandonParams {
        repo: REPO.to_owned(),
        revisions: String::new(),
        retain_bookmarks: false,
        restore_descendants: false,
    };
    assert_invalid(abandon_args(&empty), "revisions");
}

#[test]
fn undo_shape() {
    let params = UndoParams {
        repo: REPO.to_owned(),
    };
    assert_eq!(undo_args(&params), s(&["undo"]));
}

#[test]
fn file_untrack_shapes() {
    let params = FileUntrackParams {
        repo: REPO.to_owned(),
        paths: s(&["x.log"]),
    };
    assert_eq!(
        file_untrack_args(&params).unwrap(),
        s(&["file", "untrack", "--", r#"cwd:"x.log""#])
    );
    let empty = FileUntrackParams {
        repo: REPO.to_owned(),
        paths: Vec::new(),
    };
    assert_invalid(file_untrack_args(&empty), "paths");
}

#[test]
fn bookmark_set_shapes() {
    let params = BookmarkSetParams {
        repo: REPO.to_owned(),
        name: "feat".to_owned(),
        revision: None,
        allow_backwards: false,
    };
    assert_eq!(
        bookmark_set_args(&params).unwrap(),
        s(&["bookmark", "set", "--revision=@", "--", "feat"])
    );
    let params = BookmarkSetParams {
        repo: REPO.to_owned(),
        name: "-weird".to_owned(),
        revision: Some("@-".to_owned()),
        allow_backwards: true,
    };
    assert_eq!(
        bookmark_set_args(&params).unwrap(),
        s(&[
            "bookmark",
            "set",
            "--revision=@-",
            "--allow-backwards",
            "--",
            "-weird"
        ])
    );
    let empty = BookmarkSetParams {
        repo: REPO.to_owned(),
        name: " ".to_owned(),
        revision: None,
        allow_backwards: false,
    };
    assert_invalid(bookmark_set_args(&empty), "name");
}
