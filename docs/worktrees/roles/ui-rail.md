# ui-rail 任务卡

分支：`feature/0.0.7-1-ui-rail`。先读根 `AGENTS.md` 和 `docs/WORKTREE_PARTITIONS.md`。

- 合并确认的契约 C 后接入。只写 UI/CSS/api/preview 与对应测试。
- 左侧 44px 工具条和底部最大 620px 输入框均独立悬浮，不占固定网格行列。
- 保留紫色，真实锚点定位菜单，任务进度文字和箭头共用触发器。
- 即时呈现置顶，发送时读取最新 revision；处理请求失败与旧工作区响应。
- 偏好按独立渠道保存；收起只保留定位和箭头，悬停/焦点/触摸展开，菜单开启时保持展开。
- types/layout、Rust、依赖锁和构建只读；接口变化交 contracts，依赖需求交 integration。
- 交付实现提交、真实组件/浏览器证据与未执行原生项目。
