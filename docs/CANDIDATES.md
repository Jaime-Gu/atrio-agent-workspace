# 候选来源与三渠道构建

此工具只记录来源与构建事实，不把自动测试、Web 或原生验收自动标为通过。固定 Handoff 仍是当前实施状态的唯一入口。

Windows 0.0.6 的最终发布候选为 `0.0.6-win-x64-03`（Windows x64/MSVC），从 main `53e45575bfd6f0d8de7f1c1e5ada64862dfc859c` clean 冻结；源码指纹 `sha256:e90729d776580b34f50a53e4da04b118f0c2814d0c862b6eecfb128f01efbee3`。候选 02 的完整原生证据和最终 03 的复验子集分别登记，见 [验收记录](ACCEPTANCE-0.0.6-WINDOWS.md)。ACP/Node 清单为 `scripts/runtime/codex-runtime.windows-x64.lock.json`，由 Windows 实际文件生成。

[Windows 0.0.6 Release](https://github.com/Jaime-Gu/atrio-agent-workspace/releases/tag/0.0.6) 已发布，标记为 Pre-release；发布时间 2026-10-03 01:43:19（Asia/Shanghai）。仓库为 PRIVATE，下载 Release 附件需要具备该仓库的 GitHub 访问权限。

Release candidate/build JSON 是脱敏发布投影；其下载 SHA-256 以 `SHA256SUMS` 为准。metadata 单独登记 `originalFrozenManifestSha256`，保留与本机原冻结清单的关联；原 source.tar.gz 不改写。

03 的 Dev/Beta buildId 分别为 `0.0.6-win-x64-03-dev-20261002T162208955Z-fc8362`、`0.0.6-win-x64-03-beta-20261002T162535109Z-f831e2`。完整 NSIS SHA-256 分别为 `77ca4c3f0678ac39c936553ba26d909967b139b703a8c809df86d4a5aa251225`、`045c4201506bf4f957f938f88074df94e8272b93f82185f8329b9a1c56d401b7`；这些固定身份由最终 candidate/build/Release 清单记录。候选 01/02 和历史 0.0.5 不覆盖、不重命名。发布后的状态文档提交不用于生成新安装包，也不改变既有 tag 或 03 来源；今后如从更新后的源码重新构建，仍须创建新候选并重新验收。

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
