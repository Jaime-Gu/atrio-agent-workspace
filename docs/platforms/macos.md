# macOS

当前共享源码版本为 `0.0.6`，既有原生构建目标为 `aarch64-apple-darwin`，最低 macOS 版本为 12.0。候选 `0.0.6-information-03` 的验收采用 Dev → Beta。

## 现有入口

| 用途                   | 命令或路径                          |
| ---------------------- | ----------------------------------- |
| 桌面开发               | `npm run desktop:dev`               |
| Dev 应用               | `npm run desktop:dev:build`         |
| Beta 应用与 DMG        | `npm run desktop:beta:build`        |
| 构建脚本               | `scripts/build-macos.sh`            |
| Codex 运行时准备与验证 | `scripts/package-codex-runtime.mjs` |
| 当前运行时锁           | `scripts/codex-runtime.lock.json`   |

安装 npm 依赖、Rust 工具链及 Xcode Command Line Tools 后，按照 [Codex 资源说明](../CODEX_BUNDLED.md) 准备锁定资源。原生构建会核验 `work/resources.noindex/agents/codex/` 与资源锁，缺失或哈希变化时直接报错。

Node 与 ACP 资源从发布归档恢复或按锁定来源重新准备。官方 Codex CLI 和认证由本机用户管理。

## 平台代码

目前平台行为分布在 `agent/native.rs`、`agent/providers.rs`、`locks.rs`、`mcp_bridge.rs` 及文件工具中。提取公共平台接口时，同步实现 `platform/macos/` 中对应行为，并保留原有权限与恢复要求。

## 验证记录

涉及原生变更时核对：资源目录、启动与退出、Agent 子进程回收、工作区锁、IPC、文件审批和重启恢复。涉及安装时记录实际 `.app` 与 DMG 的版本、架构、签名、SHA-256 和运行路径。

Codex 真实模型调用继续登记为用户后置项目；共享前端编译检查的范围限定在依赖、版本和编译。
