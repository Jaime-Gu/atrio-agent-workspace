/**
 * Assemble and verify the Windows Codex ACP runtime.
 *
 * The runtime is deliberately staged below work/ so a source candidate never
 * accidentally archives an 80 MB Node executable.  Tauri copies this tree to
 * resources/agents/codex in the installed bundle (see tauri.windows.conf.json).
 * The payload contains the ACP JavaScript adapter and a pinned Node runtime;
 * the user's official Codex CLI and authentication remain external.
 */
import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { execFileSync, spawnSync } from "node:child_process";
import {
  copyFileSync,
  existsSync,
  lstatSync,
  mkdirSync,
  readdirSync,
  readFileSync,
  renameSync,
  rmSync,
  statSync,
  utimesSync,
  writeFileSync,
} from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

export const RESOURCE_ROOT = "work/resources.noindex/agents/codex";
export const LOCK_PATH = "scripts/runtime/codex-runtime.windows-x64.lock.json";
export const ADAPTER_VERSION = "1.13.1";
export const NODE_VERSION = "22.23.3";
export const NODE_ARCHIVE_SHA256 =
  "2b0ff57b049cda1bbcea2240eec20467018713c1efe1f7360c2681859b90ed71";
export const NODE_ARCHIVE_NAME = `node-v${NODE_VERSION}-win-x64.zip`;
export const NODE_ARCHIVE_URL = `https://nodejs.org/dist/v${NODE_VERSION}/${NODE_ARCHIVE_NAME}`;
export const NODE_CHECKSUMS_URL = `https://nodejs.org/dist/v${NODE_VERSION}/SHASUMS256.txt`;

const hash = (bytes) => createHash("sha256").update(bytes).digest("hex");
const json = (file) => JSON.parse(readFileSync(file, "utf8"));
const putJson = (file, value) =>
  writeFileSync(file, JSON.stringify(value, null, 2) + "\n");

function cleanRelative(value) {
  assert.equal(typeof value, "string");
  assert.ok(
    value &&
      !path.isAbsolute(value) &&
      !/[\\\r\n]/.test(value) &&
      value.split("/").every((part) => part && part !== "." && part !== ".."),
    `Invalid runtime path: ${value}`,
  );
  return value;
}

function executablePath(relative) {
  return /(?:\.exe|\.cmd|\.bat)$/i.test(relative);
}

/** Return the exact payload file list, including hashes and byte counts. */
export function runtimeFiles(root, excludeManifest = false) {
  const files = [];
  function walk(relative = "") {
    for (const name of readdirSync(path.join(root, relative)).sort()) {
      const rel = relative ? `${relative}/${name}` : name;
      cleanRelative(rel);
      if (excludeManifest && rel === "manifest.json") continue;
      const target = path.join(root, rel);
      const info = lstatSync(target);
      assert.ok(!info.isSymbolicLink(), `Runtime symlink is forbidden: ${rel}`);
      if (info.isDirectory()) walk(rel);
      else {
        assert.ok(info.isFile(), `Unsupported runtime entry: ${rel}`);
        files.push({
          path: rel,
          sha256: hash(readFileSync(target)),
          bytes: info.size,
          executable: executablePath(rel),
        });
      }
    }
  }
  walk();
  return files;
}

