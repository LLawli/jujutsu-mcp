//! Non-interactive diff editor behind `split` with `contents`.
//!
//! jj only splits inside a file through a diff editor, which it runs with
//! two directories: `$left` (the parent) and `$right` (the revision), and
//! whatever is left in `$right` becomes the first commit. The server stages
//! the requested contents in a directory and points jj at this binary,
//! `jujutsu-mcp split-editor <staged> <right>`, which copies them over.

use std::path::{Path, PathBuf};

/// Subcommand of the binary that acts as the diff editor.
pub const COMMAND: &str = "split-editor";

#[derive(Debug, thiserror::Error)]
pub enum SplitEditorError {
    #[error("usage: jujutsu-mcp {COMMAND} <staged> <right>")]
    Usage,
    #[error("{}: {source}", path.display())]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

/// Copies every file under `staged` into `right` at the same relative path,
/// creating directories and overwriting files. Files of `right` that
/// `staged` does not have are left alone.
pub fn fill_right(staged: &Path, right: &Path) -> Result<(), SplitEditorError> {
    let entries = std::fs::read_dir(staged).map_err(|source| io_error(staged, source))?;
    for entry in entries {
        let entry = entry.map_err(|source| io_error(staged, source))?;
        let from = entry.path();
        let to = right.join(entry.file_name());
        // `metadata` follows symlinks: a staged symlink is copied as the
        // file it points to.
        let metadata = std::fs::metadata(&from).map_err(|source| io_error(&from, source))?;
        if metadata.is_dir() {
            std::fs::create_dir_all(&to).map_err(|source| io_error(&to, source))?;
            fill_right(&from, &to)?;
        } else {
            std::fs::copy(&from, &to).map_err(|source| io_error(&to, source))?;
        }
    }
    Ok(())
}

fn io_error(path: &Path, source: std::io::Error) -> SplitEditorError {
    SplitEditorError::Io {
        path: path.to_path_buf(),
        source,
    }
}

/// `jujutsu-mcp split-editor <staged> <right>`: `args` are the arguments
/// after the subcommand.
pub fn run(args: &[String]) -> Result<(), SplitEditorError> {
    match args {
        [staged, right] => fill_right(Path::new(staged), Path::new(right)),
        _ => Err(SplitEditorError::Usage),
    }
}
