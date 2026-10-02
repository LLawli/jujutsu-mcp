//! The MCP server: tool routing and server info.

use std::time::Duration;

use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::model::{Implementation, ServerCapabilities, ServerConfig};
use rmcp::{ServerHandler, tool_handler};

use crate::jj::{JjRunner, WriteQueue};

/// How often a long call reports progress unless configured otherwise.
const DEFAULT_PROGRESS_INTERVAL: Duration = Duration::from_secs(2);

#[derive(Debug, Clone)]
pub struct JjServer {
    pub(crate) runner: JjRunner,
    pub(crate) write_queue: WriteQueue,
    tool_router: ToolRouter<Self>,
    pub(crate) progress_interval: Duration,
}

impl JjServer {
    pub fn new(runner: JjRunner) -> Self {
        Self {
            runner,
            write_queue: WriteQueue::new(),
            tool_router: Self::tool_router(),
            progress_interval: DEFAULT_PROGRESS_INTERVAL,
        }
    }

    /// Interval between progress notifications while a long jj call
    /// (`git_fetch`, `git_push`) runs, when the client asked for progress.
    pub fn with_progress_interval(mut self, interval: Duration) -> Self {
        self.progress_interval = interval;
        self
    }

    /// Every tool group, added here with `+`.
    fn tool_router() -> ToolRouter<Self> {
        Self::read_router() + Self::write_router() + Self::remote_router()
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for JjServer {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build()).with_server_info(
            Implementation::new("jujutsu-mcp", env!("CARGO_PKG_VERSION")),
        )
    }
}
