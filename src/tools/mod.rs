//! MCP tools. Each tool separates a pure `params -> argv` function from
//! execution so argv construction is unit-testable.

pub mod read;
pub mod remote;
pub mod write;

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
    /// The client cancelled the call; the jj process was killed.
    #[error("cancelled by the client")]
    Cancelled,
    /// jj succeeded but its output did not match the template's shape.
    #[error("could not parse jj output: {0}")]
    Output(#[from] serde_json::Error),
}

impl IntoCallToolResult for ToolError {
    fn into_call_tool_result(self) -> Result<CallToolResponse, rmcp::ErrorData> {
        Ok(CallToolResult::error(vec![ContentBlock::text(self.to_string())]).into())
    }
}

/// Result of a text tool: jj's stdout, then its stderr as a second block
/// when jj wrote anything there (what it did, warnings).
pub fn text_result(output: crate::jj::JjOutput) -> rmcp::model::CallToolResult {
    let mut blocks = Vec::new();
    // An empty stdout block adds nothing; skip it when stderr has the news.
    if !output.stdout.is_empty() || output.stderr.is_empty() {
        blocks.push(ContentBlock::text(output.stdout));
    }
    if !output.stderr.is_empty() {
        blocks.push(ContentBlock::text(output.stderr));
    }
    CallToolResult::success(blocks)
}

/// A fileset matching `path` literally, relative to the working directory
/// jj runs in: `cwd:"..."` with quotes and backslashes escaped, so spaces,
/// parentheses or `|` in a file name are not read as fileset syntax.
pub fn literal_path_fileset(path: &str) -> Result<String, ToolError> {
    if path.is_empty() {
        return Err(ToolError::InvalidParams(
            "paths must not contain an empty path".to_owned(),
        ));
    }
    let escaped = path.replace('\\', "\\\\").replace('"', "\\\"");
    Ok(format!("cwd:\"{escaped}\""))
}

/// Rejects empty or whitespace-only values, naming the field.
pub(crate) fn non_empty<'a>(field: &str, value: &'a str) -> Result<&'a str, ToolError> {
    if value.trim().is_empty() {
        return Err(ToolError::InvalidParams(format!(
            "{field} must not be empty"
        )));
    }
    Ok(value)
}
