//! The MCP server: tool routing and server info.

use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::tool::ToolCallContext;
use rmcp::model::{
    CallToolRequestParams, CallToolResponse, Implementation, ServerCapabilities, ServerConfig,
};
use rmcp::service::RequestContext;
use rmcp::{ErrorData, RoleServer, ServerHandler, tool_handler};

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
    /// Replaces the generated `call_tool`: rmcp's `ToolRouter::call` turns a
    /// parameter deserialization failure into a tool result with
    /// `isError: true`, but the contract in docs/decisions.md is that invalid
    /// parameters are `-32602` protocol errors. Dispatching to the route
    /// directly keeps the error as raised. No tool is ever disabled, so the
    /// router's disabled-set check has nothing to skip.
    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        let route = self
            .tool_router
            .map
            .get(request.name.as_ref())
            .ok_or_else(|| ErrorData::invalid_params("tool not found", None))?;
        let context = ToolCallContext::new(self, request, context);
        (route.call)(context).await
    }

    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build()).with_server_info(
            Implementation::new("jujutsu-mcp", env!("CARGO_PKG_VERSION")),
        )
    }
}
