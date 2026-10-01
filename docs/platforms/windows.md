# Windows

状态：`PENDING_SOURCE_IMPORT`。Windows 实机代码等待上传，接收分支为 `windows/integration`。

## 接收已有成果

1. 在 Windows 保存已有源码与改动，记录当前软件版本、候选编号和原始来源。
2. 获取 `windows/integration`，在独立目录检查当前 0.0.6 共享源码与 Windows 成果的差异。
3. 以原 Windows 接手材料 `0.0.5-atrio-02` 为比较依据，提取 Windows 平台变更；逐文件合并到接收分支，保留 0.0.6 公共界面和候选 12 的 Codex 行为。
4. 将平台实现放入 `src-tauri/src/platform/windows/`，对应构建脚本放入 `scripts/windows/`，说明公共接口变化。
5. 在 Windows 完成编译、原生功能和安装验收，提交 Pull Request 到 `main`，记录具体证据及未执行项目。

当前接收分支来源为建立目录结构后的共享 `main`。Windows 自己的旧提交或源码来源需单独记录。Mac 候选源码归档与 Codex 增量包保留原始来源身份。

## 原生实现要求

| 范围       | Windows 实现要求                                                             |
| ---------- | ---------------------------------------------------------------------------- |
| Agent 启动 | 明确程序、argv、工作目录与环境；处理 `.exe`、`.cmd`、PATHEXT、空格和中文路径 |
| 取消和退出 | 管理 Atrio 创建的进程树，验证读取线程退出及子进程回收                        |
| 环境       | 受控保留 USERPROFILE、APPDATA、LOCALAPPDATA、TEMP/TMP 等必要变量             |
| 工作区与锁 | 使用 Windows 物理目录身份和系统锁机制，覆盖占用、异常退出和跨渠道访问        |
| 本机 IPC   | Windows 本机通信与当前用户访问控制，保留 session/run scope 校验              |
| 文件持久化 | 覆盖原子替换、占用文件、路径规范化与中断恢复                                 |
| Tauri      | Windows 配置、ICO 图标、WebView2、MSVC 目标和 NSIS EXE                       |

## Codex 随包资源

使用 `@agentclientprotocol/codex-acp@1.13.1` 和目标架构 Windows Node。当前 macOS 运行时采用 Node `22.23.3`，Windows 需要验证相应发行文件并记录来源与 SHA-256。

Host 使用包内 `node.exe` 和 ACP JavaScript 入口组成完整启动参数，独立探测本机官方 Codex CLI。资源目录、入口、平台、架构、文件哈希及许可证写入 Windows manifest 和 lock。资源放入 `work/`，构建时映射到 Tauri 的 `agents/codex/`。

官方 Codex CLI、登录和工作区运行范围由 Windows 本机配置。真实 Codex 模型验证继续保持用户后置状态，另有明确要求时登记实际验证结果。

## 构建和验收

Windows 构建脚本、运行时清单和候选记录必须使用实际目标架构及 Windows 文件规则。共享 `package-lock.json` 和 `src-tauri/Cargo.lock` 通过各平台包管理器安装，平台依赖保留在同一份锁文件中。

验收记录分别包含：共享前端编译、Rust 编译、安装目录启动、真实 ACP 握手、取消回收、工作区切换、审批、重启恢复、EXE 安装与重新安装。真实 Agent 项目逐个登记；尚未执行的项目标注 `NOT_TESTED`。

GitHub 上的 `Frontend / windows-2022` 检查覆盖版本与前端编译。Windows 原生安装包及相关验收证据由本节的原生流程提供。
