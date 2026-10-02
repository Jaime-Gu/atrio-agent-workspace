# Windows 0.0.6 来源记录

本次记录区分历史来源、本机导入候选、共享基线和预发布候选。最终 main 合并提交与发布包身份由最终不可变 Release manifest、candidate/build 清单及 SHA256SUMS 记录；此处不预填未知提交或 Release 地址。

| 项目 | 已核实值 |
|---|---|
| 应用版本 / 目标 | `0.0.6` / Windows x64 / `x86_64-pc-windows-msvc` |
| `derivedFrom` | `0.0.5-atrio-02` |
| 历史平台/架构 | `darwin/arm64`，保留原 provenance |
| 原 archive SHA-256 | `b19b6fd89de9c7bdb9bb5c0f1a16110d2e7ebed4c699cdea11a7946006708aab` |
| `importedWindowsCandidate` | `0.0.5-win-x64-codex-10` |
| 共享 main 基线 | `401ad96175561a097eb485521bc1416d4f5a2f1c` |
| windows/integration 基线 | `793c12551349452a8ff3a1a18cefbf56640fc3d0` |
| 当前预发布验收候选 | `0.0.6-win-x64-02` |
| 当前候选源码提交 | `3143c49dfbec91c37b1110722cdacc6104c22885`，冻结时工作树 clean |
| 当前源码 archive SHA-256/指纹 | `e5a5c35a98d3c49ea53e03e74e680f7c264e56bb708c98daeb518a47d8e48a37` |
| 整合 PR | [#2](https://github.com/Jaime-Gu/atrio-agent-workspace/pull/2)，草稿，base `windows/integration` |
| Runtime lock | `scripts/runtime/codex-runtime.windows-x64.lock.json` |
| Runtime lock SHA-256 | `2e4aaeefbbb32e042f570f0ef09e72f432135a3f2f105c2f9a8aa168443dd9eb` |
| Runtime manifest SHA-256 | `baf81b4be8394c71829a9a6dc064a99c54721d42abf224887eb7cdf6a6ae41ad` |
| Runtime 文件数 / 字节数 | 28 / 88,685,465，含 Node LICENSE |
| ACP | `@agentclientprotocol/codex-acp@1.13.1` |
| Windows Node | 官方 `22.23.3 win-x64`，archive SHA-256 `2b0ff57b049cda1bbcea2240eec20467018713c1efe1f7360c2681859b90ed71` |
| Rust 工具链 | `rustc 1.98.1 (48a229cea 2026-09-01)` / MSVC |
| CI 回归 | [37004579179](https://github.com/Jaime-Gu/atrio-agent-workspace/actions/runs/37004579179)，四项 SUCCESS |

本机候选 02 的冻结文件位于 `work/candidates.noindex/0.0.6-win-x64-02/`，Dev/Beta build.json 和安装器位于对应 `work/builds/`。验收证据位于 `work/evidence/candidate-02-*` 与 `work/evidence/native-02/`。这些路径保留为本机证据，不把二进制或未脱敏日志提交源码仓库。

候选 01、02 与历史 0.0.5 身份不修改、不重命名。合并后源码和归档变化必须生成新的发布候选；最终安装包从最终 main 提交构建，发布 manifest 登记该提交、候选、源码指纹、工具链、runtime lock/hash、Dev/Beta 产物 SHA-256、分项验收与未执行项目，并与下载附件重新核对。

新增候选 manifest 的 `derivedFrom`、`importedWindowsCandidate`、`sharedBaselineCommit`、`receivingBaselineCommit` 为独立根字段，完整历史记录嵌入 `provenance`；`source.git.commit` 始终是实际冻结提交，不冒充最终发布提交。旧 manifest 保持可读且不改写。

真实 provider 范围保持：Hermes `SKIPPED_BY_USER`；Claude、Codex 真实认证/模型调用 `NOT_TESTED_BY_USER_SCOPE`。Codex initialize-only 是协议握手证据，与 synthetic 会话及真实模型调用分开登记。
