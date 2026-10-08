# Atrio 分区协作规则

开始前读取 `docs/WORKTREE_PARTITIONS.md` 与 `docs/worktrees/roles/` 中自己的任务卡，登记当前角色、分支、提交和 worktree 路径。

- 业务基线固定为线上标签 `0.0.6` 对应提交 `53e45575bfd6f0d8de7f1c1e5ada64862dfc859c`。分区准备提交只增加协作说明。
- 每个编码对话使用独立分支和 worktree。只在授权文件中写入，可按任务需要跨分区只读。
- 未登记文件由 integration 先指定唯一负责人。文件权限是协作约定，worktree 不限制文件读取。
- contracts 负责 TypeScript 契约、layout 和检查脚本。Rust DTO 及内嵌测试都位于 host-layout 负责的文件。
- integration 只合并成果和维护共享构建文件，不代写其他分区产品功能；产品冲突由原负责人提交修复。
- `stackOrder` 不进入业务 revision 哈希。置顶保持 modules 数组和 manifest 模块 ID 顺序，UI 按独立字段绘制。
- host-layout 只读 `policy.rs`，沿用可信 user 布局动作权限；Agent 禁止策略不能阻止用户选择模块。
- validation 在固定完整提交的独立工区检查，只写被忽略的 `work/` 证据；冻结、构建和 manifest 由 integration 的工具生成。
- 不读取或复制废弃的本机 0.0.7 源码、候选和验收记录。
- 依赖、构建缓存、应用配置、测试数据库和工作区分别管理。worktree 不会隔离 Bundle ID 或 SQLite。
- 不使用 mock、伪造结果或测试替代实现；新增检查使用真实组件、协议和存储。
- 执行检查前审查基线中的模拟测试，受影响项由所属负责人先调整为真实验证；尚未调整时登记阻碍。系统临时目录通过 TMPDIR/TMP/TEMP 指向本工区已有的 work/test-temp。
- 不执行 Git reset、revert、restore 或 checkout 覆盖代码；使用文件编辑工具和新提交修复。
- 未执行的检查登记 `NOT_TESTED`，不能继承旧候选的通过结论。
