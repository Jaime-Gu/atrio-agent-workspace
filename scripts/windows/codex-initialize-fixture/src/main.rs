//! One actual ACP initialize request, isolated local config, zero sessions/prompts.
#![allow(dead_code)]
#[path = "../../../../src-tauri/src/platform/windows/process.rs"]
mod owned_process;
mod agent {
    pub(crate) mod native {
        use std::{
            io::Read,
            sync::{
                atomic::{AtomicBool, AtomicUsize, Ordering},
                mpsc, Arc,
            },
            thread::{self, JoinHandle},
            time::Duration,
        };
        pub(crate) const MAX_FRAME: usize = 2 * 1024 * 1024;
        const MAX_QUEUE: usize = 4 * 1024 * 1024;
        pub(crate) enum IoEvent {
            Line(Vec<u8>),
            StdoutError(&'static str),
            Eof,
            Diagnostic(&'static str),
        }
        pub(crate) fn reader(
            mut source: impl Read + Send + 'static,
            sender: mpsc::Sender<IoEvent>,
            stop: Arc<AtomicBool>,
            budget: Arc<AtomicUsize>,
            stdout: bool,
        ) -> JoinHandle<()> {
            thread::spawn(move || {
                let mut buffer = Vec::new();
                let mut chunk = [0u8; 8192];
                let mut diagnostics = 0;
                while !stop.load(Ordering::Acquire) {
                    match source.read(&mut chunk) {
                        Ok(0) => {
                            if stdout {
                                let _ = sender.send(if buffer.is_empty() {
                                    IoEvent::Eof
                                } else {
                                    IoEvent::StdoutError("ACP stdout 帧未以换行结束")
                                });
                            }
                            break;
                        }
                        Ok(count) => {
                            buffer.extend_from_slice(&chunk[..count]);
                            while let Some(end) = buffer.iter().position(|byte| *byte == b'\n') {
                                let line: Vec<u8> = buffer.drain(..=end).collect();
                                if stdout {
                                    if line.len() > MAX_FRAME
                                        || budget.fetch_add(line.len(), Ordering::AcqRel)
                                            + line.len()
                                            > MAX_QUEUE
                                    {
                                        let _ = sender.send(IoEvent::StdoutError(
                                            "ACP stdout 超出帧或队列上限",
                                        ));
                                        return;
                                    }
                                    if sender.send(IoEvent::Line(line)).is_err() {
                                        return;
                                    }
                                } else if diagnostics < 64 {
                                    if let Some(summary) = diagnostic_summary(&line) {
                                        let _ = sender.send(IoEvent::Diagnostic(summary));
                                        diagnostics += 1;
                                    }
                                }
                            }
                            if buffer.len() > if stdout { MAX_FRAME } else { 8192 } {
                                if stdout {
                                    let _ = sender
                                        .send(IoEvent::StdoutError("ACP stdout 超大未终止帧"));
                                    break;
                                }
                                buffer.clear(); // Never retain unbounded or unterminated stderr.
                            }
                        }
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                            thread::sleep(Duration::from_millis(10))
                        }
                        Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                        Err(_) => {
                            if stdout {
                                let _ = sender.send(IoEvent::StdoutError("ACP stdout 读取失败"));
                            }
                            break;
                        }
                    }
                }
            })
        }
        fn diagnostic_summary(bytes: &[u8]) -> Option<&'static str> {
            // No raw stderr is retained: Hermes logs prompt text and user configuration paths.
            let lower = String::from_utf8_lossy(bytes).to_ascii_lowercase();
            if lower.contains("prompt on session") {
                return None;
            }
            if lower.contains("401")
                || lower.contains("authentication")
                || lower.contains("unauthorized")
            {
                Some("Agent 诊断提示认证异常，请在自己的终端检查模型配置")
            } else if lower.contains("timeout") || lower.contains("timed out") {
                Some("Agent 诊断提示服务超时")
            } else if lower.contains("error")
                || lower.contains("exception")
                || lower.contains("traceback")
            {
                Some("Agent 诊断报告异常；原始内容已隐藏")
            } else {
                None
            }
        }
    }
}
use agent::native::IoEvent;
use owned_process::ManagedProcess;
use serde_json::{json, Value};
use std::{
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicU32, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};

