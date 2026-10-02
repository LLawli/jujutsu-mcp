//! The MCP server: tool routing and server info.

use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::model::{Implementation, ServerCapabilities, ServerConfig};
use rmcp::{ServerHandler, tool_handler};

use crate::jj::JjRunner;

#[derive(Debug, Clone)]
pub struct JjServer {
    pub(crate) runner: JjRunner,
    tool_router: ToolRouter<Self>,
}

impl JjServer {
    pub fn new(runner: JjRunner) -> Self {
        Self {
            runner,
            tool_router: Self::tool_router(),
        }
    }

    /// Every tool group. Each new group (`write_router`, `remote_router`)
    /// is added here with `+`.
    fn tool_router() -> ToolRouter<Self> {
        Self::read_router()
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
