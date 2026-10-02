import { test } from "node:test";
import assert from "node:assert/strict";
import {
  mkdtempSync,
  mkdirSync,
  writeFileSync,
  rmSync,
  chmodSync,
  symlinkSync,
} from "node:fs";
import os from "node:os";
import path from "node:path";
import {
  artifactIdentity,
  buildIdentity,
  fileHash,
  freezeCandidate,
  frozenWebDirectory,
  selectWebBuild,
  sourceFiles,
  verifyCandidate,
  writeJson,
} from "./candidate-lib.mjs";

function fixture(t) {
  const root = mkdtempSync(path.join(os.tmpdir(), "pixel-candidate-test-"));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  const put = (name, value) => {
    const file = path.join(root, name);
    mkdirSync(path.dirname(file), { recursive: true });
    writeFileSync(file, value);
    return file;
  };
  put("package.json", '{"version":"1.2.3"}');
  put("package-lock.json", '{"version":"1.2.3"}');
  put("src-tauri/Cargo.lock", 'name = "pixel-workspace"\nversion = "1.2.3"\n');
  put("src-tauri/tauri.conf.json", '{"version":"1.2.3"}');
  put("src/main.ts", "export const value = 1;\n");
  const freeze = () =>
    freezeCandidate(root, {
      candidateId: "1.2.3-p0-test",
      tools: { fixture: true },
    });
  return { root, put, freeze };
}

test("freezes source, locks and config while excluding generated/runtime files", (t) => {
  const f = fixture(t);
  for (const name of [
    "work/log.txt",
    "outputs/old.dmg",
    "dist/web/app.js",
    "node_modules/lib/x.js",
    "src-tauri/target/bin",
    "src-tauri/gen/schema.json",
    "tsconfig.tsbuildinfo",
  ])
    f.put(name, "generated");
  const { manifest, manifestPath } = f.freeze();
  assert.equal(manifest.baseVersion, "1.2.3");
  assert.match(manifest.source.sourceFingerprint, /^sha256:[a-f0-9]{64}$/);
  assert.equal(
    manifest.source.archiveSha256,
    fileHash(path.join(path.dirname(manifestPath), "source.tar.gz")),
  );
  assert.deepEqual(
    sourceFiles(f.root)
      .map((x) => x.path)
      .sort(),
    [
      "package-lock.json",
      "package.json",
      "src-tauri/Cargo.lock",
      "src-tauri/tauri.conf.json",
      "src/main.ts",
    ].sort(),
  );
  assert.equal(manifest.locksAndConfiguration.length, 4);
  f.put("dist/dev/app.js", "later native build");
  assert.equal(
    verifyCandidate(f.root, manifestPath).candidateId,
    manifest.candidateId,
  );
  assert.throws(() => f.freeze(), /EEXIST/);
});

test("source changes and executable-mode changes cannot reuse a candidate", (t) => {
  const f = fixture(t);
  const { manifestPath } = f.freeze();
  f.put("src/main.ts", "export const value = 2;\n");
  assert.throws(
    () => verifyCandidate(f.root, manifestPath),
    /src\/main.ts.*freeze a new candidate/,
  );
  f.put("src/main.ts", "export const value = 1;\n");
  chmodSync(path.join(f.root, "src/main.ts"), 0o755);
  if (process.platform === "win32") {
    assert.equal(verifyCandidate(f.root, manifestPath).platform, "win32");
    assert.ok(sourceFiles(f.root).every(file => file.executable === false));
  } else {
    assert.throws(() => verifyCandidate(f.root, manifestPath), /source changed/);
  }
});

test("new files, source symlinks and archive corruption are rejected", (t) => {
  const f = fixture(t);
  const { manifestPath } = f.freeze();
  f.put("src/new.ts", "new dependency");
  assert.throws(() => verifyCandidate(f.root, manifestPath), /src\/new.ts/);
  rmSync(path.join(f.root, "src/new.ts"));
  symlinkSync(process.platform === "win32" ? path.join(f.root, "src") : "main.ts", path.join(f.root, "src/alias.ts"), process.platform === "win32" ? "junction" : undefined);
  assert.throws(() => verifyCandidate(f.root, manifestPath), /Source symlink/);
  rmSync(path.join(f.root, "src/alias.ts"));
  writeFileSync(
    path.join(path.dirname(manifestPath), "source.tar.gz"),
    "damaged",
  );
  assert.throws(
    () => verifyCandidate(f.root, manifestPath),
    /archive hash mismatch/,
  );
});

