//! Fixed, tested ACP provider entry points. Discovery is not a connection/auth check.
use crate::kernel::AgentDescriptor;
use serde::Serialize;
use serde_json::{json, Value};
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NativeProvider {
    Hermes,
    ClaudeCode,
    Codex,
}
impl NativeProvider {
    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "hermes" => Ok(Self::Hermes),
            "claude_code" => Ok(Self::ClaudeCode),
            "codex" => Ok(Self::Codex),
            _ => Err("不支持的 Native ACP Provider".into()),
        }
    }
    pub fn from_descriptor(d: &AgentDescriptor) -> Result<Self, String> {
        if let Some(provider) = d.provider.as_deref() {
            return Self::parse(provider);
        }
        // All pre-0.0.5 stdio descriptors were Hermes. Never infer a new provider
        // merely from a user-editable title or session ID.
        if d.transport == "stdio" {
            Ok(Self::Hermes)
        } else {
            Err("Native ACP 需要 stdio transport".into())
        }
    }
    pub fn id(self) -> &'static str {
        match self {
            Self::Hermes => "hermes",
            Self::ClaudeCode => "claude_code",
            Self::Codex => "codex",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Hermes => "Hermes",
            Self::ClaudeCode => "Claude Code",
            Self::Codex => "Codex",
        }
    }
    pub fn command(self) -> &'static str {
        match self {
            Self::Hermes => "hermes",
            Self::ClaudeCode => "claude-agent-acp",
            Self::Codex => "codex-acp",
        }
    }
    pub fn host(self) -> &'static str {
        self.host_command()
    }
    pub fn host_command(self) -> &'static str {
        match self {
            Self::Hermes => "hermes",
            Self::ClaudeCode => "claude",
            Self::Codex => "codex",
        }
    }
    pub fn args(self) -> Vec<String> {
        if self == Self::Hermes {
            vec!["acp".into()]
        } else {
            vec![]
        }
    }
}
fn executable(path: &Path) -> bool {
    path.metadata().is_ok_and(|metadata| {
        #[cfg(unix)]
        {
            metadata.is_file() && metadata.permissions().mode() & 0o111 != 0
        }
        #[cfg(not(unix))]
        {
            metadata.is_file()
        }
    })
}

// Finder-launched apps do not inherit shell startup files.  Keep the same
// latest-version-first nvm lookup for the external Codex CLI that we already
// use for the bundled Node runtime.
#[cfg(unix)]
fn nvm_command_candidates(home: &Path, command: &str) -> Vec<PathBuf> {
    if !matches!(command, "codex" | "node") {
        return Vec::new();
    }
    let Ok(entries) = std::fs::read_dir(home.join(".nvm/versions/node")) else {
        return Vec::new();
    };
    let mut dirs = entries
        .flatten()
        .map(|entry| entry.path())
        .collect::<Vec<_>>();
    dirs.sort_by(|left, right| {
        let version = |path: &Path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .map(|name| {
                    name.strip_prefix('v')
                        .unwrap_or(name)
                        .split('.')
                        .map(|part| part.parse::<u64>().unwrap_or(0))
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default()
        };
        version(right)
            .cmp(&version(left))
            .then_with(|| right.cmp(left))
    });
    dirs.into_iter()
        .map(|dir| dir.join("bin").join(command))
        .collect()
}

// Resolve only from the running .app; never persist a build-machine absolute path.
// An incomplete bundle is an installation error, not permission to silently use
// some unrelated global CLI. Explicit absolute adapter paths still work below.
#[cfg(unix)]
fn bundled_codex_program_at(app_exe: &Path, command: &str) -> Result<Option<PathBuf>, String> {
    if command != "codex-acp" {
        return Ok(None);
    }
    let Some(macos) = app_exe
        .parent()
        .filter(|p| p.file_name().is_some_and(|n| n == "MacOS"))
    else {
        return Ok(None);
    };
    let Some(contents) = macos
        .parent()
        .filter(|p| p.file_name().is_some_and(|n| n == "Contents"))
    else {
        return Ok(None);
    };
    let root = contents.join("Resources/agents/codex");
    if !root.exists() {
        return Err("内置 Codex 运行目录缺失，请重新安装完整应用包".into());
    }
    let bytes = std::fs::read(root.join("manifest.json"))
        .map_err(|_| "内置 Codex 资源清单缺失，请重新安装完整应用包")?;
    if bytes.len() > 1024 * 1024 {
        return Err("内置 Codex 资源清单异常，请重新安装完整应用包".into());
    }
    let manifest: Value = serde_json::from_slice(&bytes)
        .map_err(|_| "内置 Codex 资源清单无效，请重新安装完整应用包")?;
    if manifest["schemaVersion"] != 1
        || manifest["provider"] != "codex"
        || manifest["platform"] != "darwin"
        || manifest["architecture"] != "arm64"
        || manifest["entrypoints"]["adapter"] != "bin/codex-acp"
        || manifest["entrypoints"]["node"] != "bin/node"
    {
        return Err("内置 Codex 资源身份不匹配，请重新安装完整应用包".into());
    }
    // The packager verifies the full locked payload. Runtime discovery checks
    // all launch dependencies so a missing Node isn't mislabeled as no account.
    for relative in ["bin/codex-acp", "bin/node"] {
        if !executable(&root.join(relative)) {
            return Err(format!(
                "内置 Codex 程序 {relative} 缺失或不可执行，请重新安装完整应用包"
            ));
        }
    }
    for relative in ["adapter/index.js", "adapter/package.json"] {
        if !root.join(relative).is_file() {
            return Err(format!(
                "内置 Codex 资源 {relative} 缺失，请重新安装完整应用包"
            ));
        }
    }
    Ok(Some(root.join("bin").join(command)))
}
#[cfg(unix)]
fn bundled_codex_program(command: &str) -> Result<Option<PathBuf>, String> {
    match std::env::current_exe() {
        Ok(exe) => bundled_codex_program_at(&exe, command),
        Err(_) => Ok(None),
    }
}

/// The descriptor keeps the provider's logical command (`codex-acp`).  The
/// process we spawn may be a different executable when the adapter is bundled
/// with the Windows application.  Keeping this plan explicit prevents a
/// `.cmd`/POSIX shim from being mistaken for a native executable and makes the
/// installed-directory lookup independently testable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct LaunchPlan {
    pub program: PathBuf,
    pub args: Vec<String>,
    pub bundled: bool,
}

