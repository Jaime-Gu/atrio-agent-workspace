# 平台运行时清单

每个平台的运行时清单只由目标平台实际文件生成，不能使用另一平台的 Node、wrapper、Mach-O 或 lock。

- macOS：`scripts/codex-runtime.lock.json`，由 `scripts/package-codex-runtime.mjs` 读取。
- Windows x64：`scripts/runtime/codex-runtime.windows-x64.lock.json`，由 `scripts/windows/package-codex-runtime-windows.mjs` 读取。

Windows 清单固定 ACP `1.13.1`、Node `22.23.3`、`win32/x64`，记录 `bin/node.exe`、`adapter/index.js`、最小 `adapter/package.json`、`manifest.json` 和 `LICENSES/` 的相对路径、字节数、SHA-256、可执行标志与官方来源。清单中的 `resourceRoot` 是 `work/resources.noindex/agents/codex`；构建时由 Tauri 映射进安装包，安装后以当前 EXE 相邻的 `agents/codex` 定位。

准备和验证：

```powershell
node scripts/windows/package-codex-runtime-windows.mjs prepare [<adapter-dependencies>] [<node-v22.23.3-win-x64>]
node scripts/windows/package-codex-runtime-windows.mjs verify
```

运行时包含 ACP adapter，不包含 Codex CLI、账号、API key、个人 profile 或认证文件。`work/`、安装包和构建产物由 `.gitignore` 排除。
