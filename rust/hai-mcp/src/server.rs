use jacs_mcp::JacsMcpServer;
use rmcp::model::{
    CallToolRequestParam, Implementation, ListToolsResult, PaginatedRequestParam,
    ServerCapabilities, ServerInfo, Tool, ToolsCapability,
};
use rmcp::service::RequestContext;
use rmcp::{ErrorData as McpError, RoleServer, ServerHandler};

use crate::context::HaiServerContext;
use crate::hai_tools;

pub struct HaiMcpServer {
    jacs: JacsMcpServer,
    context: HaiServerContext,
}

impl HaiMcpServer {
    pub fn new(jacs: JacsMcpServer, context: HaiServerContext) -> Self {
        Self { jacs, context }
    }

    fn combined_tools(&self) -> Vec<Tool> {
        let mut tools = JacsMcpServer::tools();
        tools.extend(hai_tools::definitions());
        tools
    }
}

impl ServerHandler for HaiMcpServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            protocol_version: Default::default(),
            capabilities: ServerCapabilities {
                tools: Some(ToolsCapability {
                    list_changed: Some(false),
                }),
                ..Default::default()
            },
            server_info: Implementation {
                name: "hai-mcp".to_string(),
                title: Some("HAIAI MCP Server".to_string()),
                version: env!("CARGO_PKG_VERSION").to_string(),
                icons: None,
                website_url: Some("https://hai.ai".to_string()),
            },
            instructions: Some(
                "This MCP server runs locally over stdio only. It embeds the canonical JACS MCP \
                 server in-process and adds HAI platform tools for registration, authenticated \
                 agent operations, and mailbox/email workflows."
                    .to_string(),
            ),
        }
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParam>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, McpError> {
        Ok(ListToolsResult {
            tools: self.combined_tools(),
            ..Default::default()
        })
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParam,
        context: RequestContext<RoleServer>,
    ) -> Result<rmcp::model::CallToolResult, McpError> {
        if hai_tools::has_tool(request.name.as_ref()) {
            let name = request.name.to_string();
            return hai_tools::dispatch(&self.context, &name, request.arguments).await;
        }

        self.jacs.call_tool(request, context).await
    }
}
