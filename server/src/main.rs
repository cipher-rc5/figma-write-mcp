// figma-write-mcp
//
// A local MCP server that exposes four write tools for Figma. The server
// speaks MCP over stdio (so Claude can spawn it as a child process) and
// proxies write operations to a Figma plugin connected over a local
// WebSocket on 127.0.0.1:7341.
//
// The plugin is the only thing that can actually mutate the Figma document.
// This server is a thin bridge that gives Claude a typed, request-response
// view of the plugin's capabilities. Connections are gated by a
// shared-secret handshake; see SECURITY.md.

use std::collections::HashMap;
use std::env;
use std::fmt;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result, anyhow};
use base64::Engine;
use futures_util::{SinkExt, StreamExt};
use rand::Rng;
use rmcp::{
    ErrorData as McpError, RoleServer, ServerHandler, ServiceExt,
    model::{
        CallToolRequestParams, CallToolResult, Content, JsonObject, ListToolsResult,
        PaginatedRequestParams, ServerCapabilities, ServerInfo, Tool,
    },
    service::RequestContext,
    transport::stdio,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use subtle::ConstantTimeEq;
use tokio::net::TcpListener;
use tokio::sync::{Mutex, mpsc, oneshot};
use tokio_tungstenite::tungstenite::Message;
use uuid::Uuid;

const BRIDGE_ADDR: &str = "127.0.0.1:7341";
const PROTOCOL_VERSION: u32 = 1;
const CALL_TIMEOUT: Duration = Duration::from_secs(30);
const HELLO_TIMEOUT: Duration = Duration::from_secs(5);
const OUTBOUND_CAPACITY: usize = 64;
const SECRET_BYTES: usize = 32;

// -----------------------------------------------------------------------------
// Bridge: a single in-process actor that owns the live WebSocket connection
// to the plugin and dispatches request/response correlation by id.
// -----------------------------------------------------------------------------

type PendingMap = Arc<Mutex<HashMap<String, oneshot::Sender<PluginResponse>>>>;

#[derive(Clone)]
struct Bridge {
    outbound: Arc<Mutex<Option<mpsc::Sender<String>>>>,
    pending: PendingMap,
    secret: Arc<String>,
}

#[derive(Debug)]
enum BridgeError {
    NotConnected,
    Timeout,
    Send(String),
    Plugin(PluginError),
}

impl fmt::Display for BridgeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BridgeError::NotConnected => write!(
                f,
                "Figma plugin is not connected on {BRIDGE_ADDR}. Open the plugin in Figma desktop."
            ),
            BridgeError::Timeout => write!(f, "plugin response timed out after {CALL_TIMEOUT:?}"),
            BridgeError::Send(e) => write!(f, "could not send to plugin: {e}"),
            BridgeError::Plugin(p) => write!(f, "[{}] {}", p.code, p.message),
        }
    }
}

impl std::error::Error for BridgeError {}

impl BridgeError {
    fn as_json(&self) -> Value {
        match self {
            BridgeError::Plugin(p) => json!({ "code": p.code, "message": p.message }),
            BridgeError::NotConnected => json!({
                "code": "plugin_not_connected",
                "message": format!("Figma plugin is not connected on {BRIDGE_ADDR}"),
            }),
            BridgeError::Timeout => json!({
                "code": "timeout",
                "message": format!("plugin did not respond within {} seconds", CALL_TIMEOUT.as_secs()),
            }),
            BridgeError::Send(e) => json!({
                "code": "send_failed",
                "message": e,
            }),
        }
    }
}

impl Bridge {
    fn new(secret: Arc<String>) -> Self {
        Self {
            outbound: Arc::new(Mutex::new(None)),
            pending: Arc::new(Mutex::new(HashMap::new())),
            secret,
        }
    }

    async fn is_connected(&self) -> bool {
        self.outbound.lock().await.is_some()
    }

