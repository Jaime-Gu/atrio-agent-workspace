//! Private local MCP transport. The child never opens a workspace DB or writes files.
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
#[cfg(unix)]
use std::io::BufReader;
use std::io::{BufRead, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
#[cfg(unix)]
use std::{sync::atomic::AtomicUsize, time::Duration};

pub(crate) const MAX_MESSAGE: usize = 1_500_000;
pub(crate) const MAX_CLIENTS: usize = 16;

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ToolRequest {
    pub session_token: String,
    pub run_scope: String,
    pub tool: String,
    pub args: Value,
    pub request_id: String,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ToolResponse {
    pub(crate) result: Option<Value>,
    pub(crate) error: Option<String>,
}

/// No token or workspace identity is encoded in the local endpoint name.
pub struct ToolServer {
    pub(crate) socket: PathBuf,
    pub(crate) stop: Arc<AtomicBool>,
    pub(crate) thread: Option<std::thread::JoinHandle<()>>,
}

impl ToolServer {
    #[cfg(windows)]
    pub fn start(
        handler: Arc<dyn Fn(ToolRequest) -> Result<Value, String> + Send + Sync>,
    ) -> Result<Self, String> {
        crate::platform::windows::ipc::start(handler)
    }

    #[cfg(unix)]
    pub fn start(
        handler: Arc<dyn Fn(ToolRequest) -> Result<Value, String> + Send + Sync>,
    ) -> Result<Self, String> {
        use std::os::unix::{fs::PermissionsExt, net::UnixListener};
        // macOS Unix socket paths have a short maximum; TMPDIR may be too long.
        let directory = PathBuf::from("/tmp").join(format!("atrio-ipc-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&directory).map_err(|e| e.to_string())?;
        std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o700))
            .map_err(|e| e.to_string())?;
        let socket = directory.join("host.sock");
        let listener = UnixListener::bind(&socket).map_err(|e| e.to_string())?;
        std::fs::set_permissions(&socket, std::fs::Permissions::from_mode(0o600))
            .map_err(|e| e.to_string())?;
        listener.set_nonblocking(true).map_err(|e| e.to_string())?;
        let stop = Arc::new(AtomicBool::new(false));
        let stopped = stop.clone();
        let clients = Arc::new(AtomicUsize::new(0));
        let thread = std::thread::spawn(move || {
            while !stopped.load(Ordering::Acquire) {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        if clients.fetch_add(1, Ordering::AcqRel) >= MAX_CLIENTS {
                            clients.fetch_sub(1, Ordering::AcqRel);
                            continue;
                        }
                        let handler = handler.clone();
                        let clients = clients.clone();
                        std::thread::spawn(move || {
                            struct Guard(Arc<AtomicUsize>);
                            impl Drop for Guard {
                                fn drop(&mut self) {
                                    self.0.fetch_sub(1, Ordering::AcqRel);
                                }
                            }
                            let _guard = Guard(clients);
                            let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
                            let _ = stream.set_write_timeout(Some(Duration::from_secs(5)));
                            let result = (|| {
                                let mut bytes = Vec::new();
                                let mut reader = BufReader::new(&mut stream);
                                let count = reader
                                    .by_ref()
                                    .take((MAX_MESSAGE + 1) as u64)
                                    .read_until(b'\n', &mut bytes)
                                    .map_err(|_| "本机工具请求读取失败")?;
                                if count == 0 || count > MAX_MESSAGE || !bytes.ends_with(b"\n") {
                                    return Err("本机工具请求过大或未完整发送".into());
                                }
                                let request: ToolRequest = serde_json::from_slice(&bytes)
                                    .map_err(|_| "本机工具请求格式错误")?;
                                if request.session_token.len() > 256
                                    || request.run_scope.len() > 256
                                    || request.request_id.len() > 128
                                    || request.tool.len() > 128
                                {
                                    return Err("本机工具请求字段超限".into());
                                }
                                handler(request)
                            })();
                            let response = match result {
                                Ok(result) => ToolResponse {
                                    result: Some(result),
                                    error: None,
                                },
                                Err(error) => ToolResponse {
                                    result: None,
                                    error: Some(error),
                                },
                            };
                            if let Ok(mut bytes) = serde_json::to_vec(&response) {
                                if bytes.len() > MAX_MESSAGE {
                                    bytes=serde_json::to_vec(&ToolResponse{result:None,error:Some("模块结果超过本机工具传输上限；请缩小已保存文档或拆分模块后重试".into())}).unwrap();
                                }
                                bytes.push(b'\n');
                                let _ = stream.write_all(&bytes);
                            }
                        });
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(15))
                    }
                    Err(_) => break,
                }
            }
        });
        Ok(Self {
            socket,
            stop,
            thread: Some(thread),
        })
    }
    pub fn path(&self) -> &Path {
        &self.socket
    }
}

