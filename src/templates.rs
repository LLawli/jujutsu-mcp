//! jj templates and the serde types they produce.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Template for `jj log --no-graph`: one JSON object per line, matching
/// [`Commit`].
///
/// Every string goes through `json(...)`, so descriptions with quotes or
/// newlines stay on one line. `remote_bookmarks` is assembled by hand because
/// `json` cannot serialize a list of templates, and it skips the `git`
/// remote that colocated repositories mirror every bookmark to.
pub const COMMIT_TEMPLATE: &str = concat!(
    r#""{""#,
    r#" ++ "\"change_id\":" ++ json(change_id)"#,
    r#" ++ ",\"commit_id\":" ++ json(commit_id)"#,
    r#" ++ ",\"parents\":" ++ json(parents.map(|c| c.commit_id()))"#,
    r#" ++ ",\"description\":" ++ json(description)"#,
    r#" ++ ",\"author\":" ++ json(author)"#,
    r#" ++ ",\"committer\":" ++ json(committer)"#,
    r#" ++ ",\"bookmarks\":" ++ json(local_bookmarks.map(|b| b.name()))"#,
    r#" ++ ",\"remote_bookmarks\":[""#,
    r#" ++ remote_bookmarks.filter(|b| b.remote() != "git")"#,
    r#".map(|b| json(stringify(b.name() ++ "@" ++ b.remote()))).join(",")"#,
    r#" ++ "],\"working_copy\":" ++ json(current_working_copy)"#,
    r#" ++ ",\"empty\":" ++ json(empty)"#,
    r#" ++ ",\"conflict\":" ++ json(conflict)"#,
    r#" ++ ",\"immutable\":" ++ json(immutable)"#,
    r#" ++ ",\"divergent\":" ++ json(divergent)"#,
    r#" ++ "}\n""#,
);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Signature {
    pub name: String,
    pub email: String,
    /// RFC 3339 timestamp with the author's offset.
    pub timestamp: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Commit {
    /// Full change id; stable across rewrites, the id to pass back to jj.
    pub change_id: String,
    pub commit_id: String,
    /// Commit ids of the parents.
    pub parents: Vec<String>,
    pub description: String,
    pub author: Signature,
    pub committer: Signature,
    /// Local bookmarks pointing at this commit.
    pub bookmarks: Vec<String>,
    /// Remote bookmarks as `name@remote`, excluding the colocated `git` remote.
    pub remote_bookmarks: Vec<String>,
    /// This is the working-copy commit (`@`) of the current workspace.
    pub working_copy: bool,
    pub empty: bool,
    pub conflict: bool,
    /// Part of the immutable set; jj refuses to rewrite it.
    pub immutable: bool,
    pub divergent: bool,
}

/// Parses the output of [`COMMIT_TEMPLATE`], one commit per line.
pub fn parse_commits(stdout: &str) -> Result<Vec<Commit>, serde_json::Error> {
    stdout
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(serde_json::from_str)
        .collect()
}