    async fn call(&self, op: &str, params: Value) -> Result<Value, BridgeError> {
        let id = Uuid::new_v4().to_string();
        let (tx, rx) = oneshot::channel();
        self.pending.lock().await.insert(id.clone(), tx);

        let frame = json!({
            "id": id,
            "op": op,
            "protocol_version": PROTOCOL_VERSION,
            "params": params,
        })
        .to_string();

        let sender = {
            let guard = self.outbound.lock().await;
            match guard.as_ref() {
                Some(s) => s.clone(),
                None => {
                    self.pending.lock().await.remove(&id);
                    return Err(BridgeError::NotConnected);
                }
            }
        };

        if let Err(e) = sender.send(frame).await {
            self.pending.lock().await.remove(&id);
            return Err(BridgeError::Send(e.to_string()));
        }

        match tokio::time::timeout(CALL_TIMEOUT, rx).await {
            Err(_) => {
                self.pending.lock().await.remove(&id);
                Err(BridgeError::Timeout)
            }
            Ok(Err(_recv_err)) => {
                // Sender was dropped without sending — treat as disconnect.
                Err(BridgeError::Plugin(PluginError {
                    code: "plugin_disconnected".into(),
                    message: "Figma plugin disconnected before responding".into(),
                }))
            }
            Ok(Ok(response)) => {
                if response.ok {
                    Ok(response.result.unwrap_or(Value::Null))
                } else {
                    Err(BridgeError::Plugin(response.error.unwrap_or_else(|| {
                        PluginError {
                            code: "internal".into(),
                            message: "plugin returned ok:false with no error body".into(),
                        }
                    })))
                }
            }
        }
    }

    async fn accept_loop(self, listener: TcpListener) {
        tracing::info!("waiting for Figma plugin on ws://{BRIDGE_ADDR}");

        loop {
            let (stream, peer) = match listener.accept().await {
                Ok(x) => x,
                Err(e) => {
                    tracing::warn!("accept error: {e}");
                    continue;
                }
            };
            tracing::info!("incoming connection from {peer}");

            let ws = match tokio_tungstenite::accept_async(stream).await {
                Ok(w) => w,
                Err(e) => {
                    tracing::warn!("ws handshake failed: {e}");
                    continue;
                }
            };

            let (mut write, mut read) = ws.split();

            // Hello handshake: first frame must be {op:"hello", protocol_version, secret}.
            let hello_text = match tokio::time::timeout(HELLO_TIMEOUT, read.next()).await {
                Ok(Some(Ok(Message::Text(t)))) => t.to_string(),
                Ok(Some(Ok(other))) => {
                    tracing::warn!("expected text hello, got {other:?}; closing");
                    continue;
                }
                Ok(Some(Err(e))) => {
                    tracing::warn!("ws read error before hello: {e}");
                    continue;
                }
                Ok(None) => {
                    tracing::warn!("ws closed before hello");
                    continue;
                }
                Err(_) => {
                    tracing::warn!("hello timed out after {HELLO_TIMEOUT:?}");
                    let _ = write
                        .send(Message::Text(
                            hello_err("timeout", "hello timed out").into(),
                        ))
                        .await;
                    continue;
                }
            };

            match validate_hello(&hello_text, &self.secret) {
                Ok(()) => {
                    let _ = write
                        .send(Message::Text(
                            json!({
                                "op": "hello_ok",
                                "protocol_version": PROTOCOL_VERSION,
                            })
                            .to_string()
                            .into(),
                        ))
                        .await;
                }
                Err(reason) => {
                    tracing::warn!("hello rejected: {reason}");
                    let _ = write
                        .send(Message::Text(
                            hello_err(reason.code(), &reason.message()).into(),
                        ))
                        .await;
                    continue;
                }
            }

            tracing::info!("plugin authenticated from {peer}");

            let (tx, mut rx) = mpsc::channel::<String>(OUTBOUND_CAPACITY);
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
                // Reader may still be alive; close the sink to unblock it.
                let _ = write.close().await;
            });

            let pending = self.pending.clone();
            let reader_task = tokio::spawn(async move {
                while let Some(item) = read.next().await {
                    let msg = match item {
                        Ok(m) => m,
                        Err(e) => {
                            tracing::warn!("ws read error: {e}");
                            break;
                        }
                    };
                    let text = match msg {
                        Message::Text(t) => t,
                        Message::Close(_) => break,
                        Message::Ping(_)
                        | Message::Pong(_)
                        | Message::Binary(_)
                        | Message::Frame(_) => continue,
                    };
                    let resp = match serde_json::from_str::<PluginResponse>(&text) {
                        Ok(r) => r,
                        Err(e) => {
                            tracing::warn!("bad plugin frame: {e}");
                            continue;
                        }
                    };
                    if let Some(tx) = pending.lock().await.remove(&resp.id) {
                        let _ = tx.send(resp);
                    } else {
                        tracing::warn!("unsolicited plugin response id={}", resp.id);
                    }
                }
            });

