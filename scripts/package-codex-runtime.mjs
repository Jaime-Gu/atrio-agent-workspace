/** Offline assembly of one pinned, self-contained Codex ACP runtime.
 * Never reads HOME profiles, account state or model settings. Preparation is an
 * explicit developer step; candidate builds only verify the resulting payload.
 */
import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { execFileSync, spawnSync } from "node:child_process";
import {
  chmodSync,
  closeSync,
  openSync,
  copyFileSync,
  existsSync,
  lstatSync,
  mkdirSync,
  readdirSync,
  readFileSync,
  renameSync,
  rmSync,
  utimesSync,
  writeFileSync,
} from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

export const RESOURCE_ROOT = "work/resources.noindex/agents/codex";
export const LOCK_PATH = "scripts/codex-runtime.lock.json";
const VERSION = { adapter: "1.13.1" };
const MIB = 1024 * 1024;
// hdiutil's automatic image sizing has twice failed with this payload even
// when the host volume had ample free space. Keep the size derived from the
// source tree's apparent bytes, with room for the filesystem and metadata.
const DMG_HEADROOM_RATIO = 0.2;
const DMG_HEADROOM_BYTES = 64 * MIB;
const hash = (bytes) => createHash("sha256").update(bytes).digest("hex");
const json = (p) => JSON.parse(readFileSync(p, "utf8"));
const putJson = (p, v) => writeFileSync(p, JSON.stringify(v, null, 2) + "\n");
const cleanRelative = (p) => {
  assert.equal(typeof p, "string");
  assert.ok(
    p &&
      !path.isAbsolute(p) &&
      !/[\r\n\\]/.test(p) &&
      p.split("/").every((x) => x && x !== "." && x !== ".."),
    `Invalid resource path: ${p}`,
  );
  return p;
};
export function runtimeFiles(root, excludeManifest = false) {
  const files = [];
  function walk(relative = "") {
    for (const name of readdirSync(path.join(root, relative)).sort()) {
      const rel = relative ? `${relative}/${name}` : name;
      cleanRelative(rel);
      if (excludeManifest && rel === "manifest.json") continue;
      const file = path.join(root, rel),
        st = lstatSync(file);
      assert.ok(!st.isSymbolicLink(), `Runtime symlink is forbidden: ${rel}`);
      if (st.isDirectory()) walk(rel);
      else {
        assert.ok(st.isFile(), `Unsupported runtime file: ${rel}`);
        files.push({
          path: rel,
          sha256: hash(readFileSync(file)),
          bytes: st.size,
          executable: Boolean(st.mode & 0o111),
        });
      }
    }
  }
  walk();
  return files;
}
export function verifyRuntimeDirectory(root, lock) {
  assert.equal(lock.schemaVersion, 1, "Unsupported runtime lock");
  assert.equal(lock.provider, "codex");
  assert.equal(lock.platform, "darwin");
  assert.equal(lock.architecture, "arm64");
  const files = runtimeFiles(root);
  assert.deepEqual(
    files,
    lock.files,
    "Codex runtime payload changed: prepare and freeze a new candidate",
  );
  assert.equal(
    hash(readFileSync(path.join(root, "manifest.json"))),
    lock.manifestSha256,
    "Codex runtime manifest changed",
  );
  const manifest = json(path.join(root, "manifest.json"));
  assert.deepEqual(
    manifest.files,
    runtimeFiles(root, true),
    "Codex runtime manifest file hashes differ",
  );
  assert.deepEqual(
    manifest.versions,
    lock.versions,
    "Codex runtime version drift",
  );
  for (const entry of Object.values(manifest.entrypoints)) {
    cleanRelative(entry);
    assert.ok(
      files.some((f) => f.path === entry && f.executable),
      `Missing executable entrypoint: ${entry}`,
    );
  }
  return {
    provider: "codex",
    resourceRoot: lock.resourceRoot,
    manifestSha256: lock.manifestSha256,
    treeSha256: hash(JSON.stringify(files)),
    versions: lock.versions,
    fileCount: files.length,
    totalBytes: files.reduce((n, f) => n + f.bytes, 0),
  };
}
export function verifyLockedCodexRuntime(projectRoot) {
  const lockPath = path.join(projectRoot, LOCK_PATH);
  // Older candidates and standalone test fixtures have no runtime lock.
  if (!existsSync(lockPath)) return null;
  const lock = json(lockPath);
  assert.equal(
    lock.resourceRoot,
    RESOURCE_ROOT,
    "Unexpected Codex resource root",
  );
  return {
    ...verifyRuntimeDirectory(path.join(projectRoot, RESOURCE_ROOT), lock),
    lockPath: LOCK_PATH,
    lockSha256: hash(readFileSync(lockPath)),
  };
}
function version(exe, args = ["--version"]) {
  const r = spawnSync(exe, args, {
    encoding: "utf8",
    timeout: 15000,
    env: { PATH: "/usr/bin:/bin", LANG: "C" },
  });
  assert.equal(r.status, 0, `Version probe failed: ${path.basename(exe)}`);
  return r.stdout.trim();
}
function copyFile(source, dest, executable = false) {
  assert.ok(
    lstatSync(source).isFile(),
    `Expected regular source file: ${source}`,
  );
  mkdirSync(path.dirname(dest), { recursive: true });
  copyFileSync(source, dest);
  chmodSync(dest, executable ? 0o755 : 0o644);
}
function fileSource(source) {
  return {
    fileName: path.basename(source),
    sha256: hash(readFileSync(source)),
    bytes: lstatSync(source).size,
  };
}
// esbuild comments retain the publisher's nested node_modules layout. npm ci
// can hoist that same locked dependency, so resolve its actual installation
// using nearest-parent package directories without discarding nested versions.
export function bundledDependency(packages, lock, bundlePath) {
  cleanRelative(bundlePath);
  const ancestors = bundlePath.split("/node_modules/");
  const packageName = ancestors.at(-1);
  assert.ok(
    ancestors.every((name) => /^(?:@[^/]+\/)?[^/]+$/.test(name)),
    `Invalid bundled package path: ${bundlePath}`,
  );
  for (let depth = ancestors.length - 1; depth >= 0; depth--) {
    const installedPath = [...ancestors.slice(0, depth), packageName].join(
      "/node_modules/",
    );
    const directory = path.join(packages, installedPath);
    if (!existsSync(path.join(directory, "package.json"))) continue;
    const packageJson = json(path.join(directory, "package.json"));
    const lockEntry = lock.packages?.[`node_modules/${installedPath}`];
    assert.ok(lockEntry, `Missing exact npm lock for ${installedPath}`);
    assert.equal(
      packageJson.name,
      packageName,
      `Unexpected package at ${installedPath}`,
    );
    assert.equal(
      packageJson.version,
      lockEntry.version,
      `Installed adapter dependency drifted: ${installedPath}; run npm ci`,
    );
    assert.ok(
      typeof lockEntry.resolved === "string" &&
        lockEntry.resolved.startsWith("https://registry.npmjs.org/"),
      `Missing official npm source for ${installedPath}`,
    );
    assert.match(
      lockEntry.integrity ?? "",
      /^sha(?:256|384|512)-[A-Za-z0-9+/]+={0,2}$/,
      `Missing npm source integrity for ${installedPath}`,
    );
    return { directory, packageJson, lockEntry, installedPath };
  }
  throw new Error(
    `Bundled dependency is not installed: ${bundlePath}; run npm ci`,
  );
}
function machO(file) {
  return /Mach-O/.test(
    execFileSync("/usr/bin/file", ["-b", file], { encoding: "utf8" }),
  );
}
export function verifyEmbeddedSignatures(
  directory,
  files = runtimeFiles(directory),
) {
  for (const f of files) {
    const target = path.join(directory, f.path);
    if (machO(target))
      execFileSync("/usr/bin/codesign", ["--verify", "--strict", target], {
        stdio: "pipe",
      });
  }
}
function ensureSignature(file, node = false) {
  const check = spawnSync("/usr/bin/codesign", ["--verify", "--strict", file], {
    stdio: "pipe",
  });
  if (check.status === 0) return "upstream-signature-preserved";
  const args = ["--force", "--sign", "-", "--timestamp=none"];
  let entitlements;
  if (node) {
    entitlements = file + ".entitlements.tmp";
    writeFileSync(
      entitlements,
      '<?xml version="1.0" encoding="UTF-8"?><!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd"><plist version="1.0"><dict><key>com.apple.security.cs.allow-jit</key><true/><key>com.apple.security.cs.allow-unsigned-executable-memory</key><true/></dict></plist>',
    );
    args.push("--entitlements", entitlements);
  }
  try {
    execFileSync("/usr/bin/codesign", [...args, file], { stdio: "pipe" });
  } finally {
    if (entitlements) rmSync(entitlements, { force: true });
  }
  execFileSync("/usr/bin/codesign", ["--verify", "--strict", file], {
    stdio: "pipe",
  });
  return "ad-hoc-signed-before-lock";
}
export const wrapper = `#!/bin/sh
set -eu
ATRIO_CODEX_BIN="$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)"
# Native Host sets CODEX_PATH to a verified external official CLI when one is
# available. Keep an inherited override; otherwise the adapter reports host missing.
export PATH="$ATRIO_CODEX_BIN:\${PATH:-/usr/bin:/bin:/usr/sbin:/sbin}"
exec "$ATRIO_CODEX_BIN/node" "$ATRIO_CODEX_BIN/../adapter/index.js" "$@"
`;
export function prepareRuntime(projectRoot, options) {
  assert.equal(process.platform, "darwin", "Prepare macOS runtime on macOS");
  const { adapterRoot, nodeRoot } = options;
  for (const [key, value] of Object.entries({
    adapterRoot,
    nodeRoot,
  }))
    assert.ok(
      value && path.isAbsolute(value),
      `${key} must be an explicit absolute path`,
    );
  const packages = path.join(adapterRoot, "node_modules"),
    adapter = path.join(packages, "@agentclientprotocol/codex-acp");
  assert.equal(
    json(path.join(adapter, "package.json")).version,
    VERSION.adapter,
  );
  const node = path.join(nodeRoot, "bin/node"),
    nodeVersion = version(node).replace(/^v/, "");
  assert.equal(
    nodeVersion,
    "22.23.3",
    "Use the verified, pinned Node 22.23.3 runtime",
  );
  const destination = path.join(projectRoot, RESOURCE_ROOT);
  assert.ok(
    !existsSync(destination),
    "Runtime already exists; never overwrite locked payload. Move it aside explicitly before preparing another candidate.",
  );
  const staging = destination + `.preparing-${process.pid}`;
  mkdirSync(staging, { recursive: true });
  const sources = {
    adapterPackage: fileSource(path.join(adapter, "package.json")),
    adapterLock: fileSource(path.join(adapterRoot, "package-lock.json")),
    node: fileSource(node),
  };
  try {
    copyFile(node, path.join(staging, "bin/node"), true);
    copyFile(
      path.join(adapter, "dist/index.js"),
      path.join(staging, "adapter/index.js"),
    );
    putJson(path.join(staging, "adapter/package.json"), {
      name: "@agentclientprotocol/codex-acp",
      version: VERSION.adapter,
      type: "module",
    });
    writeFileSync(path.join(staging, "bin/codex-acp"), wrapper);
    chmodSync(path.join(staging, "bin/codex-acp"), 0o755);
    mkdirSync(path.join(staging, "LICENSES"), { recursive: true });
    copyFile(
      path.join(nodeRoot, "LICENSE"),
      path.join(staging, "LICENSES/node-LICENSE"),
    );
    const js = readFileSync(path.join(adapter, "dist/index.js"), "utf8");
    const bundled = [
      "@agentclientprotocol/codex-acp",
      ...new Set(
        [
          ...js.matchAll(
            /^\/\/ node_modules\/((?:@[^/]+\/)?[^/]+(?:\/node_modules\/(?:@[^/]+\/)?[^/]+)*)\//gm,
          ),
        ].map((m) => m[1]),
      ),
    ];
    const npmLock = json(path.join(adapterRoot, "package-lock.json"));
    const dependencies = new Map(
      bundled.map((pkg) => [pkg, bundledDependency(packages, npmLock, pkg)]),
    );
    const notices = [];
    for (const pkg of bundled) {
      const { directory: dir, packageJson } = dependencies.get(pkg);
      const licenses = readdirSync(dir).filter(
        (n) =>
          /^(license|copying|notice|thirdpartynotices)/i.test(n) &&
          lstatSync(path.join(dir, n)).isFile(),
      );
      assert.ok(
        licenses.length,
        `No license found for bundled dependency ${pkg}`,
      );
      for (const license of licenses)
        copyFile(
          path.join(dir, license),
          path.join(
            staging,
            "LICENSES",
            pkg.replaceAll("/", "__") + "-" + license,
          ),
        );
      notices.push({
        package: pkg,
        version: packageJson.version,
        license: packageJson.license ?? null,
      });
    }
    putJson(path.join(staging, "LICENSES/third-party.json"), notices);
    const sourcePackages = bundled;
    putJson(
      path.join(staging, "LICENSES/npm-sources.json"),
      sourcePackages.map((pkg) => {
        const { lockEntry: info, installedPath } = dependencies.get(pkg);
        return {
          packagePath: pkg,
          installedPath,
          version: info.version,
          resolved: info.resolved,
          integrity: info.integrity,
        };
      }),
    );
    putJson(path.join(staging, "LICENSES/node-source.json"), {
      version: nodeVersion,
      url: `https://nodejs.org/dist/v${nodeVersion}/node-v${nodeVersion}-darwin-arm64.tar.gz`,
      archiveSha256:
        "23b25245dcfb9af7262f8ff142e9e2e0af025368117329e7a7458a51e5922f53",
      checksumSource: `https://nodejs.org/dist/v${nodeVersion}/SHASUMS256.txt`,
    });
    mkdirSync(path.join(staging, "codex-resources"));
    writeFileSync(
      path.join(staging, "codex-resources/README.txt"),
      "Atrio bundles the Codex ACP JavaScript adapter and Node runtime. The official Codex CLI is resolved externally by Host; CLI, ripgrep, code-mode-host, voice and vendor-zsh assets are not included.\n",
    );
    const signaturePreparation = {};
    signaturePreparation["bin/node"] = ensureSignature(
      path.join(staging, "bin/node"),
      true,
    );
    const versions = {
      adapter: VERSION.adapter,
      node: nodeVersion,
    };
    const manifest = {
      schemaVersion: 1,
      provider: "codex",
      platform: "darwin",
      architecture: "arm64",
      versions,
      entrypoints: {
        adapter: "bin/codex-acp",
        node: "bin/node",
      },
      externalHostRequired: true,
      excludedOptionalFeatures: [
        "codex-cli",
        "ripgrep",
        "code-mode-host",
        "voice",
        "vendor-zsh",
      ],
      signaturePreparation,
      sourceIdentities: sources,
      files: runtimeFiles(staging, true),
    };
    putJson(path.join(staging, "manifest.json"), manifest);
    const lock = {
      schemaVersion: 1,
      provider: "codex",
      platform: "darwin",
      architecture: "arm64",
      resourceRoot: RESOURCE_ROOT,
      versions,
      manifestSha256: hash(readFileSync(path.join(staging, "manifest.json"))),
      files: runtimeFiles(staging),
    };
    verifyRuntimeDirectory(staging, lock);
    verifyEmbeddedSignatures(staging);
    // Fixed mtimes make the independent payload archive reproducible.
    for (const file of runtimeFiles(staging))
      utimesSync(path.join(staging, file.path), 0, 0);
    renameSync(staging, destination);
    putJson(path.join(projectRoot, LOCK_PATH), lock);
    return verifyLockedCodexRuntime(projectRoot);
  } catch (error) {
    rmSync(staging, { recursive: true, force: true });
    throw error;
  }
}
export function verifyAppRuntime(projectRoot, appPath) {
  const lock = json(path.join(projectRoot, LOCK_PATH));
  const directory = path.join(appPath, "Contents/Resources/agents/codex");
  const verified = verifyRuntimeDirectory(directory, lock);
  verifyEmbeddedSignatures(directory);
  return verified;
}

