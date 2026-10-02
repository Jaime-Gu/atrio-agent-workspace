//! Windows path, containment and physical-directory identity checks.
use std::{
    fs::{self, File, OpenOptions},
    io,
    os::windows::{
        ffi::OsStrExt,
        fs::{MetadataExt, OpenOptionsExt},
        io::AsRawHandle,
    },
    path::{Component, Path, Prefix},
};
use windows_sys::Win32::Storage::FileSystem::{
    FileIdInfo, GetDriveTypeW, GetFileInformationByHandleEx, FILE_ATTRIBUTE_REPARSE_POINT,
    FILE_FLAG_BACKUP_SEMANTICS, FILE_ID_INFO,
};

pub(crate) fn wide_path(path: &Path) -> io::Result<Vec<u16>> {
    let mut value: Vec<u16> = path.as_os_str().encode_wide().collect();
    if value.contains(&0) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "path contains NUL",
        ));
    }
    value.push(0);
    Ok(value)
}

pub(crate) fn open_directory(path: &Path) -> Result<File, String> {
    // Canonical Windows paths have a verbatim disk prefix. Network roots
    // need separately proven server identity and locking semantics.
    let canonical = fs::canonicalize(path).map_err(|e| e.to_string())?;
    validate_root_path(&canonical)?;
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
        .open(canonical)
        .map_err(|e| format!("无法打开目录身份句柄：{e}"))?;
    if !file.metadata().map_err(|e| e.to_string())?.is_dir() {
        return Err("锁定目标必须是文件夹".into());
    }
    Ok(file)
}

/// Run before creating a user-selected directory. Only absolute local disk
/// paths are eligible; normalization must not silently change its name.
pub(crate) fn validate_root_path(path: &Path) -> Result<(), String> {
    if !path.is_absolute() {
        return Err("Windows 工作区必须使用完整的本机磁盘绝对路径".into());
    }
    let drive = match path.components().next() {
        Some(Component::Prefix(prefix)) => match prefix.kind() {
            Prefix::Disk(drive) | Prefix::VerbatimDisk(drive) => drive,
            _ => {
                return Err(
                    "Windows 当前仅支持本机磁盘工作区；UNC/设备路径尚未验收，已停止打开".into(),
                )
            }
        },
        _ => {
            return Err("Windows 当前仅支持本机磁盘工作区；UNC/设备路径尚未验收，已停止打开".into())
        }
    };
    let drive_root: Vec<u16> = format!("{}:\\", char::from(drive))
        .encode_utf16()
        .chain(Some(0))
        .collect();
    // DRIVE_REMOVABLE=2, DRIVE_FIXED=3, DRIVE_RAMDISK=6. A mapped
    // network drive is DRIVE_REMOTE=4 even when it has a drive letter.
    if !matches!(unsafe { GetDriveTypeW(drive_root.as_ptr()) }, 2 | 3 | 6) {
        return Err("Windows 当前仅支持可验证的本机磁盘；网络映射盘或未知卷已停止打开".into());
    }
    for component in path.components().skip(1) {
        match component {
            Component::RootDir => (),
            Component::Normal(name) => validate_relative_path(
                name.to_str()
                    .ok_or("Windows 工作区名称必须是有效 Unicode")?,
            )?,
            _ => return Err("Windows 工作区路径不能包含 . 或 ..".into()),
        }
    }
    Ok(())
}

pub(crate) fn handle_identity(file: &File) -> Result<String, String> {
    let mut info: FILE_ID_INFO = unsafe { std::mem::zeroed() };
    if unsafe {
        GetFileInformationByHandleEx(
            file.as_raw_handle(),
            FileIdInfo,
            &mut info as *mut _ as *mut _,
            std::mem::size_of::<FILE_ID_INFO>() as u32,
        )
    } == 0
    {
        return Err(format!(
            "无法核实 Windows 物理目录身份：{}；已停止打开",
            io::Error::last_os_error()
        ));
    }
    let file_id = info
        .FileId
        .Identifier
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    Ok(format!("{:016x}-{file_id}", info.VolumeSerialNumber))
}

pub(crate) fn directory_identity(path: &Path) -> Result<String, String> {
    handle_identity(&open_directory(path)?)
}

