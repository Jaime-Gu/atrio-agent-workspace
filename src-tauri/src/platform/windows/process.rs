//! Windows ACP ownership is a private Job, never a PID/name based process sweep.
use crate::agent::native::{reader, IoEvent, MAX_FRAME};
use serde_json::Value;
use std::io;
use std::mem::{size_of, zeroed};
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::os::windows::process::CommandExt;
use std::{
    io::{Read, Write},
    process::{Child, Command},
    sync::{
        atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering},
        mpsc, Arc,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};
use windows_sys::Win32::Foundation::{
    ERROR_BROKEN_PIPE, ERROR_PIPE_NOT_CONNECTED, HANDLE, INVALID_HANDLE_VALUE,
};
use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Thread32First, Thread32Next, TH32CS_SNAPTHREAD, THREADENTRY32,
};
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JobObjectBasicAccountingInformation,
    JobObjectExtendedLimitInformation, QueryInformationJobObject, SetInformationJobObject,
    TerminateJobObject, JOBOBJECT_BASIC_ACCOUNTING_INFORMATION,
    JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
};
use windows_sys::Win32::System::Pipes::PeekNamedPipe;
use windows_sys::Win32::System::Threading::{
    OpenThread, ResumeThread, CREATE_NO_WINDOW, CREATE_SUSPENDED, THREAD_SUSPEND_RESUME,
};
use windows_sys::Win32::System::IO::CancelSynchronousIo;

struct Job(OwnedHandle);
impl Job {
    fn new() -> Result<Self, String> {
        unsafe {
            let handle = CreateJobObjectW(std::ptr::null(), std::ptr::null());
            if handle.is_null() {
                return Err("无法创建 Agent Job Object".into());
            }
            let job = Self(OwnedHandle::from_raw_handle(handle));
            let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = zeroed();
            limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            if SetInformationJobObject(
                job.handle(),
                JobObjectExtendedLimitInformation,
                &limits as *const _ as *const _,
                size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            ) == 0
            {
                return Err("无法配置 Agent Job Object".into());
            }
            Ok(job)
        }
    }
    fn handle(&self) -> HANDLE {
        self.0.as_raw_handle()
    }
    fn terminate(&self) -> Result<(), String> {
        if unsafe { TerminateJobObject(self.handle(), 1) } == 0 {
            return Err("无法终止本次 Agent Job".into());
        }
        Ok(())
    }
    fn active(&self) -> Result<u32, String> {
        unsafe {
            let mut accounting: JOBOBJECT_BASIC_ACCOUNTING_INFORMATION = zeroed();
            if QueryInformationJobObject(
                self.handle(),
                JobObjectBasicAccountingInformation,
                &mut accounting as *mut _ as *mut _,
                size_of::<JOBOBJECT_BASIC_ACCOUNTING_INFORMATION>() as u32,
                std::ptr::null_mut(),
            ) == 0
            {
                return Err("无法确认 Agent Job 子进程状态".into());
            }
            Ok(accounting.ActiveProcesses)
        }
    }
}

// Command owns the process handle but does not expose the primary thread handle.
// With CREATE_SUSPENDED no user code runs and no descendant can escape before
// assignment. Locate that still-suspended primary thread, then resume it once.
fn assign_and_resume(job: &Job, child: &Child) -> Result<(), String> {
    unsafe {
        if AssignProcessToJobObject(job.handle(), child.as_raw_handle()) == 0 {
            return Err("无法将 Agent 放入受控 Job；拒绝无约束启动".into());
        }
        let raw = CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0);
        if raw == INVALID_HANDLE_VALUE {
            return Err("无法定位暂停的 Agent 主线程".into());
        }
        let snapshot = OwnedHandle::from_raw_handle(raw);
        let mut entry: THREADENTRY32 = zeroed();
        entry.dwSize = size_of::<THREADENTRY32>() as u32;
        let mut found = Thread32First(snapshot.as_raw_handle(), &mut entry);
        while found != 0 {
            if entry.th32OwnerProcessID == child.id() {
                let raw = OpenThread(THREAD_SUSPEND_RESUME, 0, entry.th32ThreadID);
                if raw.is_null() {
                    return Err("无法打开暂停的 Agent 主线程".into());
                }
                let thread = OwnedHandle::from_raw_handle(raw);
                if ResumeThread(thread.as_raw_handle()) != 1 {
                    return Err("Agent 主线程恢复状态异常".into());
                }
                return Ok(());
            }
            found = Thread32Next(snapshot.as_raw_handle(), &mut entry);
        }
    }
    Err("未找到暂停的 Agent 主线程".into())
}

