# Windows 平台实现

Windows x64 实现已接入。`mod.rs` 声明进程、Named Pipe IPC、物理目录锁、路径校验与原子替换模块，公共 Host 仍管理权限、审批、revision 和数据库。

| 文件 | 实现 |
| --- | --- |
| `process.rs` | 暂停创建进程、绑定私有 Job Object、恢复执行和有界读取线程回收 |
| `ipc.rs` / `ipc_tests.rs` | 仅当前用户访问的本机 Named Pipe，保留 token、run scope、长度与超时校验 |
| `locks.rs` | 基于卷与 File ID 的物理目录租约，Restart Manager 的保守占用检查 |
| `paths.rs` | 本机磁盘路径限制、Win32 名称校验、reparse 拒绝、大小写和 verbatim 前缀归一化、受控 AppContainer alias |
| `storage.rs` | Windows 原子替换，以临时文件的实际物理父目录处理 AppContainer 重定向 |

共享 `locks.rs` 保留渠道与工作区 guard 接口和 macOS 锁行为；共享 `mcp_bridge.rs` 保留 MCP stdio 协议，通过平台 IPC 实现传输。`platform/mod.rs` 分派 Windows/macOS 的原子替换。当前 Windows 平台测试覆盖 IPC、锁和文件替换；新候选的原生、安装与发布验收状态见 [Windows 文档](../../../../docs/platforms/windows.md)。