impl Drop for ToolServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
        #[cfg(unix)]
        {
            let _ = std::fs::remove_file(&self.socket);
            if let Some(dir) = self.socket.parent() {
                let _ = std::fs::remove_dir(dir);
            }
        }
    }
}

#[cfg(windows)]
fn forward(socket: &Path, request: &ToolRequest) -> Result<Value, String> {
    crate::platform::windows::ipc::forward(socket, request)
}

#[cfg(unix)]
fn forward(socket: &Path, request: &ToolRequest) -> Result<Value, String> {
    use std::os::unix::net::UnixStream;
    let mut stream =
        UnixStream::connect(socket).map_err(|_| "Atrio Host 已退出或本机工具连接不可用")?;
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .map_err(|_| "无法设置工具超时")?;
    stream
        .set_write_timeout(Some(Duration::from_secs(5)))
        .map_err(|_| "无法设置工具超时")?;
    let mut bytes = serde_json::to_vec(request).map_err(|_| "工具请求编码失败")?;
    if bytes.len() > MAX_MESSAGE {
        return Err("工具请求超出上限".into());
    }
    bytes.push(b'\n');
    stream.write_all(&bytes).map_err(|_| "工具请求发送失败")?;
    let mut bytes = Vec::new();
    BufReader::new(stream)
        .take((MAX_MESSAGE + 1) as u64)
        .read_until(b'\n', &mut bytes)
        .map_err(|_| "Host 工具请求超时或断开")?;
    if bytes.len() > MAX_MESSAGE || !bytes.ends_with(b"\n") {
        return Err("Host 工具响应过大或不完整".into());
    }
    let response: ToolResponse =
        serde_json::from_slice(&bytes).map_err(|_| "Host 工具响应格式错误")?;
    if let Some(error) = response.error {
        return Err(error);
    }
    response
        .result
        .ok_or_else(|| "Host 工具响应没有结果".into())
}

fn rpc_error(id: Value, code: i64, message: &str) -> Value {
    json!({"jsonrpc":"2.0","id":id,"error":{"code":code,"message":message}})
}

