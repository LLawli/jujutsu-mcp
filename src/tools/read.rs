//! Read-only tools.

use rmcp::handler::server::wrapper::{Json, Parameters};
use rmcp::{tool, tool_router};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use rmcp::model::CallToolResult;

use crate::repo::RepoPath;
use crate::server::JjServer;
use crate::templates::{
    BOOKMARK_TEMPLATE, Bookmark, COMMIT_TEMPLATE, Commit, OPERATION_TEMPLATE, Operation,
    parse_commits, parse_json_lines,
};
use crate::tools::{ToolError, non_empty, text_result};

pub use crate::tools::literal_path_fileset;

/// Commits returned by `log` when `limit` is not given.
pub const DEFAULT_LOG_LIMIT: u32 = 50;

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct LogParams {
    /// Absolute path of the repository or any directory inside it.
    pub repo: String,
    /// Revset to show. Defaults to jj's default log revset.
    #[serde(default)]
    pub revisions: Option<String>,
    /// Maximum number of commits, at least 1. Defaults to 50.
    #[serde(default)]
    pub limit: Option<u32>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct LogOutput {
    pub commits: Vec<Commit>,
    /// More commits matched than `limit` allowed.
    pub truncated: bool,
}

/// argv (after the global flags) for `log`. Asks jj for one commit more
/// than the limit so the tool can tell whether the result was truncated.
/// Revsets are passed as `--revisions=<value>` so a value starting with `-`
/// cannot be read as a flag.
pub fn log_args(params: &LogParams) -> Result<Vec<String>, ToolError> {
    let limit = params.limit.unwrap_or(DEFAULT_LOG_LIMIT);
    if limit == 0 {
        return Err(ToolError::InvalidParams(
            "limit must be at least 1".to_owned(),
        ));
    }
    let mut args = vec![
        "log".to_owned(),
        "--no-graph".to_owned(),
        "--template".to_owned(),
        COMMIT_TEMPLATE.to_owned(),
        "--limit".to_owned(),
        // u64 so that u32::MAX + 1 cannot overflow.
        (u64::from(limit) + 1).to_string(),
    ];
    if let Some(revisions) = &params.revisions {
        if revisions.trim().is_empty() {
            return Err(ToolError::InvalidParams(
                "revisions must not be empty".to_owned(),
            ));
        }
        args.push(format!("--revisions={revisions}"));
    }
    Ok(args)
}

/// Operations returned by `op_log` when `limit` is not given.
pub const DEFAULT_OP_LOG_LIMIT: u32 = 20;

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct StatusParams {
    /// Absolute path of the repository or any directory inside it.
    pub repo: String,
}

/// How a diff is rendered. Omitted, jj uses its configured default.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DiffFormat {
    /// Unified diff in git's format.
    Git,
    /// Per-file histogram of changed lines.
    Stat,
    /// One line per file with its status (M, A, D, R, C).
    Summary,
    /// Changed paths only, one per line.
    NameOnly,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct ShowParams {
    /// Absolute path of the repository or any directory inside it.
    pub repo: String,
    /// Revision to show. Defaults to the working-copy commit `@`.
    #[serde(default)]
    pub revision: Option<String>,
    /// Diff rendering. Defaults to jj's configured format.
    #[serde(default)]
    pub format: Option<DiffFormat>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct DiffParams {
    /// Absolute path of the repository or any directory inside it.
    pub repo: String,
    /// Show the changes in these revisions. Cannot be combined with
    /// `from`/`to`. Defaults to the working-copy commit `@`.
    #[serde(default)]
    pub revisions: Option<String>,
    /// Compare from this revision.
    #[serde(default)]
    pub from: Option<String>,
    /// Compare to this revision.
    #[serde(default)]
    pub to: Option<String>,
    /// Limit the diff to these paths, relative to `repo`. Taken literally:
    /// no fileset syntax or globs.
    #[serde(default)]
    pub paths: Vec<String>,
    /// Diff rendering. Defaults to jj's configured format.
    #[serde(default)]
    pub format: Option<DiffFormat>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct BookmarkListParams {
    /// Absolute path of the repository or any directory inside it.
    pub repo: String,
    /// Also list remote bookmarks that no local bookmark tracks.
    #[serde(default)]
    pub all_remotes: bool,
    /// Only bookmarks matching these jj string patterns (glob by default).
    #[serde(default)]
    pub names: Vec<String>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct BookmarkListOutput {
    pub bookmarks: Vec<Bookmark>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct OpLogParams {
    /// Absolute path of the repository or any directory inside it.
    pub repo: String,
    /// Maximum number of operations, at least 1. Defaults to 20.
    #[serde(default)]
    pub limit: Option<u32>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct OpLogOutput {
    /// Newest first.
    pub operations: Vec<Operation>,
    /// More operations exist than `limit` allowed.
    pub truncated: bool,
}

pub fn status_args(params: &StatusParams) -> Vec<String> {
    let _ = params;
    vec!["status".to_owned()]
}

fn format_flag(format: DiffFormat) -> &'static str {
    match format {
        DiffFormat::Git => "--git",
        DiffFormat::Stat => "--stat",
        DiffFormat::Summary => "--summary",
        DiffFormat::NameOnly => "--name-only",
    }
}

/// The revision goes after `--`, where a value starting with `-` cannot be
/// read as a flag.
pub fn show_args(params: &ShowParams) -> Result<Vec<String>, ToolError> {
    let revision = match &params.revision {
        Some(revision) => non_empty("revision", revision)?,
        None => "@",
    };
    let mut args = vec!["show".to_owned()];
    if let Some(format) = params.format {
        args.push(format_flag(format).to_owned());
    }
    args.push("--".to_owned());
    args.push(revision.to_owned());
    Ok(args)
}

pub fn diff_args(params: &DiffParams) -> Result<Vec<String>, ToolError> {
    let mut args = vec!["diff".to_owned()];
    if let Some(format) = params.format {
        args.push(format_flag(format).to_owned());
    }
    if let Some(revisions) = &params.revisions {
        if params.from.is_some() || params.to.is_some() {
            return Err(ToolError::InvalidParams(
                "revisions cannot be combined with from or to".to_owned(),
            ));
        }
        args.push(format!(
            "--revisions={}",
            non_empty("revisions", revisions)?
        ));
    }
    if let Some(from) = &params.from {
        args.push(format!("--from={}", non_empty("from", from)?));
    }
    if let Some(to) = &params.to {
        args.push(format!("--to={}", non_empty("to", to)?));
    }
    if !params.paths.is_empty() {
        args.push("--".to_owned());
        for path in &params.paths {
            args.push(literal_path_fileset(path)?);
        }
    }
    Ok(args)
}

pub fn bookmark_list_args(params: &BookmarkListParams) -> Result<Vec<String>, ToolError> {
    let mut args = vec![
        "bookmark".to_owned(),
        "list".to_owned(),
        "--template".to_owned(),
        BOOKMARK_TEMPLATE.to_owned(),
    ];
    if params.all_remotes {
        args.push("--all-remotes".to_owned());
    }
    if !params.names.is_empty() {
        args.push("--".to_owned());
        args.extend(params.names.iter().cloned());
    }
    Ok(args)
}

/// Like `log`, asks for one operation more than the limit to detect
/// truncation.
pub fn op_log_args(params: &OpLogParams) -> Result<Vec<String>, ToolError> {
    let limit = params.limit.unwrap_or(DEFAULT_OP_LOG_LIMIT);
    if limit == 0 {
        return Err(ToolError::InvalidParams(
            "limit must be at least 1".to_owned(),
        ));
    }
    Ok(vec![
        "operation".to_owned(),
        "log".to_owned(),
        "--no-graph".to_owned(),
        "--template".to_owned(),
        OPERATION_TEMPLATE.to_owned(),
        "--limit".to_owned(),
        // u64 so that u32::MAX + 1 cannot overflow.
        (u64::from(limit) + 1).to_string(),
    ])
}

#[tool_router(router = read_router, vis = "pub(crate)")]
impl JjServer {
    /// Show commits as structured JSON: ids, description, author, bookmarks
    /// and flags (working_copy, empty, conflict, immutable, divergent).
    #[tool(annotations(read_only_hint = true, open_world_hint = false))]
    pub async fn log(
        &self,
        Parameters(params): Parameters<LogParams>,
    ) -> Result<Json<LogOutput>, ToolError> {
        let repo = RepoPath::new(&params.repo)?;
        let args = log_args(&params)?;
        let output = self.runner.run(&repo, &args).await?;
        let mut commits = parse_commits(&output.stdout)?;
        let limit =
            usize::try_from(params.limit.unwrap_or(DEFAULT_LOG_LIMIT)).unwrap_or(usize::MAX);
        let truncated = commits.len() > limit;
        commits.truncate(limit);
        Ok(Json(LogOutput { commits, truncated }))
    }
    /// Working-copy status: the working-copy commit and its parent, changed
    /// files, conflicts.
    #[tool(annotations(read_only_hint = true, open_world_hint = false))]
    pub async fn status(
        &self,
        Parameters(params): Parameters<StatusParams>,
    ) -> Result<CallToolResult, ToolError> {
        let repo = RepoPath::new(&params.repo)?;
        let output = self.runner.run(&repo, &status_args(&params)).await?;
        Ok(text_result(output))
    }

    /// Show one revision: its description and the changes it makes.
    #[tool(annotations(read_only_hint = true, open_world_hint = false))]
    pub async fn show(
        &self,
        Parameters(params): Parameters<ShowParams>,
    ) -> Result<CallToolResult, ToolError> {
        let repo = RepoPath::new(&params.repo)?;
        let args = show_args(&params)?;
        let output = self.runner.run(&repo, &args).await?;
        Ok(text_result(output))
    }

    /// Show changes in a revision (default `@`) or between two revisions,
    /// optionally limited to paths.
    #[tool(annotations(read_only_hint = true, open_world_hint = false))]
    pub async fn diff(
        &self,
        Parameters(params): Parameters<DiffParams>,
    ) -> Result<CallToolResult, ToolError> {
        let repo = RepoPath::new(&params.repo)?;
        let args = diff_args(&params)?;
        let output = self.runner.run(&repo, &args).await?;
        Ok(text_result(output))
    }

    /// List bookmarks with their targets, as structured JSON.
    #[tool(annotations(read_only_hint = true, open_world_hint = false))]
    pub async fn bookmark_list(
        &self,
        Parameters(params): Parameters<BookmarkListParams>,
    ) -> Result<Json<BookmarkListOutput>, ToolError> {
        let repo = RepoPath::new(&params.repo)?;
        let args = bookmark_list_args(&params)?;
        let output = self.runner.run(&repo, &args).await?;
        let bookmarks = parse_json_lines(&output.stdout)?;
        Ok(Json(BookmarkListOutput { bookmarks }))
    }

    /// Operation log, newest first, as structured JSON: what each jj command
    /// did to the repository. `undo` reverts the latest operation.
    #[tool(annotations(read_only_hint = true, open_world_hint = false))]
    pub async fn op_log(
        &self,
        Parameters(params): Parameters<OpLogParams>,
    ) -> Result<Json<OpLogOutput>, ToolError> {
        let repo = RepoPath::new(&params.repo)?;
        let args = op_log_args(&params)?;
        let output = self.runner.run(&repo, &args).await?;
        let mut operations: Vec<Operation> = parse_json_lines(&output.stdout)?;
        let limit =
            usize::try_from(params.limit.unwrap_or(DEFAULT_OP_LOG_LIMIT)).unwrap_or(usize::MAX);
        let truncated = operations.len() > limit;
        operations.truncate(limit);
        Ok(Json(OpLogOutput {
            operations,
            truncated,
        }))
    }
}
