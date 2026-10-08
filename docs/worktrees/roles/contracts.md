# contracts 任务卡

分支：`feature/0.0.7-1-contracts`。先读根 `AGENTS.md` 和 `docs/WORKTREE_PARTITIONS.md`。

- 只写所有权清单中的 TS 类型、layout/测试、契约脚本及 CONTRACTS_0071.md。
- 明确 stackOrder 兼容、哈希排除、置顶信封、UI 偏好读写类型及错误语义。
- Rust DTO 和内嵌测试位于 Host 文件，contracts 只读，Host 独占实现。
- 提交接口说明和局部检查，由 integration 合并为 C。Host/UI 按同一 C 实现。
- Rust 配套完成后才报告完整跨语言检查；禁止使用临时替代类型。
- 交付契约提交、修改路径、实际检查与等待配套的未测试项目。