#[cfg(windows)]
#[derive(Debug, Clone)]
pub(super) struct BundledCodexRuntime {
    pub node: PathBuf,
    pub adapter: PathBuf,
}

#[cfg(windows)]
fn windows_architecture() -> &'static str {
    match std::env::consts::ARCH {
        "x86_64" => "x64",
        "aarch64" => "arm64",
        "x86" => "x86",
        _ => std::env::consts::ARCH,
    }
}

#[cfg(windows)]
fn resource_roots_for(app_exe: &Path) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Some(parent) = app_exe.parent() {
        // Tauri's Windows resource mapping in tauri.windows.conf.json places
        // the source `agents/codex` tree directly beside the executable.
        // Keep the explicit resources spelling too for manually staged Dev
        // builds and older installers.
        roots.push(parent.join("agents/codex"));
        roots.push(parent.join("resources/agents/codex"));
        // Keep a case-preserving spelling for development bundles created by
        // hand. Windows comparisons are case-insensitive, but this is useful
        // on fixtures copied through a case-sensitive filesystem.
        roots.push(parent.join("Resources/agents/codex"));
    }
    roots
}

#[cfg(windows)]
fn bundled_path(root: &Path, relative: &str, label: &str) -> Result<PathBuf, String> {
    let relative_path = Path::new(relative);
    if relative_path.is_absolute()
        || relative_path
            .components()
            .any(|component| component == std::path::Component::ParentDir)
    {
        return Err(format!("内置 Codex {label} 路径无效，请重新安装完整应用包"));
    }
    let path = root.join(relative_path);
    if !path.starts_with(root) || !path.is_file() {
        return Err(format!(
            "内置 Codex 资源 {relative} 不存在或缺失，请重新安装完整应用包"
        ));
    }
    Ok(path)
}

/// Validate and resolve the runtime beside an installed Windows executable.
/// An absent root returns `None` so callers can distinguish an ordinary
/// development run from a corrupt bundle; once a root exists every required
/// file and identity field is checked and failures are explicit.
#[cfg(windows)]
pub(super) fn bundled_codex_runtime_at(
    app_exe: &Path,
) -> Result<Option<BundledCodexRuntime>, String> {
    let root = resource_roots_for(app_exe)
        .into_iter()
        .find(|candidate| candidate.is_dir());
    let Some(root) = root else {
        return Ok(None);
    };
    let manifest_path = root.join("manifest.json");
    let bytes = std::fs::read(&manifest_path)
        .map_err(|_| "内置 Codex 资源清单不存在或缺失，请重新安装完整应用包")?;
    if bytes.is_empty() || bytes.len() > 1024 * 1024 {
        return Err("内置 Codex 资源清单异常，请重新安装完整应用包".into());
    }
    let manifest: Value = serde_json::from_slice(&bytes)
        .map_err(|_| "内置 Codex 资源清单无效，请重新安装完整应用包")?;
    let expected_arch = windows_architecture();
    if manifest.get("schemaVersion") != Some(&Value::from(1))
        || manifest.get("provider").and_then(Value::as_str) != Some("codex")
        || manifest.get("platform").and_then(Value::as_str) != Some("windows")
        || manifest.get("architecture").and_then(Value::as_str) != Some(expected_arch)
        || manifest
            .pointer("/entrypoints/adapter")
            .and_then(Value::as_str)
            != Some("adapter/index.js")
        || manifest
            .pointer("/entrypoints/node")
            .and_then(Value::as_str)
            != Some("bin/node.exe")
        || manifest
            .pointer("/versions/adapter")
            .and_then(Value::as_str)
            != Some("1.13.1")
        || manifest.pointer("/versions/node").and_then(Value::as_str) != Some("22.23.3")
        || !manifest.get("files").is_some_and(Value::is_array)
    {
        return Err("内置 Codex 资源身份不匹配，请重新安装完整应用包".into());
    }
    if let Some(program) = manifest.pointer("/launch/program").and_then(Value::as_str) {
        if program != "bin/node.exe" {
            return Err("内置 Codex 启动程序身份不匹配，请重新安装完整应用包".into());
        }
    }
    if let Some(args) = manifest.pointer("/launch/args").and_then(Value::as_array) {
        let expected = [Value::from("adapter/index.js")];
        if args.as_slice() != expected.as_slice() {
            return Err("内置 Codex 启动参数身份不匹配，请重新安装完整应用包".into());
        }
    }
    let node = bundled_path(&root, "bin/node.exe", "程序")?;
    let adapter = bundled_path(&root, "adapter/index.js", "适配器")?;
    let _ = bundled_path(&root, "adapter/package.json", "适配器 package")?;
    Ok(Some(BundledCodexRuntime { node, adapter }))
}

