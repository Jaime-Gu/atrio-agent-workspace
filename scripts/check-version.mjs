import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import path from "node:path";
import assert from "node:assert/strict";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const read = (name) => readFileSync(path.join(root, name), "utf8");
const json = (name) => JSON.parse(read(name));
const version = json("package.json").version;
assert.match(
  version,
  /^\d+\.\d+\.\d+$/,
  "Use a numeric base version; channel suffix is separate",
);
assert.equal(
  json("package-lock.json").version,
  version,
  "package-lock version drift",
);
assert.equal(
  json("package-lock.json").packages[""].version,
  version,
  "locked package version drift",
);
assert.equal(
  read("src-tauri/Cargo.toml").match(/^version\s*=\s*"([^"]+)"/m)?.[1],
  version,
  "Cargo.toml version drift",
);
assert.equal(
  read("src-tauri/Cargo.lock").match(
    /name = "pixel-workspace"\nversion = "([^"]+)"/,
  )?.[1],
  version,
  "Cargo.lock version drift",
);
const beta = json("src-tauri/tauri.conf.json");
const dev = json("src-tauri/tauri.dev.conf.json");
assert.equal(beta.version, version, "Tauri version drift");
assert.ok(!dev.version || dev.version === version, "Dev version drift");
assert.equal(beta.identifier, "dev.pixel.workspace");
assert.equal(dev.identifier, "dev.pixel.workspace.dev");
assert.equal(beta.productName, "Atrio WorkSpace Beta");
assert.equal(dev.productName, "Atrio WorkSpace Dev");
assert.equal(beta.build.frontendDist, "../dist/beta");
assert.equal(dev.build.frontendDist, "../dist/dev");
console.log(`Version ${version} verified; web/dev/beta channels are isolated.`);
