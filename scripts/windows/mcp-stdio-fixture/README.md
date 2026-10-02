# Production GUI EXE MCP stdio fixture

This small Windows-only Rust harness compiles the current checkout's shared `src-tauri/src/mcp_bridge.rs` and actual `src-tauri/src/platform/windows/ipc.rs`. Its Host handler is synthetic and held in memory. The executable under test is always a caller-supplied installed or packaged production GUI EXE, launched from its own directory with `--workspace-mcp`.

The eleven checks cover inherited GUI stdio, pre-initialize rejection, initialize and expected app version, actual tools/list, production Named Pipe forwarding, UTF-8 arguments/results, stale run scope denial, missing scope rejection before IPC, ping, EOF shutdown and empty stderr. The report records **zero Provider calls and zero workspace databases opened**. Passing this fixture is transport/protocol evidence; it does not establish native GUI or real model acceptance.

```powershell
# Build without requiring an EXE yet. Dependencies use the committed Cargo.lock.
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/windows/verify-gui-mcp.ps1 -BuildOnly

# Use the installed or packaged complete app; paths may contain spaces/Chinese.
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/windows/verify-gui-mcp.ps1 -ExePath 'C:\Apps\Atrio 工作台 Beta\pixel-workspace.exe' -ExpectedVersion 0.0.6 -OutputPath 'work\evidence.noindex\beta-mcp-stdio.json'
```

Cargo outputs are written to `work/mcp-stdio-fixture-target.noindex/`; default reports go to `work/evidence.noindex/`. The report includes EXE SHA-256, expected/reported version, fixture source commit and source hashes. No development-server connection, source-directory cwd, user login, authentication file or workspace data is required by the child process.