#[cfg(windows)]
fn bundled_codex_runtime() -> Result<Option<BundledCodexRuntime>, String> {
    let executable = std::env::current_exe()
        .map_err(|_| "无法定位当前应用目录，无法加载内置 Codex ACP".to_owned())?;
    bundled_codex_runtime_at(&executable)
}

/// Resolve a descriptor into the executable and argument vector that should
/// actually be spawned. The descriptor's command/args remain validated by the
/// caller before this function is used.
pub(super) fn resolve_launch_plan(
    command: &str,
    args: &[String],
    provider: NativeProvider,
) -> Result<LaunchPlan, String> {
    if args != provider.args().as_slice() {
        return Err(format!(
            "{} 的 ACP 启动参数不符合已验证入口",
            provider.label()
        ));
    }
    #[cfg(windows)]
    if provider == NativeProvider::Codex && command == provider.command() {
        let runtime = bundled_codex_runtime()?
            .ok_or("内置 Codex ACP 运行目录不存在或缺失，请重新安装完整应用包".to_owned())?;
        return Ok(LaunchPlan {
            program: runtime.node,
            args: vec![runtime.adapter.to_string_lossy().into_owned()],
            bundled: true,
        });
    }
    let program = resolve_program(command, provider.command())?;
    #[cfg(unix)]
    let bundled = bundled_codex_program(command)?.is_some_and(|entry| entry == program);
    #[cfg(not(unix))]
    let bundled = false;
    Ok(LaunchPlan {
        program,
        args: args.to_vec(),
        bundled,
    })
}
pub fn resolve_program(command: &str, expected: &str) -> Result<PathBuf, String> {
    let path = Path::new(command);
    if path.is_absolute() {
        return if let Some(found) = program_candidates(path)
            .into_iter()
            .find(|path| executable(path))
        {
            Ok(found)
        } else {
            Err(format!("{expected} 可执行文件不存在或不可执行"))
        };
    }
    if command != expected {
        return Err(format!("需要命令 {expected} 或其绝对路径"));
    }
    #[cfg(unix)]
    if let Some(bundled) = bundled_codex_program(command)? {
        return Ok(bundled);
    }
    let mut candidates = Vec::new();
    if let Some(path) = std::env::var_os("PATH") {
        candidates.extend(std::env::split_paths(&path).map(|entry| entry.join(command)));
    }
    #[cfg(windows)]
    candidates.extend(
        windows_search_dirs()
            .into_iter()
            .map(|dir| dir.join(command)),
    );
    if matches!(command, "claude-agent-acp" | "codex-acp") {
        if let Some(dir) = std::env::var_os("PIXEL_ACP_ADAPTER_DIR") {
            candidates.insert(
                0,
                PathBuf::from(dir).join("node_modules/.bin").join(command),
            );
        }
        #[cfg(windows)]
        if let Some(local) = std::env::var_os("LOCALAPPDATA") {
            for version in ["0.0.6", "0.0.5"] {
                candidates.push(
                    PathBuf::from(&local)
                        .join(format!("Atrio/acp-adapters/{version}/node_modules/.bin"))
                        .join(command),
                );
            }
        }
    }
    #[cfg(unix)]
    if let Some(home) = std::env::var_os("HOME") {
        let home = PathBuf::from(home);
        if matches!(command, "claude-agent-acp" | "codex-acp") {
            candidates.push(
                home.join(".local/share/atrio/acp-adapters/0.0.5/node_modules/.bin")
                    .join(command),
            );
        }
        candidates.push(home.join(".local/bin").join(command));
        candidates.push(home.join(".hermes/node/bin").join(command));
        if command == "hermes" {
            candidates.push(home.join(".hermes/hermes-agent/venv/bin/hermes"));
        }
        // Node and Codex installed by nvm are common on macOS, while GUI PATH
        // is minimal. Prefer the newest nvm version after explicit PATH/local
        // candidates, preserving normal command precedence.
        candidates.extend(nvm_command_candidates(&home, command));
    }
    #[cfg(unix)]
    candidates.extend([
        PathBuf::from("/opt/homebrew/bin").join(command),
        PathBuf::from("/usr/local/bin").join(command),
    ]);
    #[cfg(target_os = "macos")]
    if command == "codex" {
        candidates.push(PathBuf::from(
            "/Applications/ChatGPT.app/Contents/Resources/codex",
        ));
        candidates.push(PathBuf::from(
            "/Applications/Codex.app/Contents/Resources/codex",
        ));
    }
    candidates
        .into_iter()
        .flat_map(|path| program_candidates(&path))
        .find(|path| executable(path))
        .ok_or_else(|| format!("未找到 {expected}；请安装已验证版本或选择其绝对路径"))
}

fn program_candidates(path: &Path) -> Vec<PathBuf> {
    #[cfg(not(windows))]
    {
        vec![path.to_owned()]
    }
    #[cfg(windows)]
    {
        if path.extension().is_some() {
            return vec![path.to_owned()];
        }
        // npm also installs an extensionless POSIX shim. On Windows prefer
        // executable suffixes, never accidentally execute that shell script.
        let extensions = std::env::var("PATHEXT").unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".into());
        let mut candidates = Vec::new();
        for extension in extensions.split(';') {
            let extension = extension.trim().to_ascii_lowercase();
            if matches!(extension.as_str(), ".exe" | ".com" | ".cmd" | ".bat") {
                candidates.push(PathBuf::from(format!("{}{extension}", path.display())));
            }
        }
        candidates
    }
}

