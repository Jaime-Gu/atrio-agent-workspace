//! A private, local, byte-stream pipe. PIPE_NOWAIT keeps every transport wait
//! bounded, including clients which connect without sending or reading data.
//! Authorization still belongs to the Host's token/run-scope checked handler.
use crate::mcp_bridge::{ToolRequest, ToolResponse, ToolServer, MAX_CLIENTS, MAX_MESSAGE};
use serde_json::Value;
use std::io;
use std::mem::size_of;
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::ptr::{null, null_mut};
use std::sync::{
    atomic::{AtomicBool, AtomicUsize, Ordering},
    Arc, Mutex,
};
use std::time::{Duration, Instant};
use windows_sys::Win32::Foundation::{
    CloseHandle, GetLastError, LocalFree, ERROR_BROKEN_PIPE, ERROR_FILE_NOT_FOUND, ERROR_NO_DATA,
    ERROR_PIPE_BUSY, ERROR_PIPE_CONNECTED, ERROR_PIPE_LISTENING, ERROR_PIPE_NOT_CONNECTED,
    GENERIC_READ, GENERIC_WRITE, HANDLE, INVALID_HANDLE_VALUE,
};
use windows_sys::Win32::Security::Authorization::{
    ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
};
use windows_sys::Win32::Security::{
    GetTokenInformation, TokenUser, SECURITY_ATTRIBUTES, TOKEN_QUERY, TOKEN_USER,
};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, ReadFile, WriteFile, FILE_FLAG_FIRST_PIPE_INSTANCE, OPEN_EXISTING,
    PIPE_ACCESS_DUPLEX,
};
use windows_sys::Win32::System::Pipes::{
    ConnectNamedPipe, CreateNamedPipeW, SetNamedPipeHandleState, WaitNamedPipeW, PIPE_NOWAIT,
    PIPE_READMODE_BYTE, PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE,
};
use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

const PIPE_PREFIX: &str = r"\\.\pipe\atrio-mcp-";
const POLL: Duration = Duration::from_millis(5);
const SERVER_TIMEOUT: Duration = Duration::from_secs(5);
const RESPONSE_TIMEOUT: Duration = Duration::from_secs(10);
type Handler = Arc<dyn Fn(ToolRequest) -> Result<Value, String> + Send + Sync>;

struct OwnedHandle(HANDLE);
// This handle has one owner. It is moved, never concurrently used by two
// threads; the Win32 kernel handle itself is not thread-affine.
unsafe impl Send for OwnedHandle {}
impl Drop for OwnedHandle {
    fn drop(&mut self) {
        unsafe { CloseHandle(self.0) };
    }
}

struct PipeInstance {
    handle: Option<OwnedHandle>,
    instances: Arc<Mutex<usize>>,
}
impl std::ops::Deref for PipeInstance {
    type Target = OwnedHandle;
    fn deref(&self) -> &Self::Target {
        self.handle.as_ref().expect("live pipe")
    }
}
impl Drop for PipeInstance {
    fn drop(&mut self) {
        let mut count = self.instances.lock().unwrap_or_else(|e| e.into_inner());
        drop(self.handle.take());
        *count -= 1;
    }
}

struct LocalAllocation(*mut core::ffi::c_void);
// The descriptor is immutable and moves to the sole listener thread.
unsafe impl Send for LocalAllocation {}
impl Drop for LocalAllocation {
    fn drop(&mut self) {
        unsafe { LocalFree(self.0) };
    }
}

fn wide(value: &std::ffi::OsStr) -> Vec<u16> {
    value.encode_wide().chain(Some(0)).collect()
}

