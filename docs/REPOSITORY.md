# 目录与分支

## 当前来源

共享源码版本为 `0.0.6`，建立本目录结构前的 `main` 提交为 `0b6e74c75cc57e500888d78971cfff741f567ea3`。历史 `0.0.5-atrio-12` 标签继续指向 `7b6fc1817bb5c9afabbc80f3c674effd78a5d443`。

Windows 实机成果已从 `0.0.5-win-x64-codex-10` 逐文件导入；历史来源为 `0.0.5-atrio-02`。整合保留当前 0.0.6 公共界面、Codex 行为和原始 darwin/arm64 provenance。本次来源见 `windows-provenance.json`，候选与验收见 Windows 平台说明。

## 目录职责

| 路径                            | 内容                                                       |
| ------------------------------- | ---------------------------------------------------------- |
| `src/`                          | 两个平台共享的 React 界面、类型和交互                      |
| `src-tauri/src/`                | Rust Host、模块工具、权限与持久化                          |
| `src-tauri/src/platform/`       | 实际 Windows 进程、IPC、锁、路径、文件操作与 macOS 对应进程/文件实现 |
| `src-tauri/tauri.conf.json`     | 当前 Beta 配置，包含现有 macOS 资源和打包设置              |
| `src-tauri/tauri.dev.conf.json` | 当前 Dev 渠道配置                                          |
| `scripts/`                      | 已接入的构建、版本、候选与运行时脚本                       |
| `scripts/macos/`                | macOS 构建目录与现有入口说明                               |
| `scripts/windows/`              | Windows runtime 准备/校验、Dev/Beta 构建、安装与 MCP 验证入口 |
| `scripts/runtime/`              | 平台运行时清单、资源锁的组织说明                           |
| `docs/platforms/`               | 平台状态、构建方法与验收要求                               |
| `.github/workflows/`            | macOS/Windows 共享前端及两个平台原生编译与 fixture 检查 |
| `work/`                         | 被 Git 忽略的依赖准备文件、工作树、构建产物和验证记录      |

Cargo 清单和锁文件的实际位置为 `src-tauri/Cargo.toml`、`src-tauri/Cargo.lock`。npm 清单和锁文件位于仓库根目录。

## 平台接入规则

现有 macOS 配置和脚本继续使用当前路径。平台配置拆分时，在同一个提交中更新脚本引用和配置校验；Windows 配置接入时添加 `src-tauri/tauri.windows.conf.json`，明确覆盖目标安装格式、图标和资源路径，并与 Dev/Beta 渠道配置组合验证。

Rust 平台边界包括子进程、锁、本机 IPC、资源与用户目录、文件替换及目录同步。实现文件在平台目录归属清楚，共享 Host 继续统一处理权限、审批、版本冲突和数据库写入。

## CI 与发布状态

`Shared frontend compilation` 工作流在 `macos-14` 和 `windows-2022` 运行 `npm ci`、版本检查、Dev/Beta 前端编译。CI 的 Node 版本为 `24.18.0`。Codex ACP 随包 Node 版本由运行时锁独立管理，当前 macOS 锁定为 `22.23.3`。

Windows 平台实现、锁与构建入口已接入。编译、fixture、原生界面、安装和真实模型调用分项记录在 `docs/ACCEPTANCE-0.0.6-WINDOWS.md`；冻结候选和构建清单位于被忽略的 `work/`，最终发布附件另附不可变清单。源码接入不等于安装验收完成。

macOS 和 Windows 的构建记录分别标注平台、架构与渠道。Release 附件按照 `Atrio-WorkSpace-<版本>-<渠道>-<平台>-<架构>.<扩展名>` 命名，安装包、运行时归档和 SHA-256 清单按发布需要上传。

分支和独立 Git worktree 的协作规则见 [CONTRIBUTING.md](../CONTRIBUTING.md)。
