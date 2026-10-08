# host-layout 任务卡

分支：`feature/0.0.7-1-host-layout`。先读根 `AGENTS.md` 和 `docs/WORKTREE_PARTITIONS.md`。

- 合并契约 C 后同步 Rust DTO、动作和内嵌契约测试，独占 kernel/lib/module_tools、偏好和恢复辅助文件。
- 保存 stackOrder，覆盖旧 manifest、snapshot、pending_commit、创建/复制/移除；纯置顶不进入业务哈希，不重排 modules 数组。
- 旧 Agent 提案应用保留当前层级。可信 user 布局动作保持既有权限，policy.rs 只读。
- UI 偏好在渠道应用设置保存，使用已有平台文件入口，错误直接暴露。
- 自动测试使用专用真实目录和 SQLite，不启动用户 GUI 或访问日常数据。
- 交付 Host 提交、契约往返、持久化恢复、幂等、revision 与权限证据。
