use std::fs;
use std::path::Path;

use jujutsu_mcp::repo::{RepoPath, RepoPathError};

fn temp_root() -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().expect("temp dir");
    let root = dir.path().canonicalize().expect("canonicalize");
    (dir, root)
}

#[test]
fn rejects_relative_paths() {
    for relative in ["repo", "./repo", "../repo", ""] {
        let err = RepoPath::new(relative).expect_err(relative);
        assert!(
            matches!(err, RepoPathError::NotAbsolute(ref p) if p == Path::new(relative)),
            "{relative:?} gave {err:?}"
        );
    }
}

#[test]
fn rejects_missing_directory() {
    let (_dir, root) = temp_root();
    let missing = root.join("does-not-exist");
    let err = RepoPath::new(&missing).expect_err("missing path");
    assert!(
        matches!(err, RepoPathError::Inaccessible { ref path, .. } if *path == missing),
        "{err:?}"
    );
}

#[test]
fn rejects_regular_file() {
    let (_dir, root) = temp_root();
    let file = root.join("file.txt");
    fs::write(&file, "x").expect("write");
    let err = RepoPath::new(&file).expect_err("file");
    assert!(
        matches!(err, RepoPathError::NotADirectory(ref p) if *p == file),
        "{err:?}"
    );
}

#[test]
fn accepts_existing_directory() {
    let (_dir, root) = temp_root();
    let repo = RepoPath::new(&root).expect("valid");
    assert_eq!(repo.as_path(), root);
}

#[test]
fn canonicalizes_dot_dot_and_trailing_slash() {
    let (dir, root) = temp_root();
    let sub = root.join("a");
    fs::create_dir(&sub).expect("mkdir");
    // Built from the path as a caller would write it: a canonical Windows
    // path is verbatim (`\\?\`), and verbatim paths take no `..` or `/`.
    let messy = format!("{}/a/../a/./", dir.path().display());
    let repo = RepoPath::new(&messy).expect("valid");
    assert_eq!(repo.as_path(), sub);
}

#[cfg(unix)]
#[test]
fn resolves_symlinks() {
    let (_dir, root) = temp_root();
    let target = root.join("target");
    fs::create_dir(&target).expect("mkdir");
    let link = root.join("link");
    std::os::unix::fs::symlink(&target, &link).expect("symlink");
    let repo = RepoPath::new(&link).expect("valid");
    assert_eq!(repo.as_path(), target);
}

#[test]
fn error_messages_name_the_path() {
    let err = RepoPath::new("relative/dir").expect_err("relative");
    assert!(err.to_string().contains("relative/dir"), "{err}");
}
