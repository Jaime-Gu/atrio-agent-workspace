# macOS 平台实现

接收从 `agent/native.rs`、`agent/providers.rs`、`locks.rs`、`mcp_bridge.rs` 和文件工具提取的 macOS 系统调用。

当前运行路径继续使用原文件。平台模块接入时，在同一个提交中更新 `platform/mod.rs`、调用方与构建条件，并验证进程回收、锁、本机 IPC、路径和恢复行为。

构建与验收见 [macOS 文档](../../../../docs/platforms/macos.md)。
