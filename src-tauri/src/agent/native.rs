//! Provider-neutral newline-delimited ACP transport. Authorization and workspace tools stay in Host.
use super::providers::{resolve_program, NativeProvider};
use super::{
    AgentConnector, ConnectorContext, ConnectorEvent, ConnectorEventKind, ConnectorPrompt,
    PermissionOption,
};
use crate::kernel::AgentDescriptor;
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::io::Read;
#[cfg(unix)]
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{
    atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering},
    mpsc, Arc,
};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

pub(crate) const MAX_FRAME: usize = 2 * 1024 * 1024;
const MAX_QUEUE: usize = 4 * 1024 * 1024;
const MAX_REPLY: usize = 1024 * 1024;
#[cfg(unix)]
const FORCE_KILL: i32 = libc::SIGKILL;
#[cfg(unix)]
const TERMINATE: i32 = libc::SIGTERM;
#[cfg(windows)]
const FORCE_KILL: i32 = 9;
#[cfg(windows)]
const TERMINATE: i32 = 15;
#[cfg(unix)]
use crate::platform::macos::process::ManagedProcess;
#[cfg(windows)]
use crate::platform::windows::process::ManagedProcess;

#[derive(Clone)]
struct Limits {
    initialize: Duration,
    session: Duration,
    prompt: Duration,
    cancel: Duration,
    terminate: Duration,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            initialize: Duration::from_secs(30),
            session: Duration::from_secs(60),
            prompt: Duration::from_secs(1800),
            cancel: Duration::from_secs(2),
            terminate: Duration::from_secs(2),
        }
    }
}

enum Control {
    Initialize(ConnectorContext),
    NewSession(ConnectorContext),
    Confirm(ConnectorContext, String),
    Prompt(ConnectorContext, String),
    Cancel(ConnectorContext),
    Permission(ConnectorContext, Value, Option<String>),
    Dispose,
    Shutdown,
}
struct EventPacket {
    event: ConnectorEvent,
    bytes: usize,
}