/**
 * Return the apparent (file-content) size of a DMG staging directory.
 *
 * `du -k` alone reports allocated blocks, which can be much larger than the
 * bytes that must fit in the image for sparse files. `-A` asks BSD du for
 * apparent size and keeps image sizing independent of filesystem allocation.
 */
export function apparentBytes(directory) {
  // macOS uses BSD du so sparse/allocated blocks do not affect DMG sizing.
  // Windows has no /usr/bin/du; sum regular file lengths for cross-platform
  // fixture checks and keep the same apparent-content-byte semantics.
  if (process.platform === "win32") {
    let total = 0;
    function walk(relative = "") {
      for (const name of readdirSync(path.join(directory, relative))) {
        const rel = relative ? path.join(relative, name) : name;
        const target = path.join(directory, rel);
        const stat = lstatSync(target);
        if (stat.isDirectory()) walk(rel);
        else if (stat.isFile()) total += stat.size;
      }
    }
    walk();
    return total;
  }
  const output = execFileSync("/usr/bin/du", ["-A", "-k", "-s", directory], {
    encoding: "utf8",
  });
  const kibibytes = Number(output.trim().split(/\s+/)[0]);
  assert.ok(
    Number.isFinite(kibibytes) && kibibytes >= 0,
    `Unable to determine apparent size for ${directory}`,
  );
  return kibibytes * 1024;
}