function verifyManifest(root, lock, files) {
  assert.equal(lock.schemaVersion, 1, "Unsupported Codex runtime lock schema");
  assert.equal(lock.provider, "codex");
  assert.equal(lock.platform, "windows");
  assert.equal(lock.architecture, "x64");
  assert.deepEqual(files, lock.files, "Codex runtime payload changed");
  const manifestPath = path.join(root, "manifest.json");
  assert.equal(
    hash(readFileSync(manifestPath)),
    lock.manifestSha256,
    "Codex runtime manifest changed",
  );
  const manifest = json(manifestPath);
  assert.equal(manifest.schemaVersion, 1);
  assert.equal(manifest.provider, "codex");
  assert.equal(manifest.platform, "windows");
  assert.equal(manifest.architecture, "x64");
  assert.deepEqual(manifest.versions, lock.versions);
  assert.deepEqual(
    manifest.files,
    runtimeFiles(root, true),
    "Codex runtime manifest file hashes differ",
  );
  assert.deepEqual(manifest.entrypoints, {
    adapter: "adapter/index.js",
    node: "bin/node.exe",
  });
  assert.deepEqual(manifest.launch, {
    program: "bin/node.exe",
    args: ["adapter/index.js"],
  });
  for (const entry of Object.values(manifest.entrypoints)) {
    cleanRelative(entry);
    const file = files.find((candidate) => candidate.path === entry);
    assert.ok(file, `Missing runtime entrypoint: ${entry}`);
    if (entry.endsWith(".exe")) assert.equal(file.executable, true);
  }
  return {
    provider: "codex",
    resourceRoot: lock.resourceRoot,
    platform: lock.platform,
    architecture: lock.architecture,
    manifestSha256: lock.manifestSha256,
    treeSha256: hash(JSON.stringify(files)),
    versions: lock.versions,
    fileCount: files.length,
    totalBytes: files.reduce((sum, file) => sum + file.bytes, 0),
  };
}

export function verifyRuntimeDirectory(root, lock) {
  assert.ok(existsSync(root), `Codex runtime directory missing: ${root}`);
  return verifyManifest(root, lock, runtimeFiles(root));
}

export function verifyLockedCodexRuntime(projectRoot) {
  const lockPath = path.join(projectRoot, LOCK_PATH);
  if (!existsSync(lockPath)) return null;
  const lock = json(lockPath);
  assert.equal(lock.resourceRoot, RESOURCE_ROOT, "Unexpected Codex resource root");
  return {
    ...verifyRuntimeDirectory(path.join(projectRoot, RESOURCE_ROOT), lock),
    lockPath: LOCK_PATH,
    lockSha256: hash(readFileSync(lockPath)),
  };
}

function fileSource(file) {
  const info = statSync(file);
  return { fileName: path.basename(file), sha256: hash(readFileSync(file)), bytes: info.size };
}

function copyRegularFile(source, destination) {
  const info = lstatSync(source);
  assert.ok(info.isFile() && !info.isSymbolicLink(), `Expected regular file: ${source}`);
  mkdirSync(path.dirname(destination), { recursive: true });
  copyFileSync(source, destination);
}

function nodeVersion(node) {
  const result = spawnSync(node, ["--version"], {
    encoding: "utf8",
    timeout: 15000,
    windowsHide: true,
  });
  assert.equal(result.status, 0, `Unable to execute ${node}`);
  return result.stdout.trim().replace(/^v/, "");
}

/** Ensure the pinned adapter manifest has an installed, lockfile-resolved tree. */
function ensureAdapterDependencies(adapterRoot) {
  const packageJsonPath = path.join(adapterRoot, "package.json");
  const lockPath = path.join(adapterRoot, "package-lock.json");
  assert.ok(existsSync(packageJsonPath), `Adapter manifest missing: ${packageJsonPath}`);
  assert.ok(existsSync(lockPath), `Adapter package-lock missing: ${lockPath}`);
  const manifest = json(packageJsonPath);
  assert.equal(
    manifest.dependencies?.["@agentclientprotocol/codex-acp"],
    ADAPTER_VERSION,
    `Adapter manifest must pin @agentclientprotocol/codex-acp ${ADAPTER_VERSION}`,
  );
  const installed = path.join(adapterRoot, "node_modules/@agentclientprotocol/codex-acp/dist/index.js");
  if (existsSync(installed)) return;
  const npmCli = process.env.npm_execpath ?? path.join(path.dirname(process.execPath), "node_modules/npm/bin/npm-cli.js");
  assert.ok(existsSync(npmCli), "npm-cli.js must be installed beside the active Node runtime");
  execFileSync(process.execPath, [npmCli, "ci", "--ignore-scripts", "--no-audit", "--no-fund"], {
    cwd: adapterRoot,
    stdio: "inherit",
    windowsHide: true,
  });
  assert.ok(existsSync(installed), "npm ci did not install the pinned ACP adapter");
}

