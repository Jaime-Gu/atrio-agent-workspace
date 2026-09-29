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
fn bundled_codex_program(command: &str) -> Result<Option<PathBuf>, String> {
    match std::env::current_exe() {
        Ok(exe) => bundled_codex_program_at(&exe, command),
        Err(_) => Ok(None),
    }
}

pub fn resolve_program(command: &str, expected: &str) -> Result<PathBuf, String> {
    let path = Path::new(command);
    if path.is_absolute() {
        return if executable(path) {
            Ok(path.to_owned())
        } else {
            Err(format!("{expected} 可执行文件不存在或不可执行"))
        };
    }
    if command != expected {
        return Err(format!("需要命令 {expected} 或其绝对路径"));
    }
    if let Some(bundled) = bundled_codex_program(command)? {
        return Ok(bundled);
    }
    let mut candidates = Vec::new();
    if let Some(path) = std::env::var_os("PATH") {
        candidates.extend(std::env::split_paths(&path).map(|entry| entry.join(command)));
    }
    if matches!(command, "claude-agent-acp" | "codex-acp") {
        if let Some(dir) = std::env::var_os("PIXEL_ACP_ADAPTER_DIR") {
            candidates.insert(
                0,
                PathBuf::from(dir).join("node_modules/.bin").join(command),
            );
        }
    }
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
    candidates.extend([
        PathBuf::from("/opt/homebrew/bin").join(command),
        PathBuf::from("/usr/local/bin").join(command),
    ]);
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
        .find(|path| executable(path))
        .ok_or_else(|| format!("未找到 {expected}；请安装已验证版本或选择其绝对路径"))
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
    let executable = resolve_program(command.unwrap_or(provider.command()), provider.command());
    let host = if provider == NativeProvider::Hermes {
        executable.clone()
    } else {
        resolve_program(provider.host_command(), provider.host_command())
    };
    let mut details = Vec::new();
    if provider == NativeProvider::Codex {
        if let Ok(Some(bundled)) = bundled_codex_program("codex-acp") {
            if executable.as_ref().is_ok_and(|entry| entry == &bundled) {
                details.push(
                    "使用本应用内置 Codex ACP 与 Node；Codex 宿主使用本机已安装的官方 CLI".into(),
                );
            } else {
                details.push("使用所选外部 ACP 入口；Codex 宿主路径见下方详情".into());
            }
        }
    }
    let host_version = host
        .as_ref()
        .map_err(Clone::clone)
        .and_then(|path| safe_version(&super::native::probe_command(path, &["--version"])?));
    let version = executable.as_ref().map_err(Clone::clone).and_then(|path| {
        if provider == NativeProvider::Hermes {
            Ok(super::native::probe_hermes(path.to_str())?.version)
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
            .unwrap_or_else(|_| command.unwrap_or(provider.command()).into()),
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
pub(super) fn configure_process(cmd: &mut Command, provider: NativeProvider) -> Result<(), String> {
    match provider {
        NativeProvider::Hermes => {}
        NativeProvider::ClaudeCode => {
            cmd.env(
                "CLAUDE_CODE_EXECUTABLE",
                resolve_program("claude", "claude")?,
            );
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
            cmd.env("CODEX_PATH", resolve_program("codex", "codex")?);
            if let Some(profile) = isolated_profile("PIXEL_CODEX_TEST_HOME")? {
                // Both roots are isolated: Codex also discovers ~/.agents skills.
                cmd.env("CODEX_HOME", &profile).env("HOME", &profile);
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

    #[test]
    fn discovery_version_has_no_multiline_payload() {
        assert_eq!(safe_version("codex 1.0\nignored").unwrap(), "codex 1.0");
        assert!(safe_version("").is_err());
        assert!(safe_version("version\u{1b}hidden").is_err());
    }

    #[test]
    fn nvm_codex_candidates_are_newest_first_for_minimal_gui_path() {
        let temp = std::env::temp_dir().join(format!(
            "atrio-nvm-discovery-{}",
            uuid::Uuid::new_v4()
        ));
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
}
