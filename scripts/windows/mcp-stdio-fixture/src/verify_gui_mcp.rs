//! Exercises the actual GUI executable in --workspace-mcp mode, forwarding to
//! the production Windows pipe server with a synthetic, in-memory Host handler.
#![allow(dead_code)]
#[path = "../../../../src-tauri/src/mcp_bridge.rs"]
mod mcp_bridge;
#[path = "../../../../src-tauri/src/platform/windows/ipc.rs"]
pub(crate) mod windows_ipc;
mod platform {
    pub mod windows {
        pub(crate) use crate::windows_ipc as ipc;
    }
}
mod kernel {
    pub mod module_tools {
        pub fn tool_definitions() -> serde_json::Value {
            serde_json::json!([{
                "name":"workspace_list_modules",
                "description":"List synthetic workspace modules.",
                "inputSchema":{"type":"object","properties":{},"additionalProperties":true}
            }])
        }
    }
}

use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Read, Write};
use std::os::windows::process::CommandExt;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

struct ChildGuard(Child);
impl Drop for ChildGuard {
    fn drop(&mut self) {
        if self.0.try_wait().ok().flatten().is_none() {
            let _ = self.0.kill();
            let deadline = Instant::now() + Duration::from_secs(2);
            while self.0.try_wait().ok().flatten().is_none() && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(10));
            }
        }
    }
}