function verifyWindowsX64Executable(file) {
  const bytes = readFileSync(file);
  assert.equal(bytes.toString("ascii", 0, 2), "MZ", "Node runtime is not a Windows PE executable");
  const peOffset = bytes.readUInt32LE(0x3c);
  assert.equal(bytes.toString("ascii", peOffset, peOffset + 4), "PE\0\0", "Node runtime PE header is invalid");
  assert.equal(bytes.readUInt16LE(peOffset + 4), 0x8664, "Node runtime is not the x64 Windows build");
}

async function download(url) {
  const response = await fetch(url);
  assert.ok(response.ok, `Download failed (${response.status}): ${url}`);
  return Buffer.from(await response.arrayBuffer());
}

/** Download and verify the official Node zip, then return its extracted root. */
export async function downloadNodeRuntime(cacheRoot = path.join(os.tmpdir(), "atrio-node-runtime")) {
  mkdirSync(cacheRoot, { recursive: true });
  const archive = path.join(cacheRoot, NODE_ARCHIVE_NAME);
  if (!existsSync(archive)) {
    const checksums = (await download(NODE_CHECKSUMS_URL)).toString("utf8");
    const line = checksums
      .split(/\r?\n/)
      .find((entry) => entry.trim().endsWith(`  ${NODE_ARCHIVE_NAME}`));
    assert.ok(line, `Node checksum entry missing for ${NODE_ARCHIVE_NAME}`);
    assert.equal(
      line.trim().split(/\s+/)[0],
      NODE_ARCHIVE_SHA256,
      "Pinned Node SHA-256 differs from the official SHASUMS256.txt",
    );
    writeFileSync(archive, await download(NODE_ARCHIVE_URL));
  }
  assert.equal(hash(readFileSync(archive)), NODE_ARCHIVE_SHA256, "Node archive SHA-256 mismatch");
  const extracted = path.join(cacheRoot, `node-v${NODE_VERSION}-win-x64`);
  const node = path.join(extracted, "node.exe");
  if (!existsSync(node)) {
    mkdirSync(cacheRoot, { recursive: true });
    // Windows 10+ ships tar.exe.  It handles the zip without requiring a
    // global 7-Zip install and keeps extraction inside the chosen cache root.
    execFileSync("tar.exe", ["-xf", archive, "-C", cacheRoot], { windowsHide: true });
  }
  assert.equal(nodeVersion(node), NODE_VERSION, "Downloaded Node version drifted");
  return extracted;
}

function packageNamesFromBundle(source) {
  const names = new Set(["@agentclientprotocol/codex-acp"]);
  // esbuild annotates each bundled module with its original node_modules path.
  // Keep only package roots; nested paths are represented by their root package.
  const pattern = /^\/\/ node_modules\/((?:@[^/]+\/)?[^/]+)(?:\/|$)/gm;
  for (const match of source.matchAll(pattern)) names.add(match[1]);
  return [...names].sort();
}

function packageLockEntry(lock, packageName) {
  const direct = lock.packages?.[`node_modules/${packageName}`];
  if (direct) return direct;
  const suffix = `/node_modules/${packageName}`;
  const key = Object.keys(lock.packages ?? {}).find((candidate) => candidate.endsWith(suffix));
  assert.ok(key, `Missing exact npm lock entry for ${packageName}`);
  return lock.packages[key];
}

function licenseFiles(packageDir) {
  return readdirSync(packageDir)
    .filter((name) => /^(license|copying|notice|thirdpartynotices)/i.test(name))
    .filter((name) => lstatSync(path.join(packageDir, name)).isFile());
}

function safeLicenseName(packageName, fileName) {
  return `${packageName.replaceAll("/", "__")}-${fileName}`.replace(/[^A-Za-z0-9@._-]/g, "_");
}