#[cfg(windows)]
pub(super) fn windows_codex_search_dirs(local_app_data: &Path) -> Vec<PathBuf> {
    let root = local_app_data.join("OpenAI/Codex/bin");
    let mut dirs = Vec::new();
    if root.is_dir() {
        dirs.push(root.clone());
        if let Ok(entries) = std::fs::read_dir(&root) {
            let mut versioned = entries
                .flatten()
                .map(|entry| entry.path())
                .filter(|path| path.is_dir())
                .collect::<Vec<_>>();
            versioned.sort_by(|left, right| right.cmp(left));
            dirs.extend(versioned);
        }
    }
    dirs
}

#[cfg(windows)]
pub(super) fn windows_search_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(root) = std::env::var_os("SystemRoot") {
        dirs.push(PathBuf::from(&root).join("System32"));
        dirs.push(PathBuf::from(root));
    }
    if let Some(profile) = std::env::var_os("USERPROFILE") {
        dirs.push(PathBuf::from(&profile).join(".local/bin"));
        dirs.push(PathBuf::from(profile).join(".cargo/bin"));
    }
    if let Some(appdata) = std::env::var_os("APPDATA") {
        dirs.push(PathBuf::from(appdata).join("npm"));
    }
    if let Some(local) = std::env::var_os("LOCALAPPDATA") {
        dirs.push(PathBuf::from(&local).join("Programs/nodejs"));
        dirs.push(PathBuf::from(&local).join("Microsoft/WinGet/Links"));
        dirs.push(PathBuf::from(&local).join("pi-node/current"));
        dirs.extend(windows_codex_search_dirs(&PathBuf::from(&local)));
    }
    for variable in ["ProgramFiles", "ProgramFiles(x86)"] {
        if let Some(programs) = std::env::var_os(variable) {
            dirs.push(PathBuf::from(&programs).join("nodejs"));
            dirs.push(PathBuf::from(&programs).join("Git/cmd"));
        }
    }
    dirs
}

#[cfg(windows)]
fn strip_verbatim_prefix(path: &Path) -> PathBuf {
    let Some(value) = path.to_str() else {
        return path.to_owned();
    };
    const VERBATIM: &str = "\\\\?\\";
    const VERBATIM_UNC: &str = "\\\\?\\UNC\\";
    if let Some(rest) = value.strip_prefix(VERBATIM_UNC) {
        return PathBuf::from(format!("\\\\{rest}"));
    }
    value
        .strip_prefix(VERBATIM)
        .map(PathBuf::from)
        .unwrap_or_else(|| path.to_owned())
}

#[cfg(windows)]
pub(super) fn windows_command(path: &Path) -> Result<Command, String> {
    let extension = path
        .extension()
        .and_then(|v| v.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    #[cfg(test)]
    if extension == "py" {
        let python = std::env::var_os("ATRIO_TEST_PYTHON")
            .map(PathBuf::from)
            .ok_or("Windows fixture 需要 ATRIO_TEST_PYTHON 的绝对解释器路径")?;
        if !python.is_absolute() || !python.is_file() {
            return Err("Windows fixture Python 不存在".into());
        }
        let mut command = Command::new(python);
        // Python's Windows runtime cannot use a verbatim `\\?\` script path
        // when `__file__` is later converted back into a normal file path.
        // Keep long-path support for native entries, while passing a normal
        // spelling to the interpreter so provider scripts can open siblings.
        command.arg(strip_verbatim_prefix(path));
        return Ok(command);
    }
    if matches!(extension.as_str(), "exe" | "com") {
        return Ok(Command::new(path));
    }
    let entry = if matches!(extension.as_str(), "cmd" | "bat") {
        npm_entry_for_shim(path)?
    } else if matches!(extension.as_str(), "js" | "mjs" | "cjs") {
        path.to_owned()
    } else {
        return Err("Windows ACP 入口需要 EXE 或已安装官方 npm adapter 的 CMD/JS 路径".into());
    };
    let node = resolve_program("node", "node")?;
    if !node
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("exe"))
    {
        return Err("Windows ACP 需要 node.exe".into());
    }
    let mut command = Command::new(node);
    command.arg(strip_verbatim_prefix(&entry));
    Ok(command)
}

