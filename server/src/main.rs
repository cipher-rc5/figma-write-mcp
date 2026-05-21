// figma-write-mcp
//
// A local MCP server that exposes four write tools for Figma. The server
// speaks MCP over stdio (so Claude can spawn it as a child process) and
// proxies write operations to a Figma plugin connected over a local
// WebSocket on 127.0.0.1:7341.
//
// The plugin is the only thing that can actually mutate the Figma document.
// This server is a thin bridge that gives Claude a typed, request-response
// view of the plugin's capabilities.

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::{Result, anyhow};
use futures_util::{SinkExt, StreamExt};
use rmcp::{
    ErrorData as McpError, RoleServer, ServerHandler, ServiceExt,
    model::{
        CallToolRequestParams, CallToolResult, Content, ListToolsResult, PaginatedRequestParams,
        ServerCapabilities, ServerInfo, Tool,
    },
    service::RequestContext,
    transport::stdio,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::net::TcpListener;
use tokio::sync::{Mutex, oneshot};
use tokio_tungstenite::tungstenite::Message;
use uuid::Uuid;

const BRIDGE_ADDR: &str = "127.0.0.1:7341";

// -----------------------------------------------------------------------------
// Bridge: a single in-process actor that owns the live WebSocket connection
// to the plugin and dispatches request/response correlation by id.
// -----------------------------------------------------------------------------

type PendingMap = Arc<Mutex<HashMap<String, oneshot::Sender<PluginResponse>>>>;

#[derive(Clone)]
struct Bridge {
    outbound: Arc<Mutex<Option<tokio::sync::mpsc::UnboundedSender<String>>>>,
    pending: PendingMap,
}

impl Bridge {
    fn new() -> Self {
        Self {
            outbound: Arc::new(Mutex::new(None)),
            pending: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    async fn is_connected(&self) -> bool {
        self.outbound.lock().await.is_some()
    }

    async fn call(&self, op: &str, params: Value) -> Result<Value> {
        let id = Uuid::new_v4().to_string();
        let (tx, rx) = oneshot::channel();

        {
            let mut pending = self.pending.lock().await;
            pending.insert(id.clone(), tx);
        }

        let frame = json!({ "id": id, "op": op, "params": params }).to_string();

        let sender = {
            let guard = self.outbound.lock().await;
            guard
                .as_ref()
                .ok_or_else(|| anyhow!("Figma plugin is not connected on {}", BRIDGE_ADDR))?
                .clone()
        };
        sender
            .send(frame)
            .map_err(|e| anyhow!("send failed: {e}"))?;

        // 30s timeout so Claude never hangs forever on a stuck plugin.
        let response = tokio::time::timeout(std::time::Duration::from_secs(30), rx)
            .await
            .map_err(|_| anyhow!("plugin response timed out after 30s"))?
            .map_err(|_| anyhow!("plugin response channel dropped"))?;

        if response.ok {
            Ok(response.result.unwrap_or(Value::Null))
        } else {
            let err = response.error.unwrap_or(PluginError {
                code: "internal".into(),
                message: "unknown".into(),
            });
            Err(anyhow!("[{}] {}", err.code, err.message))
        }
    }

    async fn accept_loop(self) {
        let listener = match TcpListener::bind(BRIDGE_ADDR).await {
            Ok(l) => l,
            Err(e) => {
                tracing::error!("could not bind {BRIDGE_ADDR}: {e}");
                return;
            }
        };
        tracing::info!("waiting for Figma plugin on ws://{BRIDGE_ADDR}");

        loop {
            let (stream, peer) = match listener.accept().await {
                Ok(x) => x,
                Err(e) => {
                    tracing::warn!("accept error: {e}");
                    continue;
                }
            };
            tracing::info!("plugin connected from {peer}");
            let ws = match tokio_tungstenite::accept_async(stream).await {
                Ok(w) => w,
                Err(e) => {
                    tracing::warn!("ws handshake failed: {e}");
                    continue;
                }
            };

            let (mut write, mut read) = ws.split();
            let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<String>();

            {
                let mut guard = self.outbound.lock().await;
                *guard = Some(tx);
            }

            let writer_task = tokio::spawn(async move {
                while let Some(msg) = rx.recv().await {
                    if write.send(Message::Text(msg.into())).await.is_err() {
                        break;
                    }
                }
            });

            let pending = self.pending.clone();
            let reader_task = tokio::spawn(async move {
                while let Some(Ok(msg)) = read.next().await {
                    if let Message::Text(text) = msg
                        && let Ok(resp) = serde_json::from_str::<PluginResponse>(&text)
                        && let Some(tx) = pending.lock().await.remove(&resp.id)
                    {
                        let _ = tx.send(resp);
                    }
                }
            });

            let _ = tokio::join!(writer_task, reader_task);
            {
                let mut guard = self.outbound.lock().await;
                *guard = None;
            }
            tracing::info!("plugin disconnected, waiting for reconnect");
        }
    }
}

#[derive(Debug, Deserialize, Serialize)]
struct PluginResponse {
    id: String,
    ok: bool,
    #[serde(default)]
    result: Option<Value>,
    #[serde(default)]
    error: Option<PluginError>,
}

#[derive(Debug, Deserialize, Serialize)]
struct PluginError {
    code: String,
    message: String,
}

// -----------------------------------------------------------------------------
// MCP server: exposes the four tools and forwards them through the bridge.
// -----------------------------------------------------------------------------

#[derive(Clone)]
struct FigmaServer {
    bridge: Bridge,
}

impl FigmaServer {
    fn tools() -> Vec<Tool> {
        vec![
            Tool::new(
                "set_text",
                "Set the text content of an existing Figma TEXT node by id.",
                Self::schema(json!({
                    "type": "object",
                    "required": ["node_id", "text"],
                    "properties": {
                        "node_id": {"type": "string"},
                        "text": {"type": "string"}
                    }
                })),
            ),
            Tool::new(
                "delete_node",
                "Delete a Figma node by id.",
                Self::schema(json!({
                    "type": "object",
                    "required": ["node_id"],
                    "properties": {"node_id": {"type": "string"}}
                })),
            ),
            Tool::new(
                "create_text_node",
                "Create a new TEXT node inside a parent frame.",
                Self::schema(json!({
                    "type": "object",
                    "required": ["parent_id", "text"],
                    "properties": {
                        "parent_id": {"type": "string"},
                        "text": {"type": "string"},
                        "x": {"type": "number"},
                        "y": {"type": "number"},
                        "width": {"type": "number"},
                        "font_family": {"type": "string"},
                        "font_style": {"type": "string"},
                        "font_size": {"type": "number"},
                        "fill_hex": {"type": "string"},
                        "line_height_pct": {"type": "number"}
                    }
                })),
            ),
            Tool::new(
                "update_node_properties",
                "Update mutable properties (position, size, opacity, name, etc.) on a node.",
                Self::schema(json!({
                    "type": "object",
                    "required": ["node_id", "set"],
                    "properties": {
                        "node_id": {"type": "string"},
                        "set": {
                            "type": "object",
                            "properties": {
                                "x": {"type": "number"},
                                "y": {"type": "number"},
                                "width": {"type": "number"},
                                "height": {"type": "number"},
                                "rotation": {"type": "number"},
                                "opacity": {"type": "number"},
                                "visible": {"type": "boolean"},
                                "name": {"type": "string"}
                            }
                        }
                    }
                })),
            ),
        ]
    }

    fn schema(schema: Value) -> Arc<rmcp::model::JsonObject> {
        Arc::new(serde_json::from_value(schema).expect("tool input schema must be a JSON object"))
    }
}

impl ServerHandler for FigmaServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_instructions("Bridge write operations to a connected local Figma plugin.")
    }

    fn get_tool(&self, name: &str) -> Option<Tool> {
        Self::tools().into_iter().find(|tool| tool.name == name)
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _ctx: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, McpError> {
        Ok(ListToolsResult {
            tools: Self::tools(),
            ..Default::default()
        })
    }

    async fn call_tool(
        &self,
        req: CallToolRequestParams,
        _ctx: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, McpError> {
        if !self.bridge.is_connected().await {
            return Ok(CallToolResult::error(vec![Content::text(
                "Figma plugin is not connected. Open the plugin in the Figma desktop app.",
            )]));
        }

        let params = Value::Object(req.arguments.unwrap_or_default());
        let result = self.bridge.call(&req.name, params).await;

        match result {
            Ok(value) => Ok(CallToolResult::success(vec![Content::text(
                value.to_string(),
            )])),
            Err(error) => Ok(CallToolResult::error(vec![Content::text(format!(
                "error: {error}"
            ))])),
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "figma_write_mcp=info".into()),
        )
        .with_writer(std::io::stderr) // never write logs on stdout, that's the MCP channel
        .init();

    let bridge = Bridge::new();
    tokio::spawn(bridge.clone().accept_loop());

    let server = FigmaServer { bridge };
    let transport = stdio();
    let service = server.serve(transport).await?;
    service.waiting().await?;
    Ok(())
}