export async function prepareRuntime(projectRoot, options = {}) {
  assert.equal(process.platform, "win32", "Prepare the Windows runtime on Windows");
  assert.equal(process.arch, "x64", "Prepare the x64 runtime on Windows x64");
  const destination = path.join(projectRoot, RESOURCE_ROOT);
  if (existsSync(destination)) return verifyLockedCodexRuntime(projectRoot);
  const adapterRoot = options.adapterRoot
    ? path.resolve(options.adapterRoot)
    : path.join(projectRoot, "work/dependencies/windows-codex-acp");
  if (!options.adapterRoot) {
    mkdirSync(adapterRoot, { recursive: true });
    for (const name of ["package.json", "package-lock.json"])
      copyFileSync(path.join(projectRoot, "scripts/windows/adapter", name), path.join(adapterRoot, name));
  }
  ensureAdapterDependencies(adapterRoot);
  const adapterPackage = path.join(adapterRoot, "node_modules/@agentclientprotocol/codex-acp");
  const adapterPackageJson = json(path.join(adapterPackage, "package.json"));
  assert.equal(adapterPackageJson.version, ADAPTER_VERSION, "Codex ACP adapter version drifted");
  const adapter = path.join(adapterPackage, "dist/index.js");
  assert.ok(existsSync(adapter), `ACP adapter entry missing: ${adapter}`);

  const nodeRoot = options.nodeRoot
    ? path.resolve(options.nodeRoot)
    : await downloadNodeRuntime(options.nodeCacheRoot);
  const node = path.join(nodeRoot, "node.exe");
  verifyWindowsX64Executable(node);
  assert.equal(nodeVersion(node), NODE_VERSION, "Use the pinned Node 22.23.3 runtime");

  const staging = `${destination}.preparing-${process.pid}`;
  mkdirSync(staging, { recursive: true });
  try {
    const sources = {
      adapterPackage: fileSource(path.join(adapterPackage, "package.json")),
      adapterLock: fileSource(path.join(adapterRoot, "package-lock.json")),
      node: fileSource(node),
      nodeArchive: {
        fileName: NODE_ARCHIVE_NAME,
        sha256: NODE_ARCHIVE_SHA256,
        url: NODE_ARCHIVE_URL,
      },
    };
    copyRegularFile(node, path.join(staging, "bin/node.exe"));
    copyRegularFile(adapter, path.join(staging, "adapter/index.js"));
    putJson(path.join(staging, "adapter/package.json"), {
      name: "@agentclientprotocol/codex-acp",
      version: ADAPTER_VERSION,
      type: "module",
      main: "index.js",
    });
    mkdirSync(path.join(staging, "LICENSES"), { recursive: true });
    copyRegularFile(path.join(nodeRoot, "LICENSE"), path.join(staging, "LICENSES/node-LICENSE"));
    const adapterLicense = path.join(adapterPackage, "LICENSE");
    assert.ok(existsSync(adapterLicense), "ACP adapter license is missing");
    copyRegularFile(adapterLicense, path.join(staging, "LICENSES/codex-acp-LICENSE"));

    const bundled = packageNamesFromBundle(readFileSync(adapter, "utf8"));
    const npmLock = json(path.join(adapterRoot, "package-lock.json"));
    assert.equal(npmLock.lockfileVersion >= 1, true, "Adapter npm lockfile is required");
    const notices = [];
    for (const packageName of bundled) {
      const packageDir = path.join(adapterRoot, "node_modules", packageName);
      const packageJson = json(path.join(packageDir, "package.json"));
      const lockEntry = packageLockEntry(npmLock, packageName);
      assert.equal(
        packageJson.version,
        lockEntry.version,
        `Installed adapter dependency drifted: ${packageName}; run npm ci`,
      );
      const files = licenseFiles(packageDir);
      if (files.length) {
        for (const file of files)
          copyRegularFile(path.join(packageDir, file), path.join(staging, "LICENSES", safeLicenseName(packageName, file)));
      } else {
        // Preserve a machine-readable notice when npm declares a license in
        // metadata but ships no standalone file.
        writeFileSync(
          path.join(staging, "LICENSES", safeLicenseName(packageName, "LICENSE.json")),
          JSON.stringify({ package: packageName, license: packageJson.license ?? null }, null, 2) + "\n",
        );
      }
      notices.push({ package: packageName, version: lockEntry.version, license: packageJson.license ?? null });
    }
    putJson(path.join(staging, "LICENSES/third-party.json"), notices);
    putJson(
      path.join(staging, "LICENSES/npm-sources.json"),
      bundled.map((packageName) => {
        const info = packageLockEntry(npmLock, packageName);
        return { packagePath: packageName, version: info.version, resolved: info.resolved, integrity: info.integrity };
      }),
    );
    putJson(path.join(staging, "LICENSES/node-source.json"), {
      version: NODE_VERSION,
      url: NODE_ARCHIVE_URL,
      archiveSha256: NODE_ARCHIVE_SHA256,
      checksumSource: `https://nodejs.org/dist/v${NODE_VERSION}/SHASUMS256.txt`,
      architecture: "x64",
      platform: "win32",
    });
    mkdirSync(path.join(staging, "codex-resources"), { recursive: true });
    writeFileSync(
      path.join(staging, "codex-resources/README.txt"),
      "Atrio bundles the Codex ACP JavaScript adapter and Node runtime. The official Codex CLI is resolved externally by Host; no CLI, credentials, or personal profile is included.\r\n",
    );

    const versions = { adapter: ADAPTER_VERSION, node: NODE_VERSION };
    const manifest = {
      schemaVersion: 1,
      provider: "codex",
      platform: "windows",
      architecture: "x64",
      versions,
      entrypoints: { adapter: "adapter/index.js", node: "bin/node.exe" },
      launch: { program: "bin/node.exe", args: ["adapter/index.js"] },
      externalHostRequired: true,
      excludedOptionalFeatures: ["codex-cli", "ripgrep", "code-mode-host", "voice", "vendor-zsh"],
      sourceIdentities: sources,
      files: runtimeFiles(staging, true),
    };
    putJson(path.join(staging, "manifest.json"), manifest);
    const lock = {
      schemaVersion: 1,
      provider: "codex",
      platform: "windows",
      architecture: "x64",
      resourceRoot: RESOURCE_ROOT,
      versions,
      manifestSha256: hash(readFileSync(path.join(staging, "manifest.json"))),
      files: runtimeFiles(staging),
    };
    verifyManifest(staging, lock, lock.files);
    const lockedPath = path.join(projectRoot, LOCK_PATH);
    if (existsSync(lockedPath) && !options.refreshLock) {
      const expected = json(lockedPath);
      assert.deepEqual(lock, expected, "Prepared runtime differs from committed Windows lock; review and refresh the lock explicitly");
    } else putJson(lockedPath, lock);
    // Stable timestamps make payload hashes and candidate evidence reproducible.
    for (const file of runtimeFiles(staging)) utimesSync(path.join(staging, file.path), 0, 0);
    renameSync(staging, destination);
    return verifyLockedCodexRuntime(projectRoot);
  } catch (error) {
    rmSync(staging, { recursive: true, force: true });
    throw error;
  }
}

