//! Process locks belong to directory inodes, never to a removable PID/lock file.
//!
//! A workspace guard must be acquired before opening SQLite or running migrations,
//! held until its connection is dropped, and checked before mutations. `flock` is
//! advisory: historical applications do not participate, so acquisition also probes
//! their open writable handles without reading any user document or database.

use std::{
    fs,
    path::{Path, PathBuf},
};

#[cfg(unix)]
use std::{
    fs::File,
    io::Read,
    os::{
        fd::AsRawFd,
        unix::{ffi::OsStringExt, fs::MetadataExt},
    },
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

#[derive(Debug)]
struct DirectoryLock {
    root: PathBuf,
    #[cfg(unix)]
    file: File,
    #[cfg(unix)]
    device: u64,
    #[cfg(unix)]
    inode: u64,
    #[cfg(windows)]
    guard: crate::platform::windows::locks::DirectoryGuard,
}

impl DirectoryLock {
    fn acquire(path: &Path, occupied: &str) -> Result<Self, String> {
        #[cfg(not(any(unix, windows)))]
        {
            let _ = (path, occupied);
            Err("当前平台尚未实现目录进程锁；为保护工作区，已停止打开".into())
        }
        #[cfg(windows)]
        {
            let root = fs::canonicalize(path).map_err(|e| format!("无法解析锁定目录：{e}"))?;
            let guard = crate::platform::windows::locks::DirectoryGuard::acquire(&root, occupied)?;
            let lock = Self { root, guard };
            lock.ensure_valid()?;
            Ok(lock)
        }
        #[cfg(unix)]
        {
            let root =
                fs::canonicalize(path).map_err(|error| format!("无法解析锁定目录：{error}"))?;
            let file = File::open(&root)
                .map_err(|error| format!("无法打开锁定目录 {}：{error}", root.display()))?;
            let metadata = file.metadata().map_err(|error| error.to_string())?;
            if !metadata.is_dir() {
                return Err("锁定目标必须是文件夹".into());
            }
            let started = Instant::now();
            loop {
                // SAFETY: `file` owns an open fd and remains alive throughout
                // this guard. Close releases this open file description's lock,
                // including process crash / SIGKILL. A concurrently forked child
                // can briefly inherit a just-closed lock until its CLOEXEC runs.
                if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0 {
                    break;
                }
                let error = std::io::Error::last_os_error();
                if error.kind() == std::io::ErrorKind::WouldBlock
                    && started.elapsed() < Duration::from_millis(150)
                {
                    thread::sleep(Duration::from_millis(10));
                    continue;
                }
                return Err(if error.kind() == std::io::ErrorKind::WouldBlock {
                    format!(
                        "{occupied}：{}。请保存内容并正常退出占用它的应用后重试",
                        root.display()
                    )
                } else {
                    format!(
                        "无法取得目录进程锁 {}：{error}；已停止打开以保护数据",
                        root.display()
                    )
                });
            }
            let lock = Self {
                root,
                file,
                device: metadata.dev(),
                inode: metadata.ino(),
            };
            lock.ensure_valid()?;
            Ok(lock)
        }
    }

    fn ensure_valid(&self) -> Result<(), String> {
        #[cfg(not(any(unix, windows)))]
        {
            Err("当前平台尚未实现目录进程锁".into())
        }
        #[cfg(windows)]
        {
            self.guard.ensure_valid(&self.root)
        }
        #[cfg(unix)]
        {
            let metadata = fs::metadata(&self.root)
                .map_err(|error| format!("已锁定目录消失或不可访问：{error}；请重新打开工作区"))?;
            let held = self.file.metadata().map_err(|error| error.to_string())?;
            if !metadata.is_dir()
                || metadata.dev() != self.device
                || metadata.ino() != self.inode
                || held.dev() != self.device
                || held.ino() != self.inode
            {
                return Err("已锁定目录已被移动、替换或重建；为防止写入错误目录，操作已停止，请重新打开工作区".into());
            }
            Ok(())
        }
    }
}

/// One instance per channel's canonical application data directory.
#[derive(Debug)]
pub struct ChannelInstanceLock(DirectoryLock);

impl ChannelInstanceLock {
    pub fn acquire(channel_data_directory: &Path) -> Result<Self, String> {
        #[cfg(windows)]
        crate::platform::windows::paths::validate_root_path(channel_data_directory)?;
        fs::create_dir_all(channel_data_directory)
            .map_err(|error| format!("无法创建应用数据目录：{error}"))?;
        DirectoryLock::acquire(channel_data_directory, "此渠道的 Atrio WorkSpace 已在运行")
            .map(Self)
    }

    pub fn ensure_valid(&self) -> Result<(), String> {
        self.0.ensure_valid()
    }
}

/// An exclusive lease for one physical workspace across all application channels.
#[derive(Debug)]
pub struct WorkspaceLock(DirectoryLock);

impl WorkspaceLock {
    pub fn acquire(root: &Path) -> Result<Self, String> {
        let guard = Self(DirectoryLock::acquire(
            root,
            "工作区已由另一个 Atrio WorkSpace 实例占用",
        )?);
        guard.check_external_writers()?;
        guard.ensure_valid()?;
        Ok(guard)
    }

    pub fn root(&self) -> &Path {
        &self.0.root
    }

    /// Cheap inode validation suitable for every mutation and active timer tick.
    pub fn ensure_valid(&self) -> Result<(), String> {
        self.0.ensure_valid()
    }

    /// Check non-cooperating / historical writers before a legacy takeover or
    /// explicit write boundary. This is a conservative observation, not a sandbox:
    /// no advisory protocol can stop an old application started after this check.
    pub fn check_external_writers(&self) -> Result<(), String> {
        self.check_external_writers_excluding(None)
    }

    pub fn check_external_writers_excluding(
        &self,
        managed_group: Option<u32>,
    ) -> Result<(), String> {
        self.ensure_valid()?;
        #[cfg(target_os = "macos")]
        {
            let mut previous = probe_writers(self.root(), Path::new("/usr/sbin/lsof"))?;
            let is_managed = |writer: &ExternalWriter| {
                managed_group.is_some_and(|group| unsafe {
                    libc::getpgid(group as i32) == group as i32
                        && libc::getpgid(writer.pid as i32) == group as i32
                })
            };
            previous.retain(|writer| !is_managed(writer));
            // A fork/exec briefly inherits its parent's SQLite descriptors even
            // with CLOEXEC. Only repeated observations of the same PID and path
            // establish another writer. If observations keep changing, fail
            // conservatively instead of granting an unverified takeover.
            for _ in 0..3 {
                if previous.is_empty() {
                    return Ok(());
                }
                thread::sleep(Duration::from_millis(40));
                self.ensure_valid()?;
                let mut current = probe_writers(self.root(), Path::new("/usr/sbin/lsof"))?;
                current.retain(|writer| !is_managed(writer));
                let stable = current
                    .iter()
                    .filter(|writer| previous.contains(writer))
                    .collect::<Vec<_>>();
                if stable.is_empty() {
                    previous = current;
                    continue;
                }
                let details = stable
                    .iter()
                    .take(4)
                    .map(|writer| format!("PID {} ({})", writer.pid, writer.path.display()))
                    .collect::<Vec<_>>()
                    .join("；");
                return Err(format!(
                    "检测到工作区仍被其他进程打开写入，可能包括不支持新锁的旧版应用：{details}。请先保存草稿并正常退出相关应用；本应用未接管、未终止任何进程"
                ));
            }
            if previous.is_empty() {
                return Ok(());
            }
            Err("工作区外部写入句柄持续变化，无法确认旧应用已退出；为保护数据已停止接管，请稍后重试".into())
        }
        #[cfg(windows)]
        {
            // A PID alone is not proof of Job Object ownership. Do not exempt
            // another process without its retained creation identity.
            let _ = managed_group;
            crate::platform::windows::locks::check_external_users(self.root())
        }
        #[cfg(not(any(target_os = "macos", windows)))]
        {
            Err("当前平台尚未实现历史写入者检查；为保护旧数据，已停止接管".into())
        }
    }
}

#[cfg(unix)]
#[derive(Debug, PartialEq, Eq)]
struct ExternalWriter {
    pid: u32,
    path: PathBuf,
}

#[cfg(unix)]
fn probe_writers(root: &Path, executable: &Path) -> Result<Vec<ExternalWriter>, String> {
    // +D restricts the inventory to this physical workspace; -F0 emits structured
    // NUL-delimited PID, fd, access mode and path fields. Never request contents,
    // process argv, environment, credentials, or account configuration.
    let mut child = Command::new(executable)
        .args(["-n", "-P", "-F0pfan", "+D"])
        .arg(root)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| {
            format!("无法检查旧应用是否仍在写入（lsof：{error}）；为保护数据，已停止接管")
        })?;
    let probe_pid = child.id();
    const OUTPUT_LIMIT: u64 = 2 * 1024 * 1024;
    let stdout = child.stdout.take().expect("piped stdout");
    let stderr = child.stderr.take().expect("piped stderr");
    let output_reader = thread::spawn(move || {
        let mut bytes = Vec::new();
        stdout
            .take(OUTPUT_LIMIT + 1)
            .read_to_end(&mut bytes)
            .map(|_| bytes)
    });
    let error_reader = thread::spawn(move || {
        let mut bytes = Vec::new();
        stderr
            .take(OUTPUT_LIMIT + 1)
            .read_to_end(&mut bytes)
            .map(|_| bytes)
    });
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Ok(status),
            Ok(None) if started.elapsed() < Duration::from_secs(5) => {
                thread::sleep(Duration::from_millis(20))
            }
            result => {
                let _ = child.kill(); // Only this invocation's own lsof process.
                let _ = child.wait();
                break Err(match result {
                    Err(error) => format!("历史写入者检查失败：{error}；已停止接管"),
                    _ => "历史写入者检查超时；无法确认旧应用已退出，已停止接管".into(),
                });
            }
        }
    };
    let output = output_reader
        .join()
        .map_err(|_| "历史写入者检查输出读取失败")?
        .map_err(|error| error.to_string())?;
    let errors = error_reader
        .join()
        .map_err(|_| "历史写入者检查诊断读取失败")?
        .map_err(|error| error.to_string())?;
    let status = status?;
    if output.len() as u64 > OUTPUT_LIMIT || errors.len() as u64 > OUTPUT_LIMIT {
        return Err("历史写入者检查结果超出安全上限；无法完整确认，已停止接管".into());
    }
    if !errors.is_empty() {
        return Err(format!(
            "无法完整检查历史写入者，已停止接管：{}",
            String::from_utf8_lossy(&errors).trim()
        ));
    }
    // +D expands into path search arguments. macOS lsof exits 1 if any of those
    // files has no open handle, even when other files produced valid matches.
    // Its real errors have stderr diagnostics (rejected above); both 0 and 1
    // are therefore valid inventory exits. Signals and other codes fail closed.
    if !status.success() && status.code() != Some(1) {
        return Err(format!("历史写入者检查异常退出（{status}）；已停止接管"));
    }
    parse_scoped_writers(&output, &[std::process::id(), probe_pid])
}

