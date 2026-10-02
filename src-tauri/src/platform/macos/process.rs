//! macOS Agent ACP process ownership and guarded process-group cleanup.
#![allow(dead_code)]
use crate::agent::native::{reader, IoEvent, MAX_FRAME};
use serde_json::Value;
use std::{
    collections::{HashMap, HashSet},
    io::{Read, Write},
    os::unix::io::AsRawFd,
    process::{Child, ChildStdin, Command, Stdio},
    sync::{
        atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering},
        mpsc, Arc,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

#[cfg(unix)]
#[derive(Clone)]
struct ProcessIdentity {
    pid: u32,
    parent: u32,
    pub(crate) group: u32,
    started: String,
    zombie: bool,
}
#[cfg(unix)]
fn process_snapshot() -> Result<HashMap<u32, ProcessIdentity>, String> {
    // Read metadata only: never argv or environment. The launch identity is rechecked before signals.
    let mut child = Command::new("/bin/ps")
        .args(["-axo", "pid=,ppid=,pgid=,lstart=,stat="])
        .env("LC_ALL", "C")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| "无法核查 Agent 子进程血缘")?;
    let mut stdout = child.stdout.take().ok_or("无法读取进程元数据")?;
    #[cfg(unix)]
    nonblocking(&stdout);
    let deadline = Instant::now() + Duration::from_millis(250);
    let mut bytes = Vec::new();
    let mut chunk = [0u8; 8192];
    let result = loop {
        match stdout.read(&mut chunk) {
            Ok(0) => break Ok(()),
            Ok(count) => {
                bytes.extend_from_slice(&chunk[..count]);
                if bytes.len() > MAX_FRAME {
                    break Err("进程元数据超出上限");
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {}
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(_) => break Err("进程元数据读取失败"),
        }
        if Instant::now() >= deadline {
            break Err("进程血缘核查超时");
        }
        thread::sleep(Duration::from_millis(2));
    };
    if let Err(error) = result {
        let _ = child.kill();
        let _ = child.wait();
        return Err(error.into());
    }
    let status = loop {
        if let Some(status) = child.try_wait().map_err(|_| "进程元数据核查无法回收")? {
            break status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err("进程血缘核查超时".into());
        }
        thread::sleep(Duration::from_millis(2));
    };
    if !status.success() {
        return Err("进程元数据核查失败".into());
    }
    let mut snapshot = HashMap::new();
    for line in String::from_utf8_lossy(&bytes).lines() {
        let parts = line.split_whitespace().collect::<Vec<_>>();
        if parts.len() < 9 {
            continue;
        }
        let (Ok(pid), Ok(parent), Ok(group)) = (
            parts[0].parse::<u32>(),
            parts[1].parse::<u32>(),
            parts[2].parse::<u32>(),
        ) else {
            continue;
        };
        snapshot.insert(
            pid,
            ProcessIdentity {
                pid,
                parent,
                group,
                started: parts[3..8].join(" "),
                zombie: parts[8].starts_with('Z'),
            },
        );
    }
    Ok(snapshot)
}
#[cfg(unix)]
pub(crate) struct ManagedProcess {
    pub(crate) child: Child,
    stdin: Option<ChildStdin>,
    pub(crate) group: u32,
    pub(crate) io: mpsc::Receiver<IoEvent>,
    pub(crate) byte_budget: Arc<AtomicUsize>,
    stop: Arc<AtomicBool>,
    readers: Vec<JoinHandle<()>>,
    finished: bool,
    pub(crate) tracked_pid: Option<Arc<AtomicU32>>,
    known_descendants: HashMap<u32, ProcessIdentity>,
    leader_identity: Option<String>,
    pub(crate) leader_reaped: bool,
    pub(crate) last_tracking: Instant,
}
#[cfg(unix)]
impl ManagedProcess {
    pub(crate) fn spawn(mut command: Command) -> Result<Self, String> {
        let mut child = command
            .spawn()
            .map_err(|error| format!("无法启动 Agent：{error}"))?;
        let group = child.id();
        let stdin = child.stdin.take();
        let stdout = child.stdout.take().ok_or("Agent stdout 不可用")?;
        let stderr = child.stderr.take().ok_or("Agent stderr 不可用")?;
        #[cfg(unix)]
        {
            if let Some(input) = &stdin {
                nonblocking(input);
            }
            nonblocking(&stdout);
            nonblocking(&stderr);
        }
        let (tx, io) = mpsc::channel();
        let stop = Arc::new(AtomicBool::new(false));
        let byte_budget = Arc::new(AtomicUsize::new(0));
        let readers = vec![
            reader(stdout, tx.clone(), stop.clone(), byte_budget.clone(), true),
            reader(stderr, tx, stop.clone(), byte_budget.clone(), false),
        ];
        Ok(Self {
            child,
            stdin,
            group,
            io,
            byte_budget,
            stop,
            readers,
            finished: false,
            tracked_pid: None,
            known_descendants: HashMap::new(),
            leader_identity: None,
            leader_reaped: false,
            last_tracking: Instant::now(),
        })
    }
    pub(crate) fn send(&mut self, value: &Value) -> Result<(), String> {
        let mut bytes = serde_json::to_vec(value).map_err(|_| "ACP 消息序列化失败")?;
        bytes.push(b'\n');
        let input = self.stdin.as_mut().ok_or("ACP stdin 已关闭")?;
        let deadline = Instant::now() + Duration::from_secs(1);
        let mut cursor = 0;
        while cursor < bytes.len() {
            match input.write(&bytes[cursor..]) {
                Ok(0) => return Err("ACP stdin 已断开".into()),
                Ok(count) => cursor += count,
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(error)
                    if error.kind() == std::io::ErrorKind::WouldBlock
                        && Instant::now() < deadline =>
                {
                    thread::sleep(Duration::from_millis(5))
                }
                Err(_) => return Err("ACP stdin 写入失败或超时".into()),
            }
        }
        Ok(())
    }
    pub(crate) fn capture_descendants(&mut self) -> Result<(), String> {
        self.capture_snapshot().map(|_| ())
    }
    fn capture_snapshot(&mut self) -> Result<HashMap<u32, ProcessIdentity>, String> {
        let snapshot = process_snapshot()?;
        self.last_tracking = Instant::now();
        let mut parents = HashSet::new();
        if !self.leader_reaped {
            if let Some(leader) = snapshot.get(&self.group) {
                if self
                    .leader_identity
                    .as_ref()
                    .is_some_and(|start| start != &leader.started)
                {
                    return Err("Agent 主进程启动身份已经改变".into());
                }
                self.leader_identity = Some(leader.started.clone());
                parents.insert(leader.pid);
            }
        }
        for (pid, known) in &self.known_descendants {
            if snapshot
                .get(pid)
                .is_some_and(|current| current.started == known.started && !current.zombie)
            {
                parents.insert(*pid);
            }
        }
        loop {
            let additions = snapshot
                .values()
                .filter(|identity| {
                    parents.contains(&identity.parent) && !parents.contains(&identity.pid)
                })
                .cloned()
                .collect::<Vec<_>>();
            if additions.is_empty() {
                break;
            }
            if self.known_descendants.len() + additions.len() > 256 {
                return Err("Agent 子进程数量超出安全跟踪上限".into());
            }
            for identity in additions {
                parents.insert(identity.pid);
                self.known_descendants.insert(identity.pid, identity);
            }
        }
        Ok(snapshot)
    }
    pub(crate) fn signal(&mut self, signal: i32) {
        let snapshot = self.capture_snapshot().ok();
        if let Some(snapshot) = &snapshot {
            for (pid, known) in &self.known_descendants {
                if snapshot
                    .get(pid)
                    .is_some_and(|current| current.started == known.started && !current.zombie)
                {
                    #[cfg(unix)]
                    unsafe {
                        libc::kill(*pid as i32, signal);
                    }
                }
            }
        }
        let group_owned = !self.leader_reaped
            || snapshot.as_ref().is_some_and(|snapshot| {
                self.known_descendants.iter().any(|(pid, known)| {
                    snapshot.get(pid).is_some_and(|current| {
                        current.started == known.started
                            && current.group == self.group
                            && !current.zombie
                    })
                })
            });
        #[cfg(unix)]
        unsafe {
            if group_owned {
                libc::kill(-(self.group as i32), signal);
            }
        }
        #[cfg(not(unix))]
        {
            let _ = signal;
            let _ = self.child.kill();
        }
    }
    pub(crate) fn finish(&mut self) -> Result<(), String> {
        if self.finished {
            return Ok(());
        }
        // Snapshot while parentage still exists; include descendants with a different process group.
        let tracking = self.capture_snapshot();
        self.stop.store(true, Ordering::Release);
        self.stdin.take();
        self.signal(libc::SIGKILL); // Also reap same-group descendants holding pipes after leader exit.
        if !self.leader_reaped {
            let _ = self.child.kill();
        }
        self.child
            .wait()
            .map_err(|_| "无法确认 Agent 子进程已经回收")?;
        self.leader_reaped = true;
        for reader in self.readers.drain(..) {
            let _ = reader.join();
        }
        if self.tracked_pid.is_some() {
            tracking?;
            let deadline = Instant::now() + Duration::from_millis(500);
            loop {
                let snapshot = self.capture_snapshot()?;
                let alive = self.known_descendants.iter().any(|(pid, known)| {
                    snapshot
                        .get(pid)
                        .is_some_and(|current| current.started == known.started && !current.zombie)
                });
                if !alive {
                    break;
                }
                if Instant::now() >= deadline {
                    return Err("Agent 已知子进程仍未退出；停止未确认".into());
                }
                self.signal(libc::SIGKILL);
                thread::sleep(Duration::from_millis(10));
            }
        }
        if let Some(pid) = &self.tracked_pid {
            pid.store(0, Ordering::Release);
        }
        self.finished = true;
        Ok(())
    }
}
#[cfg(unix)]
impl Drop for ManagedProcess {
    fn drop(&mut self) {
        let _ = self.finish();
    }
}
#[cfg(unix)]
fn nonblocking(fd: &impl AsRawFd) {
    unsafe {
        let flags = libc::fcntl(fd.as_raw_fd(), libc::F_GETFL);
        if flags >= 0 {
            libc::fcntl(fd.as_raw_fd(), libc::F_SETFL, flags | libc::O_NONBLOCK);
        }
    }
}
