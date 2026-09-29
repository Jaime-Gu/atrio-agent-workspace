import {
  readFileSync,
  mkdirSync,
  mkdtempSync,
  renameSync,
  copyFileSync,
  writeFileSync,
} from "node:fs";
import { fileURLToPath } from "node:url";
import path from "node:path";
import assert from "node:assert/strict";
import { buildIdentity, recordBuild } from "./candidate-lib.mjs";
const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const channel = process.argv[2];
assert.ok(channel === "dev" || channel === "beta");
const identity = buildIdentity(root, channel);
const version = JSON.parse(
  readFileSync(path.join(root, "package.json"), "utf8"),
).version;
const appName =
  channel === "dev" ? "Atrio WorkSpace Dev" : "Atrio WorkSpace Beta";
const targetRoot = process.env.CARGO_TARGET_DIR
  ? path.resolve(root, "src-tauri", process.env.CARGO_TARGET_DIR)
  : path.join(root, "src-tauri/target");
const bundle = path.join(targetRoot, "aarch64-apple-darwin/release/bundle");
const buildRoot = path.join(root, "work/builds.noindex");
mkdirSync(buildRoot, { recursive: true });
const staged = mkdtempSync(path.join(buildRoot, `${version}-${channel}-`));
const app = path.join(staged, `${appName}.app`);
// Keep temporary .app bundles out of Spotlight; never move an installed or running app.
renameSync(path.join(bundle, "macos", `${appName}.app`), app);
let dmg = null;
if (channel === "beta") {
  dmg = path.join(staged, `Atrio-WorkSpace-arm64-${version}-beta.dmg`);
  copyFileSync(
    path.join(bundle, "dmg", `${appName}_${version}_aarch64.dmg`),
    dmg,
  );
}
const record = recordBuild(root, channel, dmg ? [app, dmg] : [app]);
const info = {
  ...identity,
  candidateBuildRecord: record
    ? path.join(
        path.dirname(process.env.PIXEL_CANDIDATE_MANIFEST),
        "builds",
        identity.buildId,
        "build.json",
      )
    : null,
  version,
  channel,
  displayVersion: `${version}-${channel}`,
  app,
  dmg,
};
const latest = path.join(buildRoot, `latest-${channel}.json`);
writeFileSync(`${latest}.tmp`, JSON.stringify(info, null, 2) + "\n");
renameSync(`${latest}.tmp`, latest);
console.log(JSON.stringify(info, null, 2));