            let _ = tokio::join!(writer_task, reader_task);

            // Drop the outbound sender so any subsequent Bridge::call sees NotConnected.
            {
                let mut guard = self.outbound.lock().await;
                *guard = None;
            }
            // Wake every in-flight caller with a structured disconnect error
            // instead of letting them wait the full 30s timeout.
            let drained: Vec<oneshot::Sender<PluginResponse>> = {
                let mut pending = self.pending.lock().await;
                pending.drain().map(|(_, tx)| tx).collect()
            };
            for tx in drained {
                let _ = tx.send(PluginResponse {
                    id: String::new(),
                    ok: false,
                    result: None,
                    error: Some(PluginError {
                        code: "plugin_disconnected".into(),
                        message: "Figma plugin disconnected mid-request".into(),
                    }),
                });
            }
            tracing::info!("plugin disconnected, waiting for reconnect");
        }
    }
}

#[derive(Debug)]
enum HelloRejection {
    BadJson(String),
    WrongOp(String),
    UnsupportedVersion(u64),
    MissingSecret,
    AuthFailed,
}

impl HelloRejection {
    fn code(&self) -> &'static str {
        match self {
            HelloRejection::BadJson(_) => "invalid_params",
            HelloRejection::WrongOp(_) => "invalid_params",
            HelloRejection::UnsupportedVersion(_) => "unsupported_version",
            HelloRejection::MissingSecret => "invalid_params",
            HelloRejection::AuthFailed => "auth_failed",
        }
    }
    fn message(&self) -> String {
        match self {
            HelloRejection::BadJson(e) => format!("hello must be JSON: {e}"),
            HelloRejection::WrongOp(o) => {
                format!("first frame op must be \"hello\", got \"{o}\"")
            }
            HelloRejection::UnsupportedVersion(v) => {
                format!("unsupported protocol_version {v}; server speaks {PROTOCOL_VERSION}")
            }
            HelloRejection::MissingSecret => "hello.secret is required".into(),
            HelloRejection::AuthFailed => {
                "hello.secret does not match the server's on-disk secret".into()
            }
        }
    }
}

impl fmt::Display for HelloRejection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[{}] {}", self.code(), self.message())
    }
}

fn hello_err(code: &str, message: &str) -> String {
    json!({
        "op": "hello_err",
        "code": code,
        "message": message,
    })
    .to_string()
}

fn validate_hello(text: &str, secret: &str) -> Result<(), HelloRejection> {
    let v: Value =
        serde_json::from_str(text).map_err(|e| HelloRejection::BadJson(e.to_string()))?;
    let op = v.get("op").and_then(Value::as_str).unwrap_or("");
    if op != "hello" {
        return Err(HelloRejection::WrongOp(op.to_string()));
    }
    let claimed_version = v
        .get("protocol_version")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    if claimed_version != PROTOCOL_VERSION as u64 {
        return Err(HelloRejection::UnsupportedVersion(claimed_version));
    }
    let claimed = match v.get("secret").and_then(Value::as_str) {
        Some(s) if !s.is_empty() => s,
        _ => return Err(HelloRejection::MissingSecret),
    };
    if constant_time_eq(claimed.as_bytes(), secret.as_bytes()) {
        Ok(())
    } else {
        Err(HelloRejection::AuthFailed)
    }
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.ct_eq(b).unwrap_u8() == 1
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
// Secret storage: read or generate the shared bridge secret on first launch.
// -----------------------------------------------------------------------------

fn secret_dir() -> PathBuf {
    if let Ok(p) = env::var("FIGMA_WRITE_MCP_HOME") {
        return PathBuf::from(p);
    }
    #[cfg(target_os = "macos")]
    {
        if let Ok(home) = env::var("HOME") {
            return PathBuf::from(home).join("Library/Application Support/figma-write-mcp");
        }
    }
    if let Ok(xdg) = env::var("XDG_CONFIG_HOME") {
        return PathBuf::from(xdg).join("figma-write-mcp");
    }
    if let Ok(home) = env::var("HOME") {
        return PathBuf::from(home).join(".config/figma-write-mcp");
    }
    PathBuf::from(".figma-write-mcp")
}

fn load_or_create_secret(dir: &std::path::Path) -> Result<(String, bool)> {
    std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    let path = dir.join("secret");
    if path.exists() {
        let s = std::fs::read_to_string(&path)
            .with_context(|| format!("reading {}", path.display()))?;
        let trimmed = s.trim().to_string();
        if trimmed.is_empty() {
            return Err(anyhow!("{} exists but is empty", path.display()));
        }
        return Ok((trimmed, false));
    }

    let mut buf = [0u8; SECRET_BYTES];
    rand::rng().fill_bytes(&mut buf);
    let secret = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(buf);

    std::fs::write(&path, &secret).with_context(|| format!("writing {}", path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))
            .with_context(|| format!("chmod 600 {}", path.display()))?;
    }
    Ok((secret, true))
}