#[cfg(windows)]
fn npm_entry_for_shim(shim: &Path) -> Result<PathBuf, String> {
    let name = shim
        .file_stem()
        .and_then(|v| v.to_str())
        .ok_or("npm shim 名称无效")?;
    let package = match name {
        "claude-agent-acp" => "@agentclientprotocol/claude-agent-acp",
        "codex-acp" => "@agentclientprotocol/codex-acp",
        "claude" => "@anthropic-ai/claude-code",
        "codex" => "@openai/codex",
        _ => return Err("仅支持已知 npm 入口；请选择原生 EXE 或实际 JS 文件".into()),
    };
    let parent = shim.parent().ok_or("npm shim 目录无效")?;
    let modules = if parent.file_name().is_some_and(|v| v == ".bin") {
        parent
            .parent()
            .ok_or("npm node_modules 目录无效")?
            .to_owned()
    } else {
        parent.join("node_modules")
    };
    let package_root = modules
        .join(package)
        .canonicalize()
        .map_err(|_| "npm package 未安装；无法解析实际入口")?;
    let manifest = std::fs::read(package_root.join("package.json"))
        .map_err(|_| "无法读取 npm package 入口")?;
    if manifest.len() > 1024 * 1024 {
        return Err("npm package manifest 超出上限".into());
    }
    let manifest: Value =
        serde_json::from_slice(&manifest).map_err(|_| "npm package manifest 无效")?;
    let bin = manifest
        .get("bin")
        .and_then(|bin| {
            bin.as_str()
                .or_else(|| bin.get(name).and_then(Value::as_str))
        })
        .ok_or("npm package 没有所需入口")?;
    let entry = package_root
        .join(bin)
        .canonicalize()
        .map_err(|_| "npm package JS 入口不存在")?;
    if !entry.starts_with(&package_root) || !entry.is_file() {
        return Err("npm package 入口超出 package 目录".into());
    }
    Ok(entry)
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderProbe {
    pub provider: String,
    pub host_executable: String,
    pub host_version: String,
    pub executable: String,
    pub version: String,
    pub host_available: bool,
    pub adapter_available: bool,
    pub acp_available: bool,
    pub authentication_status: String,
    pub handshake_status: String,
    pub session_status: String,
    pub restricted_supported: bool,
    pub detail: String,
}
fn safe_version(output: &str) -> Result<String, String> {
    let line = output.lines().next().unwrap_or("").trim();
    if line.is_empty() || line.len() > 160 || line.chars().any(char::is_control) {
        return Err("版本探测输出无法识别".into());
    }
    Ok(line.into())
}
pub fn probe_provider(provider: &str, command: Option<&str>) -> Result<ProviderProbe, String> {
    let provider = NativeProvider::parse(provider)?;
    let command = command.unwrap_or(provider.command());
    let launch = if provider == NativeProvider::Codex && command == provider.command() {
        Some(resolve_launch_plan(command, &provider.args(), provider)?)
    } else {
        None
    };
    let executable = launch
        .as_ref()
        .map(|plan| Ok(plan.program.clone()))
        .unwrap_or_else(|| resolve_program(command, provider.command()));
    let host = if provider == NativeProvider::Hermes {
        executable.clone()
    } else {
        resolve_program(provider.host_command(), provider.host_command())
    };
    let mut details = Vec::new();
    let host_version = host
        .as_ref()
        .map_err(Clone::clone)
        .and_then(|path| safe_version(&super::native::probe_command(path, &["--version"])?));
    let version = executable.as_ref().map_err(Clone::clone).and_then(|path| {
        if provider == NativeProvider::Hermes {
            Ok(super::native::probe_hermes(path.to_str())?.version)
        } else if let Some(plan) = launch.as_ref() {
            safe_version(&super::native::probe_launch_plan(
                plan,
                &["--version"],
                provider,
            )?)
        } else {
            safe_version(&super::native::probe_command(path, &["--version"])?)
        }
    });
    if let Err(error) = &host_version {
        details.push(format!("宿主：{error}"));
    }
    if let Err(error) = &version {
        details.push(format!("ACP 入口：{error}"));
    }
    if launch.as_ref().is_some_and(|plan| plan.bundled) {
        details.push("使用本应用内置 Codex ACP 与 Node；Codex 宿主使用本机已安装的官方 CLI".into());
    } else if provider == NativeProvider::Codex {
        details.push("使用所选外部 ACP 入口；Codex 宿主路径见下方详情".into());
    }
    let ready = host_version.is_ok() && version.is_ok();
    if ready {
        details.push("仅已核实本机宿主与 ACP 入口版本；认证、握手和会话需真实连接验证".into());
        if provider == NativeProvider::Codex {
            details.push("连接时自动检查 ACP，并由 Codex 复用本机已有登录；Atrio 不收集账号或密钥。尚未登录请先在 Codex 完成登录后重试".into());
        }
    }
    Ok(ProviderProbe {
        provider: provider.id().into(),
        host_executable: host
            .as_ref()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default(),
        host_version: host_version.unwrap_or_default(),
        executable: executable
            .as_ref()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|_| command.into()),
        version: version.unwrap_or_default(),
        host_available: host.is_ok(),
        adapter_available: executable.is_ok(),
        acp_available: ready,
        authentication_status: "not_checked".into(),
        handshake_status: "not_checked".into(),
        session_status: "not_checked".into(),
        restricted_supported: false,
        detail: details.join("；"),
    })
}

