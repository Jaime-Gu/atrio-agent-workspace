# Windows 0.0.6

Windows 0.0.6 follows the shared 0.0.6 UI, DTO, revision, approval, permission, workspace persistence, and recovery contracts. The receiving branch is `windows/integration`; the release target is Windows x64 with `x86_64-pc-windows-msvc`.

## Source and candidate identity

- Candidate: `0.0.6-win-x64-01` (check the remote tags/releases before freezing; never rename a historical candidate).
- `derivedFrom`: `0.0.5-atrio-02`.
- `importedWindowsCandidate`: `0.0.5-win-x64-codex-10`.
- Shared baseline and final merge commit are recorded in `docs/PROVENANCE-0.0.6.md` after the integration PR is merged.
- The source tree has no inherited Git history. Freeze records a source fingerprint, runtime lock/hash, target, toolchain, and build artifact hashes.

## Platform layout

- Shared UI and contracts stay under `src/`.
- Windows process, environment, job, IPC, path, lock, and filesystem code lives under `src-tauri/src/platform/windows/`; platform dispatch remains in `src-tauri/src/platform/mod.rs` and macOS files remain available for conditional compilation.
- `scripts/windows/` contains runtime preparation, verification, candidate build, and adapter setup. `scripts/runtime/codex-runtime.windows-x64.lock.json` is generated from Windows files only.
- `src-tauri/tauri.windows.conf.json` adds the Windows ICO, NSIS current-user installer, WebView2 bootstrapper, and runtime resource mapping. It is composed with `tauri.dev.conf.json` for Dev builds.

## Codex ACP payload

The payload pins `@agentclientprotocol/codex-acp@1.13.1` and official Node `22.23.3` win-x64. It contains `bin/node.exe`, `adapter/index.js`, a minimal adapter package manifest, `manifest.json`, and `LICENSES/`. The launch plan is `node.exe adapter/index.js`; the official Codex CLI and user authentication stay outside the application.

```powershell
npm ci
node scripts/windows/package-codex-runtime-windows.mjs prepare [<adapter-dependencies>] [<node-v22.23.3-win-x64>]
node scripts/windows/package-codex-runtime-windows.mjs verify
npm run windows:dev:build
npm run windows:beta:build
```

The Tauri bundle maps staged resources into `agents/codex`. Installed builds resolve runtime relative to the production EXE and also accept the Dev staging path `resources/agents/codex`; this preserves the candidate-10 installer fix. The AppContainer `LocalCache\\Roaming` alias and physical-parent containment fixes remain required. Unsupported UNC, network mapped drives, and untrusted reparse paths stay explicitly rejected.

## Checks and acceptance

Every candidate report separates compilation, fixtures, ACP protocol handshake, native window/UI, installation, and real provider calls. The Windows CI job prepares the pinned runtime, runs version checks, frontend builds, runtime fixtures, and Rust MSVC tests. macOS CI prepares pinned source archives into an ephemeral runtime for arm64 native compilation and tests; that generated lock is compile evidence and does not replace a macOS release lock or installer acceptance. A successful fixture or MCP stdio check is not a real model call.

The 0.0.6 report is [docs/ACCEPTANCE-0.0.6-WINDOWS.md](../ACCEPTANCE-0.0.6-WINDOWS.md). Existing 0.0.5 evidence is historical context only; run the checks again for the 0.0.6 candidate. At minimum record Dev/Beta identity and data directories, production startup without a dev server, proposal/approval/revision/permission behavior, ACP handshake and cancellation, workspace switching and restart recovery, clear missing CLI/login/adapter errors, NSIS install/reinstall, paths with spaces and Chinese characters, moved runtime directories, AppContainer aliases, and production EXE MCP stdio.

Provider status for this release remains explicit:

- Hermes: `SKIPPED_BY_USER` (not installed locally).
- Claude: `NOT_TESTED` (the user has not logged in).
- Codex: `NOT_TESTED` for real model calls; use the user's official Windows login only if they later authorize that acceptance.

Unsigned development artifacts and unsupported hardware/storage paths must remain labelled in the release notes. Do not commit `node_modules`, `target`, runtime binaries, credentials, user workspaces, or unsanitized logs.
