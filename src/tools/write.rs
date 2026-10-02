//! Local write tools. Every one of them is undoable with `undo`.

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::{tool, tool_router};
use schemars::JsonSchema;
use serde::Deserialize;

use crate::repo::RepoPath;
use crate::server::JjServer;
use crate::tools::{ToolError, literal_path_fileset, non_empty, text_result};

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct DescribeParams {
    /// Absolute path of the repository or any directory inside it.
    pub repo: String,
    /// New description; an empty string clears it.
    pub message: String,
    /// Revision to describe. Defaults to `@`.
    #[serde(default)]
    pub revision: Option<String>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct NewParams {
    /// Absolute path of the repository or any directory inside it.
    pub repo: String,
    /// Parents of the new commit; several make a merge. Defaults to `@`.
    #[serde(default)]
    pub parents: Vec<String>,
    #[serde(default)]
    pub message: Option<String>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct CommitParams {
    /// Absolute path of the repository or any directory inside it.
    pub repo: String,
    pub message: String,
    /// Commit only these paths (relative to `repo`, taken literally); the
    /// rest stays in the new working-copy commit. Defaults to everything.
    #[serde(default)]
    pub paths: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct SquashParams {
    /// Absolute path of the repository or any directory inside it.
    pub repo: String,
    /// Squash this revision into its parent. Cannot be combined with
    /// `from`/`into`. Without any of the three, squashes `@` into `@-`.
    #[serde(default)]
    pub revision: Option<String>,
    /// Move changes out of these revisions.
    #[serde(default)]
    pub from: Option<String>,
    /// Move changes into this revision.
    #[serde(default)]
    pub into: Option<String>,
    /// Move only these paths (relative to `repo`, taken literally).
    #[serde(default)]
    pub paths: Vec<String>,
    /// Description of the result. Needed when both sides have one, unless
    /// `use_destination_message` is set.
    #[serde(default)]
    pub message: Option<String>,
    #[serde(default)]
    pub use_destination_message: bool,
    /// Keep the source commit even if it becomes empty.
    #[serde(default)]
    pub keep_emptied: bool,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct SplitParams {
    /// Absolute path of the repository or any directory inside it.
    pub repo: String,
    /// Revision to split. Defaults to `@`.
    #[serde(default)]
    pub revision: Option<String>,
    /// Paths (relative to `repo`, taken literally) that go into the first
    /// commit; the rest stays in the second.
    pub paths: Vec<String>,
    /// Description of the first commit. The second keeps the original.
    pub message: String,
    /// Make the two commits siblings instead of parent and child.
    #[serde(default)]
    pub parallel: bool,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct EditParams {
    /// Absolute path of the repository or any directory inside it.
    pub repo: String,
    /// Revision to make the working-copy commit.
    pub revision: String,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct RebaseParams {
    /// Absolute path of the repository or any directory inside it.
    pub repo: String,
    /// Rebase only these revisions; their descendants stay in place. At
    /// most one of `revisions`, `source`, `branch`; none means
    /// `branch: "@"`.
    #[serde(default)]
    pub revisions: Option<String>,
    /// Rebase these revisions and their descendants.
    #[serde(default)]
    pub source: Option<String>,
    /// Rebase the whole branch relative to the destination.
    #[serde(default)]
    pub branch: Option<String>,
    /// New parents. Exactly one of `onto`, `insert_after`, `insert_before`.
    #[serde(default)]
    pub onto: Option<String>,
    #[serde(default)]
    pub insert_after: Option<String>,
    #[serde(default)]
    pub insert_before: Option<String>,
    /// Abandon commits that become empty.
    #[serde(default)]
    pub skip_emptied: bool,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct RestoreParams {
    /// Absolute path of the repository or any directory inside it.
    pub repo: String,
    /// Restore from this revision. Defaults to the parent of `into`.
    #[serde(default)]
    pub from: Option<String>,
    /// Restore into this revision. Defaults to `@`.
    #[serde(default)]
    pub into: Option<String>,
    /// Undo the changes made in this revision. Cannot be combined with
    /// `from`/`into`.
    #[serde(default)]
    pub changes_in: Option<String>,
    /// Restore only these paths (relative to `repo`, taken literally).
    #[serde(default)]
    pub paths: Vec<String>,
    /// Keep the content of descendants as it is.
    #[serde(default)]
    pub restore_descendants: bool,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct AbandonParams {
    /// Absolute path of the repository or any directory inside it.
    pub repo: String,
    /// Revset of the commits to abandon.
    pub revisions: String,
    /// Keep bookmarks pointing at abandoned commits where they are.
    #[serde(default)]
    pub retain_bookmarks: bool,
    /// Keep the content of descendants as it is.
    #[serde(default)]
    pub restore_descendants: bool,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct UndoParams {
    /// Absolute path of the repository or any directory inside it.
    pub repo: String,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct FileUntrackParams {
    /// Absolute path of the repository or any directory inside it.
    pub repo: String,
    /// Paths to stop tracking (relative to `repo`, taken literally). They
    /// must already be ignored, or jj tracks them again on the next snapshot.
    pub paths: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct BookmarkSetParams {
    /// Absolute path of the repository or any directory inside it.
    pub repo: String,
    /// Bookmark to create or move.
    pub name: String,
    /// Target revision. Defaults to `@`.
    #[serde(default)]
    pub revision: Option<String>,
    /// Allow moving the bookmark backwards or sideways.
    #[serde(default)]
    pub allow_backwards: bool,
}

/// `--flag=value` for an optional free-text value; the value is kept as is.
fn push_opt(
    args: &mut Vec<String>,
    flag: &str,
    field: &str,
    value: &Option<String>,
) -> Result<(), ToolError> {
    if let Some(value) = value {
        args.push(format!("--{flag}={}", non_empty(field, value)?));
    }
    Ok(())
}

fn push_flag(args: &mut Vec<String>, flag: &str, enabled: bool) {
    if enabled {
        args.push(format!("--{flag}"));
    }
}

/// Appends `-- <fileset>...` for literal paths; nothing when there are none.
fn push_paths(args: &mut Vec<String>, paths: &[String]) -> Result<(), ToolError> {
    if paths.is_empty() {
        return Ok(());
    }
    args.push("--".to_owned());
    for path in paths {
        args.push(literal_path_fileset(path)?);
    }
    Ok(())
}

fn require_paths(paths: &[String]) -> Result<(), ToolError> {
    if paths.is_empty() {
        return Err(ToolError::InvalidParams(
            "paths must contain at least one path".to_owned(),
        ));
    }
    Ok(())
}

/// Messages go after `=` so one starting with `-` stays a value. An empty
/// message is valid: it clears the description.
pub fn describe_args(params: &DescribeParams) -> Result<Vec<String>, ToolError> {
    let revision = match &params.revision {
        Some(revision) => non_empty("revision", revision)?,
        None => "@",
    };
    Ok(vec![
        "describe".to_owned(),
        format!("--message={}", params.message),
        "--".to_owned(),
        revision.to_owned(),
    ])
}

pub fn new_args(params: &NewParams) -> Result<Vec<String>, ToolError> {
    let mut args = vec!["new".to_owned()];
    if let Some(message) = &params.message {
        args.push(format!("--message={message}"));
    }
    if !params.parents.is_empty() {
        args.push("--".to_owned());
        for parent in &params.parents {
            args.push(non_empty("parents", parent)?.to_owned());
        }
    }
    Ok(args)
}

pub fn commit_args(params: &CommitParams) -> Result<Vec<String>, ToolError> {
    let mut args = vec!["commit".to_owned(), format!("--message={}", params.message)];
    push_paths(&mut args, &params.paths)?;
    Ok(args)
}

pub fn squash_args(params: &SquashParams) -> Result<Vec<String>, ToolError> {
    if params.revision.is_some() && (params.from.is_some() || params.into.is_some()) {
        return Err(ToolError::InvalidParams(
            "revision cannot be combined with from or into".to_owned(),
        ));
    }
    if params.message.is_some() && params.use_destination_message {
        return Err(ToolError::InvalidParams(
            "message cannot be combined with use_destination_message".to_owned(),
        ));
    }
    let mut args = vec!["squash".to_owned()];
    push_opt(&mut args, "revision", "revision", &params.revision)?;
    push_opt(&mut args, "from", "from", &params.from)?;
    push_opt(&mut args, "into", "into", &params.into)?;
    if let Some(message) = &params.message {
        args.push(format!("--message={message}"));
    }
    push_flag(
        &mut args,
        "use-destination-message",
        params.use_destination_message,
    );
    push_flag(&mut args, "keep-emptied", params.keep_emptied);
    push_paths(&mut args, &params.paths)?;
    Ok(args)
}

pub fn split_args(params: &SplitParams) -> Result<Vec<String>, ToolError> {
    require_paths(&params.paths)?;
    let mut args = vec!["split".to_owned()];
    push_opt(&mut args, "revision", "revision", &params.revision)?;
    args.push(format!("--message={}", params.message));
    push_flag(&mut args, "parallel", params.parallel);
    push_paths(&mut args, &params.paths)?;
    Ok(args)
}

pub fn edit_args(params: &EditParams) -> Result<Vec<String>, ToolError> {
    Ok(vec![
        "edit".to_owned(),
        "--".to_owned(),
        non_empty("revision", &params.revision)?.to_owned(),
    ])
}

pub fn rebase_args(params: &RebaseParams) -> Result<Vec<String>, ToolError> {
    let destinations = [&params.onto, &params.insert_after, &params.insert_before]
        .iter()
        .filter(|value| value.is_some())
        .count();
    if destinations != 1 {
        return Err(ToolError::InvalidParams(
            "exactly one of onto, insert_after, insert_before is required".to_owned(),
        ));
    }
    let selectors = [&params.revisions, &params.source, &params.branch]
        .iter()
        .filter(|value| value.is_some())
        .count();
    if selectors > 1 {
        return Err(ToolError::InvalidParams(
            "at most one of revisions, source, branch is allowed (source and branch conflict)"
                .to_owned(),
        ));
    }
    let mut args = vec!["rebase".to_owned()];
    push_opt(&mut args, "revision", "revisions", &params.revisions)?;
    push_opt(&mut args, "source", "source", &params.source)?;
    push_opt(&mut args, "branch", "branch", &params.branch)?;
    push_opt(&mut args, "onto", "onto", &params.onto)?;
    push_opt(
        &mut args,
        "insert-after",
        "insert_after",
        &params.insert_after,
    )?;
    push_opt(
        &mut args,
        "insert-before",
        "insert_before",
        &params.insert_before,
    )?;
    push_flag(&mut args, "skip-emptied", params.skip_emptied);
    Ok(args)
}

pub fn restore_args(params: &RestoreParams) -> Result<Vec<String>, ToolError> {
    if params.changes_in.is_some() && (params.from.is_some() || params.into.is_some()) {
        return Err(ToolError::InvalidParams(
            "changes_in cannot be combined with from or into".to_owned(),
        ));
    }
    let mut args = vec!["restore".to_owned()];
    push_opt(&mut args, "from", "from", &params.from)?;
    push_opt(&mut args, "into", "into", &params.into)?;
    push_opt(&mut args, "changes-in", "changes_in", &params.changes_in)?;
    push_flag(&mut args, "restore-descendants", params.restore_descendants);
    push_paths(&mut args, &params.paths)?;
    Ok(args)
}

pub fn abandon_args(params: &AbandonParams) -> Result<Vec<String>, ToolError> {
    let mut args = vec!["abandon".to_owned()];
    push_flag(&mut args, "retain-bookmarks", params.retain_bookmarks);
    push_flag(&mut args, "restore-descendants", params.restore_descendants);
    args.push("--".to_owned());
    args.push(non_empty("revisions", &params.revisions)?.to_owned());
    Ok(args)
}

pub fn undo_args(params: &UndoParams) -> Vec<String> {
    let _ = params;
    vec!["undo".to_owned()]
}

pub fn file_untrack_args(params: &FileUntrackParams) -> Result<Vec<String>, ToolError> {
    require_paths(&params.paths)?;
    let mut args = vec!["file".to_owned(), "untrack".to_owned()];
    push_paths(&mut args, &params.paths)?;
    Ok(args)
}

pub fn bookmark_set_args(params: &BookmarkSetParams) -> Result<Vec<String>, ToolError> {
    let revision = match &params.revision {
        Some(revision) => non_empty("revision", revision)?,
        None => "@",
    };
    let mut args = vec![
        "bookmark".to_owned(),
        "set".to_owned(),
        format!("--revision={revision}"),
    ];
    push_flag(&mut args, "allow-backwards", params.allow_backwards);
    args.push("--".to_owned());
    args.push(non_empty("name", &params.name)?.to_owned());
    Ok(args)
}

#[tool_router(router = write_router, vis = "pub(crate)")]
impl JjServer {
    /// Set the description of a revision (default `@`).
    #[tool(annotations(
        read_only_hint = false,
        destructive_hint = false,
        open_world_hint = false
    ))]
    pub async fn describe(
        &self,
        Parameters(params): Parameters<DescribeParams>,
    ) -> Result<CallToolResult, ToolError> {
        let repo = RepoPath::new(&params.repo)?;
        let args = describe_args(&params)?;
        let _guard = self.write_queue.lock(&repo).await;
        let output = self.runner.run(&repo, &args).await?;
        Ok(text_result(output))
    }

    /// Create a new empty commit on top of `parents` (default `@`) and make
    /// it the working copy.
    #[tool(
        name = "new",
        annotations(
            read_only_hint = false,
            destructive_hint = false,
            open_world_hint = false
        )
    )]
    pub async fn new_change(
        &self,
        Parameters(params): Parameters<NewParams>,
    ) -> Result<CallToolResult, ToolError> {
        let repo = RepoPath::new(&params.repo)?;
        let args = new_args(&params)?;
        let _guard = self.write_queue.lock(&repo).await;
        let output = self.runner.run(&repo, &args).await?;
        Ok(text_result(output))
    }

    /// Describe the working-copy commit (or only `paths` of it) and start a
    /// new one on top.
    #[tool(annotations(
        read_only_hint = false,
        destructive_hint = false,
        open_world_hint = false
    ))]
    pub async fn commit(
        &self,
        Parameters(params): Parameters<CommitParams>,
    ) -> Result<CallToolResult, ToolError> {
        let repo = RepoPath::new(&params.repo)?;
        let args = commit_args(&params)?;
        let _guard = self.write_queue.lock(&repo).await;
        let output = self.runner.run(&repo, &args).await?;
        Ok(text_result(output))
    }

    /// Move changes from one revision into another (default: `@` into its
    /// parent). When both have descriptions, pass `message` or
    /// `use_destination_message`.
    #[tool(annotations(
        read_only_hint = false,
        destructive_hint = false,
        open_world_hint = false
    ))]
    pub async fn squash(
        &self,
        Parameters(params): Parameters<SquashParams>,
    ) -> Result<CallToolResult, ToolError> {
        let repo = RepoPath::new(&params.repo)?;
        let args = squash_args(&params)?;
        let _guard = self.write_queue.lock(&repo).await;
        let output = self.runner.run(&repo, &args).await?;
        Ok(text_result(output))
    }

    /// Split a revision in two by paths: `paths` go into the first commit.
    #[tool(annotations(
        read_only_hint = false,
        destructive_hint = false,
        open_world_hint = false
    ))]
    pub async fn split(
        &self,
        Parameters(params): Parameters<SplitParams>,
    ) -> Result<CallToolResult, ToolError> {
        let repo = RepoPath::new(&params.repo)?;
        let args = split_args(&params)?;
        let _guard = self.write_queue.lock(&repo).await;
        let output = self.runner.run(&repo, &args).await?;
        Ok(text_result(output))
    }

    /// Make a revision the working-copy commit, to amend it in place.
    #[tool(annotations(
        read_only_hint = false,
        destructive_hint = false,
        open_world_hint = false
    ))]
    pub async fn edit(
        &self,
        Parameters(params): Parameters<EditParams>,
    ) -> Result<CallToolResult, ToolError> {
        let repo = RepoPath::new(&params.repo)?;
        let args = edit_args(&params)?;
        let _guard = self.write_queue.lock(&repo).await;
        let output = self.runner.run(&repo, &args).await?;
        Ok(text_result(output))
    }

    /// Move revisions to new parents.
    #[tool(annotations(
        read_only_hint = false,
        destructive_hint = false,
        open_world_hint = false
    ))]
    pub async fn rebase(
        &self,
        Parameters(params): Parameters<RebaseParams>,
    ) -> Result<CallToolResult, ToolError> {
        let repo = RepoPath::new(&params.repo)?;
        let args = rebase_args(&params)?;
        let _guard = self.write_queue.lock(&repo).await;
        let output = self.runner.run(&repo, &args).await?;
        Ok(text_result(output))
    }

    /// Discard changes: restore paths of a revision (default `@`) from
    /// another (default its parent).
    #[tool(annotations(
        read_only_hint = false,
        destructive_hint = true,
        open_world_hint = false
    ))]
    pub async fn restore(
        &self,
        Parameters(params): Parameters<RestoreParams>,
    ) -> Result<CallToolResult, ToolError> {
        let repo = RepoPath::new(&params.repo)?;
        let args = restore_args(&params)?;
        let _guard = self.write_queue.lock(&repo).await;
        let output = self.runner.run(&repo, &args).await?;
        Ok(text_result(output))
    }

    /// Abandon commits; descendants are rebased onto their parents.
    #[tool(annotations(
        read_only_hint = false,
        destructive_hint = true,
        open_world_hint = false
    ))]
    pub async fn abandon(
        &self,
        Parameters(params): Parameters<AbandonParams>,
    ) -> Result<CallToolResult, ToolError> {
        let repo = RepoPath::new(&params.repo)?;
        let args = abandon_args(&params)?;
        let _guard = self.write_queue.lock(&repo).await;
        let output = self.runner.run(&repo, &args).await?;
        Ok(text_result(output))
    }

    /// Undo the last operation. Repeat to go further back; `op_log` shows
    /// the history.
    #[tool(annotations(
        read_only_hint = false,
        destructive_hint = false,
        open_world_hint = false
    ))]
    pub async fn undo(
        &self,
        Parameters(params): Parameters<UndoParams>,
    ) -> Result<CallToolResult, ToolError> {
        let repo = RepoPath::new(&params.repo)?;
        let args = undo_args(&params);
        let _guard = self.write_queue.lock(&repo).await;
        let output = self.runner.run(&repo, &args).await?;
        Ok(text_result(output))
    }

    /// Stop tracking files that are now ignored.
    #[tool(annotations(
        read_only_hint = false,
        destructive_hint = false,
        open_world_hint = false
    ))]
    pub async fn file_untrack(
        &self,
        Parameters(params): Parameters<FileUntrackParams>,
    ) -> Result<CallToolResult, ToolError> {
        let repo = RepoPath::new(&params.repo)?;
        let args = file_untrack_args(&params)?;
        let _guard = self.write_queue.lock(&repo).await;
        let output = self.runner.run(&repo, &args).await?;
        Ok(text_result(output))
    }

    /// Create a bookmark or move it to a revision (default `@`).
    #[tool(annotations(
        read_only_hint = false,
        destructive_hint = false,
        open_world_hint = false
    ))]
    pub async fn bookmark_set(
        &self,
        Parameters(params): Parameters<BookmarkSetParams>,
    ) -> Result<CallToolResult, ToolError> {
        let repo = RepoPath::new(&params.repo)?;
        let args = bookmark_set_args(&params)?;
        let _guard = self.write_queue.lock(&repo).await;
        let output = self.runner.run(&repo, &args).await?;
        Ok(text_result(output))
    }
}
