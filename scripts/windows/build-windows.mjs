import { spawnSync } from "node:child_process";
import { cpSync, existsSync, mkdirSync, readdirSync } from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { buildIdentity, recordBuild } from "../candidate-lib.mjs";
import { verifyLockedCodexRuntime } from "./package-codex-runtime-windows.mjs";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../..");
const [channel = "beta", ...extra] = process.argv.slice(2);
if (process.platform !== "win32" || process.arch !== "x64")
  throw new Error("This build script targets native Windows x64/MSVC only.");
if (!["dev", "beta"].includes(channel)) throw new Error("Expected dev or beta");
const env = { ...process.env, PATH: [path.join(os.homedir(), ".cargo/bin"), process.env.PATH].join(path.delimiter) };
const runtime = verifyLockedCodexRuntime(root);
if (!runtime) throw new Error("Windows Codex runtime is not prepared; run package-codex-runtime-windows.mjs prepare first");
const identity = buildIdentity(root, channel, env);
const args = ["build", "--target", "x86_64-pc-windows-msvc"];
if (channel === "dev") args.push("--config", "src-tauri/tauri.dev.conf.json");
args.push("--config", "src-tauri/tauri.windows.conf.json", ...extra);
const result = spawnSync(process.execPath, [path.join(root, "node_modules/@tauri-apps/cli/tauri.js"), ...args], { cwd: root, env, stdio: "inherit" });
if (result.error) throw result.error;
if (result.status !== 0) process.exit(result.status ?? 1);
const release = path.join(root, "src-tauri/target/x86_64-pc-windows-msvc/release");
const output = path.join(root, "work/builds", identity.buildId, channel);
mkdirSync(output, { recursive: true });
const app = path.join(output, "Atrio-WorkSpace.exe");
cpSync(path.join(release, "pixel-workspace.exe"), app);
// `--no-bundle` still needs the same installed-style runtime layout so a
// copied Dev/Beta executable can exercise the bundled Codex resolver without
// a development server or a machine-specific absolute path.
const runtimeSource = path.join(root, "work/resources.noindex/agents/codex");
const runtimeOutput = path.join(output, "resources/agents/codex");
cpSync(runtimeSource, runtimeOutput, { recursive: true });
const artifacts = [app, runtimeOutput];
if (!extra.includes("--no-bundle")) {
  const nsis = path.join(release, "bundle/nsis");
  const installers = existsSync(nsis) ? readdirSync(nsis).filter(name => name.endsWith(".exe") && name.includes(channel === "dev" ? "Dev" : "Beta")) : [];
  if (installers.length !== 1) throw new Error(`Expected exactly one ${channel} installer, got ${installers.length}`);
  const installer = path.join(output, `Atrio-WorkSpace-0.0.6-${channel}-windows-x64.exe`);
  cpSync(path.join(nsis, installers[0]), installer);
  artifacts.push(installer);
}
console.log(JSON.stringify(recordBuild(root, channel, artifacts, env) ?? { channel, artifacts, acceptance: "NOT_TESTED" }, null, 2));
