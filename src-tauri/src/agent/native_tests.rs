use super::*;
use std::fs;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::process::Child;
use std::sync::{Mutex, MutexGuard};

// One active provider is the supported app topology. These tests also inspect
// the system process table and intentionally shorten timeouts; serialize their
// OS fixtures so parallel interpreter launches don't turn scheduling into the
// behavior under test. Independent non-transport tests may still run in parallel.
static PROCESS_FIXTURES: Mutex<()> = Mutex::new(());

struct Fixture {
    root: PathBuf,
    command: PathBuf,
    _serial: MutexGuard<'static, ()>,
}
impl Fixture {
    fn new(mode: &str) -> Self {
        let serial = PROCESS_FIXTURES
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let root =
            std::env::temp_dir().join(format!("pixel native 中文 acp {}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let root = root.canonicalize().unwrap();
        #[cfg(unix)]
        let command = root.join("hermes-fixture");
        #[cfg(windows)]
        let command = root.join("hermes-fixture.py");
        let script = format!(
            "#!/usr/bin/python3\nMODE = {}\n",
            serde_json::to_string(mode).unwrap()
        ) + r#"
import json, os, signal, subprocess, sys, time
if os.name=='nt':
    import msvcrt
    msvcrt.setmode(sys.stdout.fileno(), os.O_BINARY)
if '--version' in sys.argv:
    print('Hermes Agent v0.21.3 (fixture)'); sys.exit(0)
if '--check' in sys.argv:
    print('Hermes ACP check OK'); sys.exit(0)
ROOT = os.path.dirname(os.path.abspath(__file__))
open(ROOT + '/pid', 'w').write(str(os.getpid()))
if MODE in ('cancel_timeout', 'cancel_detached', 'drop'):
    signal.signal(signal.SIGTERM, signal.SIG_IGN)
def send(value):
    sys.stdout.write(json.dumps(value,ensure_ascii=False)+'\n');sys.stdout.flush()
def result(id_,value):send({'jsonrpc':'2.0','id':id_,'result':value})
def update(text):send({'jsonrpc':'2.0','method':'session/update','params':{'sessionId':'provider-fixture','update':{'sessionUpdate':'agent_message_chunk','content':{'type':'text','text':text}}}})
turn=0
pending=None
for line in sys.stdin:
    request=json.loads(line)
    with open(ROOT+'/wire.jsonl','a') as log:log.write(json.dumps(request)+'\n')
    method=request.get('method')
    if method=='initialize':
        if MODE=='pollution':print('INFO stdout must not contain logs',flush=True);continue
        if MODE=='timeout':continue
        result(request['id'],{'protocolVersion':1,'agentCapabilities':{'loadSession':True},'agentInfo':{'name':'hermes-agent','version':'0.21.3'}})
    elif method=='session/new':result(request['id'],{'sessionId':'provider-fixture'})
    elif method=='session/prompt':
        pending=request['id'];turn+=1
        if MODE in ('cancel_timeout','cancel_detached','drop'):
            child=subprocess.Popen([sys.executable,'-c','import signal,time; signal.signal(signal.SIGTERM,signal.SIG_IGN);time.sleep(60)'],start_new_session=MODE=='cancel_detached',creationflags=(subprocess.CREATE_NEW_PROCESS_GROUP if os.name=='nt' and MODE=='cancel_detached' else 0))
            open(ROOT+'/child-pid','w').write(str(child.pid));continue
        if MODE=='cancel':update('before cancel');continue
        if MODE=='permission':
            # Same numeric ID as the outgoing prompt tests separate duplex ID maps.
            send({'jsonrpc':'2.0','id':pending,'method':'session/request_permission','params':{'sessionId':'provider-fixture','toolCall':{'toolCallId':'edit-1','title':'Fixture edit','kind':'edit','status':'pending'},'options':[{'optionId':'opaque-once','kind':'allow_once','name':'Allow once'},{'optionId':'deny','kind':'reject_once','name':'Deny'}]}});continue
        if MODE=='typed_failure':
            result(pending,{'stopReason':'end_turn','_meta':{'jetbrains':{'air':{'version':1,'sessionFailure':{'severity':'error','category':'service','title':'native codex clients only; PRIVATE_PROVIDER_DETAIL'}}}}});continue
        if MODE=='unsupported':
            for n,method_name in enumerate(['fs/read_text_file','fs/write_text_file','terminal/create','terminal/output','terminal/kill','terminal/wait_for_exit','terminal/release']):
                send({'jsonrpc':'2.0','id':'unsupported-'+str(n),'method':method_name,'params':{'sessionId':'provider-fixture','path':ROOT+'/forbidden.txt','content':'must-not-write','command':'false'}})
        sys.stderr.write('Authorization: Bearer SYNTHETIC_SECRET\nPrompt on session fixture: PRIVATE_PROMPT\n');sys.stderr.flush()
        wire=json.dumps({'jsonrpc':'2.0','method':'session/update','params':{'sessionId':'provider-fixture','update':{'sessionUpdate':'agent_message_chunk','content':{'type':'text','text':'中文 turn '+str(turn)}}}},ensure_ascii=False).encode()+b'\n'
        cut=wire.index('中'.encode())+1
        os.write(1,wire[:cut]);time.sleep(.01);os.write(1,wire[cut:])
        result(pending,{'stopReason':'end_turn'})
    elif method=='session/cancel' and MODE=='cancel':
        update('late text ignored');result(pending,{'stopReason':'cancelled'})
    elif method is None and MODE=='permission':
        with open(ROOT+'/permission-result.json','w') as result_file:json.dump(request,result_file)
        update('permission answered');result(pending,{'stopReason':'end_turn'})
"#;
        fs::write(&command, script).unwrap();
        #[cfg(unix)]
        fs::set_permissions(&command, fs::Permissions::from_mode(0o755)).unwrap();
        Self {
            root,
            command,
            _serial: serial,
        }
    }
    fn descriptor(&self) -> AgentDescriptor {
        AgentDescriptor {
            id: "hermes".into(),
            provider: None,
            name: "Hermes fixture".into(),
            transport: "stdio".into(),
            command: self.command.to_string_lossy().into_owned(),
            args: vec!["acp".into()],
            env: Default::default(),
            cwd: String::new(),
            probe_status: "unknown".into(),
        }
    }
    fn context(&self) -> ConnectorContext {
        ConnectorContext {
            workspace_id: "a".into(),
            root: self.root.to_string_lossy().into_owned(),
            generation: "generation-a".into(),
            permission_epoch: 7,
            runtime_session_id: "runtime-a".into(),
            run_id: "run-1".into(),
            module_id: None,
        }
    }
    fn connector(&self) -> NativeAcpConnector {
        NativeAcpConnector::with_limits(
            self.descriptor(),
            Limits {
                initialize: Duration::from_secs(3),
                session: Duration::from_secs(2),
                prompt: Duration::from_secs(4),
                cancel: Duration::from_millis(80),
                terminate: Duration::from_millis(80),
            },
        )
    }
    fn wire(&self) -> Vec<Value> {
        fs::read_to_string(self.root.join("wire.jsonl"))
            .unwrap_or_default()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn until(
    connector: &mut NativeAcpConnector,
    predicate: impl Fn(&ConnectorEvent) -> bool,
) -> Vec<ConnectorEvent> {
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut all = Vec::new();
    loop {
        let events = connector.poll_events().unwrap();
        let complete = events.iter().any(&predicate);
        all.extend(events);
        if complete {
            return all;
        }
        assert!(Instant::now() < deadline, "events: {:?}", all);
        thread::sleep(Duration::from_millis(10));
    }
}
fn connect(fixture: &Fixture, connector: &mut NativeAcpConnector, confirm: bool) {
    let context = fixture.context();
    connector.initialize(&context).unwrap();
    connector.new_session(&context).unwrap();
    let events = until(connector, |event| {
        matches!(event.kind, ConnectorEventKind::SessionCreated { .. })
    });
    assert!(events.iter().any(|event| matches!(
        event.kind,
        ConnectorEventKind::Initialized {
            protocol_version: 1,
            ..
        }
    )));
    if confirm {
        connector
            .confirm_session_persisted(&context, "provider-fixture")
            .unwrap();
    }
}
fn prompt(connector: &mut NativeAcpConnector, context: ConnectorContext) {
    connector
        .prompt(ConnectorPrompt {
            context,
            text: "Synthetic safe fixture prompt".into(),
            modules: vec![super::super::ModuleContext {
                id: "private".into(),
                module_type: crate::kernel::ModuleType::Document,
                title: "private".into(),
                content: "PRIVATE_MODULE_MUST_NOT_BE_SENT".into(),
                revision: None,
            }],
        })
        .unwrap();
}

#[test]
fn native_two_turns_share_provider_and_fragmented_utf8_without_document_or_stderr_leak() {
    let fixture = Fixture::new("happy");
    let mut connector = fixture.connector();
    connect(&fixture, &mut connector, true);
    let mut context = fixture.context();
    let mut captured = Vec::new();
    for turn in 1..=2 {
        context.run_id = format!("run-{turn}");
        prompt(&mut connector, context.clone());
        let events = until(&mut connector, |event| {
            matches!(event.kind, ConnectorEventKind::Completed { .. })
        });
        assert!(events
            .iter()
            .all(|event| event.context.run_id == context.run_id));
        assert!(events.iter().any(|event|matches!(&event.kind,ConnectorEventKind::TextDelta{text}if text==&format!("中文 turn {turn}"))));
        captured.extend(events);
    }
    connector.shutdown().unwrap();
    let wire = fixture.wire();
    assert_eq!(
        wire.iter().filter(|v| v["method"] == "session/new").count(),
        1
    );
    assert_eq!(
        wire.iter()
            .filter(|v| v["method"] == "session/prompt"
                && v["params"]["sessionId"] == "provider-fixture")
            .count(),
        2
    );
    let serialized = serde_json::to_string(&wire).unwrap();
    assert!(!serialized.contains("PRIVATE_MODULE"));
    let events = serde_json::to_string(&captured).unwrap();
    assert!(!events.contains("SYNTHETIC_SECRET"));
    assert!(!events.contains("PRIVATE_PROMPT"));
    assert_eq!(wire[0]["params"]["clientCapabilities"]["terminal"], false);
    assert_eq!(
        wire[0]["params"]["clientCapabilities"]["fs"]["writeTextFile"],
        false
    );
}

#[test]
fn native_requires_host_persistence_ack_before_any_prompt() {
    let fixture = Fixture::new("happy");
    let mut connector = fixture.connector();
    connect(&fixture, &mut connector, false);
    prompt(&mut connector, fixture.context());
    let events = until(&mut connector, |event| {
        matches!(event.kind, ConnectorEventKind::Failed { .. })
    });
    assert!(events.iter().any(|event|matches!(&event.kind,ConnectorEventKind::Failed{message,..}if message.contains("持久化"))));
    connector.shutdown().unwrap();
    assert!(!fixture
        .wire()
        .iter()
        .any(|value| value["method"] == "session/prompt"));
}

#[test]
fn native_permission_uses_duplex_request_id_and_rejects_stale_epoch() {
    for stale in [false, true] {
        let fixture = Fixture::new("permission");
        let mut connector = fixture.connector();
        connect(&fixture, &mut connector, true);
        prompt(&mut connector, fixture.context());
        let events = until(&mut connector, |event| {
            matches!(event.kind, ConnectorEventKind::PermissionRequest { .. })
        });
        let (request_id, options) = events
            .iter()
            .find_map(|event| {
                if let ConnectorEventKind::PermissionRequest {
                    request_id,
                    options,
                    ..
                } = &event.kind
                {
                    Some((request_id.clone(), options.clone()))
                } else {
                    None
                }
            })
            .unwrap();
        assert_eq!(options[0].id, "opaque-once");
        let mut context = fixture.context();
        if stale {
            context.permission_epoch += 1;
        }
        connector
            .permission_reply(&context, &request_id, Some("opaque-once"))
            .unwrap();
        until(&mut connector, |event| {
            matches!(event.kind, ConnectorEventKind::Completed { .. })
        });
        connector.shutdown().unwrap();
        let response: Value =
            serde_json::from_slice(&fs::read(fixture.root.join("permission-result.json")).unwrap())
                .unwrap();
        assert_eq!(response["id"], request_id);
        assert_eq!(
            response["result"]["outcome"]["outcome"],
            if stale { "cancelled" } else { "selected" }
        );
    }
}

#[test]
fn native_unsupported_host_requests_return_error_without_host_io() {
    let fixture = Fixture::new("unsupported");
    let mut connector = fixture.connector();
    connect(&fixture, &mut connector, true);
    prompt(&mut connector, fixture.context());
    until(&mut connector, |event| {
        matches!(event.kind, ConnectorEventKind::Completed { .. })
    });
    // Wait for the fixture to consume already-sent replies before killing it.
    let deadline = Instant::now() + Duration::from_secs(1);
    while fixture
        .wire()
        .iter()
        .filter(|value| value.get("error").is_some())
        .count()
        < 7
    {
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(10));
    }
    connector.shutdown().unwrap();
    assert!(!fixture.root.join("forbidden.txt").exists());
    assert_eq!(
        fixture
            .wire()
            .iter()
            .filter(|value| value["error"]["code"] == -32601)
            .count(),
        7
    );
}

#[test]
fn native_cancel_discards_late_text_and_reaps_before_cancelled() {
    let fixture = Fixture::new("cancel");
    let mut connector = fixture.connector();
    connect(&fixture, &mut connector, true);
    prompt(&mut connector, fixture.context());
    until(&mut connector, |event| {
        matches!(event.kind, ConnectorEventKind::TextDelta { .. })
    });
    connector.cancel(&fixture.context()).unwrap();
    let events = until(&mut connector, |event| {
        matches!(event.kind, ConnectorEventKind::Cancelled)
    });
    assert!(!events
        .iter()
        .any(|event| matches!(event.kind, ConnectorEventKind::TextDelta { .. })));
    assert!(fixture
        .wire()
        .iter()
        .any(|value| value["method"] == "session/cancel"
            && !value.as_object().unwrap().contains_key("id")));
    connector.shutdown().unwrap();
    assert!(!running(read_pid(&fixture.root.join("pid"))));
}

#[test]
fn native_shutdown_gives_protocol_cancel_a_grace_window_and_clears_owned_pid() {
    let fixture = Fixture::new("cancel");
    let mut connector = fixture.connector();
    connect(&fixture, &mut connector, true);
    prompt(&mut connector, fixture.context());
    until(&mut connector, |event| {
        matches!(event.kind, ConnectorEventKind::TextDelta { .. })
    });
    let pid = connector.managed_process_id().expect("owned child");
    connector.shutdown().unwrap();
    assert_eq!(connector.managed_process_id(), None);
    assert!(!running(pid));
    assert!(fixture
        .wire()
        .iter()
        .any(|value| value["method"] == "session/cancel"));
    let events = connector.poll_events().unwrap();
    assert!(events
        .iter()
        .any(|event| matches!(event.kind, ConnectorEventKind::Cancelled)));
    assert!(!events
        .iter()
        .any(|event| matches!(event.kind, ConnectorEventKind::TextDelta { .. })));
}
fn read_pid(path: &Path) -> u32 {
    fs::read_to_string(path).unwrap().parse().unwrap()
}
#[cfg(unix)]
fn running(pid: u32) -> bool {
    let output = Command::new("/bin/ps")
        .args(["-p", &pid.to_string(), "-o", "stat="])
        .output()
        .unwrap();
    let status = String::from_utf8_lossy(&output.stdout);
    !status.trim().is_empty() && !status.trim().starts_with('Z')
}
#[cfg(windows)]
fn running(pid: u32) -> bool {
    use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
    use windows_sys::Win32::System::Threading::{
        OpenProcess, WaitForSingleObject, PROCESS_SYNCHRONIZE,
    };
    unsafe {
        let raw = OpenProcess(PROCESS_SYNCHRONIZE, 0, pid);
        if raw.is_null() {
            return false;
        }
        let process = OwnedHandle::from_raw_handle(raw);
        WaitForSingleObject(process.as_raw_handle(), 0)
            == windows_sys::Win32::Foundation::WAIT_TIMEOUT
    }
}
fn unrelated_process() -> Child {
    #[cfg(unix)]
    {
        Command::new("/bin/sleep").arg("10").spawn().unwrap()
    }
    #[cfg(windows)]
    {
        Command::new(std::env::var_os("ATRIO_TEST_PYTHON").expect("test Python"))
            .args(["-c", "import time;time.sleep(10)"])
            .spawn()
            .unwrap()
    }
}

#[test]
fn native_cancel_timeout_and_drop_reap_owned_group_without_harming_unrelated_process() {
    for mode in ["cancel_timeout", "cancel_detached", "drop"] {
        let fixture = Fixture::new(mode);
        let mut connector = fixture.connector();
        connect(&fixture, &mut connector, true);
        prompt(&mut connector, fixture.context());
        let deadline = Instant::now() + Duration::from_secs(2);
        while !fixture.root.join("child-pid").exists() {
            assert!(Instant::now() < deadline);
            thread::sleep(Duration::from_millis(10));
        }
        let parent = read_pid(&fixture.root.join("pid"));
        let child = read_pid(&fixture.root.join("child-pid"));
        let mut unrelated = unrelated_process();
        if mode != "drop" {
            connector.cancel(&fixture.context()).unwrap();
            until(&mut connector, |event| {
                matches!(event.kind, ConnectorEventKind::Cancelled)
            });
            connector.shutdown().unwrap();
        } else {
            drop(connector);
        }
        assert!(!running(parent));
        assert!(!running(child));
        assert!(unrelated.try_wait().unwrap().is_none());
        unrelated.kill().unwrap();
        unrelated.wait().unwrap();
    }
}

#[test]
fn native_protocol_pollution_and_initialize_timeout_have_distinct_errors() {
    for (mode, code) in [("pollution", "protocol_error"), ("timeout", "timeout")] {
        let fixture = Fixture::new(mode);
        let mut connector = fixture.connector();
        connector.initialize(&fixture.context()).unwrap();
        let events = until(&mut connector, |event| {
            matches!(event.kind, ConnectorEventKind::Failed { .. })
        });
        assert!(events.iter().any(
            |event| matches!(&event.kind,ConnectorEventKind::Failed{code:actual,..}if actual==code)
        ));
        assert!(!events
            .iter()
            .any(|event| matches!(event.kind, ConnectorEventKind::TextDelta { .. })));
        connector.shutdown().unwrap();
        assert!(!running(read_pid(&fixture.root.join("pid"))));
    }
}

#[test]
fn native_probe_uses_version_and_dependency_check_without_starting_session() {
    let fixture = Fixture::new("happy");
    let probe = probe_hermes(fixture.command.to_str()).unwrap();
    assert!(probe.version.contains("0.21.3"));
    assert!(probe.acp_available);
    assert!(!fixture.root.join("pid").exists());
    assert!(fixture.wire().is_empty());
}

#[test]
fn native_finished_worker_is_not_silently_polled_as_connecting_forever() {
    let fixture = Fixture::new("happy");
    let mut connector = fixture.connector();
    // A worker can exit before initialize assigns context, so there need not be a final event.
    connector.commands.send(Control::Shutdown).unwrap();
    let deadline = Instant::now() + Duration::from_secs(1);
    while !connector.worker.as_ref().unwrap().is_finished() {
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(10));
    }
    assert!(connector
        .poll_events()
        .unwrap_err()
        .contains("连接线程已经退出"));
    connector.shutdown().unwrap();
    assert!(connector.poll_events().unwrap().is_empty());
}

#[test]
fn native_injects_only_host_mcp_and_preserves_legacy_hermes() {
    let fixture = Fixture::new("happy");
    let servers = vec![
        json!({"name":"workspace","command":fixture.command.to_string_lossy(),"args":["synthetic-mcp"],"env":[{"name":"SCOPE","value":"synthetic-only"}]}),
    ];
    let mut connector = NativeAcpConnector::with_mcp(fixture.descriptor(), servers.clone());
    connect(&fixture, &mut connector, true);
    prompt(&mut connector, fixture.context());
    until(&mut connector, |e| {
        matches!(e.kind, ConnectorEventKind::Completed { .. })
    });
    connector.shutdown().unwrap();
    let wire = fixture.wire();
    let session = wire.iter().find(|v| v["method"] == "session/new").unwrap();
    assert_eq!(session["params"]["mcpServers"], json!(servers));
    assert!(session["params"].get("_meta").is_none());
}
#[test]
fn native_rejects_non_stdio_or_unstructured_mcp_configuration() {
    assert!(validate_mcp_servers(&[
        json!({"name":"remote","type":"http","url":"https://invalid"})
    ])
    .is_err());
    assert!(validate_mcp_servers(&[
        json!({"name":"relative","command":"python3","args":[],"env":[]})
    ])
    .is_err());
    assert!(validate_mcp_servers(&[json!({"name":"local","command":"/usr/bin/python3","args":[],"env":[{"name":"x","value":123}]})]).is_err());
}

#[test]
fn native_typed_provider_failure_cannot_be_reported_as_success() {
    let value = json!({"stopReason":"end_turn", "_meta":{"jetbrains":{"air":{"version":1,"sessionFailure":{"severity":"error","category":"provider_error","title":"unexpected status 403 Forbidden: native codex clients only, private endpoint"}}}}});
    let (code, message) = typed_provider_failure(&value).unwrap();
    assert_eq!(code, "provider_client_rejected");
    assert!(!message.contains("private endpoint"));
    let warning = json!({"_meta":{"jetbrains":{"air":{"sessionFailure":{"severity":"warning","title":"Reconnecting"}}}}});
    assert!(typed_provider_failure(&warning).is_none());
}

#[test]
fn native_end_turn_with_typed_failure_emits_failed_without_completed_or_private_detail() {
    let fixture = Fixture::new("typed_failure");
    let mut connector = fixture.connector();
    connect(&fixture, &mut connector, true);
    prompt(&mut connector, fixture.context());
    let events = until(&mut connector, |e| {
        matches!(e.kind, ConnectorEventKind::Failed { .. })
    });
    assert!(events.iter().any(|e| matches!(&e.kind, ConnectorEventKind::Failed { code, .. } if code == "provider_client_rejected")));
    assert!(!events
        .iter()
        .any(|e| matches!(e.kind, ConnectorEventKind::Completed { .. })));
    assert!(!serde_json::to_string(&events)
        .unwrap()
        .contains("PRIVATE_PROVIDER_DETAIL"));
    connector.shutdown().unwrap();
}

#[test]
fn legacy_descriptor_infers_hermes_and_explicit_provider_selects_only_known_entry() {
    let fixture = Fixture::new("happy");
    let mut descriptor = fixture.descriptor();
    assert_eq!(
        NativeProvider::from_descriptor(&descriptor).unwrap(),
        NativeProvider::Hermes
    );
    descriptor.provider = Some("codex".into());
    assert_eq!(
        NativeProvider::from_descriptor(&descriptor).unwrap(),
        NativeProvider::Codex
    );
    descriptor.provider = Some("unknown".into());
    assert!(NativeProvider::from_descriptor(&descriptor).is_err());
}

#[cfg(windows)]
#[test]
fn windows_stdin_backpressure_is_bounded_and_reaps_job() {
    let fixture = Fixture::new("happy");
    let python = PathBuf::from(std::env::var_os("ATRIO_TEST_PYTHON").expect("test Python"));
    let args = vec!["-c".into(), "import time;time.sleep(60)".into()];
    let mut process =
        ManagedProcess::spawn(process_command(&python, &args, &fixture.root).unwrap()).unwrap();
    let pid = process.group;
    let start = Instant::now();
    assert!(process
        .send(&json!({"payload":"x".repeat(1024 * 1024)}))
        .unwrap_err()
        .contains("超时"));
    process.finish().unwrap();
    assert!(start.elapsed() < Duration::from_secs(3));
    assert!(!running(pid));
}

#[cfg(windows)]
#[test]
fn windows_leader_exit_cannot_leave_descendant_or_reader_alive() {
    let fixture = Fixture::new("happy");
    let python = PathBuf::from(std::env::var_os("ATRIO_TEST_PYTHON").expect("test Python"));
    let script = "import subprocess,sys,time; p=subprocess.Popen([sys.executable,'-c','import time;time.sleep(60)'],creationflags=subprocess.CREATE_NEW_PROCESS_GROUP);open('orphan-pid','w').write(str(p.pid))";
    let args = vec!["-c".into(), script.into()];
    let mut process =
        ManagedProcess::spawn(process_command(&python, &args, &fixture.root).unwrap()).unwrap();
    let deadline = Instant::now() + Duration::from_secs(3);
    while process.child.try_wait().unwrap().is_none() {
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(10));
    }
    process.leader_reaped = true;
    let descendant = read_pid(&fixture.root.join("orphan-pid"));
    assert!(running(descendant));
    process.finish().unwrap();
    assert!(!running(descendant));
    assert!(process.readers_finished_for_test());
}

#[cfg(windows)]
#[test]
fn windows_child_environment_is_allowlisted() {
    let node = resolve_program("node", "node").unwrap();
    let command = process_command(&node, &[], &std::env::temp_dir()).unwrap();
    let keys = command
        .get_envs()
        .map(|(key, _)| key.to_string_lossy().to_ascii_uppercase())
        .collect::<HashSet<_>>();
    assert!(keys.contains("SYSTEMROOT"));
    assert!(keys.contains("USERPROFILE"));
    assert!(keys.contains("TEMP"));
    assert!(keys.contains("PATH"));
    assert!(!keys.contains("ANTHROPIC_API_KEY"));
    assert!(!keys.contains("OPENAI_API_KEY"));
    assert!(!keys.contains("CODEX_HOME"));
}
