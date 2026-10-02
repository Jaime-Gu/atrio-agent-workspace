//! Physical-directory leases and conservative scoped Restart Manager checks.
use super::paths::{
    directory_identity, handle_identity, open_directory, reject_reparse, wide_path,
};
use std::{fs, path::Path};
use std::{
    fs::File,
    os::windows::io::{FromRawHandle, OwnedHandle},
    thread,
    time::{Duration, Instant},
};
use windows_sys::Win32::{
    Foundation::{GetLastError, ERROR_ALREADY_EXISTS, ERROR_MORE_DATA, ERROR_SUCCESS},
    System::{
        RestartManager::{
            RmEndSession, RmGetList, RmRegisterResources, RmStartSession, RM_PROCESS_INFO,
        },
        Threading::CreateMutexW,
    },
};

#[derive(Debug)]
pub(crate) struct DirectoryGuard {
    file: File,
    identity: String,
    _object: OwnedHandle,
}

impl DirectoryGuard {
    pub(crate) fn acquire(root: &Path, occupied: &str) -> Result<Self, String> {
        let file = open_directory(root)?;
        let identity = handle_identity(&file)?;
        let name: Vec<u16> = format!("Global\\AtrioWorkspace.DirectoryLease.{identity}")
            .encode_utf16()
            .chain(Some(0))
            .collect();
        let started = Instant::now();
        loop {
            // Exclusivity uses kernel object lifetime, not mutex thread
            // ownership. Only the creator of a previously absent object
            // receives a lease. No removable lock file, inherited handle,
            // channel name, path spelling or workspace UUID affects it.
            let raw = unsafe { CreateMutexW(std::ptr::null(), 0, name.as_ptr()) };
            let status = unsafe { GetLastError() };
            if raw.is_null() {
                return Err(format!(
                    "无法取得 Windows 目录进程锁：{}；已停止打开",
                    std::io::Error::from_raw_os_error(status as i32)
                ));
            }
            let object = unsafe { OwnedHandle::from_raw_handle(raw) };
            if status != ERROR_ALREADY_EXISTS {
                return Ok(Self {
                    file,
                    identity,
                    _object: object,
                });
            }
            drop(object);
            if started.elapsed() >= Duration::from_millis(150) {
                return Err(format!(
                    "{occupied}：{}。请保存内容并正常退出占用它的应用后重试",
                    root.display()
                ));
            }
            thread::sleep(Duration::from_millis(10));
        }
    }

    pub(crate) fn ensure_valid(&self, root: &Path) -> Result<(), String> {
        if handle_identity(&self.file)? != self.identity
            || directory_identity(root)
                .map_err(|e| format!("已锁定目录消失或不可访问：{e}；请重新打开工作区"))?
                != self.identity
        {
            return Err(
                "已锁定目录已被移动、替换或重建；为防止写入错误目录，操作已停止，请重新打开工作区"
                    .into(),
            );
        }
        Ok(())
    }
}