#[cfg(unix)]
fn parse_scoped_writers(
    output: &[u8],
    ignored_pids: &[u32],
) -> Result<Vec<ExternalWriter>, String> {
    let mut pid = None;
    let mut numeric_fd = false;
    let mut writable = None;
    let mut writers = Vec::new();
    for record in output.split(|byte| *byte == 0) {
        let record = record.strip_prefix(b"\n").unwrap_or(record);
        if record.is_empty() {
            continue;
        }
        match record[0] {
            b'p' => {
                pid = Some(
                    std::str::from_utf8(&record[1..])
                        .ok()
                        .and_then(|value| value.parse::<u32>().ok())
                        .ok_or("历史写入者 PID 无法解析；已停止接管")?,
                );
                numeric_fd = false;
                writable = None;
            }
            b'f' => {
                numeric_fd = !record[1..].is_empty() && record[1..].iter().all(u8::is_ascii_digit);
                writable = None;
            }
            b'a' => {
                writable = match record.get(1) {
                    Some(b'w' | b'u') => Some(true),
                    Some(b'r') => Some(false),
                    _ => None,
                }
            }
            b'n' => {
                let pid = pid.ok_or("历史写入者检查缺少 PID；已停止接管")?;
                if !ignored_pids.contains(&pid) && numeric_fd {
                    let path = PathBuf::from(std::ffi::OsString::from_vec(record[1..].to_vec()));
                    // +D already filters by filesystem identity. Do not filter
                    // its display names again: lsof escapes control / Unicode
                    // bytes, and a file may be opened via an external hard link.
                    // These names are diagnostics only, never filesystem inputs.
                    let writable =
                        writable.ok_or("无法确认工作区外部句柄的访问模式；已停止接管")?;
                    if !writable {
                        continue;
                    }
                    let writer = ExternalWriter { pid, path };
                    if !writers.contains(&writer) {
                        writers.push(writer);
                    }
                }
            }
            _ => return Err("历史写入者检查格式异常；已停止接管".into()),
        }
    }
    Ok(writers)
}

