# Windows 0.0.6 来源记录

本次记录区分历史来源、本机导入候选、共享基线和最终发布提交。最终发布源码与候选/安装包身份已核实；发布后文档提交只登记状态，不改变候选 03、安装包或版本 tag。

| 项目 | 已核实值 |
|---|---|
| 应用版本 / 目标 | `0.0.6` / Windows x64 / `x86_64-pc-windows-msvc` |
| 实机验收系统 | Windows 11 Home 中文版 `10.0.26200` / build `26200`，x64 |
| `derivedFrom` | `0.0.5-atrio-02` |
| 历史平台/架构 | `darwin/arm64`，保留原 provenance |
| 原 archive SHA-256 | `b19b6fd89de9c7bdb9bb5c0f1a16110d2e7ebed4c699cdea11a7946006708aab` |
| `importedWindowsCandidate` | `0.0.5-win-x64-codex-10` |
| 共享 main 基线 | `401ad96175561a097eb485521bc1416d4f5a2f1c` |
| windows/integration 基线 | `793c12551349452a8ff3a1a18cefbf56640fc3d0` |
| 最终发布候选 | `0.0.6-win-x64-03` |
| Release tag | `0.0.6`，annotated tag，解析到以下固定发布源码提交 |
| 最终发布源码提交 | `53e45575bfd6f0d8de7f1c1e5ada64862dfc859c`，main 合并后冻结，工作树 clean |
| 最终源码 archive SHA-256/指纹 | `sha256:e90729d776580b34f50a53e4da04b118f0c2814d0c862b6eecfb128f01efbee3` |
| 源码树指纹 | `55b5ba3de06815cda90e913e2359fba27857f4adc21f35a011aa2ec72ed4d547` |
| 本机原冻结 candidate.json SHA-256 | `3ced13ddeeda81957526ccb09a107437632241ee8e517baa2adab2ae8d971d8c` |
| 整合 PR | [#2](https://github.com/Jaime-Gu/atrio-agent-workspace/pull/2)，已合并 `windows/integration`；merge `44d0a6ba1e37aca4d8b562417c8f38cc46b88e57` |
| main PR | [#3](https://github.com/Jaime-Gu/atrio-agent-workspace/pull/3)，已合并 main；merge 为最终发布源码提交 |
| Runtime lock | `scripts/runtime/codex-runtime.windows-x64.lock.json` |
| Runtime lock SHA-256 | `2e4aaeefbbb32e042f570f0ef09e72f432135a3f2f105c2f9a8aa168443dd9eb` |
| Runtime manifest SHA-256 | `baf81b4be8394c71829a9a6dc064a99c54721d42abf224887eb7cdf6a6ae41ad` |
| Runtime 文件数 / 字节数 | 28 / 88,685,465，含 Node LICENSE |
| ACP | `@agentclientprotocol/codex-acp@1.13.1` |
| Windows Node | 官方 `22.23.3 win-x64`，archive SHA-256 `2b0ff57b049cda1bbcea2240eec20467018713c1efe1f7360c2681859b90ed71` |
| 最终安装器签名 | Dev/Beta 均 `NotSigned`；SmartScreen 声誉未建立 |
| Rust 工具链 | `rustc 1.98.1 (48a229cea 2026-09-01)` / MSVC |
| 发布源码 CI 回归 | [CI 37032940358](https://github.com/Jaime-Gu/atrio-agent-workspace/actions/runs/37032940358)，四项 SUCCESS；macOS 初次时序失败，重跑 103 passed |

| 渠道 | buildId | 安装包字节数 | 安装包 SHA-256 |
|---|---|---:|---|
| Dev | `0.0.6-win-x64-03-dev-20261002T162208955Z-fc8362` | 25,980,376 | `77ca4c3f0678ac39c936553ba26d909967b139b703a8c809df86d4a5aa251225` |
| Beta | `0.0.6-win-x64-03-beta-20261002T162535109Z-f831e2` | 25,985,970 | `045c4201506bf4f957f938f88074df94e8272b93f82185f8329b9a1c56d401b7` |

[Windows 0.0.6 Release](https://github.com/Jaime-Gu/atrio-agent-workspace/releases/tag/0.0.6) 已发布，标记为 Pre-release；发布时间 2026-10-03 01:43:19（Asia/Shanghai）。仓库为 PRIVATE，下载 Release 附件需要具备该仓库的 GitHub 访问权限。

Release 的 candidate/build JSON 附件为去除本机绝对路径等信息的发布投影，不是原冻结文件的逐字节副本；下载投影应按 Release `SHA256SUMS` 校验。发布 metadata 的 `originalFrozenManifestSha256` 对应上述本机原清单 `3ced13ddeeda81957526ccb09a107437632241ee8e517baa2adab2ae8d971d8c`，不能当作下载投影的 SHA-256。源码归档未改写，archive SHA-256 仍为 `e90729d776580b34f50a53e4da04b118f0c2814d0c862b6eecfb128f01efbee3`。

最终冻结文件位于 `work/candidates.noindex/0.0.6-win-x64-03/`；Dev/Beta build.json 和完整 NSIS 位于对应 `work/builds/`。本机最终验收证据为 `work/evidence/final-03-*` 与 `work/evidence/native-final-03/summary.json`。最终包内版本、候选、buildId 与源码指纹已通过原生 app_info/内嵌身份核实。二进制、个人认证、用户工作区和未脱敏原始日志不提交源码仓库。

早期候选 02 仍对应 `3143c49dfbec91c37b1110722cdacc6104c22885`、archive SHA-256 `e5a5c35a98d3c49ea53e03e74e680f7c264e56bb708c98daeb518a47d8e48a37`；其全面原生证据保留在 `work/evidence/candidate-02-*` 与 `native-02/`。最终 03 重新构建并复验关键原生子集，未把 02 全面检查写为 03 已逐项重跑。候选 01、02 与历史 0.0.5 身份不改写、不重命名。

新增候选 manifest 的 `derivedFrom`、`importedWindowsCandidate`、`sharedBaselineCommit`、`receivingBaselineCommit` 为独立根字段，完整历史记录嵌入 `provenance`；`source.git.commit` 始终是实际冻结提交，不冒充最终发布提交。旧 manifest 保持可读且不改写。

真实 provider 范围保持：Hermes `SKIPPED_BY_USER`；Claude、Codex 真实认证/模型调用 `NOT_TESTED`。Codex initialize-only 是协议握手证据，与 synthetic 会话及真实模型调用分开登记。
