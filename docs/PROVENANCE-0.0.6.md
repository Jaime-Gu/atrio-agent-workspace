# 0.0.6 Windows provenance

This record keeps platform provenance separate from the historical macOS source and from the 0.0.5 Windows candidate. Values marked `TO_BE_FILLED_AFTER_MERGE` must be written only after the final merge commit is frozen and rebuilt.

| Field | Value |
|---|---|
| Product version | `0.0.6` |
| Candidate | `0.0.6-win-x64-02` |
| Platform / architecture | Windows x64 / `x86_64-pc-windows-msvc` |
| `derivedFrom` | `0.0.5-atrio-02` |
| `importedWindowsCandidate` | `0.0.5-win-x64-codex-10` |
| Original archive SHA-256 | `b19b6fd89de9c7bdb9bb5c0f1a16110d2e7ebed4c699cdea11a7946006708aab` |
| Shared baseline (`main`) | `401ad96175561a097eb485521bc1416d4f5a2f1c` (recheck remote before integration) |
| Receiving baseline (`windows/integration`) | `793c12551349452a8ff3a1a18cefbf56640fc3d0` (recheck remote before integration) |
| Integration PR | `TO_BE_FILLED_AFTER_MERGE` |
| Final merged `main` commit | `TO_BE_FILLED_AFTER_MERGE` |
| Source fingerprint / archive SHA-256 | `TO_BE_FILLED_AFTER_FREEZE` |
| Runtime lock | `scripts/runtime/codex-runtime.windows-x64.lock.json` |
| Runtime lock SHA-256 | `2e4aaeefbbb32e042f570f0ef09e72f432135a3f2f105c2f9a8aa168443dd9eb` |
| Runtime manifest SHA-256 | `baf81b4be8394c71829a9a6dc064a99c54721d42abf224887eb7cdf6a6ae41ad` |
| Node | `22.23.3` official `win-x64`, archive SHA-256 `2b0ff57b049cda1bbcea2240eec20467018713c1efe1f7360c2681859b90ed71` |
| ACP | `@agentclientprotocol/codex-acp@1.13.1` |
| Rust target | `x86_64-pc-windows-msvc` |
| Dev/Beta artifacts and SHA-256 | `TO_BE_FILLED_AFTER_FINAL_BUILD` |
| GitHub Release / tag | `TO_BE_FILLED_AFTER_RELEASE` |

The original `darwin/arm64` provenance is retained where it appears in older records and is never relabelled as Windows. The final record must include the exact source, runtime lock, installer and checksum files used for the published release.