pub struct NativeAcpConnector {
    commands: mpsc::Sender<Control>,
    events: mpsc::Receiver<EventPacket>,
    queued: Arc<AtomicUsize>,
    worker: Option<JoinHandle<()>>,
    stopped: mpsc::Receiver<()>,
    managed_pid: Arc<AtomicU32>,
}
impl NativeAcpConnector {
    pub fn new(descriptor: AgentDescriptor) -> Self {
        Self::with_mcp(descriptor, Vec::new())
    }
    pub fn with_mcp(descriptor: AgentDescriptor, mcp_servers: Vec<Value>) -> Self {
        Self::with_mcp_limits(descriptor, mcp_servers, Limits::default())
    }
    #[cfg(test)]
    fn with_limits(descriptor: AgentDescriptor, limits: Limits) -> Self {
        Self::with_mcp_limits(descriptor, Vec::new(), limits)
    }
    fn with_mcp_limits(
        descriptor: AgentDescriptor,
        mcp_servers: Vec<Value>,
        limits: Limits,
    ) -> Self {
        let (commands, receiver) = mpsc::channel();
        let (sender, events) = mpsc::channel();
        let queued = Arc::new(AtomicUsize::new(0));
        let budget = queued.clone();
        let managed_pid = Arc::new(AtomicU32::new(0));
        let child_pid = managed_pid.clone();
        let (finished, stopped) = mpsc::channel();
        let worker = thread::spawn(move || {
            Worker::new(
                descriptor,
                mcp_servers,
                receiver,
                sender,
                budget,
                child_pid,
                limits,
            )
            .run();
            let _ = finished.send(());
        });
        Self {
            commands,
            events,
            queued,
            worker: Some(worker),
            stopped,
            managed_pid,
        }
    }
    fn queue(&self, command: Control) -> Result<(), String> {
        self.commands
            .send(command)
            .map_err(|_| "Agent 连接线程已退出，请重新连接".into())
    }
    pub fn shutdown_and_wait(&mut self, timeout: Duration) -> Result<(), String> {
        if self.worker.is_none() {
            return Ok(());
        }
        let _ = self.commands.send(Control::Shutdown);
        match self.stopped.recv_timeout(timeout) {
            Ok(()) => {
                if let Some(worker) = self.worker.take() {
                    worker.join().map_err(|_| "Agent 回收线程异常")?;
                }
                Ok(())
            }
            Err(_)
                if self
                    .worker
                    .as_ref()
                    .is_some_and(|worker| worker.is_finished()) =>
            {
                if let Some(worker) = self.worker.take() {
                    worker.join().map_err(|_| "Agent 回收线程异常")?;
                }
                Ok(())
            }
            Err(_) => Err("Agent 进程回收未在期限内确认；禁止开始新任务".into()),
        }
    }
}
impl AgentConnector for NativeAcpConnector {
    fn initialize(&mut self, context: &ConnectorContext) -> Result<(), String> {
        self.queue(Control::Initialize(context.clone()))
    }
    fn new_session(&mut self, context: &ConnectorContext) -> Result<(), String> {
        self.queue(Control::NewSession(context.clone()))
    }
    fn confirm_session_persisted(
        &mut self,
        context: &ConnectorContext,
        provider_id: &str,
    ) -> Result<(), String> {
        self.queue(Control::Confirm(context.clone(), provider_id.into()))
    }
    fn prompt(&mut self, prompt: ConnectorPrompt) -> Result<(), String> {
        if prompt.text.trim().is_empty() || prompt.text.len() > 64_000 {
            return Err("Agent 文本必须非空且不超过 64 KB".into());
        }
        // Deliberately do not serialize prompt.modules or any local document content.
        self.queue(Control::Prompt(prompt.context, prompt.text))
    }
    fn cancel(&mut self, context: &ConnectorContext) -> Result<(), String> {
        self.queue(Control::Cancel(context.clone()))
    }
    fn dispose(&mut self) -> Result<(), String> {
        self.queue(Control::Dispose)
    }
    fn shutdown(&mut self) -> Result<(), String> {
        self.shutdown_and_wait(Duration::from_secs(3))
    }
    fn managed_process_id(&self) -> Option<u32> {
        let pid = self.managed_pid.load(Ordering::Acquire);
        if pid == 0 {
            None
        } else {
            Some(pid)
        }
    }
    fn poll_events(&mut self) -> Result<Vec<ConnectorEvent>, String> {
        let mut events = Vec::new();
        while let Ok(packet) = self.events.try_recv() {
            self.queued.fetch_sub(packet.bytes, Ordering::AcqRel);
            events.push(packet.event);
        }
        if events.is_empty()
            && self
                .worker
                .as_ref()
                .is_some_and(|worker| worker.is_finished())
        {
            return Err(
                "Agent 连接线程已经退出，无法继续接收事件；请确认回收状态并重新连接".into(),
            );
        }
        Ok(events)
    }
    fn permission_reply(
        &mut self,
        context: &ConnectorContext,
        request_id: &Value,
        option_id: Option<&str>,
    ) -> Result<(), String> {
        self.queue(Control::Permission(
            context.clone(),
            request_id.clone(),
            option_id.map(str::to_owned),
        ))
    }
}
impl Drop for NativeAcpConnector {
    fn drop(&mut self) {
        // Normal dispose is asynchronous. Drop is the final, bounded force-reap fallback.
        if self.shutdown_and_wait(Duration::from_secs(3)).is_err() {
            // Keep the worker owning the child until its kill/wait completes; never claim success.
            eprintln!("Agent process cleanup is still pending after the shutdown deadline");
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HermesProbe {
    pub executable: String,
    pub version: String,
    pub acp_available: bool,
}

pub fn probe_hermes(command: Option<&str>) -> Result<HermesProbe, String> {
    let path = resolve_executable(command.unwrap_or("hermes"))?;
    let version_output = probe_command(&path, &["--version"])?;
    let version = version_output
        .lines()
        .find_map(|line| {
            let value = line
                .strip_prefix("Hermes Agent v")
                .or_else(|| line.trim().starts_with("0.").then_some(line.trim()))?;
            let token = value.split_whitespace().next()?;
            (token.len() <= 64
                && token
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '+')))
            .then(|| format!("Hermes Agent v{token}"))
        })
        .ok_or("Hermes 版本响应无法识别")?;
    let check = probe_command(&path, &["acp", "--check"])?;
    if !check.contains("Hermes ACP check OK") {
        return Err("Agent 已安装，但 ACP 依赖检查未通过；请在终端运行 hermes acp --check".into());
    }
    Ok(HermesProbe {
        executable: path.to_string_lossy().into_owned(),
        version,
        acp_available: true,
    })
}

fn resolve_executable(command: &str) -> Result<PathBuf, String> {
    resolve_program(command, "hermes")
}
fn process_command(path: &Path, args: &[String], cwd: &Path) -> Result<Command, String> {
    process_command_for(path, args, cwd, None)
}
fn process_command_for(
    path: &Path,
    args: &[String],
    cwd: &Path,
    provider: Option<NativeProvider>,
) -> Result<Command, String> {
    #[cfg(unix)]
    let mut cmd = Command::new(path);
    #[cfg(windows)]
    let mut cmd = super::providers::windows_command(path)?;
    cmd.args(args)
        .current_dir(cwd)
        .env_clear()
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(unix)]
    let environment_keys = &["HOME", "TMPDIR", "LANG", "LC_ALL", "LC_CTYPE", "TZ"][..];
    #[cfg(windows)]
    let environment_keys = &[
        "SystemRoot",
        "WINDIR",
        "SystemDrive",
        "COMSPEC",
        "TEMP",
        "TMP",
        "USERPROFILE",
        "HOMEDRIVE",
        "HOMEPATH",
        "APPDATA",
        "LOCALAPPDATA",
        "PROGRAMDATA",
        "PROGRAMFILES",
        "PROGRAMFILES(X86)",
        "PATHEXT",
        "LANG",
        "LC_ALL",
        "TZ",
    ][..];
    for key in environment_keys {
        if let Some(value) = std::env::var_os(key) {
            cmd.env(key, value);
        }
    }
    #[cfg(unix)]
    let mut paths = vec![
        PathBuf::from("/usr/bin"),
        PathBuf::from("/bin"),
        PathBuf::from("/usr/sbin"),
        PathBuf::from("/sbin"),
    ];
    #[cfg(windows)]
    let mut paths = super::providers::windows_search_dirs();
    if let Some(parent) = path.parent() {
        paths.insert(0, parent.to_owned());
    }
    #[cfg(unix)]
    if let Some(home) = std::env::var_os("HOME") {
        paths.push(PathBuf::from(&home).join(".local/bin"));
        paths.push(PathBuf::from(home).join(".hermes/node/bin"));
    }
    #[cfg(unix)]
    paths.extend([
        PathBuf::from("/opt/homebrew/bin"),
        PathBuf::from("/usr/local/bin"),
    ]);
    // Finder launches do not inherit nvm PATH. Resolve the runtime once and put
    // its parent in this child-only PATH; do not source shell startup files.
    if let Ok(node) = resolve_program("node", "node") {
        if let Some(parent) = node.parent() {
            paths.insert(0, parent.to_owned());
        }
    }
    if let Ok(path) = std::env::join_paths(paths) {
        cmd.env("PATH", path);
    }
    cmd.env("PYTHONUNBUFFERED", "1").env("PYTHONUTF8", "1");
    // Explicit Host launch override for authorized isolated acceptance. It is
    // not an Agent descriptor option and cannot be supplied by the WebView.
    // Hermes officially resolves config, memory, skills and sessions via HERMES_HOME.
    if let Some(home) = std::env::var_os("PIXEL_HERMES_TEST_HOME") {
        let profile = PathBuf::from(home);
        if !profile.is_absolute() || !profile.is_dir() {
            return Err("隔离 Hermes profile 不存在；拒绝回退到个人 profile".into());
        }
        cmd.env("HERMES_HOME", profile);
    }
    if let Some(provider) = provider {
        super::providers::configure_process(&mut cmd, provider)?;
    }
    #[cfg(unix)]
    cmd.process_group(0);
    Ok(cmd)
}

