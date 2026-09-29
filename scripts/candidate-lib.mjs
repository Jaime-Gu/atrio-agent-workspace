import { createHash, randomBytes } from "node:crypto";
import { execFileSync, spawnSync } from "node:child_process";
import {
  readFileSync,
  writeFileSync,
  readdirSync,
  lstatSync,
  readlinkSync,
  mkdirSync,
  existsSync,
  renameSync,
} from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { verifyLockedCodexRuntime } from "./package-codex-runtime.mjs";

const excludedRoots = new Set([
  "node_modules",
  "dist",
  "work",
  "outputs",
  ".git",
]);
const excludedPaths = new Set(["src-tauri/target", "src-tauri/gen"]);
const channels = new Set(["web", "dev", "beta"]);
export const sha256 = (bytes) =>
  createHash("sha256").update(bytes).digest("hex");
export const fileHash = (file) => sha256(readFileSync(file));
const json = (file) => JSON.parse(readFileSync(file, "utf8"));
export const writeJson = (file, value, exclusive = false) =>
  writeFileSync(file, JSON.stringify(value, null, 2) + "\n", {
    flag: exclusive ? "wx" : "w",
  });
export const idToken = (value) => {
  if (!/^[a-zA-Z0-9][a-zA-Z0-9._-]{0,127}$/.test(value))
    throw new Error("Invalid candidate/build ID");
  return value;
};
export const timestamp = () => new Date().toISOString().replace(/[-:.]/g, "");
export const uniqueId = (prefix) =>
  `${prefix}-${timestamp()}-${randomBytes(3).toString("hex")}`;