#[cfg(all(test, windows))]
mod windows_tests {
    use super::*;
    use std::{
        io::{BufRead, BufReader, Write},
        os::windows::process::CommandExt,
        process::{Child, ChildStdout, Command, Stdio},
    };

    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            let root =
                std::env::temp_dir().join(format!("atrio-windows-lock-{}", uuid::Uuid::new_v4()));
            fs::create_dir_all(&root).unwrap();
            Self(root)
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    struct ChildGuard {
        child: Child,
        _output: BufReader<ChildStdout>,
    }
    impl ChildGuard {
        fn start(root: &Path, mode: &str) -> Self {
            let mut child = Command::new(std::env::current_exe().unwrap())
                .args(["--ignored", "windows_process_lock_helper", "--nocapture"])
                .env("ATRIO_WINDOWS_LOCK_ROOT", root)
                .env("ATRIO_WINDOWS_LOCK_MODE", mode)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .creation_flags(0x08000000)
                .spawn()
                .unwrap();
            let mut output = BufReader::new(child.stdout.take().unwrap());
            loop {
                let mut line = String::new();
                assert!(
                    output.read_line(&mut line).unwrap() > 0,
                    "Windows lock helper exited before ready"
                );
                if line.trim() == "ATRIO_WINDOWS_LOCK_READY" {
                    break;
                }
            }
            Self {
                child,
                _output: output,
            }
        }
        fn crash(&mut self) {
            self.child.kill().unwrap();
            self.child.wait().unwrap();
        }
    }
    impl Drop for ChildGuard {
        fn drop(&mut self) {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }

    #[test]
    #[ignore = "subprocess helper invoked by Windows lock tests"]
    fn windows_process_lock_helper() {
        let Ok(root) = std::env::var("ATRIO_WINDOWS_LOCK_ROOT") else {
            return;
        };
        let root = Path::new(&root);
        let mode = std::env::var("ATRIO_WINDOWS_LOCK_MODE").unwrap();
        let _workspace;
        let _channel;
        let _legacy;
        match mode.as_str() {
            "workspace" => _workspace = Some(WorkspaceLock::acquire(root).unwrap()),
            "channel" => _channel = Some(ChannelInstanceLock::acquire(root).unwrap()),
            "legacy" | "reader" => {
                fs::create_dir_all(root.join(".workspace")).unwrap();
                let database = root.join(".workspace/workspace.sqlite3");
                fs::write(&database, b"synthetic fixture; never read by writer probe").unwrap();
                _legacy = Some(
                    fs::OpenOptions::new()
                        .read(true)
                        .write(mode == "legacy")
                        .open(database)
                        .unwrap(),
                );
            }
            _ => panic!("unknown helper mode"),
        }
        println!("ATRIO_WINDOWS_LOCK_READY");
        std::io::stdout().flush().unwrap();
        let mut line = String::new();
        std::io::stdin().read_line(&mut line).unwrap();
    }

