import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import path from "node:path";
const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const args = process.argv.slice(2);
if (
  args[0] === "dev" &&
  !args.some((arg) => arg === "--config" || arg.startsWith("--config="))
) {
  args.splice(1, 0, "--config", "src-tauri/tauri.dev.conf.json");
  console.log("Using isolated Pixel Workspace Dev configuration.");
}
if (process.platform === "win32" && args[0] === "dev") {
  args.push("--config", "src-tauri/tauri.windows.conf.json");
} else if (process.platform === "win32" && !args.some((arg) => arg === "--config" || arg.startsWith("--config="))) {
  args.push("--config", "src-tauri/tauri.windows.conf.json");
}
const cli = path.join(root, "node_modules/@tauri-apps/cli/tauri.js");
const result = spawnSync(process.execPath, [cli, ...args], {
  cwd: root,
  stdio: "inherit",
  env: process.env,
});
if (result.error) {
  console.error(result.error.message);
  process.exit(1);
}
process.exit(result.status ?? 1);
