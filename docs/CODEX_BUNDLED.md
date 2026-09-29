# Codex bundled runtime (0.0.5 candidate 09)

This packaging iteration keeps the public version at `0.0.5-web/dev/beta` and
uses a new immutable candidate identity for the reduced Codex payload.

The macOS arm64 App contains exactly the runtime needed to start the ACP
adapter:

- Codex ACP `1.13.1`, represented by its bundled JavaScript adapter and
  third-party notices.
- Node.js `22.23.3` for running that adapter without a system Node/npm
  dependency.
- A file-hashed runtime manifest. The source-controlled
  `scripts/codex-runtime.lock.json` binds every payload hash and executable
  flag to the frozen source candidate.

The official Codex CLI is external. Native Host resolves the recipient's
installed `codex` command (or its explicit `CODEX_PATH`) and keeps that host
dependency separate from the bundled ACP adapter. The bundled wrapper launches
the pinned Node and adapter while preserving inherited `PATH` and
`CODEX_PATH`; it does not install packages, download files, or run login.

No login, API key, user configuration, personal memory, history, account file or
profile is bundled. Existing Codex authentication is read by the official CLI
on the recipient's computer. Atrio has no account fields or separate login
flow. If Codex is not already signed in, sign in with the official Codex setup
and retry. Live Codex inference remains a user-deferred acceptance step.

## Reproducible packaging

`node scripts/package-codex-runtime.mjs verify` checks the prepared payload
against the source lock. Candidate freeze and native builds bind that identity.
The large payload is excluded from `source.tar.gz` and delivered separately as
the deterministic runtime archive. Restore that archive to
`work/resources.noindex/agents/codex` before building the frozen source.

The packager preserves valid upstream signatures, signs unsigned embedded Mach-O
files before recording their hashes, and signs the complete App before making
the DMG. DMG capacity is calculated from the staging directory's apparent
bytes with explicit filesystem headroom, so image creation does not depend on
allocated blocks or hdiutil's automatic estimate. The package remains ad-hoc
signed and is not notarized.

Local acceptance checks the bundled ACP and Node versions and movable runtime
paths without calling Codex models. The external official Codex CLI and its
recipient-managed authentication are required for a real provider session.