fn validate_mcp_servers(servers: &[Value]) -> Result<(), String> {
    if servers.len() > 4
        || serde_json::to_vec(servers)
            .map_err(|_| "无效 MCP 配置")?
            .len()
            > 64_000
    {
        return Err("MCP 配置超出上限".into());
    }
    for server in servers {
        let command = server.get("command").and_then(Value::as_str).unwrap_or("");
        if !server.is_object()
            || server.get("type").is_some()
            || server
                .get("name")
                .and_then(Value::as_str)
                .is_none_or(|v| v.is_empty() || v.len() > 64)
            || !Path::new(command).is_absolute()
            || server
                .get("args")
                .and_then(Value::as_array)
                .is_none_or(|v| !v.iter().all(Value::is_string))
            || server.get("env").and_then(Value::as_array).is_none_or(|v| {
                !v.iter().all(|e| {
                    e.get("name").is_some_and(Value::is_string)
                        && e.get("value").is_some_and(Value::is_string)
                })
            })
        {
            return Err("仅允许 Host 提供绝对入口的结构化 stdio MCP 配置".into());
        }
    }
    Ok(())
}
fn session_params(
    descriptor: &AgentDescriptor,
    cwd: &str,
    servers: &[Value],
) -> Result<Value, String> {
    let provider = NativeProvider::from_descriptor(descriptor)?;
    let mut params = json!({"cwd":cwd,"mcpServers":servers});
    if provider == NativeProvider::ClaudeCode {
        // ACP adapter v0.81.2 delegates these SDK options. Explicit MCP only;
        // credential references stay outside the descriptor and never enter logs.
        params["_meta"] = super::providers::claude_session_meta()?;
    }
    Ok(params)
}

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
                                || budget.fetch_add(line.len(), Ordering::AcqRel) + line.len()
                                    > MAX_QUEUE
                            {
                                let _ = sender
                                    .send(IoEvent::StdoutError("ACP stdout 超出帧或队列上限"));
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
                            let _ = sender.send(IoEvent::StdoutError("ACP stdout 超大未终止帧"));
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
    if lower.contains("401") || lower.contains("authentication") || lower.contains("unauthorized") {
        Some("Agent 诊断提示认证异常，请在自己的终端检查模型配置")
    } else if lower.contains("timeout") || lower.contains("timed out") {
        Some("Agent 诊断提示服务超时")
    } else if lower.contains("error") || lower.contains("exception") || lower.contains("traceback")
    {
        Some("Agent 诊断报告异常；原始内容已隐藏")
    } else {
        None
    }
}

fn probe_process(mut process: ManagedProcess) -> Result<String, String> {
    let deadline = Instant::now() + Duration::from_secs(15);
    let mut output = String::new();
    loop {
        while let Ok(event) = process.io.try_recv() {
            match event {
                IoEvent::Line(line) => {
                    process.byte_budget.fetch_sub(line.len(), Ordering::AcqRel);
                    if output.len() + line.len() > 8192 {
                        return Err("Agent 探测输出超出上限".into());
                    }
                    output.push_str(&String::from_utf8_lossy(&line));
                }
                IoEvent::StdoutError(_) => return Err("Agent 探测输出无效".into()),
                _ => {}
            }
        }
        if let Some(status) = process
            .child
            .try_wait()
            .map_err(|_| "无法读取 Agent 探测状态")?
        {
            process.leader_reaped = true;
            // Readers may not have delivered the final line at the first try_wait.
            for _ in 0..5 {
                thread::sleep(Duration::from_millis(10));
                while let Ok(IoEvent::Line(line)) = process.io.try_recv() {
                    process.byte_budget.fetch_sub(line.len(), Ordering::AcqRel);
                    if output.len() + line.len() > 8192 {
                        return Err("Agent 探测输出超出上限".into());
                    }
                    output.push_str(&String::from_utf8_lossy(&line));
                }
            }
            process.finish()?;
            return if status.success() {
                Ok(output)
            } else {
                Err(format!(
                    "Agent 探测失败（exit {}）；请在终端检查安装",
                    status.code().unwrap_or(-1)
                ))
            };
        }
        if Instant::now() >= deadline {
            return Err("Agent 探测超时".into());
        }
        thread::sleep(Duration::from_millis(10));
    }
}