/// Invoked by the same bundled executable with --workspace-mcp. No GUI startup.
pub fn run_stdio() -> Result<(), String> {
    let socket = std::env::var_os("ATRIO_MCP_SOCKET")
        .map(PathBuf::from)
        .ok_or("缺少Host工具连接")?;
    let token = std::env::var("ATRIO_MCP_SESSION_TOKEN").map_err(|_| "缺少Host工具会话凭据")?;
    if token.is_empty() || token.len() > 256 {
        return Err("Host工具会话凭据无效".into());
    }
    // Each turn presents a Host-issued scope. The child never substitutes the
    // latest scope or allows the model to choose a root/user-origin.
    let stdin = std::io::stdin();
    let mut reader = stdin.lock();
    let stdout = std::io::stdout();
    let mut writer = stdout.lock();
    let mut initialized = false;
    loop {
        let mut bytes = Vec::new();
        let count = reader
            .by_ref()
            .take((MAX_MESSAGE + 1) as u64)
            .read_until(b'\n', &mut bytes)
            .map_err(|_| "MCP stdin读取失败")?;
        if count == 0 {
            break;
        }
        if count > MAX_MESSAGE || !bytes.ends_with(b"\n") {
            return Err("MCP请求超出限制或未完整终止".into());
        }
        let value: Value = match serde_json::from_slice(&bytes) {
            Ok(value) => value,
            Err(_) => {
                write_rpc(&mut writer, &rpc_error(Value::Null, -32700, "Parse error"))?;
                continue;
            }
        };
        let Some(id) = value.get("id").cloned() else {
            continue;
        };
        if value.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
            write_rpc(&mut writer, &rpc_error(id, -32600, "Invalid request"))?;
            continue;
        }
        let method = value.get("method").and_then(Value::as_str).unwrap_or("");
        let response = match method {
            "initialize" => {
                initialized = true;
                json!({"jsonrpc":"2.0","id":id,"result":{"protocolVersion":"2024-11-05","capabilities":{"tools":{"listChanged":false}},"serverInfo":{"name":"atrio-workspace","version":env!("CARGO_PKG_VERSION")},"instructions":"Use the runScope explicitly supplied in the current Atrio turn. Read saved modules with Host tools; proposals are pending until the user approves. Poll get_proposal_result to learn the actual outcome. Never treat pending/rejected/conflict/cancelled as applied. Do not directly edit workspace manifests or databases."}})
            }
            "ping" => json!({"jsonrpc":"2.0","id":id,"result":{}}),
            _ if !initialized => rpc_error(id, -32000, "Initialize first"),
            "tools/list" => {
                json!({"jsonrpc":"2.0","id":id,"result":{"tools":crate::kernel::module_tools::tool_definitions()}})
            }
            "tools/call" => {
                let params = &value["params"];
                let tool = params["name"].as_str().unwrap_or("");
                let args = params.get("arguments").cloned().unwrap_or(json!({}));
                let run_scope = args
                    .get("runScope")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_owned();
                let result = if run_scope.is_empty() {
                    Err("缺少本轮runScope；请使用当前用户请求附带的Host授权句柄".into())
                } else {
                    let request = ToolRequest {
                        session_token: token.clone(),
                        run_scope,
                        tool: tool.into(),
                        args,
                        request_id: uuid::Uuid::new_v4().to_string(),
                    };
                    forward(&socket, &request)
                };
                let (text, is_error) = match result {
                    Ok(value) => (
                        serde_json::to_string(&value).map_err(|_| "工具结果编码失败")?,
                        false,
                    ),
                    Err(error) => (
                        serde_json::to_string(&json!({"status":"error","message":error})).unwrap(),
                        true,
                    ),
                };
                json!({"jsonrpc":"2.0","id":id,"result":{"content":[{"type":"text","text":text}],"isError":is_error}})
            }
            _ => rpc_error(id, -32601, "Method not found"),
        };
        write_rpc(&mut writer, &response)?;
    }
    Ok(())
}
fn write_rpc(writer: &mut impl Write, value: &Value) -> Result<(), String> {
    let mut bytes = serde_json::to_vec(value).map_err(|_| "MCP响应编码失败")?;
    bytes.push(b'\n');
    writer
        .write_all(&bytes)
        .and_then(|_| writer.flush())
        .map_err(|_| "MCP stdout已关闭".into())
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::time::Instant;
    #[test]
    fn private_socket_forwards_to_host_without_trusting_token_or_root() {
        let server = ToolServer::start(Arc::new(|request| {
            if request.session_token != "session" || request.run_scope != "turn-1" {
                return Err("stale scope".into());
            }
            Ok(json!({"status":"ok","tool":request.tool,"args":request.args}))
        }))
        .unwrap();
        let mut request = ToolRequest {
            session_token: "session".into(),
            run_scope: "turn-1".into(),
            tool: "workspace_list_modules".into(),
            args: json!({}),
            request_id: "r1".into(),
        };
        assert_eq!(forward(server.path(), &request).unwrap()["status"], "ok");
        request.run_scope = "turn-0".into();
        assert!(forward(server.path(), &request)
            .unwrap_err()
            .contains("stale"));
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(server.path())
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        let path = server.path().to_owned();
        drop(server);
        assert!(!path.exists());
    }
    #[test]
    fn oversized_host_response_returns_an_explicit_tool_error() {
        let server = ToolServer::start(Arc::new(|_| {
            Ok(json!({"content":"\t".repeat(MAX_MESSAGE)}))
        }))
        .unwrap();
        let request = ToolRequest {
            session_token: "session".into(),
            run_scope: "scope".into(),
            tool: "workspace_read_module".into(),
            args: json!({}),
            request_id: "large".into(),
        };
        assert!(forward(server.path(), &request)
            .unwrap_err()
            .contains("传输上限"));
    }

    #[test]
    fn oversized_client_is_rejected_without_calling_the_host() {
        use std::os::unix::net::UnixStream;
        let calls = Arc::new(AtomicUsize::new(0));
        let c = calls.clone();
        let server = ToolServer::start(Arc::new(move |_| {
            c.fetch_add(1, Ordering::AcqRel);
            Ok(Value::Null)
        }))
        .unwrap();
        let mut stream = UnixStream::connect(server.path()).unwrap();
        stream
            .set_write_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        let _ = stream.write_all(&vec![b'x'; MAX_MESSAGE + 1]);
        let start = Instant::now();
        while start.elapsed() < Duration::from_millis(100) {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(calls.load(Ordering::Acquire), 0);
    }
}