// Anonymous stdio pipes support PeekNamedPipe. Read only the observed bytes so
// readers periodically see `stop`, even while a descendant holds the pipe open.
struct PollingPipe<T>(T);
impl<T: Read + AsRawHandle> Read for PollingPipe<T> {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        let mut available = 0;
        if unsafe {
            PeekNamedPipe(
                self.0.as_raw_handle(),
                std::ptr::null_mut(),
                0,
                std::ptr::null_mut(),
                &mut available,
                std::ptr::null_mut(),
            )
        } == 0
        {
            let error = io::Error::last_os_error();
            if matches!(
                error.raw_os_error().map(|v| v as u32),
                Some(ERROR_BROKEN_PIPE | ERROR_PIPE_NOT_CONNECTED)
            ) {
                return Ok(0);
            }
            return Err(error);
        }
        if available == 0 {
            return Err(io::ErrorKind::WouldBlock.into());
        }
        let count = bytes.len().min(available as usize);
        self.0.read(&mut bytes[..count])
    }
}

type WriteRequest = (Vec<u8>, mpsc::SyncSender<Result<(), String>>);
pub(crate) struct ManagedProcess {
    pub(crate) child: Child,
    pub(crate) group: u32,
    pub(crate) io: mpsc::Receiver<IoEvent>,
    pub(crate) byte_budget: Arc<AtomicUsize>,
    pub(crate) tracked_pid: Option<Arc<AtomicU32>>,
    pub(crate) leader_reaped: bool,
    pub(crate) last_tracking: Instant,
    job: Job,
    writes: Option<mpsc::SyncSender<WriteRequest>>,
    writer: Option<JoinHandle<()>>,
    stop: Arc<AtomicBool>,
    readers: Vec<JoinHandle<()>>,
    finished: bool,
}
impl ManagedProcess {
    #[cfg(test)]
    pub(crate) fn readers_finished_for_test(&self) -> bool {
        self.readers.is_empty() && self.writer.is_none()
    }
    pub(crate) fn spawn(mut command: Command) -> Result<Self, String> {
        let job = Job::new()?;
        command.creation_flags(CREATE_SUSPENDED | CREATE_NO_WINDOW);
        let mut child = command
            .spawn()
            .map_err(|error| format!("无法启动 Agent：{error}"))?;
        if let Err(error) = assign_and_resume(&job, &child) {
            let _ = job.terminate();
            let _ = child.kill();
            let _ = wait_child(&mut child, Duration::from_secs(1));
            return Err(error);
        }
        let group = child.id();
        let pipes = (child.stdin.take(), child.stdout.take(), child.stderr.take());
        let (Some(mut stdin), Some(stdout), Some(stderr)) = pipes else {
            let _ = job.terminate();
            let _ = wait_child(&mut child, Duration::from_secs(1));
            return Err("Agent stdio 管道不可用".into());
        };
        let (tx, io) = mpsc::channel();
        let stop = Arc::new(AtomicBool::new(false));
        let byte_budget = Arc::new(AtomicUsize::new(0));
        let readers = vec![
            reader(
                PollingPipe(stdout),
                tx.clone(),
                stop.clone(),
                byte_budget.clone(),
                true,
            ),
            reader(
                PollingPipe(stderr),
                tx,
                stop.clone(),
                byte_budget.clone(),
                false,
            ),
        ];
        let (writes, pending) = mpsc::sync_channel::<WriteRequest>(1);
        let writer_stop = stop.clone();
        let writer = thread::spawn(move || {
            while let Ok((bytes, response)) = pending.recv() {
                if writer_stop.load(Ordering::Acquire) {
                    break;
                }
                let result = stdin
                    .write_all(&bytes)
                    .map_err(|_| "ACP stdin 写入失败或已取消".to_owned());
                let failed = result.is_err();
                let _ = response.send(result);
                if failed {
                    break;
                }
            }
        });
        Ok(Self {
            child,
            group,
            io,
            byte_budget,
            tracked_pid: None,
            leader_reaped: false,
            last_tracking: Instant::now(),
            job,
            writes: Some(writes),
            writer: Some(writer),
            stop,
            readers,
            finished: false,
        })
    }
    pub(crate) fn send(&mut self, value: &Value) -> Result<(), String> {
        let mut bytes = serde_json::to_vec(value).map_err(|_| "ACP 消息序列化失败")?;
        if bytes.len() > MAX_FRAME {
            return Err("ACP stdin 消息超出上限".into());
        }
        bytes.push(b'\n');
        let (reply, result) = mpsc::sync_channel(1);
        self.writes
            .as_ref()
            .ok_or("ACP stdin 已关闭")?
            .try_send((bytes, reply))
            .map_err(|_| "ACP stdin 已关闭或队列已满")?;
        match result.recv_timeout(Duration::from_secs(1)) {
            Ok(result) => result,
            Err(_) => {
                self.stop.store(true, Ordering::Release);
                self.writes.take();
                let _ = self.job.terminate();
                self.cancel_writer();
                Err("ACP stdin 写入超时；已请求终止受控进程".into())
            }
        }
    }
    fn cancel_writer(&self) {
        if let Some(writer) = &self.writer {
            unsafe {
                CancelSynchronousIo(writer.as_raw_handle());
            }
        }
    }
    pub(crate) fn capture_descendants(&mut self) -> Result<(), String> {
        self.last_tracking = Instant::now();
        self.job.active().map(|_| ())
    }
    pub(crate) fn signal(&mut self, _signal: i32) {
        // Protocol session/cancel has already received its grace window.
        let _ = self.job.terminate();
    }
    pub(crate) fn finish(&mut self) -> Result<(), String> {
        if self.finished {
            return Ok(());
        }
        self.stop.store(true, Ordering::Release);
        self.writes.take();
        self.job.terminate()?;
        self.cancel_writer();
        let deadline = Instant::now() + Duration::from_secs(1);
        loop {
            if self
                .child
                .try_wait()
                .map_err(|_| "无法核实 Agent 退出")?
                .is_some()
            {
                self.leader_reaped = true;
            }
            let done = self.leader_reaped
                && self.job.active()? == 0
                && self.writer.as_ref().is_none_or(JoinHandle::is_finished)
                && self.readers.iter().all(JoinHandle::is_finished);
            if done {
                break;
            }
            if Instant::now() >= deadline {
                return Err("Agent Job 或管道线程未在期限内退出；停止未确认".into());
            }
            self.cancel_writer();
            thread::sleep(Duration::from_millis(5));
        }
        // Only join threads whose completion was observed above.
        if let Some(writer) = self.writer.take() {
            let _ = writer.join();
        }
        for reader in self.readers.drain(..) {
            let _ = reader.join();
        }
        if let Some(pid) = &self.tracked_pid {
            pid.store(0, Ordering::Release);
        }
        self.finished = true;
        Ok(())
    }
}
fn wait_child(child: &mut Child, limit: Duration) -> bool {
    let deadline = Instant::now() + limit;
    while Instant::now() < deadline {
        if child.try_wait().ok().flatten().is_some() {
            return true;
        }
        thread::sleep(Duration::from_millis(5));
    }
    false
}
impl Drop for ManagedProcess {
    fn drop(&mut self) {
        let _ = self.finish();
    }
}