pub(super) fn probe_command(path: &Path, args: &[&str]) -> Result<String, String> {
    let argv = args
        .iter()
        .map(|value| value.to_string())
        .collect::<Vec<_>>();
    let cwd = path.parent().ok_or("Agent 路径无父目录")?;
    let process = ManagedProcess::spawn(process_command(path, &argv, cwd)?)?;
    probe_process(process)
}

/// Probe an already-resolved launch plan. This is needed for the bundled
/// Windows Codex adapter: its descriptor command is `codex-acp`, while the
/// actual process is `node.exe adapter/index.js`.
pub(super) fn probe_launch_plan(
    plan: &super::providers::LaunchPlan,
    args: &[&str],
    provider: NativeProvider,
) -> Result<String, String> {
    let cwd = plan.program.parent().ok_or("Agent 路径无父目录")?;
    let mut command = process_command_for(&plan.program, &plan.args, cwd, Some(provider))?;
    command.args(args);
    let process = ManagedProcess::spawn(command)?;
    probe_process(process)
}

#[derive(Clone, Copy)]
enum RequestKind {
    Initialize,
    NewSession,
    Prompt,
}
struct Pending {
    kind: RequestKind,
    context: ConnectorContext,
    deadline: Instant,
}
struct Permission {
    id: Value,
    context: ConnectorContext,
    options: HashSet<String>,
}
struct Stopping {
    started: Instant,
    term_sent: bool,
    kill_sent: bool,
    cancelled: bool,
}
struct Worker {
    descriptor: AgentDescriptor,
    mcp_servers: Vec<Value>,
    commands: mpsc::Receiver<Control>,
    events: mpsc::Sender<EventPacket>,
    queued: Arc<AtomicUsize>,
    managed_pid: Arc<AtomicU32>,
    limits: Limits,
    process: Option<ManagedProcess>,
    context: Option<ConnectorContext>,
    active: Option<ConnectorContext>,
    provider: Option<String>,
    confirmed: bool,
    initialized: bool,
    deferred_session: Option<ConnectorContext>,
    pending: HashMap<u64, Pending>,
    permissions: HashMap<String, Permission>,
    next_id: u64,
    seq: u64,
    stopping: Option<Stopping>,
    reply_bytes: usize,
    diagnostic: Option<&'static str>,
    shutdown_deadline: Option<Instant>,
}
impl Worker {
    fn new(
        descriptor: AgentDescriptor,
        mcp_servers: Vec<Value>,
        commands: mpsc::Receiver<Control>,
        events: mpsc::Sender<EventPacket>,
        queued: Arc<AtomicUsize>,
        managed_pid: Arc<AtomicU32>,
        limits: Limits,
    ) -> Self {
        Self {
            descriptor,
            mcp_servers,
            commands,
            events,
            queued,
            managed_pid,
            limits,
            process: None,
            context: None,
            active: None,
            provider: None,
            confirmed: false,
            initialized: false,
            deferred_session: None,
            pending: HashMap::new(),
            permissions: HashMap::new(),
            next_id: 1,
            seq: 0,
            stopping: None,
            reply_bytes: 0,
            diagnostic: None,
            shutdown_deadline: None,
        }
    }
    fn run(mut self) {
        loop {
            match self.commands.recv_timeout(Duration::from_millis(10)) {
                Ok(Control::Shutdown) | Err(mpsc::RecvTimeoutError::Disconnected) => {
                    if self.shutdown_deadline.is_none() {
                        // Give session/cancel a real opportunity, even on policy revocation or app exit.
                        let _ = self.stop(self.active.is_some());
                        self.shutdown_deadline = Some(Instant::now() + Duration::from_millis(500));
                    }
                    thread::sleep(Duration::from_millis(10));
                }
                Ok(command) => {
                    if let Err(error) = self.control(command) {
                        self.fail("invalid_state", &error);
                    }
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
            }
            // A bounded batch ensures cancel/dispose is not starved by a busy provider.
            for _ in 0..64 {
                let event = self
                    .process
                    .as_ref()
                    .and_then(|process| process.io.try_recv().ok());
                match event {
                    Some(IoEvent::Line(line)) => {
                        if let Some(process) = &self.process {
                            process.byte_budget.fetch_sub(line.len(), Ordering::AcqRel);
                        }
                        if let Err(error) = self.frame(&line) {
                            self.fail("protocol_error", &error);
                        }
                    }
                    Some(IoEvent::StdoutError(error)) => self.fail("protocol_error", error),
                    Some(IoEvent::Diagnostic(summary)) => self.diagnostic = Some(summary),
                    Some(IoEvent::Eof) => {
                        if self.stopping.is_some() {
                            self.reap("Agent 已停止");
                        } else {
                            self.fail("process_exited", "Agent stdout 已关闭");
                        }
                    }
                    None => break,
                }
            }
            self.check_time();
            if self.shutdown_deadline.is_some() && self.process.is_none() {
                break;
            }
        }
    }
    fn emit_for(&mut self, context: ConnectorContext, kind: ConnectorEventKind) -> bool {
        self.seq += 1;
        let event = ConnectorEvent {
            context,
            seq: self.seq,
            kind,
        };
        let bytes = serde_json::to_vec(&event).map(|v| v.len()).unwrap_or(512);
        let critical = matches!(
            event.kind,
            ConnectorEventKind::Failed { .. }
                | ConnectorEventKind::Cancelled
                | ConnectorEventKind::Disconnected { .. }
        );
        // Reserve a small bounded allowance for the terminal failure event, not
        // unlimited critical messages if process cleanup keeps failing.
        let limit = MAX_QUEUE + if critical { 16 * 1024 } else { 0 };
        if self.queued.load(Ordering::Acquire) + bytes > limit {
            return false;
        }
        self.queued.fetch_add(bytes, Ordering::AcqRel);
        if self.events.send(EventPacket { event, bytes }).is_err() {
            self.queued.fetch_sub(bytes, Ordering::AcqRel);
            return false;
        }
        true
    }
    fn emit(&mut self, kind: ConnectorEventKind) {
        if let Some(context) = self.active.clone().or_else(|| self.context.clone()) {
            let critical = matches!(
                kind,
                ConnectorEventKind::Failed { .. }
                    | ConnectorEventKind::Cancelled
                    | ConnectorEventKind::Disconnected { .. }
            );
            if !self.emit_for(context, kind) && !critical {
                self.fail("backpressure", "Agent 事件队列已满；连接已停止");
            }
        }
    }
    fn send(&mut self, value: Value) -> Result<(), String> {
        self.process.as_mut().ok_or("Agent 尚未启动")?.send(&value)
    }
    fn request(
        &mut self,
        kind: RequestKind,
        context: ConnectorContext,
        method: &str,
        params: Value,
        timeout: Duration,
    ) -> Result<(), String> {
        let id = self.next_id;
        self.next_id += 1;
        self.send(json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}))?;
        self.pending.insert(
            id,
            Pending {
                kind,
                context,
                deadline: Instant::now() + timeout,
            },
        );
        Ok(())
    }
    fn control(&mut self, command: Control) -> Result<(), String> {
        match command {
            Control::Initialize(context) => {
                if self.process.is_some() {
                    return Err("Agent 连接已存在，请先释放旧连接".into());
                }
                self.context = Some(context.clone());
                self.active = None;
                self.provider = None;
                self.confirmed = false;
                self.initialized = false;
                self.diagnostic = None;
                let provider = NativeProvider::from_descriptor(&self.descriptor)?;
                if self.descriptor.args != provider.args() {
                    return Err(format!(
                        "{} 的 ACP 启动参数不符合已验证入口",
                        provider.label()
                    ));
                }
                validate_mcp_servers(&self.mcp_servers)?;
                // Keep the logical descriptor command separate from the
                // executable we actually spawn. On Windows the default Codex
                // descriptor resolves to the bundled node.exe plus the
                // installed adapter/index.js; an explicit absolute command
                // remains an external adapter launch plan.
                let launch = super::providers::resolve_launch_plan(
                    &self.descriptor.command,
                    &self.descriptor.args,
                    provider,
                )
                .map_err(|error| format!("not_installed: {error}"))?;
                let cwd = Path::new(&context.root);
                if !cwd.is_absolute()
                    || !cwd.is_dir()
                    || cwd.canonicalize().map_err(|_| "无法核实 Agent 工区")? != cwd
                {
                    return Err("Agent 需要已核实的规范化绝对工区".into());
                }
                let command =
                    process_command_for(&launch.program, &launch.args, cwd, Some(provider))
                        .map_err(|error| format!("not_installed: {error}"))?;
                let mut process = ManagedProcess::spawn(command)
                    .map_err(|error| format!("not_installed: {error}"))?;
                process.tracked_pid = Some(self.managed_pid.clone());
                self.managed_pid.store(process.group, Ordering::Release);
                self.process = Some(process);
                self.request(RequestKind::Initialize,context,"initialize",json!({"protocolVersion":1,"clientInfo":{"name":"pixel-workspace","version":env!("CARGO_PKG_VERSION")},"clientCapabilities":{"fs":{"readTextFile":false,"writeTextFile":false},"terminal":false,"auth":{"terminal":false},"_meta":{"jetbrains":{"air":{"version":1,"capabilities":["sessionFailure"]}}}}}),self.limits.initialize)?;
            }
            Control::NewSession(context) => {
                self.require_scope(&context)?;
                if self.provider.is_some()
                    || self
                        .pending
                        .values()
                        .any(|r| matches!(r.kind, RequestKind::NewSession))
                {
                    return Err("Agent 会话已创建或正在创建".into());
                }
                if !self.initialized {
                    self.deferred_session = Some(context);
                } else {
                    self.request(
                        RequestKind::NewSession,
                        context.clone(),
                        "session/new",
                        session_params(&self.descriptor, &context.root, &self.mcp_servers)?,
                        self.limits.session,
                    )?;
                }
            }
            Control::Confirm(context, provider) => {
                self.require_scope(&context)?;
                if self.provider.as_deref() != Some(provider.as_str()) {
                    return Err("Host 持久化的 Agent 会话映射不匹配".into());
                }
                self.confirmed = true;
            }
            Control::Prompt(context, text) => {
                self.require_scope(&context)?;
                if !self.confirmed || self.provider.is_none() {
                    return Err("Agent 会话映射尚未持久化；禁止发送任务".into());
                }
                if self.active.is_some() || self.stopping.is_some() {
                    return Err("Agent 尚有运行或正在停止".into());
                }
                self.active = Some(context.clone());
                self.reply_bytes = 0;
                self.diagnostic = None;
                self.request(
                    RequestKind::Prompt,
                    context,
                    "session/prompt",
                    json!({"sessionId":self.provider,"prompt":[{"type":"text","text":text}]}),
                    self.limits.prompt,
                )?;
            }
            Control::Cancel(context) => {
                self.require_scope(&context)?;
                if self.active.as_ref().is_none_or(|active| *active == context) {
                    self.stop(true)?;
                }
            }
            Control::Dispose => {
                self.stop(false)?;
            }
            Control::Permission(context, id, choice) => {
                let key = serde_json::to_string(&id).map_err(|_| "无效权限请求标识")?;
                if let Some(permission) = self.permissions.remove(&key) {
                    let allowed = self.stopping.is_none()
                        && self.active.as_ref() == Some(&context)
                        && permission.context == context
                        && choice
                            .as_ref()
                            .is_some_and(|id| permission.options.contains(id));
                    self.permission_response(
                        permission.id,
                        if allowed { choice.as_deref() } else { None },
                    )?;
                }
            }
            Control::Shutdown => unreachable!(),
        }
        Ok(())
    }
    fn require_scope(&self, context: &ConnectorContext) -> Result<(), String> {
        if self
            .context
            .as_ref()
            .is_some_and(|current| same_session(current, context))
        {
            Ok(())
        } else {
            Err("Agent 工区代次、会话或授权已改变".into())
        }
    }
    fn permission_response(&mut self, id: Value, choice: Option<&str>) -> Result<(), String> {
        let outcome = match choice {
            Some(id) => json!({"outcome":"selected","optionId":id}),
            None => json!({"outcome":"cancelled"}),
        };
        self.send(json!({"jsonrpc":"2.0","id":id,"result":{"outcome":outcome}}))
    }
    fn stop(&mut self, cancelled: bool) -> Result<(), String> {
        if self.process.is_none() {
            return Ok(());
        }
        if let Some(stopping) = self.stopping.as_mut() {
            stopping.cancelled |= cancelled;
            return Ok(());
        }
        for (_, permission) in std::mem::take(&mut self.permissions) {
            let _ = self.permission_response(permission.id, None);
        }
        if self.active.is_some() {
            if let Some(provider) = &self.provider {
                let _=self.send(json!({"jsonrpc":"2.0","method":"session/cancel","params":{"sessionId":provider}}));
            }
        }
        self.stopping = Some(Stopping {
            started: Instant::now(),
            term_sent: false,
            kill_sent: false,
            cancelled,
        });
        if self.active.is_none() {
            self.reap("Agent 连接已释放");
        }
        Ok(())
    }
    fn reap(&mut self, reason: &str) {
        if let Some(mut process) = self.process.take() {
            if let Err(error) = process.finish() {
                self.process = Some(process);
                if let Some(context) = self.active.clone().or_else(|| self.context.clone()) {
                    let _ = self.emit_for(
                        context,
                        ConnectorEventKind::Failed {
                            code: "stop_failed".into(),
                            message: error,
                        },
                    );
                }
                return;
            }
        }
        let cancelled = self.stopping.take().is_some_and(|value| value.cancelled);
        self.pending.clear();
        self.permissions.clear();
        self.deferred_session = None;
        self.confirmed = false;
        self.initialized = false;
        self.provider = None;
        if cancelled {
            self.emit(ConnectorEventKind::Cancelled);
        }
        self.emit(ConnectorEventKind::Disconnected {
            reason: reason.into(),
        });
        self.active = None;
    }
    fn fail(&mut self, code: &str, message: &str) {
        let code = if message.starts_with("not_installed:") {
            "not_installed"
        } else {
            code
        };
        let message = if let Some(diagnostic) = self.diagnostic {
            format!("{message}；{diagnostic}")
        } else {
            message.to_owned()
        };
        if let Some(context) = self.active.clone().or_else(|| self.context.clone()) {
            let _ = self.emit_for(
                context,
                ConnectorEventKind::Failed {
                    code: code.into(),
                    message,
                },
            );
        }
        self.reap("Agent 连接因错误释放");
    }
    fn check_time(&mut self) {
        if self
            .shutdown_deadline
            .is_some_and(|deadline| Instant::now() >= deadline)
        {
            self.reap("Agent 已在取消等待后回收");
            return;
        }
        if let Some(process) = &mut self.process {
            if process.last_tracking.elapsed() >= Duration::from_millis(250) {
                let _ = process.capture_descendants();
            }
            match process.child.try_wait() {
                Ok(Some(status)) => {
                    process.leader_reaped = true;
                    if self.stopping.is_some() {
                        self.reap("Agent 已退出");
                    } else {
                        self.fail(
                            "process_exited",
                            &format!("Agent 进程意外退出（{}）", status.code().unwrap_or(-1)),
                        );
                    }
                    return;
                }
                Err(_) => {
                    self.fail("process_error", "无法确认 Agent 进程状态");
                    return;
                }
                _ => {}
            }
        }
        if let Some(stopping) = &mut self.stopping {
            let elapsed = stopping.started.elapsed();
            if elapsed >= self.limits.cancel + self.limits.terminate && !stopping.kill_sent {
                if let Some(process) = &mut self.process {
                    process.signal(FORCE_KILL);
                }
                stopping.kill_sent = true;
            } else if elapsed >= self.limits.cancel && !stopping.term_sent {
                if let Some(process) = &mut self.process {
                    process.signal(TERMINATE);
                }
                stopping.term_sent = true;
            }
            if elapsed >= self.limits.cancel + self.limits.terminate + Duration::from_secs(2) {
                self.reap("Agent 已强制回收");
            }
            return;
        }
        if let Some(pending) = self
            .pending
            .values()
            .find(|request| Instant::now() >= request.deadline)
        {
            let stage = match pending.kind {
                RequestKind::Initialize => "initialize",
                RequestKind::NewSession => "session/new",
                RequestKind::Prompt => "session/prompt",
            };
            self.fail("timeout", &format!("Agent {stage} 超时；连接已回收"));
        }
    }
    fn frame(&mut self, line: &[u8]) -> Result<(), String> {
        let value: Value = serde_json::from_slice(line)
            .map_err(|_| "ACP stdout 包含无效 JSON；未将原始内容作为聊天或日志保存")?;
        if value.get("jsonrpc").and_then(Value::as_str) != Some("2.0") || !value.is_object() {
            return Err("ACP 消息不是 JSON-RPC 2.0 对象".into());
        }
        if let Some(method) = value.get("method").and_then(Value::as_str) {
            if let Some(id) = value.get("id") {
                if !id.is_string() && !id.is_u64() && !id.is_i64() {
                    return Err("ACP request id 无效".into());
                }
                return self.incoming_request(
                    method,
                    id.clone(),
                    value.get("params").unwrap_or(&Value::Null),
                );
            }
            if method == "session/update" {
                self.update(value.get("params").unwrap_or(&Value::Null))?;
            }
            return Ok(());
        }
        let Some(id) = value.get("id").and_then(Value::as_u64) else {
            return Ok(());
        };
        let Some(request) = self.pending.remove(&id) else {
            return Ok(());
        };
        if value.get("result").is_some() == value.get("error").is_some() {
            return Err("ACP 响应必须且只能包含 result 或 error".into());
        }
        if let Some(error) = value.get("error") {
            let code = error.get("code").and_then(Value::as_i64).unwrap_or(-32603);
            self.fail(
                if code == -32000 {
                    "auth_required"
                } else {
                    "rpc_error"
                },
                &format!("Agent 返回 RPC 错误 {code}；原始错误内容已隐藏"),
            );
            return Ok(());
        }
        let result = &value["result"];
        match request.kind {
            RequestKind::Initialize => {
                let version = result
                    .get("protocolVersion")
                    .and_then(Value::as_u64)
                    .ok_or("initialize 缺少协议版本")?;
                if version != 1 {
                    return Err(format!("不支持 ACP 协议版本 {version}"));
                }
                self.initialized = true;
                // Preserve the actual negotiated response, including capabilities/auth method names.
                self.emit_for(
                    request.context,
                    ConnectorEventKind::Initialized {
                        protocol_version: version,
                        capabilities: result.clone(),
                    },
                );
                if let Some(context) = self.deferred_session.take() {
                    self.control(Control::NewSession(context))?;
                }
            }
            RequestKind::NewSession => {
                let provider = result
                    .get("sessionId")
                    .and_then(Value::as_str)
                    .filter(|id| !id.is_empty() && id.len() <= 512)
                    .ok_or("session/new 缺少有效 sessionId")?
                    .to_owned();
                self.provider = Some(provider.clone());
                self.confirmed = false;
                self.emit_for(
                    request.context,
                    ConnectorEventKind::SessionCreated {
                        provider_session_id: provider,
                    },
                );
            }
            RequestKind::Prompt => {
                if let Some((code, message)) = typed_provider_failure(result) {
                    self.fail(code, message);
                    return Ok(());
                }
                if self.stopping.is_some() {
                    self.reap("Agent 取消后已回收");
                    return Ok(());
                }
                let stop = result
                    .get("stopReason")
                    .and_then(Value::as_str)
                    .ok_or("prompt 响应缺少 stopReason")?
                    .to_owned();
                if ![
                    "end_turn",
                    "max_tokens",
                    "max_turn_requests",
                    "refusal",
                    "cancelled",
                ]
                .contains(&stop.as_str())
                {
                    return Err("prompt 返回未知 stopReason".into());
                }
                for (_, permission) in std::mem::take(&mut self.permissions) {
                    let _ = self.permission_response(permission.id, None);
                }
                if stop == "cancelled" {
                    self.stopping = Some(Stopping {
                        started: Instant::now(),
                        term_sent: false,
                        kill_sent: false,
                        cancelled: true,
                    });
                    self.reap("Agent 已取消并回收");
                } else if self.reply_bytes == 0 && self.diagnostic.is_some() {
                    let auth = self
                        .diagnostic
                        .is_some_and(|message| message.contains("认证"));
                    self.fail(
                        if auth { "auth_required" } else { "agent_error" },
                        "Agent 未返回可见回复且报告异常；不能将 end_turn 当成任务成功",
                    );
                } else {
                    self.emit_for(
                        request.context,
                        ConnectorEventKind::Completed { stop_reason: stop },
                    );
                    self.active = None;
                }
            }
        }
        Ok(())
    }
    fn incoming_request(&mut self, method: &str, id: Value, params: &Value) -> Result<(), String> {
        if method != "session/request_permission" {
            return self.send(json!({"jsonrpc":"2.0","id":id,"error":{"code":-32601,"message":"Method not found"}}));
        }
        if self.stopping.is_some()
            || self.active.is_none()
            || params.get("sessionId").and_then(Value::as_str) != self.provider.as_deref()
        {
            return self.permission_response(id, None);
        }
        let options = params
            .get("options")
            .and_then(Value::as_array)
            .ok_or("权限请求缺少 options")?;
        if options.len() > 32 {
            return Err("权限选项数量超出上限".into());
        }
        let mut mapped = Vec::new();
        let mut allowed = HashSet::new();
        for option in options {
            let option_id = option
                .get("optionId")
                .and_then(Value::as_str)
                .filter(|id| !id.is_empty() && id.len() < 512)
                .ok_or("权限选项 id 无效")?;
            let kind = option
                .get("kind")
                .and_then(Value::as_str)
                .ok_or("权限选项 kind 无效")?;
            if !["allow_once", "allow_always", "reject_once", "reject_always"].contains(&kind) {
                return Err("权限选项 kind 未知".into());
            }
            allowed.insert(option_id.to_owned());
            mapped.push(PermissionOption {
                id: option_id.into(),
                name: option
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or("Permission")
                    .chars()
                    .take(160)
                    .collect(),
                kind: kind.into(),
            });
        }
        let title = params
            .get("toolCall")
            .and_then(|tool| tool.get("title"))
            .and_then(Value::as_str)
            .unwrap_or("Agent 请求工具权限")
            .chars()
            .take(500)
            .collect();
        let key = serde_json::to_string(&id).map_err(|_| "权限 id 无效")?;
        if self.permissions.contains_key(&key) || self.permissions.len() >= 32 {
            return Err("权限请求 id 重复或待处理数量超限".into());
        }
        let context = self.active.clone().unwrap();
        self.permissions.insert(
            key,
            Permission {
                id: id.clone(),
                context: context.clone(),
                options: allowed,
            },
        );
        if !self.emit_for(
            context,
            ConnectorEventKind::PermissionRequest {
                request_id: id,
                title,
                options: mapped,
            },
        ) {
            return Err("权限事件队列已满".into());
        }
        Ok(())
    }
    fn update(&mut self, params: &Value) -> Result<(), String> {
        if self.stopping.is_some()
            || self.active.is_none()
            || params.get("sessionId").and_then(Value::as_str) != self.provider.as_deref()
        {
            return Ok(());
        }
        let update = &params["update"];
        if let Some((code, message)) = typed_provider_failure(update) {
            self.fail(code, message);
            return Ok(());
        }
        match update.get("sessionUpdate").and_then(Value::as_str) {
            Some("agent_message_chunk") => {
                if update["content"]["type"] == "text" {
                    if let Some(text) = update["content"]["text"].as_str() {
                        self.reply_bytes += text.len();
                        if self.reply_bytes > MAX_REPLY {
                            return Err("Agent 本轮文本超出 1 MiB 上限".into());
                        }
                        self.emit(ConnectorEventKind::TextDelta {
                            text: text.to_owned(),
                        });
                    }
                }
            }
            Some("tool_call" | "tool_call_update") => {
                let id = update
                    .get("toolCallId")
                    .and_then(Value::as_str)
                    .filter(|id| id.len() < 512)
                    .ok_or("工具通知缺少有效 id")?;
                self.emit(ConnectorEventKind::ToolStatus {
                    tool_call_id: id.into(),
                    title: update
                        .get("title")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .chars()
                        .take(500)
                        .collect(),
                    status: update
                        .get("status")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .chars()
                        .take(64)
                        .collect(),
                });
            }
            _ => {} // Thought/tool metadata and future notifications are not executable instructions.
        }
        Ok(())
    }
}
// Both pinned adapters support this opt-in terminal-failure extension. It
// prevents an upstream rejection returned alongside end_turn from looking like
// a successful task. Retain only a fixed diagnostic, never provider URLs/tokens.
fn typed_provider_failure(value: &Value) -> Option<(&'static str, &'static str)> {
    let failure = value.pointer("/_meta/jetbrains/air/sessionFailure")?;
    if failure.get("severity").and_then(Value::as_str) != Some("error") {
        return None;
    }
    let title = failure
        .get("title")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_ascii_lowercase();
    if title.contains("native codex clients only") {
        return Some((
            "provider_client_rejected",
            "模型服务只允许其指定客户端，拒绝了当前 ACP 连接；请由服务配置方允许 ACP 客户端后重试",
        ));
    }
    match failure.get("category").and_then(Value::as_str) {
        Some("authentication" | "auth_required") => Some((
            "auth_required",
            "Agent 认证未通过；请检查现有 Provider 登录或模型配置",
        )),
        Some("rate_limit" | "rate_limited") => {
            Some(("rate_limited", "Agent 模型服务限流；本轮没有成功完成"))
        }
        _ => Some((
            "provider_error",
            "Agent 模型服务报告失败；本轮没有成功完成（原始诊断已隐藏）",
        )),
    }
}

fn same_session(a: &ConnectorContext, b: &ConnectorContext) -> bool {
    a.workspace_id == b.workspace_id
        && a.root == b.root
        && a.generation == b.generation
        && a.permission_epoch == b.permission_epoch
        && a.runtime_session_id == b.runtime_session_id
}

#[cfg(test)]
#[path = "native_tests.rs"]
mod tests;