// -----------------------------------------------------------------------------
// MCP server: exposes the four tools and forwards them through the bridge.
// -----------------------------------------------------------------------------

#[derive(Clone)]
struct FigmaServer {
    bridge: Bridge,
    tools: Arc<Vec<Tool>>,
}

impl FigmaServer {
    fn tools_inner() -> Result<Vec<Tool>> {
        let specs: [(&str, &str, Value); 4] = [
            (
                "set_text",
                "Set the text content of an existing Figma TEXT node by id.",
                json!({
                    "type": "object",
                    "additionalProperties": false,
                    "required": ["node_id", "text"],
                    "properties": {
                        "node_id": {"type": "string"},
                        "text": {"type": "string"}
                    }
                }),
            ),
            (
                "delete_node",
                "Delete a Figma node by id.",
                json!({
                    "type": "object",
                    "additionalProperties": false,
                    "required": ["node_id"],
                    "properties": {"node_id": {"type": "string"}}
                }),
            ),
            (
                "create_text_node",
                "Create a new TEXT node inside a parent frame.",
                json!({
                    "type": "object",
                    "additionalProperties": false,
                    "required": ["parent_id", "text"],
                    "properties": {
                        "parent_id": {"type": "string"},
                        "text": {"type": "string"},
                        "x": {"type": "number"},
                        "y": {"type": "number"},
                        "width": {"type": "number", "exclusiveMinimum": 0},
                        "font_family": {"type": "string"},
                        "font_style": {"type": "string"},
                        "font_size": {"type": "number", "exclusiveMinimum": 0},
                        "fill_hex": {"type": "string", "pattern": "^#?[0-9a-fA-F]{3}([0-9a-fA-F]{3})?$"},
                        "line_height_pct": {"type": "number", "exclusiveMinimum": 0}
                    }
                }),
            ),
            (
                "update_node_properties",
                "Update mutable properties on a node. Returns `applied` plus `ignored` for keys the node could not accept.",
                json!({
                    "type": "object",
                    "additionalProperties": false,
                    "required": ["node_id", "set"],
                    "properties": {
                        "node_id": {"type": "string"},
                        "set": {
                            "type": "object",
                            "additionalProperties": false,
                            "properties": {
                                "x": {"type": "number"},
                                "y": {"type": "number"},
                                "width": {"type": "number", "exclusiveMinimum": 0},
                                "height": {"type": "number", "exclusiveMinimum": 0},
                                "rotation": {"type": "number"},
                                "opacity": {"type": "number", "minimum": 0, "maximum": 1},
                                "visible": {"type": "boolean"},
                                "name": {"type": "string"}
                            }
                        }
                    }
                }),
            ),
        ];

        let mut tools = Vec::with_capacity(specs.len());
        for (name, desc, schema) in specs {
            let obj: JsonObject = serde_json::from_value(schema)
                .with_context(|| format!("parsing static schema for tool `{name}`"))?;
            tools.push(Tool::new(name, desc, Arc::new(obj)));
        }
        Ok(tools)
    }
}

