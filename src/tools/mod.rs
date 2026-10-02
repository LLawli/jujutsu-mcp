//! MCP tools. Each tool separates a pure `params -> argv` function from
//! execution so argv construction is unit-testable.

pub mod read;

use rmcp::handler::server::tool::IntoCallToolResult;
use rmcp::model::{CallToolResponse, CallToolResult, ContentBlock};

use crate::jj::JjError;
use crate::repo::RepoPathError;

/// Why a tool call did not produce a result.
#[derive(Debug, thiserror::Error)]
pub enum ToolError {
    /// Parameters that are well-formed JSON but invalid for the tool. Raised
    /// before anything runs; reaches the client as a tool result with
    /// `isError: true`.
    #[error("{0}")]
    InvalidParams(String),
    #[error(transparent)]
    Repo(#[from] RepoPathError),
    /// jj ran and failed. Reaches the client as a tool result with
    /// `isError: true` carrying argv, exit code and stderr.
    #[error(transparent)]
    Jj(#[from] JjError),
    /// jj succeeded but its output did not match the template's shape.
    #[error("could not parse jj output: {0}")]
    Output(#[from] serde_json::Error),
}

impl IntoCallToolResult for ToolError {
    fn into_call_tool_result(self) -> Result<CallToolResponse, rmcp::ErrorData> {
        Ok(CallToolResult::error(vec![ContentBlock::text(self.to_string())]).into())
    }
}