/// Reject Win32 aliases before opening anything. Normalization must never
/// turn an authorized filename into an ADS, device or a different filename.
pub(crate) fn validate_relative_path(relative: &str) -> Result<(), String> {
    if relative.is_empty() || relative.starts_with('/') || relative.contains('\\') {
        return Err("无效的 Windows 工作区相对路径".into());
    }
    for part in relative.split('/') {
        if part.is_empty()
            || part == "."
            || part == ".."
            || part.ends_with(['.', ' '])
            || part.chars().any(|c| c < ' ' || "<>:\"|?*".contains(c))
        {
            return Err("Windows 文件路径含不安全名称、ADS、尾点/空格或非法字符".into());
        }
        let stem = part
            .split('.')
            .next()
            .unwrap_or("")
            .trim_end_matches(' ')
            .to_uppercase();
        let device = matches!(
            stem.as_str(),
            "CON" | "PRN" | "AUX" | "NUL" | "CLOCK$" | "CONIN$" | "CONOUT$"
        ) || ["COM", "LPT"].iter().any(|prefix| {
            stem.strip_prefix(prefix).is_some_and(|suffix| {
                suffix.len() == 1 && matches!(suffix.as_bytes()[0], b'1'..=b'9')
                    || matches!(suffix, "¹" | "²" | "³")
            })
        });
        if device {
            return Err("Windows 保留设备名不能作为工作区文件".into());
        }
    }
    Ok(())
}

pub(crate) fn reject_reparse(path: &Path) -> Result<(), String> {
    let metadata = fs::symlink_metadata(path).map_err(|e| e.to_string())?;
    if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err("工作区内部不接受 junction、符号链接或其他 reparse point".into());
    }
    Ok(())
}

/// Windows paths are case-insensitive, and `canonicalize` may return a
/// verbatim `\\?\` spelling for one component but not another. Compare
/// normalized strings with a component boundary so a valid AppContainer
/// workspace is not rejected while `root2` still cannot match `root`.
pub(crate) fn is_within(root: &Path, target: &Path) -> bool {
    let root = normalize_windows_path(root);
    let target = normalize_windows_path(target);
    is_within_normalized(&root, &target)
        || redirected_logical_path(&target)
            .is_some_and(|redirected| is_within_normalized(&root, &redirected))
}

pub(crate) fn same_path(left: &Path, right: &Path) -> bool {
    is_within(left, right) && is_within(right, left)
}

/// AppContainer may redirect writes from `%APPDATA%` to
/// `%LOCALAPPDATA%\Packages\<identity>\LocalCache\Roaming`. Treat only
/// that exact pair as aliases; arbitrary cross-directory replacements stay
/// rejected.
pub(crate) fn is_appcontainer_redirect(logical: &Path, physical: &Path) -> bool {
    let logical = normalize_windows_path(logical);
    let physical = normalize_windows_path(physical);
    redirected_logical_path(&physical).is_some_and(|redirected| redirected == logical)
}

fn normalize_windows_path(path: &Path) -> String {
    let mut value = path.to_string_lossy().replace('/', "\\");
    if let Some(rest) = value.strip_prefix("\\\\?\\UNC\\") {
        value = format!("\\\\{rest}");
    } else if let Some(rest) = value.strip_prefix("\\\\?\\") {
        value = rest.to_owned();
    }
    while value.len() > 3 && value.ends_with('\\') {
        value.pop();
    }
    value.to_ascii_lowercase()
}

fn is_within_normalized(root: &str, target: &str) -> bool {
    target == root || target.starts_with(&(root.to_owned() + "\\"))
}

fn redirected_logical_path(physical: &str) -> Option<String> {
    let marker = "\\appdata\\local\\packages\\";
    let marker_start = physical.find(marker)?;
    let package_start = marker_start + marker.len();
    let cache_start_rel = physical[package_start..].find("\\localcache\\roaming\\")?;
    let suffix = &physical[package_start + cache_start_rel + "\\localcache\\roaming\\".len()..];
    Some(format!(
        "{}\\appdata\\roaming\\{suffix}",
        &physical[..marker_start]
    ))
}