fn current_user_sid() -> io::Result<String> {
    let mut token = null_mut();
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let token = OwnedHandle(token);
    let mut needed = 0;
    unsafe { GetTokenInformation(token.0, TokenUser, null_mut(), 0, &mut needed) };
    if needed == 0 {
        return Err(io::Error::last_os_error());
    }
    // TOKEN_USER contains a pointer, so preserve its alignment.
    let mut storage = vec![0usize; (needed as usize).div_ceil(size_of::<usize>())];
    if unsafe {
        GetTokenInformation(
            token.0,
            TokenUser,
            storage.as_mut_ptr().cast(),
            needed,
            &mut needed,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    let user = unsafe { &*storage.as_ptr().cast::<TOKEN_USER>() };
    let mut sid = null_mut();
    if unsafe { ConvertSidToStringSidW(user.User.Sid, &mut sid) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let _sid_allocation = LocalAllocation(sid.cast());
    let mut length = 0;
    unsafe {
        while *sid.add(length) != 0 {
            length += 1;
        }
        Ok(String::from_utf16_lossy(std::slice::from_raw_parts(
            sid, length,
        )))
    }
}

fn private_security() -> io::Result<LocalAllocation> {
    // Explicit protected DACL: only this process's user may open the pipe.
    // Never fall back to the permissive default named-pipe DACL on failure.
    let sddl = wide(std::ffi::OsStr::new(&format!(
        "D:P(A;;GA;;;{})",
        current_user_sid()?
    )));
    let mut descriptor = null_mut();
    if unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            sddl.as_ptr(),
            SDDL_REVISION_1,
            &mut descriptor,
            null_mut(),
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(LocalAllocation(descriptor))
}

fn create_pipe(
    endpoint: &Path,
    security: &LocalAllocation,
    instances: &Arc<Mutex<usize>>,
) -> io::Result<PipeInstance> {
    // Creation and last-handle closure share a lock. If every instance has
    // closed, FIRST_PIPE_INSTANCE prevents joining an endpoint squatted by
    // another process in the brief interval before the next listener exists.
    let mut count = instances.lock().unwrap_or_else(|e| e.into_inner());
    let name = wide(endpoint.as_os_str());
    let attributes = SECURITY_ATTRIBUTES {
        nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: security.0,
        bInheritHandle: 0,
    };
    let access = PIPE_ACCESS_DUPLEX
        | if *count == 0 {
            FILE_FLAG_FIRST_PIPE_INSTANCE
        } else {
            0
        };
    let handle = unsafe {
        CreateNamedPipeW(
            name.as_ptr(),
            access,
            PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_NOWAIT | PIPE_REJECT_REMOTE_CLIENTS,
            MAX_CLIENTS as u32,
            64 * 1024,
            64 * 1024,
            SERVER_TIMEOUT.as_millis() as u32,
            &attributes,
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        Err(io::Error::last_os_error())
    } else {
        *count += 1;
        Ok(PipeInstance {
            handle: Some(OwnedHandle(handle)),
            instances: instances.clone(),
        })
    }
}

fn expired(stop: &AtomicBool, deadline: Instant) -> io::Result<()> {
    if stop.load(Ordering::Acquire) {
        Err(io::Error::new(io::ErrorKind::Interrupted, "Host stopped"))
    } else if Instant::now() >= deadline {
        Err(io::Error::new(
            io::ErrorKind::TimedOut,
            "local tool timeout",
        ))
    } else {
        Ok(())
    }
}

fn read_line(pipe: &OwnedHandle, stop: &AtomicBool, deadline: Instant) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    let mut chunk = [0u8; 16 * 1024];
    loop {
        expired(stop, deadline)?;
        let capacity = chunk.len().min(MAX_MESSAGE + 1 - bytes.len());
        let mut read = 0;
        let success = unsafe {
            ReadFile(
                pipe.0,
                chunk.as_mut_ptr(),
                capacity as u32,
                &mut read,
                null_mut(),
            )
        };
        if success != 0 && read > 0 {
            let data = &chunk[..read as usize];
            if let Some(end) = data.iter().position(|byte| *byte == b'\n') {
                bytes.extend_from_slice(&data[..=end]);
                return Ok(bytes);
            }
            bytes.extend_from_slice(data);
            if bytes.len() > MAX_MESSAGE {
                return Ok(bytes);
            }
        } else if success == 0 {
            match unsafe { GetLastError() } {
                ERROR_NO_DATA | ERROR_PIPE_LISTENING => std::thread::sleep(POLL),
                ERROR_BROKEN_PIPE | ERROR_PIPE_NOT_CONNECTED => return Ok(bytes),
                error => return Err(io::Error::from_raw_os_error(error as i32)),
            }
        } else {
            std::thread::sleep(POLL);
        }
    }
}

fn write_all(
    pipe: &OwnedHandle,
    bytes: &[u8],
    stop: &AtomicBool,
    deadline: Instant,
) -> io::Result<()> {
    let mut offset = 0;
    while offset < bytes.len() {
        expired(stop, deadline)?;
        let mut written = 0;
        let success = unsafe {
            WriteFile(
                pipe.0,
                bytes[offset..].as_ptr(),
                (bytes.len() - offset).min(16 * 1024) as u32,
                &mut written,
                null_mut(),
            )
        };
        if success == 0 {
            return Err(io::Error::last_os_error());
        }
        offset += written as usize;
        if written == 0 {
            std::thread::sleep(POLL);
        }
    }
    Ok(())
}

fn serve(pipe: PipeInstance, handler: &Handler, stop: &AtomicBool) {
    let result = (|| {
        let bytes = read_line(&pipe, stop, Instant::now() + SERVER_TIMEOUT)
            .map_err(|_| "本机工具请求读取失败")?;
        if bytes.is_empty() || bytes.len() > MAX_MESSAGE || !bytes.ends_with(b"\n") {
            return Err("本机工具请求过大或未完整发送".into());
        }
        let request: ToolRequest =
            serde_json::from_slice(&bytes).map_err(|_| "本机工具请求格式错误")?;
        if request.session_token.len() > 256
            || request.run_scope.len() > 256
            || request.request_id.len() > 128
            || request.tool.len() > 128
        {
            return Err("本机工具请求字段超限".into());
        }
        if stop.load(Ordering::Acquire) {
            return Err("Atrio Host 已退出或本机工具连接不可用".into());
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
        if bytes.len() + 1 > MAX_MESSAGE {
            bytes = serde_json::to_vec(&ToolResponse {
                result: None,
                error: Some(
                    "模块结果超过本机工具传输上限；请缩小已保存文档或拆分模块后重试".into(),
                ),
            })
            .expect("a string tool response is serializable");
        }
        bytes.push(b'\n');
        if write_all(&pipe, &bytes, stop, Instant::now() + SERVER_TIMEOUT).is_ok() {
            // Keep the response buffer alive until the one-request client
            // closes, with a fixed deadline. FlushFileBuffers would instead
            // wait indefinitely for a client that never reads its response.
            let deadline = Instant::now() + SERVER_TIMEOUT;
            let mut discard = [0u8; 1024];
            while expired(stop, deadline).is_ok() {
                let mut read = 0;
                if unsafe {
                    ReadFile(
                        pipe.0,
                        discard.as_mut_ptr(),
                        discard.len() as u32,
                        &mut read,
                        null_mut(),
                    )
                } == 0
                {
                    if unsafe { GetLastError() } != ERROR_NO_DATA {
                        break;
                    }
                }
                std::thread::sleep(POLL);
            }
        }
    }
}

pub(crate) fn start(handler: Handler) -> Result<ToolServer, String> {
    let socket = PathBuf::from(format!("{PIPE_PREFIX}{}", uuid::Uuid::new_v4()));
    let security = private_security().map_err(|e| format!("无法创建当前用户专用工具管道: {e}"))?;
    let instances = Arc::new(Mutex::new(0));
    let first = create_pipe(&socket, &security, &instances)
        .map_err(|e| format!("无法创建本机工具管道: {e}"))?;
    let endpoint = socket.clone();
    let stop = Arc::new(AtomicBool::new(false));
    let stopped = stop.clone();
    let thread = std::thread::Builder::new()
        .name("atrio-mcp-listener".into())
        .spawn(move || {
            let clients = Arc::new(AtomicUsize::new(0));
            let mut listening = Some(first);
            let mut workers: Vec<std::thread::JoinHandle<()>> = Vec::new();
            while !stopped.load(Ordering::Acquire) {
                let mut index = 0;
                while index < workers.len() {
                    if workers[index].is_finished() {
                        let _ = workers.swap_remove(index).join();
                    } else {
                        index += 1;
                    }
                }
                if listening.is_none() {
                    if clients.load(Ordering::Acquire) >= MAX_CLIENTS {
                        std::thread::sleep(POLL);
                        continue;
                    }
                    match create_pipe(&endpoint, &security, &instances) {
                        Ok(pipe) => listening = Some(pipe),
                        Err(error) if error.raw_os_error() == Some(ERROR_PIPE_BUSY as i32) => {
                            std::thread::sleep(POLL);
                            continue;
                        }
                        Err(_) => break,
                    }
                }
                let pipe = listening.as_ref().expect("listener exists");
                // In NOWAIT mode a true return merely makes a disconnected
                // instance available. ERROR_PIPE_CONNECTED confirms a client.
                let available = unsafe { ConnectNamedPipe(pipe.0, null_mut()) };
                let error = if available == 0 {
                    unsafe { GetLastError() }
                } else {
                    0
                };
                if error == ERROR_PIPE_CONNECTED {
                    let pipe = listening.take().expect("connected listener exists");
                    let handler = handler.clone();
                    let stopped = stopped.clone();
                    let clients = clients.clone();
                    clients.fetch_add(1, Ordering::AcqRel);
                    match std::thread::Builder::new()
                        .name("atrio-mcp-client".into())
                        .spawn(move || {
                            struct Guard(Arc<AtomicUsize>);
                            impl Drop for Guard {
                                fn drop(&mut self) {
                                    self.0.fetch_sub(1, Ordering::AcqRel);
                                }
                            }
                            let _guard = Guard(clients);
                            serve(pipe, &handler, &stopped);
                        }) {
                        Ok(worker) => workers.push(worker),
                        Err(_) => break,
                    }
                } else if available != 0 || error == ERROR_PIPE_LISTENING {
                    std::thread::sleep(POLL);
                } else {
                    // A client may close before it is accepted. Retire that
                    // instance and keep the private endpoint usable.
                    listening.take();
                }
            }
            // Wake idle readers/writers before joining any completed worker.
            // Host callbacks are outside the transport's control; never wait
            // indefinitely for a callback during application shutdown.
            stopped.store(true, Ordering::Release);
            drop(listening);
            let deadline = Instant::now() + Duration::from_millis(250);
            while workers.iter().any(|worker| !worker.is_finished()) && Instant::now() < deadline {
                std::thread::sleep(POLL);
            }
            for worker in workers {
                if worker.is_finished() {
                    let _ = worker.join();
                }
            }
        })
        .map_err(|e| format!("无法启动本机工具管道: {e}"))?;
    Ok(ToolServer {
        socket,
        stop,
        thread: Some(thread),
    })
}

fn open_client(endpoint: &Path, timeout: Duration) -> io::Result<OwnedHandle> {
    // The child accepts only the local namespace and the exact Host-generated
    // UUID format. Environment tampering cannot turn forwarding into SMB IPC.
    let valid = endpoint
        .to_str()
        .and_then(|name| name.strip_prefix(PIPE_PREFIX))
        .and_then(|id| uuid::Uuid::parse_str(id).ok())
        .is_some();
    if !valid {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid local pipe endpoint",
        ));
    }
    let endpoint = wide(endpoint.as_os_str());
    let deadline = Instant::now() + timeout;
    loop {
        let handle = unsafe {
            CreateFileW(
                endpoint.as_ptr(),
                GENERIC_READ | GENERIC_WRITE,
                0,
                null(),
                OPEN_EXISTING,
                0,
                null_mut(),
            )
        };
        if handle != INVALID_HANDLE_VALUE {
            let pipe = OwnedHandle(handle);
            let mode = PIPE_READMODE_BYTE | PIPE_NOWAIT;
            if unsafe { SetNamedPipeHandleState(pipe.0, &mode, null(), null()) } == 0 {
                return Err(io::Error::last_os_error());
            }
            return Ok(pipe);
        }
        let error = unsafe { GetLastError() };
        if (error != ERROR_PIPE_BUSY && error != ERROR_FILE_NOT_FOUND) || Instant::now() >= deadline
        {
            return Err(io::Error::from_raw_os_error(error as i32));
        }
        // WaitNamedPipe is capped, and FILE_NOT_FOUND returns immediately when
        // the listener is between instances. The overall deadline is fixed.
        unsafe { WaitNamedPipeW(endpoint.as_ptr(), 20) };
        std::thread::sleep(POLL);
    }
}

pub(crate) fn forward(socket: &Path, request: &ToolRequest) -> Result<Value, String> {
    let mut bytes = serde_json::to_vec(request).map_err(|_| "工具请求编码失败")?;
    if bytes.len() + 1 > MAX_MESSAGE {
        return Err("工具请求超出上限".into());
    }
    bytes.push(b'\n');
    let pipe =
        open_client(socket, SERVER_TIMEOUT).map_err(|_| "Atrio Host 已退出或本机工具连接不可用")?;
    let stop = AtomicBool::new(false);
    write_all(&pipe, &bytes, &stop, Instant::now() + SERVER_TIMEOUT)
        .map_err(|_| "工具请求发送失败")?;
    let bytes = read_line(&pipe, &stop, Instant::now() + RESPONSE_TIMEOUT)
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

#[cfg(test)]
#[path = "ipc_tests.rs"]
mod tests;