    fn junction(target: &Path, alias: &Path) {
        // Powershell literal strings escape apostrophes without interpolation.
        let literal = |path: &Path| {
            path.to_string_lossy()
                .replace('\\', "/")
                .replace('\'', "''")
        };
        let script = format!("$ErrorActionPreference='Stop'; New-Item -ItemType Junction -Path '{}' -Target '{}' | Out-Null", literal(alias), literal(target));
        let executable = PathBuf::from(std::env::var_os("SystemRoot").unwrap())
            .join("System32/WindowsPowerShell/v1.0/powershell.exe");
        let output = Command::new(executable)
            .args(["-NoProfile", "-NonInteractive", "-Command", &script])
            .creation_flags(0x08000000)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    #[test]
    fn windows_cross_process_lock_crash_releases_and_same_process_also_competes() {
        let fixture = Fixture::new();
        let mut child = ChildGuard::start(&fixture.0, "workspace");
        assert!(WorkspaceLock::acquire(&fixture.0)
            .unwrap_err()
            .contains("另一个"));
        child.crash();
        let _guard = WorkspaceLock::acquire(&fixture.0).unwrap();
        assert!(WorkspaceLock::acquire(&fixture.0).is_err());
    }

    #[test]
    fn windows_channels_are_separate_but_their_workspace_lock_is_shared() {
        let fixture = Fixture::new();
        let dev = fixture.0.join("dev");
        let _child = ChildGuard::start(&dev, "channel");
        assert!(ChannelInstanceLock::acquire(&dev)
            .unwrap_err()
            .contains("此渠道"));
        let _beta = ChannelInstanceLock::acquire(&fixture.0.join("beta")).unwrap();
        let _workspace = WorkspaceLock::acquire(&fixture.0).unwrap();
        assert!(WorkspaceLock::acquire(&fixture.0).is_err());
    }

    #[test]
    fn windows_case_and_junction_aliases_compete_copies_are_independent() {
        let fixture = Fixture::new();
        let original = fixture.0.join("workspace");
        let copy = fixture.0.join("copy");
        fs::create_dir_all(&original).unwrap();
        fs::create_dir_all(&copy).unwrap();
        fs::write(original.join("manifest.json"), b"same manifest").unwrap();
        fs::copy(original.join("manifest.json"), copy.join("manifest.json")).unwrap();
        let alias = fixture.0.join("alias");
        junction(&original, &alias);
        let _child = ChildGuard::start(&original, "workspace");
        assert!(WorkspaceLock::acquire(&alias).is_err());
        assert!(WorkspaceLock::acquire(&fixture.0.join("WORKSPACE")).is_err());
        WorkspaceLock::acquire(&copy).unwrap();
    }

    #[test]
    fn windows_directory_replacement_invalidates_guard_and_lock_file_deletion_does_not() {
        let fixture = Fixture::new();
        let root = fixture.0.join("workspace");
        fs::create_dir(&root).unwrap();
        let guard = WorkspaceLock::acquire(&root).unwrap();
        fs::write(root.join("workspace.lock"), "irrelevant").unwrap();
        fs::remove_file(root.join("workspace.lock")).unwrap();
        assert!(WorkspaceLock::acquire(&root).is_err());
        let moved = fixture.0.join("moved");
        fs::rename(&root, &moved).unwrap();
        fs::create_dir(&root).unwrap();
        assert!(guard.ensure_valid().unwrap_err().contains("替换"));
        assert!(WorkspaceLock::acquire(&moved).is_err());
    }

    #[test]
    fn windows_legacy_writer_blocks_takeover_and_later_write_boundary() {
        let fixture = Fixture::new();
        let guard = WorkspaceLock::acquire(&fixture.0).unwrap();
        let mut child = ChildGuard::start(&fixture.0, "legacy");
        let error = guard.check_external_writers().unwrap_err();
        assert!(error.contains("仍被其他进程"), "{error}");
        assert!(error.contains(&child.child.id().to_string()), "{error}");
        drop(guard);
        assert!(WorkspaceLock::acquire(&fixture.0).is_err());
        child.crash();
        WorkspaceLock::acquire(&fixture.0).unwrap();
    }

    #[test]
    fn windows_readers_are_conservatively_rejected_because_access_mode_is_unknown() {
        let fixture = Fixture::new();
        let _child = ChildGuard::start(&fixture.0, "reader");
        let error = WorkspaceLock::acquire(&fixture.0).unwrap_err();
        assert!(error.contains("不能区分只读和写入"), "{error}");
    }

    #[test]
    fn windows_internal_junction_fails_closed_without_traversing_it() {
        let fixture = Fixture::new();
        let outside = Fixture::new();
        let alias = fixture.0.join("external");
        junction(&outside.0, &alias);
        assert!(crate::platform::windows::paths::reject_reparse(&alias).is_err());
        assert!(WorkspaceLock::acquire(&fixture.0)
            .unwrap_err()
            .contains("reparse"));
    }
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;
    use std::{
        io::{BufRead, BufReader, Write},
        os::unix::fs::symlink,
        process::{Child, ChildStdout},
    };

    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!(
                "pixel-directory-lock-test-{}",
                uuid::Uuid::new_v4()
            ));
            fs::create_dir_all(&root).unwrap();
            Self(fs::canonicalize(root).unwrap())
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    struct ChildGuard {
        child: Child,
        output: BufReader<ChildStdout>,
    }
    impl ChildGuard {
        fn start(root: &Path, mode: &str) -> Self {
            let mut child = Command::new(std::env::current_exe().unwrap())
                .args(["--ignored", "process_lock_helper", "--nocapture"])
                .env("PIXEL_LOCK_TEST_ROOT", root)
                .env("PIXEL_LOCK_TEST_MODE", mode)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap();
            let output = BufReader::new(child.stdout.take().unwrap());
            let mut guard = Self { child, output };
            let mut line = String::new();
            loop {
                line.clear();
                assert!(
                    guard.output.read_line(&mut line).unwrap() > 0,
                    "lock helper exited before ready"
                );
                if line.trim() == "PIXEL_LOCK_READY" {
                    break;
                }
            }
            guard
        }
        fn crash(&mut self) {
            self.child.kill().unwrap();
            self.child.wait().unwrap();
        }
    }
    impl Drop for ChildGuard {
        fn drop(&mut self) {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }

