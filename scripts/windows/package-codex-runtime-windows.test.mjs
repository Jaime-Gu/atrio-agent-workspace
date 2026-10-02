import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { test } from "node:test";
import {
  chmodSync,
  mkdtempSync,
  mkdirSync,
  readFileSync,
  rmSync,
  symlinkSync,
  writeFileSync,
} from "node:fs";
import os from "node:os";
import path from "node:path";
import { freezeCandidate, verifyCandidate } from "../candidate-lib.mjs";
import {
  ADAPTER_VERSION,
  LOCK_PATH,
  NODE_VERSION,
  RESOURCE_ROOT,
  runtimeFiles,
  verifyLockedCodexRuntime,
} from "./package-codex-runtime-windows.mjs";

const sha = (bytes) => createHash("sha256").update(bytes).digest("hex");

function fixture(t) {
  const project = mkdtempSync(path.join(os.tmpdir(), "atrio-codex-win-runtime-"));
  const root = path.join(project, RESOURCE_ROOT);
  mkdirSync(path.join(root, "adapter"), { recursive: true });
  mkdirSync(path.join(root, "bin"), { recursive: true });
  writeFileSync(path.join(root, "adapter/index.js"), "console.log('fixture');\n");
  writeFileSync(path.join(root, "bin/node.exe"), "MZ fixture");
  chmodSync(path.join(root, "bin/node.exe"), 0o755);
  const versions = { adapter: ADAPTER_VERSION, node: NODE_VERSION };
  const manifest = {
    schemaVersion: 1,
    provider: "codex",
    platform: "windows",
    architecture: "x64",
    versions,
    entrypoints: { adapter: "adapter/index.js", node: "bin/node.exe" },
    launch: { program: "bin/node.exe", args: ["adapter/index.js"] },
    files: runtimeFiles(root, true),
  };
  writeFileSync(path.join(root, "manifest.json"), JSON.stringify(manifest));
  const lock = {
    schemaVersion: 1,
    provider: "codex",
    platform: "windows",
    architecture: "x64",
    resourceRoot: RESOURCE_ROOT,
    versions,
    manifestSha256: sha(readFileSync(path.join(root, "manifest.json"))),
    files: runtimeFiles(root),
  };
  mkdirSync(path.join(project, "scripts/runtime"), { recursive: true });
  writeFileSync(path.join(project, LOCK_PATH), JSON.stringify(lock));
  t.after(() => rmSync(project, { recursive: true, force: true }));
  return { project, root };
}

test("Windows runtime lock verifies the node.exe and adapter entrypoints", (t) => {
  const f = fixture(t);
  const verified = verifyLockedCodexRuntime(f.project);
  assert.equal(verified.platform, "windows");
  assert.equal(verified.architecture, "x64");
  assert.equal(verified.versions.adapter, "1.13.1");
  assert.ok(verified.totalBytes > 0);
  assert.equal(runtimeFiles(f.root).find((entry) => entry.path === "bin/node.exe").executable, true);
});

test("Windows runtime verification rejects changed bytes, extra files, and symlinks", (t) => {
  const f = fixture(t);
  writeFileSync(path.join(f.root, "adapter/index.js"), "tampered");
  assert.throws(() => verifyLockedCodexRuntime(f.project), /payload changed/);
  writeFileSync(path.join(f.root, "adapter/index.js"), "console.log('fixture');\n");
  writeFileSync(path.join(f.root, "extra.txt"), "unexpected");
  assert.throws(() => verifyLockedCodexRuntime(f.project), /payload changed/);
  rmSync(path.join(f.root, "extra.txt"));
  symlinkSync(
    process.platform === "win32" ? path.join(f.root, "adapter") : "adapter/index.js",
    path.join(f.root, "alias.js"),
    process.platform === "win32" ? "junction" : undefined,
  );
  assert.throws(() => verifyLockedCodexRuntime(f.project), /symlink is forbidden/);
});

test("Windows candidate binds verified runtime payload without archiving it", { skip: process.platform !== "win32" }, (t) => {
  const f = fixture(t);
  writeFileSync(path.join(f.project, "package.json"), '{"version":"0.0.6"}');
  writeFileSync(path.join(f.project, "package-lock.json"), '{"version":"0.0.6"}');
  const frozen = freezeCandidate(f.project, { candidateId: "windows-runtime-bound", tools: { fixture: true } });
  assert.equal(frozen.manifest.runtimeResources.platform, "windows");
  assert.equal(frozen.manifest.runtimeResources.versions.node, "22.23.3");
  assert.ok(!frozen.manifest.source.files.some(file => file.path.startsWith("work/")));
  verifyCandidate(f.project, frozen.manifestPath);
  writeFileSync(path.join(f.root, "adapter/index.js"), "changed");
  assert.throws(() => verifyCandidate(f.project, frozen.manifestPath), /payload changed/);
});

test("old source trees without a Windows runtime lock remain inspectable", (t) => {
  const f = fixture(t);
  rmSync(path.join(f.project, LOCK_PATH));
  assert.equal(verifyLockedCodexRuntime(f.project), null);
});
