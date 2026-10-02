import { test } from "node:test";
import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import {
  mkdtempSync,
  mkdirSync,
  readFileSync,
  writeFileSync,
  chmodSync,
  rmSync,
  symlinkSync,
  utimesSync,
} from "node:fs";
import os from "node:os";
import path from "node:path";
import {
  RESOURCE_ROOT,
  LOCK_PATH,
  runtimeFiles,
  verifyLockedCodexRuntime,
  archiveRuntime,
  restoreRuntime,
  wrapper,
  apparentBytes,
  dmgSizeMiB,
} from "./package-codex-runtime.mjs";
import { freezeCandidate, verifyCandidate } from "./candidate-lib.mjs";
const sha = (data) => createHash("sha256").update(data).digest("hex");
function fixture(t) {
  const project = mkdtempSync(path.join(os.tmpdir(), "atrio-runtime-test-"));
  t.after(() => rmSync(project, { recursive: true, force: true }));
  const root = path.join(project, RESOURCE_ROOT);
  mkdirSync(path.join(root, "bin"), { recursive: true });
  mkdirSync(path.join(project, "scripts"), { recursive: true });
  for (const [p, v] of [
    ["package.json", '{"version":"0.0.5"}'],
    ["package-lock.json", '{"version":"0.0.5"}'],
  ])
    writeFileSync(path.join(project, p), v);
  writeFileSync(path.join(root, "bin/codex-acp"), wrapper);
  chmodSync(path.join(root, "bin/codex-acp"), 0o755);
  const versions = { adapter: "1.13.1", node: "22.23.3" };
  const manifest = {
    versions,
    entrypoints: { adapter: "bin/codex-acp" },
    files: runtimeFiles(root, true),
  };
  writeFileSync(path.join(root, "manifest.json"), JSON.stringify(manifest));
  const lock = {
    schemaVersion: 1,
    provider: "codex",
    platform: "darwin",
    architecture: "arm64",
    resourceRoot: RESOURCE_ROOT,
    versions,
    manifestSha256: sha(readFileSync(path.join(root, "manifest.json"))),
    files: runtimeFiles(root),
  };
  writeFileSync(path.join(project, LOCK_PATH), JSON.stringify(lock));
  for (const f of runtimeFiles(root)) utimesSync(path.join(root, f.path), 0, 0);
  return { project, root };
}
test("candidate binds external runtime bytes and executable bits without archiving payload", { skip: process.platform !== "darwin" }, (t) => {
  const f = fixture(t),
    frozen = freezeCandidate(f.project, {
      candidateId: "payload-test",
      tools: { fixture: true },
    });
  assert.equal(frozen.manifest.runtimeResources.versions.adapter, "1.13.1");
  assert.ok(
    !frozen.manifest.source.files.some((x) => x.path.startsWith("work/")),
  );
  assert.ok(frozen.manifest.source.files.some((x) => x.path === LOCK_PATH));
  verifyCandidate(f.project, frozen.manifestPath);
  writeFileSync(path.join(f.root, "bin/codex-acp"), "tampered");
  assert.throws(
    () => verifyCandidate(f.project, frozen.manifestPath),
    /runtime payload changed/,
  );
});
test("runtime verifies exact manifest and rejects unlisted files, symlinks and changed modes", { skip: process.platform !== "darwin" }, (t) => {
  const f = fixture(t);
  verifyLockedCodexRuntime(f.project);
  writeFileSync(path.join(f.root, "extra-auth.json"), "synthetic-only");
  assert.throws(
    () => verifyLockedCodexRuntime(f.project),
    /runtime payload changed/,
  );
  rmSync(path.join(f.root, "extra-auth.json"));
  symlinkSync("bin/codex-acp", path.join(f.root, "alias"));
  assert.throws(
    () => verifyLockedCodexRuntime(f.project),
    /symlink is forbidden/,
  );
  rmSync(path.join(f.root, "alias"));
  chmodSync(path.join(f.root, "bin/codex-acp"), 0o644);
  assert.throws(
    () => verifyLockedCodexRuntime(f.project),
    /runtime payload changed/,
  );
});
test("old candidates without runtime resources remain valid", (t) => {
  const f = fixture(t);
  rmSync(path.join(f.project, LOCK_PATH));
  assert.equal(verifyLockedCodexRuntime(f.project), null);
  const { manifest, manifestPath } = freezeCandidate(f.project, {
    candidateId: "old",
    tools: { fixture: true },
  });
  assert.equal(manifest.runtimeResources, undefined);
  verifyCandidate(f.project, manifestPath);
});
test(
  "runtime archive is byte reproducible and cannot overwrite an earlier archive",
  { skip: process.platform !== "darwin" },
  async (t) => {
    const f = fixture(t),
      a = path.join(f.project, "work/a.tar.gz"),
      b = path.join(f.project, "work/b.tar.gz");
    const first = archiveRuntime(f.project, a);
    await new Promise((resolve) => setTimeout(resolve, 1100));
    const second = archiveRuntime(f.project, b);
    assert.equal(
      readFileSync(a).readUInt32LE(4),
      0,
      "gzip timestamp must be zero",
    );
    assert.equal(
      readFileSync(b).readUInt32LE(4),
      0,
      "gzip timestamp must be zero",
    );
    assert.equal(first.archiveSha256, second.archiveSha256);
    assert.throws(() => archiveRuntime(f.project, a), /already exists/);
    rmSync(f.root, { recursive: true });
    assert.throws(
      () => restoreRuntime(f.project, a, "0".repeat(64)),
      /checksum mismatch/,
    );
    restoreRuntime(f.project, a, first.archiveSha256);
    verifyLockedCodexRuntime(f.project);
    assert.throws(
      () => restoreRuntime(f.project, a, first.archiveSha256),
      /destination already exists/,
    );
  },
);
test("ACP wrapper uses sibling runtime and preserves existing account discovery", () => {
  assert.doesNotMatch(wrapper, /CODEX_PATH=/);
  assert.match(wrapper, /exec "\$ATRIO_CODEX_BIN\/node"/);
  assert.match(wrapper, /\$\{PATH:-\/usr\/bin:\/bin:\/usr\/sbin:\/sbin\}/);
  assert.doesNotMatch(
    wrapper,
    /npx|npm|HOME|\/Users\/|command -v|\blogin\b|--profile/,
  );
});

test("DMG sizing uses staging apparent bytes plus explicit headroom", (t) => {
  const f = fixture(t);
  const apparent = apparentBytes(path.join(f.project, RESOURCE_ROOT));
  const contentBytes = runtimeFiles(path.join(f.project, RESOURCE_ROOT)).reduce(
    (sum, entry) => sum + entry.bytes,
    0,
  );
  assert.ok(apparent >= contentBytes);
  assert.equal(dmgSizeMiB(0), 64);
  assert.equal(dmgSizeMiB(100 * 1024 * 1024), 184);
  assert.ok(
    dmgSizeMiB(apparent) * 1024 * 1024 > apparent,
    "explicit image capacity must exceed apparent staging bytes",
  );
  assert.throws(() => dmgSizeMiB(-1), /non-negative number/);
});
