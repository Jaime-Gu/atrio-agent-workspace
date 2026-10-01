# macOS 构建脚本目录

现有入口位于 `scripts/build-macos.sh`、`scripts/dev-macos.sh`、`scripts/stage-macos.mjs` 和 `scripts/package-codex-runtime.mjs`，继续由根目录的 npm 命令调用。

新增 macOS 专用脚本放入本目录。已有脚本迁入时，必须在同一个提交中更新 npm 命令、相对路径、候选工具、文档和验证，确保全部引用使用实际路径。

完整入口见 [macOS 文档](../../docs/platforms/macos.md)。