    #[test]
    #[ignore = "subprocess helper, only invoked with isolated fixture environment"]
    fn process_lock_helper() {
        let Ok(root) = std::env::var("PIXEL_LOCK_TEST_ROOT") else {
            return;
        };
        let root = Path::new(&root);
        let mode = std::env::var("PIXEL_LOCK_TEST_MODE").unwrap();
        let _workspace;
        let _channel;
        let _legacy;
        match mode.as_str() {
            "workspace" => _workspace = Some(WorkspaceLock::acquire(root).unwrap()),
            "channel" => _channel = Some(ChannelInstanceLock::acquire(root).unwrap()),
            "legacy" | "reader" => {
                fs::create_dir_all(root.join(".workspace")).unwrap();
                let database = root.join(".workspace/workspace.sqlite3");
                fs::write(&database, "contents must not be read by the lock probe").unwrap();
                _legacy = Some(
                    fs::OpenOptions::new()
                        .read(true)
                        .write(mode == "legacy")
                        .open(database)
                        .unwrap(),
                );
            }
            _ => panic!("unknown helper mode"),
        }
        println!("PIXEL_LOCK_READY");
        std::io::stdout().flush().unwrap();
        let mut line = String::new();
        std::io::stdin().read_line(&mut line).unwrap();
    }

    #[test]
    fn workspace_competition_is_cross_process_and_crash_releases_lock() {
        let fixture = Fixture::new();
        let mut child = ChildGuard::start(&fixture.0, "workspace");
        assert!(WorkspaceLock::acquire(&fixture.0)
            .unwrap_err()
            .contains("另一个"));
        child.crash();
        WorkspaceLock::acquire(&fixture.0).unwrap();
    }

    #[test]
    fn channel_instances_compete_but_different_channels_can_coexist() {
        let fixture = Fixture::new();
        let dev = fixture.0.join("dev");
        let mut child = ChildGuard::start(&dev, "channel");
        assert!(ChannelInstanceLock::acquire(&dev)
            .unwrap_err()
            .contains("此渠道"));
        ChannelInstanceLock::acquire(&fixture.0.join("beta"))
            .unwrap()
            .ensure_valid()
            .unwrap();
        child.crash();
        ChannelInstanceLock::acquire(&dev).unwrap();
    }

    #[test]
    fn a_briefly_inherited_process_lock_is_retried_before_reporting_conflict() {
        let fixture = Fixture::new();
        let mut child = ChildGuard::start(&fixture.0, "channel");
        let release = thread::spawn(move || {
            thread::sleep(Duration::from_millis(40));
            child
                .child
                .stdin
                .as_mut()
                .unwrap()
                .write_all(b"\n")
                .unwrap();
            assert!(child.child.wait().unwrap().success());
        });
        let guard = ChannelInstanceLock::acquire(&fixture.0).unwrap();
        release.join().unwrap();
        guard.ensure_valid().unwrap();
    }

    #[test]
    fn a_brief_external_descriptor_does_not_look_like_a_persistent_legacy_writer() {
        let fixture = Fixture::new();
        let mut child = ChildGuard::start(&fixture.0, "legacy");
        let release = thread::spawn(move || {
            thread::sleep(Duration::from_millis(40));
            child
                .child
                .stdin
                .as_mut()
                .unwrap()
                .write_all(b"\n")
                .unwrap();
            assert!(child.child.wait().unwrap().success());
        });
        WorkspaceLock::acquire(&fixture.0).unwrap();
        release.join().unwrap();
    }