export function verifyInstalledRuntime(runtimeRoot, projectRoot) {
  const lock = json(path.join(projectRoot, LOCK_PATH));
  return verifyRuntimeDirectory(runtimeRoot, lock);
}

async function main() {
  const projectRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../..");
  const [command, ...args] = process.argv.slice(2);
  if (command === "prepare")
    console.log(JSON.stringify(await prepareRuntime(projectRoot, { adapterRoot: args.filter(a => a !== "--refresh-lock")[0] || undefined, nodeRoot: args.filter(a => a !== "--refresh-lock")[1], refreshLock: args.includes("--refresh-lock") }), null, 2));
  else if (command === "verify") console.log(JSON.stringify(verifyLockedCodexRuntime(projectRoot), null, 2));
  else if (command === "download-node") console.log(await downloadNodeRuntime(args[0]));
  else if (command === "verify-installed") console.log(JSON.stringify(verifyInstalledRuntime(path.resolve(args[0]), projectRoot), null, 2));
  else throw new Error("Usage: package-codex-runtime-windows.mjs prepare [adapterRoot] [nodeRoot] | verify | download-node [cacheRoot] | verify-installed <runtimeRoot>");
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url))
  main().catch((error) => {
    console.error(`Codex Windows runtime: ${error.message}`);
    process.exitCode = 1;
  });
