# 提交与合并

## 分支

`main` 保存共享源码和已合并的平台实现。使用任务分支开发，通过 Pull Request 合并到 `main`。

| 分支                  | 用途                                                    |
| --------------------- | ------------------------------------------------------- |
| `windows/integration` | 接收 Windows 实机实现，登记旧版本来源并合并当前公共改动 |
| `windows/<任务名称>`  | Windows 平台任务                                        |
| `macos/<任务名称>`    | macOS 平台任务                                          |
| `feature/<任务名称>`  | 公共功能任务                                            |
| `chore/<任务名称>`    | 构建、目录和文档任务                                    |

`windows/integration` 从建立仓库结构后的 `main` 创建。该分支初始内容对应共享源码；Windows 实机成果的接收进度见 [Windows 说明](docs/platforms/windows.md)。完成整合并合并后，通过新的任务分支继续开发。

## 两台电脑

macOS 和 Windows 分别克隆仓库，分别提交自己的任务分支。提交前获取远端更新，检查当前分支和工作目录。已有未提交内容必须保留；合并冲突按文件审查，恢复代码时使用文件编辑工具。

同一台电脑同时运行多个开发 Agent 时，每个任务使用独立分支和独立 Git worktree。各 worktree 的 `node_modules`、Rust 编译目录和 `work/` 分别维护。两台电脑通过远端分支交换提交。

## 共享文件

`src/lib/types.ts`、`src-tauri/src/kernel.rs`、权限与模块工具、依赖清单、依赖锁及公共 Tauri 配置由当前集成人协调修改。任务说明应列出负责文件；两个任务同时修改同一文件时，指定一个任务负责整合。

平台代码放入 [平台目录](src-tauri/src/platform/README.md)。共享调用方通过统一接口使用平台实现，平台实现保留独立的编译条件与验收记录。

## 提交前验证

在仓库根目录执行：

```sh
npm ci
npm run version:check
npm run build:dev
npm run build:beta
git diff --check
```

这些命令安装锁定依赖、检查版本，并编译 Dev/Beta 前端。原生功能、安装、真实 Agent、进程回收和恢复检查按照平台文档记录。当前 0.0.6 页面简介化使用 Dev → Beta 验收规则。

CI 自动在 macOS 与 Windows 执行共享前端测试/编译及对应原生编译和 fixture；依赖资源先按锁定来源与哈希准备。原生界面、安装包与人工验收状态在 Pull Request 中单独记录，未执行项填写 `NOT_TESTED`。

## 候选与产物

提交 `package-lock.json`、`src-tauri/Cargo.lock`、构建脚本及平台运行时来源和锁文件。编译输出、运行时二进制、安装器和验证日志写入被 Git 忽略的 `work/`。用户配置、认证和工作区数据保存在各自本机。

源码变化后创建新的候选编号，保留历史候选及其哈希。平台构建记录应包含提交 SHA、软件版本、渠道、目标架构、运行时清单与安装包 SHA-256。两个平台从同一提交发布时使用同一个版本标签，Release 附件名称包含平台和架构。