impl ServerHandler for FigmaServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_instructions("Bridge write operations to a connected local Figma plugin.")
    }

    fn get_tool(&self, name: &str) -> Option<Tool> {
        self.tools.iter().find(|tool| tool.name == name).cloned()
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _ctx: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, McpError> {
        Ok(ListToolsResult {
            tools: (*self.tools).clone(),
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
                BridgeError::NotConnected.as_json().to_string(),
            )]));
        }
        let params = Value::Object(req.arguments.unwrap_or_default());
        match self.bridge.call(&req.name, params).await {
            Ok(value) => Ok(CallToolResult::success(vec![Content::text(
                value.to_string(),
            )])),
            Err(e) => Ok(CallToolResult::error(vec![Content::text(
                e.as_json().to_string(),
            )])),
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

    let dir = secret_dir();
    let (secret, freshly_created) = load_or_create_secret(&dir).context("loading bridge secret")?;
    if freshly_created {
        tracing::info!(
            "generated new bridge secret at {}\n\n  Paste this into the Figma plugin window (one-time):\n\n    {}\n",
            dir.join("secret").display(),
            secret
        );
    } else {
        tracing::info!("loaded bridge secret from {}", dir.join("secret").display());
    }

    let listener = TcpListener::bind(BRIDGE_ADDR)
        .await
        .with_context(|| format!("could not bind {BRIDGE_ADDR} (is another instance running?)"))?;

    let tools = FigmaServer::tools_inner().context("building tool schemas")?;
    let bridge = Bridge::new(Arc::new(secret));
    let accept_handle = tokio::spawn(bridge.clone().accept_loop(listener));

    let server = FigmaServer {
        bridge,
        tools: Arc::new(tools),
    };
    let transport = stdio();
    let service = server.serve(transport).await?;

    tokio::select! {
        biased;
        _ = tokio::signal::ctrl_c() => {
            tracing::info!("ctrl-c received, shutting down");
        }
        res = service.waiting() => {
            res?;
        }
        join = accept_handle => {
            if let Err(e) = join {
                tracing::error!("accept loop joined with error: {e}");
            }
        }
    }
    Ok(())
}