fn command(program: &Path, isolation: &Path) -> Command {
    let mut cmd = Command::new(program);
    cmd.env_clear()
        .current_dir(isolation.join("cwd"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for key in ["SystemRoot", "WINDIR", "SystemDrive", "COMSPEC", "PATHEXT"] {
        if let Some(value) = std::env::var_os(key) {
            cmd.env(key, value);
        }
    }
    cmd.env("CODEX_HOME", isolation.join("codex-home"))
        .env("HOME", isolation.join("home"))
        .env("USERPROFILE", isolation.join("home"))
        .env("APPDATA", isolation.join("roaming"))
        .env("LOCALAPPDATA", isolation.join("local"))
        .env("TEMP", isolation.join("temp"))
        .env("TMP", isolation.join("temp"));
    if let Some(root) = std::env::var_os("SystemRoot") {
        cmd.env("PATH", PathBuf::from(root).join("System32"));
    }
    cmd
}

fn local_version(cli: &Path, isolation: &Path) -> Result<String, String> {
    let mut cmd = command(cli, isolation);
    cmd.arg("--version");
    let mut child = ManagedProcess::spawn(cmd)?;
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut output = String::new();
    let mut error = None;
    loop {
        if Instant::now() >= deadline {
            error = Some("CLI version deadline exceeded".into());
            break;
        }
        match child.io.recv_timeout(Duration::from_millis(30)) {
            Ok(IoEvent::Line(line)) => {
                child.byte_budget.fetch_sub(line.len(), Ordering::AcqRel);
                if output.len() + line.len() > 8192 {
                    error = Some("CLI version output exceeded limit".into());
                    break;
                }
                output.push_str(&String::from_utf8_lossy(&line));
            }
            Ok(IoEvent::StdoutError(message)) => {
                error = Some(message.into());
                break;
            }
            Ok(IoEvent::Eof) => break,
            Ok(IoEvent::Diagnostic(_)) => (),
            Err(_) if Instant::now() < deadline => (),
            Err(_) => {
                error = Some("CLI version deadline exceeded".into());
                break;
            }
        }
    }
    child.finish()?;
    if let Some(error) = error {
        Err(error)
    } else {
        Ok(output.trim().into())
    }
}

fn run() -> Result<bool, String> {
    let mut args = std::env::args_os().skip(1);
    let runtime = std::path::absolute(PathBuf::from(
        args.next().ok_or("expected runtime directory")?,
    ))
    .map_err(|e| e.to_string())?;
    let cli = std::path::absolute(PathBuf::from(
        args.next().ok_or("expected official CLI path")?,
    ))
    .map_err(|e| e.to_string())?;
    let isolation = PathBuf::from(args.next().ok_or("expected fresh isolated directory")?);
    if isolation.exists() {
        return Err("isolation directory must be new; no user data may be reused".into());
    }
    for part in ["codex-home", "home", "cwd", "roaming", "local", "temp"] {
        std::fs::create_dir_all(isolation.join(part)).map_err(|e| e.to_string())?;
    }
    let isolation = std::path::absolute(isolation).map_err(|e| e.to_string())?;
    std::fs::write(
        isolation.join("codex-home/config.toml"),
        "[analytics]\nenabled = false\n",
    )
    .map_err(|e| e.to_string())?;
    let cli_version = local_version(&cli, &isolation)?;
    if !cli_version.contains("0.159.0-alpha.12.1") {
        return Err(format!("unexpected official CLI version: {cli_version}"));
    }
    let manifest: Value = serde_json::from_slice(
        &std::fs::read(runtime.join("manifest.json")).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    if manifest["platform"] != "windows"
        || manifest["architecture"] != "x64"
        || manifest["versions"]["node"] != "22.23.3"
        || manifest["versions"]["adapter"] != "1.13.1"
    {
        return Err("unexpected Windows runtime identity".into());
    }
    let mut cmd = command(&runtime.join("bin/node.exe"), &isolation);
    cmd.arg(runtime.join("adapter/index.js"))
        .env("CODEX_PATH", &cli);
    let mut child = ManagedProcess::spawn(cmd)?;
    let pid = child.child.id();
    let tracked = Arc::new(AtomicU32::new(pid));
    child.tracked_pid = Some(tracked.clone());
    let started = Instant::now();
    let result = (|| -> Result<Value, String> {
        child.send(&json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{
            "protocolVersion":1,
            "clientInfo":{"name":"atrio-windows-initialize-only-acceptance","version":"0.0.6"},
            "clientCapabilities":{"fs":{"readTextFile":false,"writeTextFile":false},"terminal":false,"auth":{"terminal":false}}
        }}))?;
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            if Instant::now() >= deadline {
                return Err("ACP initialize deadline exceeded (15s)".into());
            }
            match child.io.recv_timeout(Duration::from_millis(30)) {
                Ok(IoEvent::Line(line)) => {
                    child.byte_budget.fetch_sub(line.len(), Ordering::AcqRel);
                    let response: Value = serde_json::from_slice(&line)
                        .map_err(|_| "ACP stdout contained non-JSON output")?;
                    if response.get("method").is_some() {
                        if response.get("id").is_some() {
                            return Err("initialize attempted a Host request; no file, terminal, auth or session request was executed".into());
                        }
                        continue; // Record no raw identity/config notification payload.
                    }
                    if response["jsonrpc"] != "2.0" || response["id"] != 1 {
                        return Err("unexpected initialize response id/framing".into());
                    }
                    if response.get("error").is_some() {
                        return Err(format!(
                            "ACP initialize returned protocol error: {}",
                            response["error"]
                        ));
                    }
                    let result = response
                        .get("result")
                        .ok_or("ACP initialize had no result")?
                        .clone();
                    if result["protocolVersion"] != 1 || result["agentInfo"]["version"] != "1.13.1"
                    {
                        return Err("unexpected negotiated protocol or adapter version".into());
                    }
                    return Ok(result);
                }
                Ok(IoEvent::StdoutError(message)) => return Err(message.into()),
                Ok(IoEvent::Eof) => return Err("ACP exited before initialize response".into()),
                Ok(IoEvent::Diagnostic(_)) => (), // Production reader hides raw stderr.
                Err(_) if Instant::now() < deadline => (),
                Err(_) => return Err("ACP initialize deadline exceeded (15s)".into()),
            }
        }
    })();
    let adapter_exit_before_cleanup = child
        .child
        .try_wait()
        .ok()
        .flatten()
        .and_then(|status| status.code());
    let cleanup = child.finish();
    let cleanup_confirmed = cleanup.is_ok();
    let passed = result.is_ok() && cleanup.is_ok();
    println!("{}", serde_json::to_string_pretty(&json!({
        "status":if passed {"PASS"} else {"FAIL"},
        "validationKind":"ACTUAL_BUNDLED_CODEX_ACP_INITIALIZE_ONLY",
        "runtime":runtime,"cli":cli,"cliVersion":cli_version,"runtimeVersions":manifest["versions"],
        "sourceCommit":std::env::var("ATRIO_FIXTURE_SOURCE_COMMIT").unwrap_or_default(),
        "isolatedDirectory":isolation,"personalConfigurationRead":false,"loginPerformed":false,
        "requestedMethods":["initialize"],"newSessionRequests":0,"promptRequests":0,
        "modelCalls":0,"fileToolCalls":0,"terminalToolCalls":0,"workspaceDatabasesOpened":0,
        "initializeElapsedMs":started.elapsed().as_millis(),"adapterExitBeforeCleanup":adapter_exit_before_cleanup,
        "initializeResult":result.as_ref().ok(),"initializeError":result.as_ref().err(),
        "ownedPid":pid,"cleanup":{"implementation":"production platform/windows/process.rs ManagedProcess::finish",
            "confirmed":cleanup.is_ok(),"error":cleanup.err(),"leaderReaped":child.leader_reaped,
            "jobActiveProcessesAfterConfirmedFinish":if cleanup_confirmed {Some(0)} else {None},
            "trackedPidAfterFinish":tracked.load(Ordering::Acquire),"readerAndWriterThreadsJoined":cleanup_confirmed,
            "unrelatedProcessesTargeted":0},
        "realModelAcceptance":"NOT_TESTED","loginStatusReportedBeforeTest":"NOT_LOGGED_IN"
    })).map_err(|e|e.to_string())?);
    Ok(passed)
}
fn main() {
    match run() {
        Ok(true) => (),
        Ok(false) => std::process::exit(1),
        Err(error) => {
            eprintln!("initialize-only validation failed before result: {error}");
            std::process::exit(1);
        }
    }
}