export function sourceFiles(root) {
  const files = [];
  function walk(relative = "") {
    for (const name of readdirSync(path.join(root, relative)).sort()) {
      const rel = relative ? `${relative}/${name}` : name;
      if (
        (!relative && excludedRoots.has(name)) ||
        excludedPaths.has(rel) ||
        name === ".DS_Store" ||
        name.endsWith(".tsbuildinfo")
      )
        continue;
      if (/[\n\r]/.test(rel))
        throw new Error(`Unsupported source filename: ${JSON.stringify(rel)}`);
      const stat = lstatSync(path.join(root, rel));
      if (stat.isSymbolicLink())
        throw new Error(
          `Source symlink must be resolved before freezing: ${rel}`,
        );
      if (stat.isDirectory()) walk(rel);
      else if (stat.isFile())
        files.push({
          path: rel,
          sha256: fileHash(path.join(root, rel)),
          executable: Boolean(stat.mode & 0o111),
        });
      else throw new Error(`Unsupported source entry: ${rel}`);
    }
  }
  walk();
  return files;
}
export const treeFingerprint = (files) => sha256(JSON.stringify(files));
function toolVersion(command, args) {
  const result = spawnSync(command, args, { encoding: "utf8" });
  return result.status === 0 ? result.stdout.trim() : null;
}
export function toolchains(root) {
  const rust = spawnSync(
    "bash",
    [
      "-c",
      'APP_PROJECT_ROOT="$1"; source "$1/scripts/rust-env.sh" && rustc --version && cargo --version',
      "candidate-toolchain",
      root,
    ],
    { encoding: "utf8" },
  );
  const versions = rust.status === 0 ? rust.stdout.trim().split("\n") : [];
  return {
    node: process.version,
    npm: toolVersion("npm", ["--version"]),
    rustc: versions[0] ?? null,
    cargo: versions[1] ?? null,
    xcode: toolVersion("xcodebuild", ["-version"]),
    developerDirectory: toolVersion("xcode-select", ["-p"]),
    clang: toolVersion("clang", ["--version"]),
    macosSdk: toolVersion("xcrun", ["--sdk", "macosx", "--show-sdk-version"]),
    hostPlatform: process.platform,
    hostArchitecture: process.arch,
    target: "aarch64-apple-darwin",
  };
}
export function installedDependencies(root) {
  const lockPath = path.join(root, "package-lock.json");
  const packages = existsSync(lockPath) ? (json(lockPath).packages ?? {}) : {};
  return Object.entries(packages)
    .filter(([name]) => name.startsWith("node_modules/"))
    .sort(([a], [b]) => a.localeCompare(b))
    .map(([name, locked]) => {
      const manifestPath = path.join(root, name, "package.json");
      if (!existsSync(manifestPath)) {
        if (locked.optional)
          return { path: name, installed: false, optional: true };
        throw new Error(
          `Locked dependency is missing: ${name}; run npm ci before freezing`,
        );
      }
      const installed = json(manifestPath);
      if (installed.version !== locked.version)
        throw new Error(
          `Installed dependency version drift: ${name}; run npm ci before freezing`,
        );
      return {
        path: name,
        version: installed.version,
        packageJsonSha256: fileHash(manifestPath),
      };
    });
}
function gitProvenance(root) {
  const commit = spawnSync("git", ["-C", root, "rev-parse", "HEAD"], {
    encoding: "utf8",
  });
  if (commit.status !== 0) return null;
  const status = spawnSync(
    "git",
    ["-C", root, "status", "--porcelain", "--", "."],
    { encoding: "utf8" },
  );
  return {
    commit: commit.stdout.trim(),
    dirty: Boolean(status.stdout.trim()),
    status: status.stdout.trim(),
  };
}
export function freezeCandidate(root, { candidateId, directory, tools } = {}) {
  const baseVersion = json(path.join(root, "package.json")).version;
  candidateId = idToken(candidateId ?? uniqueId(baseVersion));
  const parent = directory ?? path.join(root, "work/candidates.noindex");
  mkdirSync(parent, { recursive: true });
  const folder = path.join(parent, candidateId);
  mkdirSync(folder); // Never replace an existing candidate or its evidence.
  const runtimeResources = verifyLockedCodexRuntime(root);
  const files = sourceFiles(root);
  const sourceTreeFingerprint = treeFingerprint(files);
  const archive = path.join(folder, "source.tar.gz");
  const fileList = path.join(folder, "source-files.txt");
  writeFileSync(fileList, files.map((entry) => entry.path).join("\n") + "\n");
  execFileSync("tar", ["-czf", archive, "-C", root, "-T", fileList], {
    env: { ...process.env, COPYFILE_DISABLE: "1" },
    stdio: "pipe",
  });
  if (treeFingerprint(sourceFiles(root)) !== sourceTreeFingerprint)
    throw new Error(
      "Source changed while freezing; discard this incomplete candidate and freeze a new one.",
    );
  if (
    JSON.stringify(verifyLockedCodexRuntime(root)) !==
    JSON.stringify(runtimeResources)
  )
    throw new Error(
      "Runtime resources changed while freezing; freeze a new candidate.",
    );
  const sourceFingerprint = `sha256:${fileHash(archive)}`;
  const configPaths = files.filter(
    ({ path: name }) =>
      name === "package.json" ||
      name === "package-lock.json" ||
      name === "src-tauri/Cargo.lock" ||
      name === "src-tauri/Cargo.toml" ||
      name === "src-tauri/build.rs" ||
      name.startsWith("scripts/") ||
      name.startsWith("src-tauri/capabilities/") ||
      /(^|\/)(vite\.config\.ts|tsconfig.*\.json|tauri.*\.conf\.json)$/.test(
        name,
      ),
  );
  const manifest = {
    schemaVersion: 1,
    candidateId,
    baseVersion,
    createdAt: new Date().toISOString(),
    source: {
      originRoot: path.resolve(root),
      archive: "source.tar.gz",
      archiveSha256: fileHash(archive),
      sourceFingerprint,
      sourceTreeFingerprint,
      git: gitProvenance(root),
      files,
    },
    locksAndConfiguration: configPaths,
    toolchains: tools ?? toolchains(root),
    installedDependencies: installedDependencies(root),
    ...(runtimeResources ? { runtimeResources } : {}),
    expectedChannelDifferences: {
      web: {
        identifier: "browser origin",
        previewPort: 1422,
        runtime: "browser Mock",
        frontendMode: "web",
      },
      dev: {
        identifier: "dev.pixel.workspace.dev",
        runtime: "native isolated data",
        frontendMode: "native-dev",
      },
      beta: {
        identifier: "dev.pixel.workspace",
        runtime: "native daily data compatibility",
        frontendMode: "native-beta",
        signing: "ad-hoc / not notarized",
      },
    },
    evidence:
      "Per-build build.json and optional evidence files in builds/<buildId>; freezing does not imply acceptance.",
  };
  const manifestPath = path.join(folder, "candidate.json");
  writeJson(manifestPath, manifest, true);
  return { manifestPath, manifest };
}
export function readCandidate(manifestPath) {
  const manifest = json(manifestPath);
  if (manifest.schemaVersion !== 1 || !Array.isArray(manifest.source?.files))
    throw new Error("Unsupported candidate manifest");
  idToken(manifest.candidateId);
  if (
    manifest.source.archive !== "source.tar.gz" ||
    !/^[a-f0-9]{64}$/.test(manifest.source.archiveSha256)
  )
    throw new Error("Invalid source archive identity");
  const hash = fileHash(
    path.join(path.dirname(manifestPath), manifest.source.archive),
  );
  if (
    hash !== manifest.source.archiveSha256 ||
    manifest.source.sourceFingerprint !== `sha256:${hash}`
  )
    throw new Error("Frozen source archive hash mismatch");
  if (
    treeFingerprint(manifest.source.files) !==
    manifest.source.sourceTreeFingerprint
  )
    throw new Error("Source file manifest hash mismatch");
  return manifest;
}
export function verifyCandidate(root, manifestPath) {
  const manifest = readCandidate(manifestPath);
  const actual = sourceFiles(root);
  if (treeFingerprint(actual) !== manifest.source.sourceTreeFingerprint) {
    const before = new Map(
      manifest.source.files.map((item) => [
        item.path,
        `${item.sha256}:${item.executable}`,
      ]),
    );
    const after = new Map(
      actual.map((item) => [item.path, `${item.sha256}:${item.executable}`]),
    );
    const changed = [...new Set([...before.keys(), ...after.keys()])].filter(
      (name) => before.get(name) !== after.get(name),
    );
    throw new Error(
      `Candidate source changed (${changed.slice(0, 12).join(", ")}); freeze a new candidate before building.`,
    );
  }
  if (json(path.join(root, "package.json")).version !== manifest.baseVersion)
    throw new Error("Candidate base version mismatch");
  if (
    JSON.stringify(installedDependencies(root)) !==
    JSON.stringify(manifest.installedDependencies)
  )
    throw new Error(
      "Installed dependency metadata changed; freeze a new candidate before building.",
    );
  const runtimeResources = verifyLockedCodexRuntime(root);
  if (
    JSON.stringify(runtimeResources) !==
    JSON.stringify(manifest.runtimeResources ?? null)
  )
    throw new Error(
      "Candidate runtime resource identity changed; freeze a new candidate before building.",
    );
  return manifest;
}
export function buildIdentity(root, channel, env = process.env) {
  if (!channels.has(channel)) throw new Error("Invalid build channel");
  if (!env.PIXEL_CANDIDATE_MANIFEST) {
    if (env.PIXEL_BUILD_ID)
      throw new Error("A build ID requires a frozen candidate manifest");
    return {
      candidateId: null,
      buildId: `local-${channel}`,
      sourceFingerprint: null,
    };
  }
  const manifest = verifyCandidate(
    root,
    path.resolve(env.PIXEL_CANDIDATE_MANIFEST),
  );
  if (!env.PIXEL_BUILD_ID)
    throw new Error(
      "Frozen build requires PIXEL_BUILD_ID; use candidate:build",
    );
  idToken(env.PIXEL_BUILD_ID);
  if (env.PIXEL_BUILD_CHANNEL !== channel)
    throw new Error(
      `Expected candidate channel ${env.PIXEL_BUILD_CHANNEL}; received ${channel}`,
    );
  return {
    candidateId: manifest.candidateId,
    buildId: env.PIXEL_BUILD_ID,
    sourceFingerprint: manifest.source.sourceFingerprint,
  };
}
export function artifactIdentity(file) {
  const stat = lstatSync(file);
  if (stat.isFile())
    return {
      path: path.resolve(file),
      kind: "file",
      sha256: fileHash(file),
      bytes: stat.size,
    };
  if (!stat.isDirectory()) throw new Error(`Unsupported artifact: ${file}`);
  const entries = [];
  function walk(relative = "") {
    for (const name of readdirSync(path.join(file, relative)).sort()) {
      const rel = relative ? `${relative}/${name}` : name;
      const item = path.join(file, rel);
      const entry = lstatSync(item);
      if (entry.isSymbolicLink())
        entries.push({ path: rel, symlink: readlinkSync(item) });
      else if (entry.isDirectory()) walk(rel);
      else
        entries.push({
          path: rel,
          sha256: fileHash(item),
          executable: Boolean(entry.mode & 0o111),
        });
    }
  }
  walk();
  return {
    path: path.resolve(file),
    kind: "directory",
    sha256: sha256(JSON.stringify(entries)),
    hashMethod: "sha256 of sorted JSON file/symlink manifest",
    files: entries,
  };
}
export function recordBuild(root, channel, artifacts, env = process.env) {
  const identity = buildIdentity(root, channel, env);
  if (!identity.candidateId) return null;
  const manifestPath = path.resolve(env.PIXEL_CANDIDATE_MANIFEST);
  const buildDir = path.join(
    path.dirname(manifestPath),
    "builds",
    identity.buildId,
  );
  mkdirSync(buildDir, { recursive: true });
  const record = {
    schemaVersion: 1,
    ...identity,
    baseVersion: json(path.join(root, "package.json")).version,
    channel,
    builtAt: new Date().toISOString(),
    toolchains: toolchains(root),
    candidateManifest: manifestPath,
    candidateManifestSha256: fileHash(manifestPath),
    artifacts: artifacts.map(artifactIdentity),
    acceptance: "NOT_TESTED",
  };
  writeJson(path.join(buildDir, "build.json"), record, true);
  return record;
}
export function selectWebBuild(root, buildRecord) {
  if (buildRecord.channel !== "web")
    throw new Error("Preview must select a web build");
  const pointer = path.join(root, "work/candidates.noindex/preview.json");
  mkdirSync(path.dirname(pointer), { recursive: true });
  writeJson(`${pointer}.tmp`, {
    buildJson: path.join(
      path.dirname(buildRecord.candidateManifest),
      "builds",
      buildRecord.buildId,
      "build.json",
    ),
  });
  renameSync(`${pointer}.tmp`, pointer);
}
export function frozenWebDirectory(root, buildJson) {
  if (
    !buildJson &&
    !existsSync(path.join(root, "work/candidates.noindex/preview.json"))
  )
    throw new Error(
      "No frozen Web candidate; run candidate:freeze and candidate:build -- web first.",
    );
  buildJson ??= json(
    path.join(root, "work/candidates.noindex/preview.json"),
  ).buildJson;
  const build = json(buildJson);
  if (build.channel !== "web")
    throw new Error("Only a frozen Web build can use port 1422");
  const candidate = readCandidate(build.candidateManifest);
  if (
    fileHash(build.candidateManifest) !== build.candidateManifestSha256 ||
    candidate.candidateId !== build.candidateId ||
    candidate.source.sourceFingerprint !== build.sourceFingerprint
  )
    throw new Error("Web candidate identity mismatch");
  const artifact = build.artifacts.find((entry) => entry.kind === "directory");
  if (!artifact || artifactIdentity(artifact.path).sha256 !== artifact.sha256)
    throw new Error("Frozen Web artifact hash mismatch");
  return { directory: artifact.path, build };
}

export function viteBuildContext(channel) {
  const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
  const identity = buildIdentity(root, channel);
  const webOutDir = process.env.PIXEL_WEB_OUT_DIR;
  if (webOutDir && (channel !== "web" || !identity.candidateId))
    throw new Error(
      "A frozen Web output directory requires a Web candidate build",
    );
  return { identity, webOutDir };
}
