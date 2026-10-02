# 平台接口目录

本目录承接 Host 的操作系统接口。`mod.rs` 通过条件编译选择 Windows 与 macOS 实现；权限、审批、revision、模块规则和数据库访问保持在公共 Host 中。

| 接口范围 | 共享调用方 | 平台实现 |
| --- | --- | --- |
| 进程创建和回收 | `agent/native.rs` | `windows/process.rs`、`macos/process.rs` |
| 渠道和工作区锁 | `locks.rs` | `windows/locks.rs`；macOS flock 与 lsof 行为保留在共享 guard 的 Unix 条件编译中 |
| 本机 IPC | `mcp_bridge.rs` | `windows/ipc.rs`；macOS 私有 Unix socket 保留在共享传输的 Unix 条件编译中 |
| 路径和物理目录身份 | `kernel.rs`、`policy.rs`、`locks.rs` | `windows/paths.rs`；macOS canonicalize/inode 行为由共享调用方保留 |
| 原子替换和目录同步 | `kernel.rs`、`policy.rs`、`lib.rs` | `windows/storage.rs`、`macos/storage.rs` |

平台操作失败必须直接返回明确错误，不得静默降级 Mock 或跳过目录边界与权限检查。Windows 支持范围与验收见 [Windows 文档](../../../docs/platforms/windows.md)，macOS 验收见 [macOS 文档](../../../docs/platforms/macos.md)。