test("channel identity is bound to the same source and invalid environment fails closed", (t) => {
  const f = fixture(t);
  const { manifestPath, manifest } = f.freeze();
  assert.deepEqual(buildIdentity(f.root, "web", {}), {
    candidateId: null,
    buildId: "local-web",
    sourceFingerprint: null,
  });
  const env = {
    PIXEL_CANDIDATE_MANIFEST: manifestPath,
    PIXEL_BUILD_ID: "p0-web-1",
    PIXEL_BUILD_CHANNEL: "web",
  };
  assert.deepEqual(buildIdentity(f.root, "web", env), {
    candidateId: manifest.candidateId,
    buildId: "p0-web-1",
    sourceFingerprint: manifest.source.sourceFingerprint,
  });
  assert.throws(
    () => buildIdentity(f.root, "dev", env),
    /Expected candidate channel/,
  );
  assert.throws(
    () => buildIdentity(f.root, "web", { ...env, PIXEL_BUILD_ID: undefined }),
    /requires PIXEL_BUILD_ID/,
  );
  assert.throws(
    () => buildIdentity(f.root, "web", { PIXEL_BUILD_ID: "invented" }),
    /requires a frozen candidate/,
  );
});

test("frozen preview survives later source/native builds but rejects modified Web artifacts", (t) => {
  const f = fixture(t);
  const { manifestPath, manifest } = f.freeze();
  const buildId = "frozen-web";
  const buildDir = path.join(path.dirname(manifestPath), "builds", buildId);
  mkdirSync(path.join(buildDir, "dist"), { recursive: true });
  writeFileSync(path.join(buildDir, "dist/index.html"), "frozen page");
  const build = {
    candidateId: manifest.candidateId,
    sourceFingerprint: manifest.source.sourceFingerprint,
    candidateManifest: manifestPath,
    candidateManifestSha256: fileHash(manifestPath),
    channel: "web",
    buildId,
    artifacts: [artifactIdentity(path.join(buildDir, "dist"))],
  };
  writeJson(path.join(buildDir, "build.json"), build);
  selectWebBuild(f.root, build);
  f.put("src/main.ts", "next iteration");
  f.put("dist/beta/index.html", "beta build");
  assert.equal(
    frozenWebDirectory(f.root).directory,
    path.join(buildDir, "dist"),
  );
  writeFileSync(path.join(buildDir, "dist/index.html"), "accidental overwrite");
  assert.throws(() => frozenWebDirectory(f.root), /Web artifact hash mismatch/);
});

test("changed installed dependency metadata requires a new candidate", (t) => {
  const f = fixture(t);
  f.put(
    "package-lock.json",
    JSON.stringify({
      version: "1.2.3",
      packages: { "node_modules/example": { version: "1.0.0" } },
    }),
  );
  assert.throws(() => f.freeze(), /Locked dependency is missing/);
  // An incomplete freeze is retained for diagnosis; use a different candidate ID.
  f.put(
    "node_modules/example/package.json",
    '{"name":"example","version":"1.0.0"}',
  );
  const { manifestPath } = freezeCandidate(f.root, {
    candidateId: "deps-test-2",
    tools: { fixture: true },
  });
  f.put(
    "node_modules/example/package.json",
    '{"name":"example","version":"1.0.0","changed":true}',
  );
  assert.throws(
    () => verifyCandidate(f.root, manifestPath),
    /dependency metadata changed/,
  );
  f.put(
    "node_modules/example/package.json",
    '{"name":"example","version":"2.0.0"}',
  );
  assert.throws(
    () => verifyCandidate(f.root, manifestPath),
    /dependency version drift/,
  );
});
