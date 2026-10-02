//! Synthetic ACP endpoint for production Host acceptance. This is not Hermes.
use serde_json::{json, Value};
use std::io::{self, BufRead, Read, Write};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::Duration;

const MAX_FRAME: u64 = 2 * 1024 * 1024;

#[derive(Default)]
struct OwnedChildren(Vec<Child>);
impl Drop for OwnedChildren {
    fn drop(&mut self) {
        for process in &mut self.0 {
            let _ = process.kill();
            let _ = process.wait();
        }
    }
}

fn emit(value: Value) -> io::Result<()> {
    let stdout = io::stdout();
    let mut output = stdout.lock();
    serde_json::to_writer(&mut output, &value)?;
    output.write_all(b"\n")?;
    output.flush()
}

fn result(id: Value, value: Value) -> io::Result<()> {
    emit(json!({"jsonrpc":"2.0", "id":id, "result":value}))
}

fn error(id: Value, code: i32, message: &str) -> io::Result<()> {
    emit(json!({"jsonrpc":"2.0", "id":id, "error":{"code":code,"message":message}}))
}

fn update(session: &str, turn: u64, waiting: bool) -> io::Result<()> {
    let status = if waiting {
        "等待取消"
    } else {
        "合成任务完成"
    };
    emit(json!({"jsonrpc":"2.0","method":"session/update","params":{
        "sessionId":session,"update":{"sessionUpdate":"agent_message_chunk",
        "content":{"type":"text","text":format!("Windows ACP Fixture · synthetic · 第 {turn} 轮 · {status}")}}
    }}))
}

fn child() -> io::Result<Child> {
    // This descendant inherits this launch's Job and stdio. It deliberately
    // keeps the pipe open, allowing production cancellation to prove that Host
    // reclaims the process tree and readers, rather than only the leader PID.
    Command::new(std::env::current_exe()?)
        .arg("--owned-child")
        .stdin(Stdio::null())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
}

fn completion_marker(block: &Value) -> bool {
    match block {
        Value::String(text) => text.contains("fixture:complete"),
        Value::Array(blocks) => blocks.iter().any(completion_marker),
        Value::Object(block) => ["text", "content", "blocks"]
            .iter()
            .any(|key| block.get(*key).is_some_and(completion_marker)),
        _ => false,
    }
}

fn run() -> io::Result<()> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if args == ["--owned-child"] {
        loop {
            thread::sleep(Duration::from_secs(60));
        }
    }
    if args == ["--version"] {
        // The fixed Host Hermes probe accepts this version prefix. The suffix
        // explicitly discloses synthetic status and must remain in evidence.
        println!("Hermes Agent v0.21.3 (synthetic Windows ACP Fixture; not Hermes)");
        return Ok(());
    }
    if args == ["acp", "--check"] {
        println!("Hermes ACP check OK (synthetic Windows ACP Fixture; not Hermes)");
        return Ok(());
    }
    if args != ["acp"] {
        eprintln!("Windows ACP Fixture is synthetic; use acp, acp --check, or --version");
        return Err(io::ErrorKind::InvalidInput.into());
    }

    let input = io::stdin();
    let mut input = input.lock();
    let session = format!("synthetic-windows-acp-{}", std::process::id());
    let mut initialized = false;
    let mut session_created = false;
    let mut turn = 0;
    let mut pending: Option<Value> = None;
    let mut descendants = OwnedChildren::default();
    loop {
        let mut bytes = Vec::new();
        let count = input
            .by_ref()
            .take(MAX_FRAME + 1)
            .read_until(b'\n', &mut bytes)?;
        if count == 0 {
            break;
        }
        if count as u64 > MAX_FRAME || bytes.last() != Some(&b'\n') {
            return Err(io::ErrorKind::InvalidData.into());
        }
        let value: Value = serde_json::from_slice(&bytes)
            .map_err(|_| io::Error::from(io::ErrorKind::InvalidData))?;
        let id = value.get("id").cloned().unwrap_or(Value::Null);
        let method = value.get("method").and_then(Value::as_str).unwrap_or("");
        if value["jsonrpc"] != "2.0" {
            error(id, -32600, "Synthetic fixture requires JSON-RPC 2.0")?;
            continue;
        }
        match method {
            "initialize" => {
                initialized = true;
                result(
                    id,
                    json!({"protocolVersion":1,"agentCapabilities":{"loadSession":false},
                    "agentInfo":{"name":"windows-acp-fixture-synthetic","version":"0.0.6"},
                    "authMethods":[]}),
                )?;
            }
            "session/new" if initialized => {
                // The MCP server configuration can contain scoped Host tokens.
                // We neither access those fields nor preserve or log this frame.
                session_created = true;
                result(id, json!({"sessionId":session}))?;
            }
            "session/prompt" if session_created => {
                if value.pointer("/params/sessionId").and_then(Value::as_str)
                    != Some(session.as_str())
                {
                    error(id, -32602, "Unknown synthetic session")?;
                } else if pending.is_some() {
                    error(id, -32001, "Synthetic prompt is already pending")?;
                } else {
                    turn += 1;
                    // Host wraps user text with its module and safety context.
                    // Find this synthetic command inside text/content blocks,
                    // rather than requiring the entire wrapped prompt to match.
                    // Every other prompt stays pending indefinitely.
                    let complete = value
                        .pointer("/params/prompt")
                        .is_some_and(completion_marker);
                    update(&session, turn, !complete)?;
                    if complete {
                        result(id, json!({"stopReason":"end_turn"}))?;
                    } else {
                        descendants.0.push(child()?);
                        pending = Some(id);
                    }
                }
            }
            "session/cancel"
                if value.pointer("/params/sessionId").and_then(Value::as_str)
                    == Some(session.as_str()) =>
            {
                if let Some(id) = pending.take() {
                    result(id, json!({"stopReason":"cancelled"}))?;
                }
                // Leave the descendant and leader alive for Host to reclaim.
                // Normal Host cancellation disconnects this transport; reconnect
                // before another turn. This fixture never claims cleanup passed.
            }
            _ if !id.is_null() => error(id, -32601, "Unsupported synthetic ACP request")?,
            _ => {}
        }
    }
    // A standalone stdin smoke test also owns its child: no orphan on clean EOF.
    drop(descendants);
    Ok(())
}

fn main() {
    if run().is_err() {
        eprintln!("Synthetic Windows ACP Fixture failed; input and private fields were not logged");
        std::process::exit(1);
    }
}
