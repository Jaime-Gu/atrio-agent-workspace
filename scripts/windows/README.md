# Windows 构建与运行时脚本

这些脚本只在 Windows x64/MSVC 上执行，并将大文件和构建结果写入被 Git 忽略的 `work/` 和 `src-tauri/target/`。可交付产物写入 `work/builds/<buildId>/<channel>/`。

## 常用入口

```powershell
npm ci
npm run version:check
npm run windows:runtime:verify
npm run windows:dev:build       # native-dev 前端 + x86_64-pc-windows-msvc
npm run windows:beta:build      # native-beta 前端 + NSIS
```

准备随包 Codex ACP（不会安装 CLI，也不会复制认证）时，默认使用仓库内锁定的 `scripts/windows/adapter/package.json` 与 `package-lock.json`；也可传入包含 `@agentclientprotocol/codex-acp@1.13.1` 的依赖根目录。可选第二个参数为已下载的 Node 22.23.3 win-x64 解压目录：

```powershell
npm run windows:runtime:prepare
node scripts/windows/package-codex-runtime-windows.mjs verify
```

准备脚本先对该 manifest 执行 `npm ci --ignore-scripts`，缺少 Node 时下载并核对 Node 官方 `SHASUMS256.txt` 和固定 archive SHA-256，再核对 PE x64 头、ACP 版本、npm lock、入口、许可证、字节数和 SHA-256，并原子写入 `work/resources.noindex/agents/codex` 及 `scripts/runtime/codex-runtime.windows-x64.lock.json`。源码仓库不提交 Node、`node_modules`、target、安装包或个人登录资料。

普通 `prepare` 对已存在目录直接验证，对清洁 checkout 生成的 payload 必须匹配提交的 lock。仅在经过审查的资源版本更新时用 `prepare --refresh-lock` 生成新的 lock；冻结后更新 lock 必须使用新候选编号。

`build-windows.mjs` 将 `node.exe adapter/index.js` 组成启动计划，使用 `src-tauri/tauri.windows.conf.json` 的 NSIS 资源映射，并把验证过的输出复制为 `Atrio-WorkSpace-0.0.6-<channel>-windows-x64.exe`。安装后 resolver 以生产 EXE 相邻的 `agents/codex` 为准，同时兼容 Dev 的 `resources/agents/codex` staging 布局。

`setup-adapters-windows.ps1` 仅准备用户选择的 ACP adapter 依赖，默认安装到 `%LOCALAPPDATA%\Atrio\acp-adapters\0.0.6`；官方 Codex CLI、登录和认证始终由本机用户管理。

验收必须把 fixture、协议握手、原生 UI、安装包和真实模型调用分项记录。Hermes 按用户决定跳过，Claude 未登录，Codex 真实模型调用仍为 `NOT_TESTED`；fixture/MCP stdio 通过不能代替真实 provider 结果。
