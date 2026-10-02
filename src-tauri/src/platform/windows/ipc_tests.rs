use super::*;
use serde_json::json;

fn request() -> ToolRequest {
    ToolRequest {
        session_token: "session".into(),
        run_scope: "turn-1".into(),
        tool: "workspace_list_modules".into(),
        args: json!({}),
        request_id: "r1".into(),
    }
}

fn exchange_raw(endpoint: &Path, bytes: &[u8]) -> ToolResponse {
    let pipe = open_client(endpoint, SERVER_TIMEOUT).unwrap();
    let stop = AtomicBool::new(false);
    // An over-limit peer can be disconnected as soon as the limit is reached.
    let _ = write_all(&pipe, bytes, &stop, Instant::now() + SERVER_TIMEOUT);
    let response = read_line(&pipe, &stop, Instant::now() + SERVER_TIMEOUT).unwrap();
    serde_json::from_slice(&response).unwrap()
}

#[test]
fn private_pipe_preserves_host_token_and_run_scope_authorization() {
    let server = start(Arc::new(|request| {
        if request.session_token != "session" {
            return Err("invalid session".into());
        }
        if request.run_scope != "turn-1" {
            return Err("stale scope".into());
        }
        Ok(json!({"status":"ok", "args": request.args}))
    }))
    .unwrap();
    let mut call = request();
    assert_eq!(forward(server.path(), &call).unwrap()["status"], "ok");
    call.session_token = "wrong".into();
    assert_eq!(
        forward(server.path(), &call).unwrap_err(),
        "invalid session"
    );
    call.session_token = "session".into();
    call.run_scope = "turn-0".into();
    assert_eq!(forward(server.path(), &call).unwrap_err(), "stale scope");
    let name = server.path().to_str().unwrap();
    assert!(uuid::Uuid::parse_str(name.strip_prefix(PIPE_PREFIX).unwrap()).is_ok());
    assert!(!name.contains("session") && !name.contains("turn-1"));
}

#[test]
fn private_pipe_acl_has_only_the_current_user() {
    use windows_sys::Win32::Security::Authorization::{GetSecurityInfo, SE_FILE_OBJECT};
    use windows_sys::Win32::Security::{
        AclSizeInformation, GetAce, GetAclInformation, ACCESS_ALLOWED_ACE, ACL_SIZE_INFORMATION,
        DACL_SECURITY_INFORMATION,
    };
    let server = start(Arc::new(|_| Ok(Value::Null))).unwrap();
    let pipe = open_client(server.path(), SERVER_TIMEOUT).unwrap();
    let mut dacl = null_mut();
    let mut descriptor = null_mut();
    let status = unsafe {
        GetSecurityInfo(
            pipe.0,
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION,
            null_mut(),
            null_mut(),
            &mut dacl,
            null_mut(),
            &mut descriptor,
        )
    };
    assert_eq!(status, 0);
    let _descriptor = LocalAllocation(descriptor);
    assert!(!dacl.is_null(), "a NULL DACL would allow everyone");
    let mut info: ACL_SIZE_INFORMATION = unsafe { std::mem::zeroed() };
    assert_ne!(
        unsafe {
            GetAclInformation(
                dacl,
                (&mut info as *mut ACL_SIZE_INFORMATION).cast(),
                size_of::<ACL_SIZE_INFORMATION>() as u32,
                AclSizeInformation,
            )
        },
        0
    );
    assert_eq!(
        info.AceCount, 1,
        "no Everyone/Authenticated Users/group ACEs"
    );
    let mut ace = null_mut();
    assert_ne!(unsafe { GetAce(dacl, 0, &mut ace) }, 0);
    let ace = unsafe { &*ace.cast::<ACCESS_ALLOWED_ACE>() };
    assert_eq!(ace.Header.AceType, 0, "ACCESS_ALLOWED_ACE_TYPE");
    let sid = (&ace.SidStart as *const u32).cast_mut().cast();
    let mut sid_text = null_mut();
    assert_ne!(unsafe { ConvertSidToStringSidW(sid, &mut sid_text) }, 0);
    let _sid_text = LocalAllocation(sid_text.cast());
    let mut len = 0;
    unsafe {
        while *sid_text.add(len) != 0 {
            len += 1;
        }
    }
    let actual = unsafe { String::from_utf16_lossy(std::slice::from_raw_parts(sid_text, len)) };
    assert_eq!(actual, current_user_sid().unwrap());
}

