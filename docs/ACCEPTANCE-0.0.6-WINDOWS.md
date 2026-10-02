# Windows 0.0.6 acceptance record

Candidate: `0.0.6-win-x64-01`

Platform: Windows x64 / MSVC
Status: `IN_PROGRESS`; this file is updated only with evidence from this candidate. Historical 0.0.5 logs are linked as context and do not satisfy a 0.0.6 check.

## Automated checks

| Area | Status | Evidence / command |
|---|---|---|
| Locked npm install | `NOT_RUN` | `npm ci` |
| Version identity | `NOT_RUN` | `npm run version:check` |
| Frontend tests | `NOT_RUN` | `npm test` |
| Dev/Beta frontend builds | `NOT_RUN` | `npm run build:dev`, `npm run build:beta` |
| Windows runtime lock fixtures | `PASS` | `npm run candidate:test`: 12 passed / 3 Darwin-only skips / 0 failures; runtime prepare + verify: 28 files, 88,685,465 bytes (including Node license), ACP 1.13.1, Node 22.23.3 (see provenance hashes). These are fixtures and payload verification, not native/model acceptance. |
| Windows Rust MSVC | `NOT_RUN` | `cargo test --manifest-path src-tauri/Cargo.toml --target x86_64-pc-windows-msvc` |
| macOS conditional compile | `NOT_RUN` | Run shared platform regression on macOS CI/host |
| Diff hygiene | `NOT_RUN` | `git diff --check` |

## Native and installer matrix

| Area | Status | Evidence |
|---|---|---|
| Dev/Beta identity and isolated data directories | `NOT_TESTED` | Record installed paths and app info |
| Production EXE starts without dev server | `NOT_TESTED` | Record launch log |
| Mock proposal, approval/rejection, document write and dashboard | `NOT_TESTED` | Native UI evidence |
| Revision conflict, permission revoke, stop/resume | `NOT_TESTED` | Native UI/Rust evidence |
| ACP initialize, two sessions, cancellation and process-tree cleanup | `NOT_TESTED` | ACP/Job Object evidence |
| Workspace switch, close/restart restore | `NOT_TESTED` | Isolated backup workspace evidence |
| Missing CLI, login, adapter errors | `NOT_TESTED` | Error copy and diagnostic evidence |
| NSIS install, complete resource tree, reinstall | `NOT_TESTED` | Installer logs and SHA-256 |
| Space/Chinese install and portable paths | `NOT_TESTED` | Path matrix |
| Moved runtime directory | `NOT_TESTED` | Runtime resolver evidence |
| AppContainer aliases and old workspace open | `NOT_TESTED` | platform_fs evidence |
| Installed production EXE MCP stdio | `NOT_TESTED` | stdio transcript; not a real model call |

## Provider scope

- Hermes: `SKIPPED_BY_USER` — not installed on this machine.
- Claude: `NOT_TESTED` — user has not logged in.
- Codex real model call: `NOT_TESTED` — user performs the official Windows login and model acceptance later if authorized.

Do not mark a fixture, protocol handshake, or MCP stdio result as a real provider call. Do not include credentials, user workspaces, runtime binaries or unsanitized logs in the source repository.
