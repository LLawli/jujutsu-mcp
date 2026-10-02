//! `run`: any jj subcommand the dedicated tools do not cover.

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::service::RequestContext;
use rmcp::{RoleServer, tool, tool_router};
use schemars::JsonSchema;
use serde::Deserialize;

use crate::repo::RepoPath;
use crate::server::JjServer;
use crate::tools::{ToolError, text_result};

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct RunParams {
    /// Absolute path of the repository or any directory inside it.
    pub repo: String,
    /// jj arguments after `jj`, one element per argument, e.g.
    /// `["bookmark", "delete", "old"]`. No shell: quotes and `$` are literal.
    pub args: Vec<String>,
}

/// argv (after the global flags) for `run`: `args` as given, after checking
/// there is a subcommand.
pub fn run_args(params: &RunParams) -> Result<Vec<String>, ToolError> {
    match params.args.first() {
        Some(first) if !first.trim().is_empty() => Ok(params.args.clone()),
        _ => Err(ToolError::InvalidParams(
            "args must start with a jj subcommand or flag".to_owned(),
        )),
    }
}

#[tool_router(router = free_router, vis = "pub(crate)")]
impl JjServer {
    /// Run any jj command that no other tool covers (e.g. `bookmark delete`,
    /// `bookmark track`, `duplicate`, `file track`, `workspace add`). Prefer
    /// the dedicated tools: they validate parameters and return structured
    /// output. Interactive commands fail, since no editor or terminal is
    /// available. Runs in the repository's write queue.
    #[tool(annotations(
        read_only_hint = false,
        destructive_hint = true,
        idempotent_hint = false,
        open_world_hint = true
    ))]
    pub async fn run(
        &self,
        Parameters(params): Parameters<RunParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ToolError> {
        let repo = RepoPath::new(&params.repo)?;
        let args = run_args(&params)?;
        // Held for every command: it cannot be told apart from a write.
        let _guard = self.lock_write(&repo, &context).await?;
        let output = self.run_watched(&repo, &args, &context, None).await?;
        Ok(text_result(output))
    }
}