#[test]
fn malformed_unknown_fields_and_overlong_fields_never_reach_host() {
    let calls = Arc::new(AtomicUsize::new(0));
    let count = calls.clone();
    let server = start(Arc::new(move |_| {
        count.fetch_add(1, Ordering::AcqRel);
        Ok(Value::Null)
    }))
    .unwrap();
    assert!(exchange_raw(server.path(), b"{not-json}\n")
        .error
        .unwrap()
        .contains("格式错误"));
    let mut value = serde_json::to_value(request()).unwrap();
    value["workspaceRoot"] = json!(r"C:\untrusted");
    let mut bytes = serde_json::to_vec(&value).unwrap();
    bytes.push(b'\n');
    assert!(exchange_raw(server.path(), &bytes)
        .error
        .unwrap()
        .contains("格式错误"));
    for field in ["sessionToken", "runScope", "requestId", "tool"] {
        let mut value = serde_json::to_value(request()).unwrap();
        value[field] = json!("x".repeat(257));
        let mut bytes = serde_json::to_vec(&value).unwrap();
        bytes.push(b'\n');
        assert!(exchange_raw(server.path(), &bytes)
            .error
            .unwrap()
            .contains("字段超限"));
    }
    assert_eq!(calls.load(Ordering::Acquire), 0);
}

#[test]
fn oversized_client_is_rejected_without_host_dispatch() {
    let calls = Arc::new(AtomicUsize::new(0));
    let count = calls.clone();
    let server = start(Arc::new(move |_| {
        count.fetch_add(1, Ordering::AcqRel);
        Ok(Value::Null)
    }))
    .unwrap();
    let error = exchange_raw(server.path(), &vec![b'x'; MAX_MESSAGE + 1])
        .error
        .unwrap();
    assert!(error.contains("过大"));
    let mut call = request();
    call.args = json!({"content":"x".repeat(MAX_MESSAGE)});
    assert!(forward(server.path(), &call)
        .unwrap_err()
        .contains("超出上限"));
    assert_eq!(calls.load(Ordering::Acquire), 0);
}

#[test]
fn a_request_without_its_line_terminator_is_never_dispatched() {
    let calls = Arc::new(AtomicUsize::new(0));
    let count = calls.clone();
    let server = start(Arc::new(move |_| {
        count.fetch_add(1, Ordering::AcqRel);
        Ok(json!({"accepted":true}))
    }))
    .unwrap();
    let pipe = open_client(server.path(), SERVER_TIMEOUT).unwrap();
    let incomplete = serde_json::to_vec(&request()).unwrap();
    write_all(
        &pipe,
        &incomplete,
        &AtomicBool::new(false),
        Instant::now() + SERVER_TIMEOUT,
    )
    .unwrap();
    drop(pipe);
    // A second valid call also proves that closing a partial request does not
    // poison the endpoint or get replayed as a request on another connection.
    assert_eq!(
        forward(server.path(), &request()).unwrap()["accepted"],
        true
    );
    assert_eq!(calls.load(Ordering::Acquire), 1);
}

#[test]
fn valid_json_without_response_line_terminator_is_an_error() {
    let endpoint = PathBuf::from(format!("{PIPE_PREFIX}{}", uuid::Uuid::new_v4()));
    let security = private_security().unwrap();
    let instances = Arc::new(Mutex::new(0));
    let pipe = create_pipe(&endpoint, &security, &instances).unwrap();
    let server = std::thread::spawn(move || {
        let stop = AtomicBool::new(false);
        let deadline = Instant::now() + SERVER_TIMEOUT;
        loop {
            assert!(Instant::now() < deadline);
            if unsafe { ConnectNamedPipe(pipe.0, null_mut()) } == 0
                && unsafe { GetLastError() } == ERROR_PIPE_CONNECTED
            {
                break;
            }
            std::thread::sleep(POLL);
        }
        let _ = read_line(&pipe, &stop, deadline).unwrap();
        write_all(
            &pipe,
            br#"{"result":{"ok":true},"error":null}"#,
            &stop,
            deadline,
        )
        .unwrap();
        std::thread::sleep(Duration::from_millis(20));
    });
    assert!(forward(&endpoint, &request())
        .unwrap_err()
        .contains("不完整"));
    server.join().unwrap();
}