fn main() {
    if let Err(error) = run() {
        eprintln!("FAIL: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let executable =
        PathBuf::from(std::env::args_os().nth(1).ok_or("expected GUI EXE path")?).canonicalize()?;
    let expected_version = std::env::args()
        .nth(2)
        .unwrap_or_else(|| env!("CARGO_PKG_VERSION").into());
    let executable_bytes = std::fs::read(&executable)?;
    let pe_offset = u32::from_le_bytes(
        executable_bytes
            .get(0x3c..0x40)
            .ok_or("not PE")?
            .try_into()?,
    ) as usize;
    if executable_bytes.get(pe_offset..pe_offset + 4) != Some(b"PE\0\0") {
        return Err("not PE".into());
    }
    let machine = u16::from_le_bytes(
        executable_bytes
            .get(pe_offset + 4..pe_offset + 6)
            .ok_or("invalid PE machine header")?
            .try_into()?,
    );
    if machine != 0x8664 {
        return Err(format!("expected Windows x64 PE machine 0x8664, got {machine:#x}").into());
    }
    // IMAGE_OPTIONAL_HEADER (PE32 and PE32+) stores Subsystem at offset 68.
    let offset = pe_offset + 24 + 68;
    let subsystem = u16::from_le_bytes(
        executable_bytes
            .get(offset..offset + 2)
            .ok_or("invalid PE header")?
            .try_into()?,
    );
    if subsystem != 2 {
        return Err(format!("expected GUI subsystem 2, got {subsystem}").into());
    }
    let token = uuid::Uuid::new_v4().to_string();
    let scope = uuid::Uuid::new_v4().to_string();
    let accepted_token = token.clone();
    let accepted_scope = scope.clone();
    let requests = Arc::new(Mutex::new(Vec::new()));
    let observed = requests.clone();
    let server = mcp_bridge::ToolServer::start(Arc::new(move |request| {
        let valid_token = request.session_token == accepted_token;
        let valid_scope = request.run_scope == accepted_scope;
        observed.lock().unwrap().push(json!({
            "validToken":valid_token,"validScope":valid_scope,
            "tool":request.tool,"requestId":request.request_id,
            "args":request.args
        }));
        if !valid_token {
            return Err("synthetic invalid session".into());
        }
        if !valid_scope {
            return Err("synthetic stale scope".into());
        }
        if request.tool != "workspace_list_modules" {
            return Err("unexpected tool".into());
        }
        Ok(json!({"status":"ok","modules":[{"id":"synthetic-document","title":"合成 MCP 文档"}]}))
    }))?;
    let mut command = Command::new(&executable);
    command
        .arg("--workspace-mcp")
        .current_dir(executable.parent().ok_or("GUI EXE has no parent")?)
        .creation_flags(0x08000000) // CREATE_NO_WINDOW
        .env_clear()
        .env("ATRIO_MCP_SOCKET", server.path())
        .env("ATRIO_MCP_SESSION_TOKEN", token)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for key in ["SystemRoot", "WINDIR", "TEMP", "TMP"] {
        if let Some(value) = std::env::var_os(key) {
            command.env(key, value);
        }
    }
    let mut child = ChildGuard(command.spawn()?);
    let mut stdin = child.0.stdin.take().ok_or("missing child stdin")?;
    let stdout = child.0.stdout.take().ok_or("missing child stdout")?;
    let mut stderr = child.0.stderr.take().ok_or("missing child stderr")?;
    let (sender, replies) = mpsc::channel();
    let reader = std::thread::spawn(move || {
        let mut reader = BufReader::new(stdout);
        loop {
            let mut bytes = Vec::new();
            match reader
                .by_ref()
                .take(1_500_001)
                .read_until(b'\n', &mut bytes)
            {
                Ok(0) => break,
                Ok(_) => {
                    if bytes.len() > 1_500_000 || !bytes.ends_with(b"\n") {
                        let _ = sender.send(Err("invalid stdout framing".to_owned()));
                        break;
                    }
                    let value = serde_json::from_slice::<Value>(&bytes).map_err(|e| e.to_string());
                    if sender.send(value).is_err() {
                        break;
                    }
                }
                Err(error) => {
                    let _ = sender.send(Err(error.to_string()));
                    break;
                }
            }
        }
    });
    let (err_sender, err_receiver) = mpsc::channel();
    let err_reader = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let result = stderr.by_ref().take(64 * 1024).read_to_end(&mut bytes);
        let _ = err_sender.send((result, bytes));
    });
    let mut rpc = |value: Value| -> Result<Value, Box<dyn std::error::Error>> {
        serde_json::to_writer(&mut stdin, &value)?;
        stdin.write_all(b"\n")?;
        stdin.flush()?;
        let reply = replies.recv_timeout(Duration::from_secs(5))??;
        if reply["jsonrpc"] != "2.0" || reply["id"] != value["id"] {
            return Err(format!("unexpected RPC response: {reply}").into());
        }
        Ok(reply)
    };
    let before = rpc(json!({"jsonrpc":"2.0","id":1,"method":"tools/list"}))?;
    assert_eq!(before["error"]["code"], -32000);
    let initialize = rpc(
        json!({"jsonrpc":"2.0","id":2,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"synthetic-windows-acceptance","version":"1"}}}),
    )?;
    assert_eq!(
        initialize["result"]["serverInfo"]["name"],
        "atrio-workspace"
    );
    if initialize["result"]["serverInfo"]["version"] != expected_version {
        return Err(format!(
            "MCP version does not match {expected_version}: {}",
            initialize["result"]["serverInfo"]["version"]
        )
        .into());
    }
    let tools = rpc(json!({"jsonrpc":"2.0","id":3,"method":"tools/list"}))?;
    let tool_names: Vec<_> = tools["result"]["tools"]
        .as_array()
        .ok_or("missing tools")?
        .iter()
        .filter_map(|tool| tool["name"].as_str())
        .map(str::to_owned)
        .collect();
    let expected_tools = [
        "workspace_list_modules",
        "workspace_read_module",
        "workspace_get_selection",
        "workspace_propose_changes",
        "workspace_get_proposal_result",
    ];
    if tool_names.len() != expected_tools.len()
        || expected_tools
            .iter()
            .any(|name| !tool_names.iter().any(|actual| actual == name))
    {
        return Err(format!("unexpected production Host tools: {tool_names:?}").into());
    }
    let result = rpc(
        json!({"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"workspace_list_modules","arguments":{"runScope":scope,"fixtureMarker":"合成中文 / with spaces"}}}),
    )?;
    assert_eq!(result["result"]["isError"], false);
    let content: Value = serde_json::from_str(
        result["result"]["content"][0]["text"]
            .as_str()
            .ok_or("missing tool content")?,
    )?;
    assert_eq!(content["modules"][0]["title"], "合成 MCP 文档");
    let stale = rpc(
        json!({"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"workspace_list_modules","arguments":{"runScope":"stale-synthetic-scope"}}}),
    )?;
    assert_eq!(stale["result"]["isError"], true);
    assert!(stale["result"]["content"][0]["text"]
        .as_str()
        .unwrap()
        .contains("synthetic stale scope"));
    let missing = rpc(
        json!({"jsonrpc":"2.0","id":6,"method":"tools/call","params":{"name":"workspace_list_modules","arguments":{}}}),
    )?;
    assert_eq!(missing["result"]["isError"], true);
    let ping = rpc(json!({"jsonrpc":"2.0","id":7,"method":"ping"}))?;
    assert_eq!(ping["result"], json!({}));
    drop(rpc);
    drop(stdin);
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        if let Some(status) = child.0.try_wait()? {
            if !status.success() {
                return Err(format!("MCP exited {status}").into());
            }
            break;
        }
        if Instant::now() >= deadline {
            return Err("GUI MCP did not exit after stdin EOF".into());
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let (error_read, errors) = err_receiver.recv_timeout(Duration::from_secs(1))?;
    error_read?;
    assert!(
        errors.is_empty(),
        "MCP stderr must be empty on successful synthetic run"
    );
    let readers_deadline = Instant::now() + Duration::from_secs(1);
    while (!reader.is_finished() || !err_reader.is_finished()) && Instant::now() < readers_deadline
    {
        std::thread::sleep(Duration::from_millis(5));
    }
    if !reader.is_finished() || !err_reader.is_finished() {
        return Err("MCP stdio handles remained open after process exit".into());
    }
    reader.join().map_err(|_| "stdout reader panicked")?;
    err_reader.join().map_err(|_| "stderr reader panicked")?;
    let requests = requests.lock().unwrap();
    assert_eq!(requests.len(), 2, "missing scope must fail before IPC");
    assert_eq!(requests[0]["validToken"], true);
    assert_eq!(requests[0]["validScope"], true);
    assert_eq!(
        requests[0]["args"]["fixtureMarker"],
        "合成中文 / with spaces"
    );
    assert!(uuid::Uuid::parse_str(requests[0]["requestId"].as_str().unwrap()).is_ok());
    assert_eq!(requests[1]["validScope"], false);
    println!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "status":"PASS","validationKind":"MCP_STDIO_SYNTHETIC_HOST",
            "executable":executable,"peSubsystem":subsystem,"peMachine":machine,"expectedVersion":expected_version,
            "reportedVersion":initialize["result"]["serverInfo"]["version"],
            "checks":["GUI executable inherited stdio", "pre-initialize rejection", "initialize", "actual tools/list", "production named-pipe forward", "UTF-8 argument/result preservation", "Host scope denial", "missing scope rejected before IPC", "ping", "EOF shutdown", "empty stderr"],
            "toolNames":tool_names,"syntheticHostRequestCount":requests.len(),
            "providerCalls":0,"workspaceDatabasesOpened":0
        }))?
    );
    Ok(())
}
