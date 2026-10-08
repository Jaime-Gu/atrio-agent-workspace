# validation 任务卡

分支：`feature/0.0.7-1-validation`。先读根 `AGENTS.md` 和 `docs/WORKTREE_PARTITIONS.md`。

- 等待完整集成 I 和对应候选，在自己的 worktree 使用 `git merge --ff-only <I>`，核对 HEAD 精确等于 I 和工作区干净；无法快进时停止并报告。
- 所有跟踪文件和 manifest 只读，证据放 work/validation/<提交>。
- 真实浏览器检查菜单锚点、完整点击、悬浮布局、即时层级与恢复、收起/展开和窄窗口。
- 与 integration 交接端口与原生实例，渠道设置完整备份/恢复，专用 A/B/A-copy 验收。
- Dev 通过后接收同候选 Beta，检查镜像、安装和独立启动。
- 不修产品、不更改候选；缺陷交负责人，新提交/候选后复验。
- 每项登记提交/候选/buildId/平台、输入预期实际、证据和 NOT_TESTED。