struct RestartSession(u32);
impl Drop for RestartSession {
    fn drop(&mut self) {
        unsafe {
            RmEndSession(self.0);
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
struct ExternalUser {
    pid: u32,
    created: u64,
}

fn scoped_files(root: &Path) -> Result<Vec<Vec<u16>>, String> {
    let mut directories = vec![root.to_owned()];
    let mut files = Vec::new();
    let mut visited = 0;
    while let Some(directory) = directories.pop() {
        for entry in fs::read_dir(directory)
            .map_err(|e| format!("历史写入者检查无法列举目录：{e}；已停止接管"))?
        {
            let entry = entry.map_err(|e| format!("历史写入者检查不完整：{e}；已停止接管"))?;
            visited += 1;
            if visited > 20_000 {
                return Err("历史写入者检查超过 20000 项安全上限；无法完整确认，已停止接管".into());
            }
            let path = entry.path();
            reject_reparse(&path)?;
            let kind = entry.file_type().map_err(|e| e.to_string())?;
            if kind.is_dir() {
                directories.push(path);
            } else if kind.is_file() {
                files.push(wide_path(&path).map_err(|e| e.to_string())?);
            } else {
                return Err("历史写入者检查遇到不支持的文件类型；已停止接管".into());
            }
        }
    }
    Ok(files)
}

fn probe_users(root: &Path) -> Result<Vec<ExternalUser>, String> {
    let files = scoped_files(root)?;
    if files.is_empty() {
        return Ok(Vec::new());
    }
    let mut handle = 0;
    let mut key = [0u16; 33];
    let status = unsafe { RmStartSession(&mut handle, 0, key.as_mut_ptr()) };
    if status != ERROR_SUCCESS {
        return Err(format!(
            "无法启动 Windows 历史写入者检查（{status}）；已停止接管"
        ));
    }
    let session = RestartSession(handle);
    for batch in files.chunks(512) {
        let names: Vec<*const u16> = batch.iter().map(|path| path.as_ptr()).collect();
        let status = unsafe {
            RmRegisterResources(
                session.0,
                names.len() as u32,
                names.as_ptr(),
                0,
                std::ptr::null(),
                0,
                std::ptr::null(),
            )
        };
        if status != ERROR_SUCCESS {
            return Err(format!("无法注册工作区文件检查（{status}）；已停止接管"));
        }
    }
    let mut needed = 0;
    let mut count = 0;
    let mut reasons = 0;
    let status = unsafe {
        RmGetList(
            session.0,
            &mut needed,
            &mut count,
            std::ptr::null_mut(),
            &mut reasons,
        )
    };
    if status == ERROR_SUCCESS {
        return Ok(Vec::new());
    }
    if status != ERROR_MORE_DATA {
        return Err(format!(
            "Windows 历史写入者检查失败（{status}）；已停止接管"
        ));
    }
    for _ in 0..3 {
        if needed > 4096 {
            return Err("历史写入者数量超过安全上限；已停止接管".into());
        }
        let mut processes: Vec<RM_PROCESS_INFO> =
            (0..needed).map(|_| unsafe { std::mem::zeroed() }).collect();
        count = processes.len() as u32;
        let status = unsafe {
            RmGetList(
                session.0,
                &mut needed,
                &mut count,
                processes.as_mut_ptr(),
                &mut reasons,
            )
        };
        if status == ERROR_MORE_DATA {
            continue;
        }
        if status != ERROR_SUCCESS {
            return Err(format!(
                "Windows 历史写入者检查失败（{status}）；已停止接管"
            ));
        }
        return Ok(processes
            .into_iter()
            .take(count as usize)
            .filter(|info| info.Process.dwProcessId != std::process::id())
            .map(|info| ExternalUser {
                pid: info.Process.dwProcessId,
                created: (u64::from(info.Process.ProcessStartTime.dwHighDateTime) << 32)
                    | u64::from(info.Process.ProcessStartTime.dwLowDateTime),
            })
            .collect());
    }
    Err("历史写入者清单持续变化，无法完整确认；已停止接管".into())
}

pub(crate) fn check_external_users(root: &Path) -> Result<(), String> {
    // Restart Manager reports scoped file users, not handle access masks.
    // Consequently read-only users are conservatively blocked too. Never
    // call RmShutdown/RmRestart or enumerate unrelated process arguments.
    let mut previous = probe_users(root)?;
    for _ in 0..3 {
        if previous.is_empty() {
            return Ok(());
        }
        thread::sleep(Duration::from_millis(40));
        let current = probe_users(root)?;
        let stable: Vec<_> = current
            .iter()
            .filter(|user| previous.contains(user))
            .collect();
        if !stable.is_empty() {
            let details = stable
                .iter()
                .take(4)
                .map(|user| format!("PID {}", user.pid))
                .collect::<Vec<_>>()
                .join("；");
            return Err(format!("检测到工作区仍被其他进程打开：{details}。Windows 检查不能区分只读和写入句柄；请正常关闭相关程序后重试，本应用未接管、未终止任何进程"));
        }
        previous = current;
    }
    if previous.is_empty() {
        Ok(())
    } else {
        Err("工作区外部句柄持续变化，无法确认旧应用已退出；已停止接管".into())
    }
}