/**
 * Convert staging bytes to an explicit hdiutil image size in MiB.
 *
 * The fixed allowance covers HFS/APFS image metadata and the percentage
 * allowance covers small changes in the copied App between builds.
 */
export function dmgSizeMiB(stagingApparentBytes) {
  assert.ok(
    Number.isFinite(stagingApparentBytes) && stagingApparentBytes >= 0,
    "Staging apparent bytes must be a non-negative number",
  );
  return Math.max(
    1,
    Math.ceil(
      (stagingApparentBytes * (1 + DMG_HEADROOM_RATIO) +
        DMG_HEADROOM_BYTES) /
        MIB,
    ),
  );
}

export function archiveRuntime(projectRoot, output) {
  const identity = verifyLockedCodexRuntime(projectRoot);
  assert.ok(identity, "No runtime prepared");
  assert.ok(!existsSync(output), "Runtime archive already exists");
  const root = path.join(projectRoot, RESOURCE_ROOT);
  const files = runtimeFiles(root);
  for (const entry of files)
    assert.equal(
      lstatSync(path.join(root, entry.path)).mtimeMs,
      0,
      "Runtime archive requires fixed file mtimes; re-prepare before freezing",
    );
  const list = output + ".files.tmp";
  const rawArchive = output + ".tar.tmp";
  writeFileSync(list, files.map((f) => f.path).join("\n") + "\n");
  try {
    execFileSync(
      "/usr/bin/tar",
      [
        "--format",
        "ustar",
        "--uid",
        "0",
        "--gid",
        "0",
        "--uname",
        "root",
        "--gname",
        "wheel",
        "-cf",
        rawArchive,
        "-C",
        root,
        "-T",
        list,
      ],
      { env: { ...process.env, COPYFILE_DISABLE: "1" }, stdio: "pipe" },
    );
    // BSD tar's -z adds the current time to the gzip header even when every
    // member mtime is zero. gzip -n omits both timestamp and source filename.
    const outputFd = openSync(output, "wx");
    try {
      execFileSync("/usr/bin/gzip", ["-n", "-c", rawArchive], {
        stdio: ["ignore", outputFd, "pipe"],
      });
    } finally {
      closeSync(outputFd);
    }
  } finally {
    rmSync(list, { force: true });
    rmSync(rawArchive, { force: true });
  }
  return {
    ...identity,
    archiveSha256: hash(readFileSync(output)),
    archive: path.resolve(output),
  };
}
export function restoreRuntime(projectRoot, archive, expectedSha256) {
  assert.match(
    expectedSha256,
    /^[a-f0-9]{64}$/,
    "A verified archive SHA-256 is required",
  );
  assert.equal(
    hash(readFileSync(archive)),
    expectedSha256,
    "Runtime archive checksum mismatch",
  );
  const lock = json(path.join(projectRoot, LOCK_PATH));
  assert.equal(lock.resourceRoot, RESOURCE_ROOT);
  const names = execFileSync("/usr/bin/tar", ["-tzf", archive], {
    encoding: "utf8",
  })
    .trim()
    .split("\n");
  names.forEach(cleanRelative);
  assert.deepEqual(
    names,
    lock.files.map((f) => f.path),
    "Archive members differ from locked file list",
  );
  const types = execFileSync("/usr/bin/tar", ["-tvzf", archive], {
    encoding: "utf8",
  })
    .trim()
    .split("\n");
  assert.ok(
    types.every((line) => line.startsWith("-")),
    "Only regular files are allowed in a runtime archive",
  );
  const destination = path.join(projectRoot, RESOURCE_ROOT);
  assert.ok(!existsSync(destination), "Runtime destination already exists");
  const temporary = destination + `.restore-${process.pid}`;
  mkdirSync(temporary, { recursive: true });
  try {
    execFileSync("/usr/bin/tar", ["-xzf", archive, "-C", temporary], {
      env: { ...process.env, COPYFILE_DISABLE: "1" },
      stdio: "pipe",
    });
    verifyRuntimeDirectory(temporary, lock);
    renameSync(temporary, destination);
  } catch (error) {
    rmSync(temporary, { recursive: true, force: true });
    throw error;
  }
  return verifyLockedCodexRuntime(projectRoot);
}
export function createDmg(projectRoot, appPath, output) {
  assert.equal(process.platform, "darwin");
  verifyLockedCodexRuntime(projectRoot);
  verifyAppRuntime(projectRoot, appPath);
  // Child signatures are final and locked already. Signing the outer bundle must
  // not use --deep, which could rewrite child Mach-O bytes after the lock.
  execFileSync(
    "/usr/bin/codesign",
    ["--force", "--sign", "-", "--timestamp=none", appPath],
    { stdio: "pipe" },
  );
  verifyAppRuntime(projectRoot, appPath);
  execFileSync(
    "/usr/bin/codesign",
    ["--verify", "--deep", "--strict", appPath],
    { stdio: "pipe" },
  );
  const imageRoot = path.join(projectRoot, "work/dmg-roots.noindex");
  mkdirSync(imageRoot, { recursive: true });
  const staging = path.join(imageRoot, `codex-${process.pid}-${Date.now()}`);
  mkdirSync(staging);
  try {
    execFileSync(
      "/usr/bin/ditto",
      [appPath, path.join(staging, path.basename(appPath))],
      { stdio: "pipe" },
    );
    execFileSync("/bin/ln", [
      "-s",
      "/Applications",
      path.join(staging, "Applications"),
    ]);
    mkdirSync(path.dirname(output), { recursive: true });
    assert.ok(
      !existsSync(output),
      "DMG already exists; never overwrite a prior candidate image",
    );
    execFileSync(
      "/usr/bin/hdiutil",
      [
        "create",
        "-volname",
        "Atrio WorkSpace Beta",
        "-size",
        `${dmgSizeMiB(apparentBytes(staging))}m`,
        "-srcfolder",
        staging,
        "-format",
        "UDZO",
        "-ov",
        output,
      ],
      { stdio: "inherit" },
    );
    execFileSync("/usr/bin/hdiutil", ["verify", output], { stdio: "inherit" });
  } finally {
    rmSync(staging, { recursive: true, force: true });
  }
  return { app: appPath, dmg: output, dmgSha256: hash(readFileSync(output)) };
}

