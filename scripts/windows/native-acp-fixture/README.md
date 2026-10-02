# Synthetic Windows ACP production Host fixture

This is **Windows ACP Fixture**, not Hermes, Codex, Claude or a real model. Its version/check strings match the existing Hermes probe grammar solely so the production Host can select an explicit native EXE without changing or weakening provider validation. Record every result as synthetic ACP/Host acceptance. A probe pass is not provider, authentication or model-call acceptance.

Build on Windows x64 with PowerShell 7 and the MSVC Rust toolchain:

```powershell
pwsh -File scripts/windows/native-acp-fixture/build.ps1
```

The printed EXE path is inside ignored `work/native-acp-fixture-target.noindex/`. Select this exact absolute path in the Agent settings, set the display name to `Windows ACP Fixture`, provider to `hermes`, transport to `stdio`, arguments to the fixed `["acp"]`, and working directory to the currently opened isolated test workspace. Do not install it as `hermes`, place it on PATH, or attach it to a release installer.

The production Host permits absolute existing `.exe` provider paths and fixed Hermes `acp` arguments. Workspace containment, persisted session mapping, run scope, permissions and Job ownership remain Host-enforced. The fixture ignores MCP server configuration, does not call tools or models, does not read or write workspace files, does not read credentials or profiles, makes no network requests and produces no protocol/input logs.

- `--version` and `acp --check` explicitly include `synthetic ... not Hermes`.
- `initialize` and `session/new` establish a real newline JSON-RPC ACP session with synthetic agent identity.
- Send exactly `fixture:complete` twice to verify two completed prompts reuse the same provider session; the synthetic update identifies the turn number.
- Send any other text to create an indefinitely pending prompt and an owned descendant holding stdio open. Click Stop, disconnect, switch workspace or close the production app to exercise Host process-tree and reader reclamation. The fixture reports protocol `cancelled` when asked but deliberately stays alive until Host reclaims it; independently check that both owned processes exit.
- Host cancellation disconnects its native transport. Reconnect before further prompts; no fixture change attempts to bypass this existing lifecycle.

No binary, runtime, personal data or logs belong in this source directory. `Cargo.lock` pins fixture build dependencies; output stays in `work/`. This artifact is solely a validation input and does not add a production provider.
