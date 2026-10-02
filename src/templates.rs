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
    parse_json_lines(stdout)
}

/// Template for `jj bookmark list`: one JSON object per line, matching
/// [`Bookmark`], skipping the colocated `git` remote.
///
/// `added_targets` yields one id for a normal bookmark, several when it is
/// conflicted and none when it was deleted locally but still exists on a
/// remote.
pub const BOOKMARK_TEMPLATE: &str = concat!(
    r#"if(self.remote() != "git", "{\"name\":" ++ json(self.name())"#,
    r#" ++ ",\"remote\":" ++ json(self.remote())"#,
    r#" ++ ",\"target\":" ++ json(self.added_targets().map(|c| c.commit_id()))"#,
    r#" ++ ",\"conflict\":" ++ json(self.conflict())"#,
    r#" ++ ",\"tracked\":" ++ json(self.tracked())"#,
    r#" ++ "}\n")"#,
);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Bookmark {
    pub name: String,
    /// `None` for a local bookmark.
    pub remote: Option<String>,
    /// Commit ids; more than one when the bookmark is conflicted, none when
    /// it was deleted locally but still exists on a remote.
    pub target: Vec<String>,
    pub conflict: bool,
    /// For a remote bookmark, whether a local bookmark tracks it.
    pub tracked: bool,
}

/// Template for `jj operation log --no-graph`: one JSON object per line,
/// matching [`Operation`].
///
/// jj renders `attributes` as `key: value` lines; the recorded command line
/// is the `args` entry, which sorts first.
pub const OPERATION_TEMPLATE: &str = concat!(
    r#""{\"id\":" ++ json(id.short())"#,
    r#" ++ ",\"description\":" ++ json(description)"#,
    r#" ++ ",\"time\":" ++ json(time.end())"#,
    r#" ++ ",\"args\":" ++ if(attributes.starts_with("args: "),"#,
    r#" json(attributes.remove_prefix("args: ")), "null")"#,
    r#" ++ ",\"snapshot\":" ++ json(snapshot)"#,
    r#" ++ "}\n""#,
);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Operation {
    /// Short operation id (12 hex digits), accepted by `undo` and `--at-op`.
    pub id: String,
    pub description: String,
    /// RFC 3339 time the operation finished.
    pub time: String,
    /// The command line that created the operation, when jj recorded one.
    pub args: Option<String>,
    /// Automatic snapshot of the working copy, not a user command.
    pub snapshot: bool,
}

/// Parses template output with one JSON value per non-empty line.
pub fn parse_json_lines<T: serde::de::DeserializeOwned>(
    stdout: &str,
) -> Result<Vec<T>, serde_json::Error> {
    stdout
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(serde_json::from_str)
        .collect()
}