#[test]
fn large_byte_stream_round_trip_and_oversized_response_have_explicit_outcomes() {
    let server = start(Arc::new(|request| Ok(request.args))).unwrap();
    let mut call = request();
    call.args = json!({"content":"中文路径 with spaces\n".repeat(12_000)});
    assert_eq!(forward(server.path(), &call).unwrap(), call.args);
    let huge = start(Arc::new(|_| {
        Ok(json!({"content":"\t".repeat(MAX_MESSAGE)}))
    }))
    .unwrap();
    assert!(forward(huge.path(), &request())
        .unwrap_err()
        .contains("传输上限"));
}

#[test]
fn absent_response_times_out_and_remote_endpoints_are_rejected() {
    let server = start(Arc::new(|_| Ok(Value::Null))).unwrap();
    let pipe = open_client(server.path(), SERVER_TIMEOUT).unwrap();
    let start = Instant::now();
    let error = read_line(
        &pipe,
        &AtomicBool::new(false),
        start + Duration::from_millis(40),
    )
    .unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::TimedOut);
    assert!(start.elapsed() < Duration::from_secs(1));
    let remote = format!(r"\\example.invalid\pipe\atrio-mcp-{}", uuid::Uuid::new_v4());
    assert_eq!(
        open_client(Path::new(&remote), Duration::from_millis(20))
            .err()
            .unwrap()
            .kind(),
        io::ErrorKind::InvalidInput
    );
}

#[test]
fn client_capacity_is_bounded_and_recovers_after_clients_disconnect() {
    let server = start(Arc::new(|_| Ok(json!({"alive":true})))).unwrap();
    let mut clients = Vec::new();
    for _ in 0..MAX_CLIENTS {
        clients.push(open_client(server.path(), SERVER_TIMEOUT).unwrap());
    }
    assert!(open_client(server.path(), Duration::from_millis(40)).is_err());
    drop(clients);
    assert_eq!(forward(server.path(), &request()).unwrap()["alive"], true);
}

#[test]
fn shutdown_with_idle_clients_is_bounded_and_endpoint_disappears() {
    let server = start(Arc::new(|_| Ok(Value::Null))).unwrap();
    let endpoint = server.path().to_owned();
    let client = open_client(&endpoint, SERVER_TIMEOUT).unwrap();
    let start = Instant::now();
    drop(server);
    assert!(start.elapsed() < Duration::from_secs(1));
    let _ = read_line(
        &client,
        &AtomicBool::new(false),
        Instant::now() + Duration::from_millis(40),
    );
    drop(client);
    assert!(open_client(&endpoint, Duration::from_millis(40)).is_err());
}

#[test]
fn shutdown_does_not_wait_forever_for_a_host_callback() {
    let (entered, entry) = std::sync::mpsc::channel();
    let (release, released) = std::sync::mpsc::channel();
    let released = Mutex::new(released);
    let server = start(Arc::new(move |_| {
        entered.send(()).unwrap();
        let _ = released
            .lock()
            .unwrap()
            .recv_timeout(Duration::from_secs(3));
        Ok(Value::Null)
    }))
    .unwrap();
    let endpoint = server.path().to_owned();
    let client = std::thread::spawn(move || forward(&endpoint, &request()));
    entry.recv_timeout(Duration::from_secs(2)).unwrap();
    let start = Instant::now();
    drop(server);
    assert!(start.elapsed() < Duration::from_secs(1));
    release.send(()).unwrap();
    assert!(client.join().unwrap().is_err());
}

#[test]
fn server_instances_have_distinct_unpredictable_endpoints() {
    let one = start(Arc::new(|_| Ok(Value::Null))).unwrap();
    let two = start(Arc::new(|_| Ok(Value::Null))).unwrap();
    assert_ne!(one.path(), two.path());
}
