# Actual bundled Codex ACP initialize-only acceptance

This standalone Windows harness compiles the actual `src-tauri/src/platform/windows/process.rs`, including `CREATE_SUSPENDED` → private Job assignment → resume, bounded IO, and Job/read-thread cleanup. The small shared reader shim uses the production reader rules and hides raw stderr. The CLI `--version` probe and adapter run use fresh isolated `CODEX_HOME`, `HOME`, `USERPROFILE`, `APPDATA`, `LOCALAPPDATA` and temporary directories under `work/`. No personal authentication or configuration is read or copied.

The harness sends exactly one ACP `initialize` request to bundled Node `22.23.3` and Codex ACP `1.13.1`, using the supplied official Windows Codex CLI `0.159.0-alpha.12.1`. It sends no `session/new`, prompt, authentication, provider route, file or terminal request. Adapter-produced auth status notifications are not persisted. Real model acceptance remains `NOT_TESTED`.

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/windows/verify-codex-initialize.ps1 -BuildOnly
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/windows/verify-codex-initialize.ps1 -CliPath 'C:\path\to\official\codex.exe'
```

The wrapper validates actual resource bytes against the Windows runtime lock before launching. Target output is `work/codex-initialize-fixture-target.noindex/`; reports and fresh isolated data are kept in `work/evidence.noindex/`. Reports include source commit, runtime lock/manifest/CLI/source SHA-256, actual initialize response, elapsed time and confirmed Job/leader/thread cleanup. A failure is preserved as `FAIL`; the fixture never retries authentication or changes remote routing.

Use ordinary absolute Win32 disk paths for launch arguments. The pinned adapter invokes its host via `cmd.exe`, so blindly converting `CODEX_PATH` to a verbatim `\\?\` spelling changes shell behavior. The fixture rejects reused isolation directories and never targets unrelated CLI processes.