async function main() {
  const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
  const [command, ...args] = process.argv.slice(2);
  if (command === "prepare")
    console.log(
      JSON.stringify(
        prepareRuntime(root, {
          adapterRoot: args[0],
          nodeRoot: args[1],
        }),
        null,
        2,
      ),
    );
  else if (command === "verify")
    console.log(JSON.stringify(verifyLockedCodexRuntime(root), null, 2));
  else if (command === "verify-app")
    console.log(JSON.stringify(verifyAppRuntime(root, args[0]), null, 2));
  else if (command === "restore")
    console.log(
      JSON.stringify(
        restoreRuntime(root, path.resolve(args[0]), args[1]),
        null,
        2,
      ),
    );
  else if (command === "create-dmg")
    console.log(
      JSON.stringify(
        createDmg(root, path.resolve(args[0]), path.resolve(args[1])),
        null,
        2,
      ),
    );
  else if (command === "archive")
    console.log(
      JSON.stringify(archiveRuntime(root, path.resolve(args[0])), null, 2),
    );
  else
    throw new Error(
      "Usage: package-codex-runtime.mjs prepare <adapterRoot> <nodeRoot> | verify | verify-app <app> | archive <output.tar.gz>",
    );
}
if (
  process.argv[1] &&
  path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)
)
  main().catch((e) => {
    console.error(`Codex runtime: ${e.message}`);
    process.exitCode = 1;
  });