    #[test]
    fn physical_alias_competes_and_copied_directory_has_independent_lock() {
        let fixture = Fixture::new();
        let original = fixture.0.join("original");
        let copy = fixture.0.join("copy");
        fs::create_dir_all(original.join(".workspace")).unwrap();
        fs::create_dir_all(copy.join(".workspace")).unwrap();
        fs::write(
            original.join(".workspace/workspace.json"),
            br#"{"id":"same-manifest-id"}"#,
        )
        .unwrap();
        fs::copy(
            original.join(".workspace/workspace.json"),
            copy.join(".workspace/workspace.json"),
        )
        .unwrap();
        symlink(&original, fixture.0.join("alias")).unwrap();
        let _guard = ChildGuard::start(&original, "workspace");
        assert!(WorkspaceLock::acquire(&fixture.0.join("alias")).is_err());
        WorkspaceLock::acquire(&copy).unwrap();
    }

    #[test]
    fn deleting_a_lock_file_cannot_break_directory_lock_and_replacement_is_detected() {
        let fixture = Fixture::new();
        let original = fixture.0.join("workspace");
        fs::create_dir(&original).unwrap();
        let guard = WorkspaceLock::acquire(&original).unwrap();
        fs::write(original.join("workspace.lock"), "discardable").unwrap();
        fs::remove_file(original.join("workspace.lock")).unwrap();
        fs::write(original.join("workspace.lock"), "replacement").unwrap();
        assert!(WorkspaceLock::acquire(&original).is_err());
        let moved = fixture.0.join("moved");
        fs::rename(&original, &moved).unwrap();
        fs::create_dir(&original).unwrap();
        assert!(guard.ensure_valid().unwrap_err().contains("替换"));
        assert!(WorkspaceLock::acquire(&moved).is_err());
    }

    #[test]
    fn legacy_open_writer_blocks_takeover_until_it_exits() {
        let fixture = Fixture::new();
        let mut child = ChildGuard::start(&fixture.0, "legacy");
        let error = WorkspaceLock::acquire(&fixture.0).unwrap_err();
        assert!(error.contains("仍被其他进程"), "{error}");
        assert!(error.contains(&child.child.id().to_string()), "{error}");
        child.crash();
        WorkspaceLock::acquire(&fixture.0).unwrap();
    }

    #[test]
    fn external_read_only_handle_does_not_block_a_new_workspace_writer() {
        let fixture = Fixture::new();
        let _child = ChildGuard::start(&fixture.0, "reader");
        WorkspaceLock::acquire(&fixture.0).unwrap();
    }

    #[test]
    fn unavailable_legacy_probe_fails_closed() {
        let fixture = Fixture::new();
        assert!(probe_writers(&fixture.0, &fixture.0.join("missing-lsof"))
            .unwrap_err()
            .contains("已停止接管"));
    }

    #[test]
    fn explicit_write_boundary_detects_legacy_writer_started_after_acquisition() {
        let fixture = Fixture::new();
        let guard = WorkspaceLock::acquire(&fixture.0).unwrap();
        let _child = ChildGuard::start(&fixture.0, "legacy");
        assert!(guard
            .check_external_writers()
            .unwrap_err()
            .contains("仍被其他进程"));
    }

    #[test]
    fn escaped_workspace_names_cannot_hide_a_legacy_writer() {
        let fixture = Fixture::new();
        let root = fixture.0.join("中文工区\nwith-newline");
        fs::create_dir(&root).unwrap();
        let _child = ChildGuard::start(&root, "legacy");
        assert!(WorkspaceLock::acquire(&root)
            .unwrap_err()
            .contains("仍被其他进程"));
    }

    #[test]
    fn scoped_parser_keeps_external_writable_handles_including_hardlink_names() {
        let bytes = b"p10\0\nf3\0au\0n/workspace/.workspace/workspace.sqlite3\0\np20\0\nfcwd\0a \0n/workspace\0\nf3\0ar\0n/workspace/notes/read.md\0\nf4\0aw\0n/workspace-copy/notes/wrong.md\0\nf5\0au\0n/workspace/notes/right\nname.md\0\n";
        assert_eq!(
            parse_scoped_writers(bytes, &[10]).unwrap(),
            vec![
                ExternalWriter {
                    pid: 20,
                    path: PathBuf::from("/workspace-copy/notes/wrong.md")
                },
                ExternalWriter {
                    pid: 20,
                    path: PathBuf::from("/workspace/notes/right\nname.md")
                }
            ]
        );
        assert!(parse_scoped_writers(b"pwat\0", &[10]).is_err());
        assert!(parse_scoped_writers(b"p20\0f3\0a \0n/workspace/db\0", &[10]).is_err());
    }
}
