//! Read-only tools.

use rmcp::handler::server::wrapper::{Json, Parameters};
use rmcp::{tool, tool_router};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::repo::RepoPath;
use crate::server::JjServer;
use crate::templates::{COMMIT_TEMPLATE, Commit, parse_commits};
use crate::tools::ToolError;

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
}