fn isolated_profile(variable: &str) -> Result<Option<PathBuf>, String> {
    let Some(path) = std::env::var_os(variable) else {
        return Ok(None);
    };
    let path = PathBuf::from(path);
    if !path.is_absolute() || !path.is_dir() {
        return Err(format!("{variable} 隔离 profile 不存在；拒绝回退个人配置"));
    }
    Ok(Some(path))
}
#[cfg(windows)]
fn validate_codex_host_path(path: &Path) -> Result<(), String> {
    // Pinned codex-acp 1.13.1 interpolates CODEX_PATH into a quoted
    // `cmd.exe /c` command. Percent expansion still occurs inside quotes.
    // Exclamation expansion can also be enabled through the user's cmd
    // configuration, which this launcher must not silently assume is off.
    let value = path
        .to_str()
        .ok_or("Codex ACP 不支持当前 Windows 宿主路径编码")?;
    if value
        .chars()
        .any(|character| matches!(character, '%' | '!' | '"') || character.is_control())
    {
        return Err("Codex ACP 1.13.1 在 Windows 通过 cmd.exe 启动宿主；宿主路径含不支持的命令扩展字符。请使用不含 %、!、引号或控制字符的 Codex 安装路径".into());
    }
    Ok(())
}
pub(super) fn configure_process(cmd: &mut Command, provider: NativeProvider) -> Result<(), String> {
    match provider {
        NativeProvider::Hermes => {}
        NativeProvider::ClaudeCode => {
            let host = resolve_program("claude", "claude")?;
            // The Claude SDK understands .js entry points and launches them with
            // node. Passing a Windows npm .cmd directly would invoke no shell
            // and fail before ACP can create a session.
            #[cfg(windows)]
            let host = if host.extension().is_some_and(|ext| {
                ext.eq_ignore_ascii_case("cmd") || ext.eq_ignore_ascii_case("bat")
            }) {
                npm_entry_for_shim(&host)?
            } else {
                host
            };
            cmd.env("CLAUDE_CODE_EXECUTABLE", host);
            if let Some(profile) = isolated_profile("PIXEL_CLAUDE_TEST_HOME")? {
                cmd.env("CLAUDE_CONFIG_DIR", profile);
                // The launch-only reference is never a descriptor field or a
                // persisted copy. Import only authentication/routing environment
                // into the child, without user hooks/plugins/memory/settings.
                if let Some(reference) = std::env::var_os("PIXEL_CLAUDE_AUTH_SETTINGS") {
                    let reference = PathBuf::from(reference);
                    if !reference.is_absolute() {
                        return Err("Claude 认证配置引用必须为绝对路径".into());
                    }
                    let bytes = std::fs::read(reference)
                        .map_err(|_| "无法读取指定的 Claude 认证配置引用")?;
                    if bytes.len() > 1024 * 1024 {
                        return Err("Claude 认证配置超出上限".into());
                    }
                    let settings: Value =
                        serde_json::from_slice(&bytes).map_err(|_| "Claude 认证配置格式错误")?;
                    if let Some(model) = settings.get("model").and_then(Value::as_str) {
                        cmd.env("ANTHROPIC_MODEL", model);
                    }
                    for key in [
                        "ANTHROPIC_AUTH_TOKEN",
                        "ANTHROPIC_API_KEY",
                        "ANTHROPIC_BASE_URL",
                        "ANTHROPIC_MODEL",
                    ] {
                        if let Some(value) = settings
                            .get("env")
                            .and_then(|env| env.get(key))
                            .and_then(Value::as_str)
                        {
                            cmd.env(key, value);
                        }
                    }
                }
            }
        }
        NativeProvider::Codex => {
            let host = resolve_program("codex", "codex")?;
            #[cfg(windows)]
            validate_codex_host_path(&host)?;
            cmd.env("CODEX_PATH", host);
            if let Some(profile) = isolated_profile("PIXEL_CODEX_TEST_HOME")? {
                // Both roots are isolated: Codex also discovers ~/.agents skills.
                cmd.env("CODEX_HOME", &profile).env("HOME", &profile);
                #[cfg(windows)]
                cmd.env("USERPROFILE", &profile);
                if let Some(reference) = std::env::var_os("PIXEL_CODEX_AUTH_CONFIG") {
                    let reference = PathBuf::from(reference);
                    if !reference.is_absolute() {
                        return Err("Codex 路由配置引用必须为绝对路径".into());
                    }
                    let bytes = std::fs::read(reference)
                        .map_err(|_| "无法读取指定的 Codex 路由配置引用")?;
                    if bytes.len() > 1024 * 1024 {
                        return Err("Codex 路由配置超出上限".into());
                    }
                    let source: toml::Value = toml::from_str(
                        std::str::from_utf8(&bytes).map_err(|_| "Codex 路由配置不是 UTF-8")?,
                    )
                    .map_err(|_| "Codex 路由配置格式错误")?;
                    let settings =
                        serde_json::to_value(source).map_err(|_| "Codex 路由配置转换失败")?;
                    let mut config = json!({"project_doc_max_bytes":0,"features":{"memories":false},"mcp_servers":{}});
                    for key in [
                        "model",
                        "model_provider",
                        "model_providers",
                        "model_catalog_json",
                        "model_reasoning_effort",
                    ] {
                        if let Some(value) = settings.get(key) {
                            config[key] = value.clone();
                        }
                    }
                    cmd.env(
                        "CODEX_CONFIG",
                        serde_json::to_string(&config).map_err(|_| "Codex 路由配置转换失败")?,
                    );
                }
            }
        }
    }
    Ok(())
}
pub(super) fn claude_session_meta() -> Result<Value, String> {
    // Only the authorized acceptance launcher can set an isolation profile.
    // Normal sessions keep the provider's own auth discovery. The MCP endpoint
    // remains explicit and independent of these provider SDK options.
    if isolated_profile("PIXEL_CLAUDE_TEST_HOME")?.is_some() {
        return Ok(
            json!({"claudeCode":{"options":{"settingSources":[],"tools":[],"allowDangerouslySkipPermissions":false,"strictMcpConfig":true,"settings":{"disableAllHooks":true,"autoMemoryEnabled":false},"extraArgs":{"disable-slash-commands":""}}}}),
        );
    }
    Ok(
        json!({"claudeCode":{"options":{"allowDangerouslySkipPermissions":false,"strictMcpConfig":true}}}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn provider_entries_are_exact_and_unknown_is_rejected() {
        assert_eq!(NativeProvider::Hermes.args(), vec!["acp"]);
        assert!(NativeProvider::ClaudeCode.args().is_empty());
        assert!(NativeProvider::Codex.args().is_empty());
        assert!(NativeProvider::parse("custom").is_err());
        assert!(resolve_program("sh", "hermes").is_err());
    }
    #[cfg(unix)]
    #[test]
    fn bundled_codex_follows_relocated_app_and_checks_launch_dependencies() {
        let temp =
            std::env::temp_dir().join(format!("atrio-bundled-discovery-{}", uuid::Uuid::new_v4()));
        let exe =
            temp.join("Folder with spaces/Atrio WorkSpace Beta.app/Contents/MacOS/pixel-workspace");
        let root = exe
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("Resources/agents/codex");
        assert!(bundled_codex_program_at(&exe, "codex-acp")
            .unwrap_err()
            .contains("运行目录缺失"));
        std::fs::create_dir_all(root.join("bin")).unwrap();
        std::fs::create_dir_all(root.join("adapter")).unwrap();
        std::fs::create_dir_all(root.join("codex-path")).unwrap();
        for name in ["adapter/index.js", "adapter/package.json"] {
            std::fs::write(root.join(name), "{}").unwrap();
        }
        for name in ["bin/node", "bin/codex-acp"] {
            let file = root.join(name);
            std::fs::write(&file, "#!/bin/sh\nexit 0\n").unwrap();
            #[cfg(unix)]
            std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let manifest = json!({"schemaVersion":1,"provider":"codex","platform":"darwin","architecture":"arm64",
            "entrypoints":{"adapter":"bin/codex-acp","node":"bin/node"}});
        std::fs::write(root.join("manifest.json"), manifest.to_string()).unwrap();
        assert_eq!(
            bundled_codex_program_at(&exe, "codex-acp").unwrap(),
            Some(root.join("bin/codex-acp"))
        );
        assert_eq!(bundled_codex_program_at(&exe, "codex").unwrap(), None);
        assert_eq!(bundled_codex_program_at(&exe, "hermes").unwrap(), None);
        assert_eq!(
            bundled_codex_program_at(&temp.join("pixel-workspace"), "codex").unwrap(),
            None
        );
        for name in ["bin/node", "adapter/package.json"] {
            let file = root.join(name);
            let saved = std::fs::read(&file).unwrap();
            let mode = std::fs::metadata(&file).unwrap().permissions();
            std::fs::remove_file(&file).unwrap();
            assert!(bundled_codex_program_at(&exe, "codex-acp")
                .unwrap_err()
                .contains(name));
            std::fs::write(&file, saved).unwrap();
            std::fs::set_permissions(&file, mode).unwrap();
        }
        std::fs::write(root.join("manifest.json"), "{}").unwrap();
        assert!(bundled_codex_program_at(&exe, "codex-acp")
            .unwrap_err()
            .contains("身份"));
        std::fs::remove_file(root.join("manifest.json")).unwrap();
        assert!(bundled_codex_program_at(&exe, "codex-acp")
            .unwrap_err()
            .contains("清单缺失"));
        std::fs::remove_dir_all(temp).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn nvm_codex_candidates_are_newest_first_for_minimal_gui_path() {
        let temp =
            std::env::temp_dir().join(format!("atrio-nvm-discovery-{}", uuid::Uuid::new_v4()));
        let versions = ["v22.18.0", "v24.18.0"];
        for version in versions {
            let bin = temp.join(format!(".nvm/versions/node/{version}/bin"));
            std::fs::create_dir_all(&bin).unwrap();
            let codex = bin.join("codex");
            std::fs::write(&codex, "#!/bin/sh\nexit 0\n").unwrap();
            std::fs::set_permissions(&codex, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let candidates = nvm_command_candidates(&temp, "codex");
        assert_eq!(
            candidates,
            vec![
                temp.join(".nvm/versions/node/v24.18.0/bin/codex"),
                temp.join(".nvm/versions/node/v22.18.0/bin/codex"),
            ]
        );
        assert_eq!(
            candidates.iter().find(|path| executable(path)),
            Some(&candidates[0])
        );
        assert!(nvm_command_candidates(&temp, "hermes").is_empty());
        std::fs::remove_dir_all(temp).unwrap();
    }
    #[test]
    fn discovery_version_has_no_multiline_payload() {
        assert_eq!(safe_version("codex 1.0\nignored").unwrap(), "codex 1.0");
        assert!(safe_version("").is_err());
        assert!(safe_version("version\u{1b}hidden").is_err());
    }
    #[cfg(windows)]
    #[test]
    fn windows_codex_host_path_rejects_cmd_expansion_without_spawning() {
        for path in [
            r"C:\Users\%USERNAME%\codex.exe",
            r"C:\literal percent %\codex.cmd",
            r"C:\Users\!USERNAME!\codex.exe",
            "C:\\bad\"quote\\codex.exe",
            "C:\\bad\nnewline\\codex.exe",
        ] {
            assert!(
                validate_codex_host_path(Path::new(path)).is_err(),
                "{path:?}"
            );
        }
        for path in [
            r"C:\Program Files\Codex\codex.exe",
            r"C:\Users\中文 & literal ^ name\codex.cmd",
        ] {
            assert!(
                validate_codex_host_path(Path::new(path)).is_ok(),
                "{path:?}"
            );
        }
    }
    #[cfg(windows)]
    #[test]
    fn windows_npm_shim_resolves_js_and_preserves_literal_argv() {
        let root = std::env::temp_dir().join(format!(
            "atrio adapter 中文 & percent % {}",
            uuid::Uuid::new_v4()
        ));
        let bin = root.join("node_modules/.bin");
        let package = root.join("node_modules/@agentclientprotocol/claude-agent-acp");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::create_dir_all(&package).unwrap();
        let shim = bin.join("claude-agent-acp.cmd");
        std::fs::write(&shim, "@echo should-never-run").unwrap();
        std::fs::write(
            package.join("package.json"),
            r#"{"bin":{"claude-agent-acp":"entry.js"}}"#,
        )
        .unwrap();
        std::fs::write(
            package.join("entry.js"),
            "process.stdout.write(JSON.stringify(process.argv.slice(2)))",
        )
        .unwrap();
        assert_eq!(
            resolve_program(
                bin.join("claude-agent-acp").to_str().unwrap(),
                "claude-agent-acp"
            )
            .unwrap(),
            shim
        );
        let args = [
            "中文 with spaces",
            "& echo never",
            "%PATH%",
            "literal\"quote",
            "trailing\\",
        ];
        let output = windows_command(&shim).unwrap().args(args).output().unwrap();
        assert!(output.status.success());
        assert_eq!(
            serde_json::from_slice::<Value>(&output.stdout).unwrap(),
            json!(args)
        );
        std::fs::write(
            package.join("package.json"),
            r#"{"bin":{"claude-agent-acp":"../../../.bin/claude-agent-acp.cmd"}}"#,
        )
        .unwrap();
        assert!(windows_command(&shim).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn windows_bundled_codex_resolves_after_relocation_with_spaces_and_unicode() {
        let temp = std::env::temp_dir().join(format!(
            "Atrio Codex 中文 relocated {}",
            uuid::Uuid::new_v4()
        ));
        let exe = temp.join("安装目录 with spaces/Atrio WorkSpace.exe");
        // NSIS maps the resource source to `agents/codex` beside the EXE;
        // manually staged Dev builds may use `resources/agents/codex`.
        let root = exe.parent().unwrap().join("agents/codex");
        std::fs::create_dir_all(root.join("bin")).unwrap();
        std::fs::create_dir_all(root.join("adapter")).unwrap();
        std::fs::write(root.join("bin/node.exe"), b"node fixture").unwrap();
        std::fs::write(root.join("adapter/index.js"), b"adapter fixture").unwrap();
        std::fs::write(root.join("adapter/package.json"), b"{}").unwrap();
        let manifest = json!({
            "schemaVersion": 1,
            "provider": "codex",
            "platform": "windows",
            "architecture": windows_architecture(),
            "versions": {"adapter": "1.13.1", "node": "22.23.3"},
            "entrypoints": {"adapter": "adapter/index.js", "node": "bin/node.exe"},
            "launch": {"program": "bin/node.exe", "args": ["adapter/index.js"]},
            "externalHostRequired": true,
            "files": []
        });
        std::fs::write(root.join("manifest.json"), manifest.to_string()).unwrap();

        let runtime = bundled_codex_runtime_at(&exe).unwrap().unwrap();
        assert_eq!(runtime.node, root.join("bin/node.exe"));
        assert_eq!(runtime.adapter, root.join("adapter/index.js"));
        std::fs::remove_file(root.join("adapter/index.js")).unwrap();
        let error = bundled_codex_runtime_at(&exe).unwrap_err();
        assert!(error.contains("adapter/index.js"));

        std::fs::remove_dir_all(&root).unwrap();
        let staged_root = exe.parent().unwrap().join("resources/agents/codex");
        std::fs::create_dir_all(staged_root.join("bin")).unwrap();
        std::fs::create_dir_all(staged_root.join("adapter")).unwrap();
        std::fs::write(staged_root.join("bin/node.exe"), b"node fixture").unwrap();
        std::fs::write(staged_root.join("adapter/index.js"), b"adapter fixture").unwrap();
        std::fs::write(staged_root.join("adapter/package.json"), b"{}").unwrap();
        std::fs::write(staged_root.join("manifest.json"), manifest.to_string()).unwrap();
        let staged = bundled_codex_runtime_at(&exe).unwrap().unwrap();
        assert_eq!(staged.node.parent().unwrap().parent().unwrap(), staged_root);
        std::fs::remove_dir_all(temp).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn windows_codex_cli_discovery_includes_versioned_localappdata_bins() {
        let temp =
            std::env::temp_dir().join(format!("Atrio Codex cli 中文 {}", uuid::Uuid::new_v4()));
        let root = temp.join("OpenAI/Codex/bin");
        std::fs::create_dir_all(root.join("0.5.0")).unwrap();
        std::fs::create_dir_all(root.join("0.6.0")).unwrap();
        std::fs::write(root.join("codex.exe"), b"root").unwrap();
        std::fs::write(root.join("0.5.0/codex.exe"), b"old").unwrap();
        std::fs::write(root.join("0.6.0/codex.exe"), b"new").unwrap();
        let dirs = windows_codex_search_dirs(&temp);
        assert_eq!(dirs[0], root);
        assert!(dirs.iter().any(|dir| dir.ends_with("0.6.0")));
        assert!(dirs.iter().any(|dir| dir.ends_with("0.5.0")));
        std::fs::remove_dir_all(temp).unwrap();
    }
}