// -----------------------------------------------------------------------------
// Tests
// -----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tools_inner_parses_all_schemas() {
        let tools = FigmaServer::tools_inner().expect("static schemas must parse");
        let names: Vec<&str> = tools.iter().map(|t| t.name.as_ref()).collect();
        assert_eq!(
            names,
            vec![
                "set_text",
                "delete_node",
                "create_text_node",
                "update_node_properties"
            ]
        );
    }

    #[test]
    fn plugin_response_deserializes_ok() {
        let raw = r#"{"id":"abc","ok":true,"result":{"node_id":"1:2"}}"#;
        let r: PluginResponse = serde_json::from_str(raw).unwrap();
        assert!(r.ok);
        assert_eq!(r.id, "abc");
        assert_eq!(r.result.unwrap()["node_id"], "1:2");
    }

    #[test]
    fn plugin_response_deserializes_err() {
        let raw = r#"{"id":"abc","ok":false,"error":{"code":"node_not_found","message":"x"}}"#;
        let r: PluginResponse = serde_json::from_str(raw).unwrap();
        assert!(!r.ok);
        let e = r.error.unwrap();
        assert_eq!(e.code, "node_not_found");
    }

    #[test]
    fn plugin_response_missing_fields_default() {
        let raw = r#"{"id":"abc","ok":true}"#;
        let r: PluginResponse = serde_json::from_str(raw).unwrap();
        assert!(r.ok);
        assert!(r.result.is_none());
        assert!(r.error.is_none());
    }

    #[test]
    fn constant_time_eq_equal_strings() {
        assert!(constant_time_eq(b"hello", b"hello"));
    }

    #[test]
    fn constant_time_eq_different_strings() {
        assert!(!constant_time_eq(b"hello", b"world"));
    }

    #[test]
    fn constant_time_eq_different_lengths() {
        assert!(!constant_time_eq(b"hi", b"hello"));
        assert!(!constant_time_eq(b"", b"hi"));
    }

    #[test]
    fn validate_hello_accepts_correct_secret() {
        let payload = json!({
            "op": "hello",
            "protocol_version": PROTOCOL_VERSION,
            "secret": "topsecret",
        })
        .to_string();
        assert!(validate_hello(&payload, "topsecret").is_ok());
    }

    #[test]
    fn validate_hello_rejects_wrong_secret() {
        let payload = json!({
            "op": "hello",
            "protocol_version": PROTOCOL_VERSION,
            "secret": "wrong",
        })
        .to_string();
        let err = validate_hello(&payload, "right").unwrap_err();
        assert_eq!(err.code(), "auth_failed");
    }

    #[test]
    fn validate_hello_rejects_wrong_op() {
        let payload = json!({
            "op": "set_text",
            "protocol_version": PROTOCOL_VERSION,
            "secret": "topsecret",
        })
        .to_string();
        let err = validate_hello(&payload, "topsecret").unwrap_err();
        assert_eq!(err.code(), "invalid_params");
    }

    #[test]
    fn validate_hello_rejects_unsupported_version() {
        let payload = json!({
            "op": "hello",
            "protocol_version": 999,
            "secret": "topsecret",
        })
        .to_string();
        let err = validate_hello(&payload, "topsecret").unwrap_err();
        assert_eq!(err.code(), "unsupported_version");
    }

    #[test]
    fn validate_hello_rejects_missing_secret() {
        let payload = json!({
            "op": "hello",
            "protocol_version": PROTOCOL_VERSION,
        })
        .to_string();
        let err = validate_hello(&payload, "topsecret").unwrap_err();
        assert_eq!(err.code(), "invalid_params");
    }

    #[test]
    fn validate_hello_rejects_bad_json() {
        let err = validate_hello("not json", "anything").unwrap_err();
        assert_eq!(err.code(), "invalid_params");
    }

    #[test]
    fn load_or_create_secret_generates_then_reads() {
        let tmp = std::env::temp_dir().join(format!("figma-write-mcp-test-{}", Uuid::new_v4()));
        let (s1, fresh1) = load_or_create_secret(&tmp).expect("generate");
        assert!(fresh1);
        assert!(s1.len() >= 32);
        let (s2, fresh2) = load_or_create_secret(&tmp).expect("read");
        assert!(!fresh2);
        assert_eq!(s1, s2);
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn bridge_error_as_json_shapes() {
        let nc = BridgeError::NotConnected.as_json();
        assert_eq!(nc["code"], "plugin_not_connected");

        let t = BridgeError::Timeout.as_json();
        assert_eq!(t["code"], "timeout");

        let p = BridgeError::Plugin(PluginError {
            code: "node_not_found".into(),
            message: "missing".into(),
        })
        .as_json();
        assert_eq!(p["code"], "node_not_found");
        assert_eq!(p["message"], "missing");
    }

    #[tokio::test]
    async fn bridge_call_returns_not_connected_when_no_plugin() {
        let bridge = Bridge::new(Arc::new("s".into()));
        let err = bridge.call("set_text", json!({})).await.unwrap_err();
        assert!(matches!(err, BridgeError::NotConnected));
        // Pending map must not retain the cancelled entry.
        assert!(bridge.pending.lock().await.is_empty());
    }

    #[tokio::test]
    async fn bridge_call_timeout_cleans_pending() {
        // Install a sender that swallows messages without producing responses.
        let bridge = Bridge::new(Arc::new("s".into()));
        let (tx, mut rx) = mpsc::channel::<String>(8);
        *bridge.outbound.lock().await = Some(tx);
        // Drain outbound so send succeeds but no response ever arrives.
        let _drain = tokio::spawn(async move { while rx.recv().await.is_some() {} });

        // Patch the timeout via tokio::time pause to keep the test fast.
        tokio::time::pause();
        let call_fut = bridge.call("ping", json!({}));
        tokio::pin!(call_fut);

        // Advance past the call timeout.
        tokio::time::advance(CALL_TIMEOUT + Duration::from_millis(1)).await;
        let err = (&mut call_fut).await.unwrap_err();
        assert!(matches!(err, BridgeError::Timeout));
        assert!(bridge.pending.lock().await.is_empty());
    }

    // Property-style randomised parser fuzz: parse_hello must never panic.
    #[test]
    fn validate_hello_never_panics_on_random_input() {
        use rand::{Rng, RngExt};
        let mut rng = rand::rng();
        for _ in 0..1024 {
            let len = rng.random_range(0..256);
            let bytes: Vec<u8> = (0..len).map(|_| rng.random::<u8>()).collect();
            // Force valid UTF-8 by replacing invalid bytes.
            let s: String = bytes
                .iter()
                .map(|b| char::from_u32((*b as u32) % 0x80).unwrap_or('?'))
                .collect();
            let _ = validate_hello(&s, "secret");
        }
    }

    #[test]
    fn plugin_response_never_panics_on_random_input() {
        use rand::{Rng, RngExt};
        let mut rng = rand::rng();
        for _ in 0..1024 {
            let len = rng.random_range(0..256);
            let bytes: Vec<u8> = (0..len).map(|_| rng.random::<u8>()).collect();
            let s: String = bytes
                .iter()
                .map(|b| char::from_u32((*b as u32) % 0x80).unwrap_or('?'))
                .collect();
            let _ = serde_json::from_str::<PluginResponse>(&s);
        }
    }
}
