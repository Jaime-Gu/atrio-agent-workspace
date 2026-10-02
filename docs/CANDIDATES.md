# 候选来源与三渠道构建

此工具只记录来源与构建事实，不把自动测试、Web 或原生验收自动标为通过。固定 Handoff 仍是当前实施状态的唯一入口。

Windows 0.0.6 使用候选 `0.0.6-win-x64-01`（Windows x64/MSVC）；其 ACP/Node 清单位于 `scripts/runtime/codex-runtime.windows-x64.lock.json`，由 `scripts/windows/package-codex-runtime-windows.mjs` 以 Windows 实际文件生成。该编号不能覆盖历史 `0.0.5-win-x64-*` 候选；最终合并提交后若源码或归档改变，必须重新冻结新编号。

日常 1421 使用 `npm run dev`，未冻结构建在设置页显示“未冻结 · 日常开发”。待本次改动和自动回归收拢后，在源码根目录执行：

```sh
npm run candidate:freeze -- 0.1.3-p0-01
npm run candidate:check -- work/candidates.noindex/0.1.3-p0-01/candidate.json
npm run candidate:build -- web work/candidates.noindex/0.1.3-p0-01/candidate.json
npm run preview
```

`preview` 只服务最近成功冻结的 Web 构建，也可用 `npm run candidate:preview -- /absolute/path/build.json` 指定历史候选。1422 绑定 localhost 且 strictPort，不杀进程、不跳号。尚无冻结候选时明确失败，不回退共享 dist。

Web 验收后，保持源码、锁文件和依赖不变，继续：

```sh
npm run candidate:build -- dev work/candidates.noindex/0.1.3-p0-01/candidate.json
npm run candidate:build -- beta work/candidates.noindex/0.1.3-p0-01/candidate.json
```

每次构建生成独立 buildId；Web/Dev/Beta 的 candidateId 与源码归档 SHA-256 相同。Web 静态文件保留在候选自身的 `builds/<buildId>/dist`，Dev/Beta 的共享前端 dist 不会覆盖它。设置页显示候选编号、构建编号和源码指纹；原生包与内嵌前端身份不符时明确报错。手工运行普通 `npm run build` 或 `desktop:*:build` 仍可用于日常开发，但未冻结包没有验收候选身份。

候选目录包含：

- `candidate.json`：基础版本、源码逐文件清单、归档 SHA-256、真实 Git 信息（没有仓库则为 null）、锁文件与配置 SHA-256、工具链、目标架构、实际依赖包元数据及允许的渠道差异。
- `source.tar.gz`：源码、锁文件、脚本、配置与文档。排除 node_modules、dist、work、outputs、.git、src-tauri/target、自动生成 schema 与 tsbuildinfo。不追踪外部源码软链接，遇到时拒绝冻结。
- `builds/<buildId>/build.json`：该次渠道身份、实际工具链、候选清单 hash、实际产物绝对路径和 hash。文件使用直接 SHA-256；应用目录使用排序后文件/软链接清单的 SHA-256，并附各文件 hash。初始验收状态为 NOT_TESTED。
- `builds/<webBuildId>/dist`：1422 使用的独立 Web 产物。

源码、锁文件、配置、源码执行权限位或安装依赖元数据变更时，绑定旧候选的后续构建会拒绝。修改后应创建新的 candidateId 并重新验收受影响部分；不要修改旧 manifest 或把旧产物改名。构建后也复核源码，避免构建期间的并行编辑被误记成已冻结来源。已冻结的 Web 预览可在工作树进入下一迭代后继续运行，启动时核对其自身 manifest、归档与静态产物哈希。

构建后将实际验收结果、证据、测试适用范围登记到固定 Handoff 和对应验收报告。最终交付应复制候选 manifest、源码归档、build.json、DMG 与校验清单到用户成果目录，保留先前已发布版本。

工具自身回归：`npm run candidate:test`。这不等于 P0 原生验收。
