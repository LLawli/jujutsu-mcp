//! Validated repository location passed to every tool.

use std::io;
use std::path::{Path, PathBuf};

/// An absolute, existing, canonicalized directory where jj is run.
///
/// jj finds the repository root from any subdirectory, so the path only has
/// to be inside a workspace, not at its root.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RepoPath(PathBuf);

#[derive(Debug, thiserror::Error)]
pub enum RepoPathError {
    #[error("repo must be an absolute path, got {0:?}")]
    NotAbsolute(PathBuf),
    #[error("repo {path:?} is not accessible: {source}")]
    Inaccessible {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("repo {0:?} is not a directory")]
    NotADirectory(PathBuf),
}

impl RepoPath {
    pub fn new(path: impl AsRef<Path>) -> Result<Self, RepoPathError> {
        let path = path.as_ref();
        if !path.is_absolute() {
            return Err(RepoPathError::NotAbsolute(path.to_path_buf()));
        }
        let canonical =
            std::fs::canonicalize(path).map_err(|source| RepoPathError::Inaccessible {
                path: path.to_path_buf(),
                source,
            })?;
        if !canonical.is_dir() {
            return Err(RepoPathError::NotADirectory(path.to_path_buf()));
        }
        Ok(Self(canonical))
    }

    pub fn as_path(&self) -> &Path {
        &self.0
    }
}
