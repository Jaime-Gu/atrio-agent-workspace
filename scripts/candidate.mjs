import { spawnSync } from "node:child_process";
import { mkdirSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import {
  freezeCandidate,
  verifyCandidate,
  buildIdentity,
  recordBuild,
  selectWebBuild,
  frozenWebDirectory,
  uniqueId,
} from "./candidate-lib.mjs";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
function run(command, args, env = process.env) {
  const result = spawnSync(command, args, { cwd: root, env, stdio: "inherit" });
  if (result.error) throw result.error;
  if (result.status !== 0)
    throw new Error(`${command} exited with ${result.status ?? result.signal}`);
}
async function main() {
  const [command, ...args] = process.argv.slice(2);
  if (command === "freeze") {
    run(process.execPath, ["scripts/check-version.mjs"]);
    const result = freezeCandidate(root, { candidateId: args[0] });
    console.log(
      JSON.stringify(
        {
          candidateManifest: result.manifestPath,
          candidateId: result.manifest.candidateId,
          sourceFingerprint: result.manifest.source.sourceFingerprint,
        },
        null,
        2,
      ),
    );
  } else if (command === "check") {
    if (!args[0])
      throw new Error(
        "Usage: candidate:check -- /absolute/path/candidate.json",
      );
    const manifest = verifyCandidate(root, path.resolve(args[0]));
    console.log(
      `Candidate ${manifest.candidateId} source verified: ${manifest.source.sourceFingerprint}`,
    );
  } else if (command === "verify-env" || command === "identity-values") {
    const channel = process.env.PIXEL_BUILD_CHANNEL;
    const identity = buildIdentity(root, channel);
    if (command === "identity-values")
      console.log(
        [
          identity.candidateId ?? "",
          identity.buildId,
          identity.sourceFingerprint ?? "",
        ].join("\n"),
      );
    else console.log(JSON.stringify(identity));
  } else if (command === "build") {
    const [channel, file] = args;
    if (!["web", "dev", "beta"].includes(channel) || !file)
      throw new Error(
        "Usage: candidate:build -- <web|dev|beta> /absolute/path/candidate.json",
      );
    const manifestPath = path.resolve(file);
    const manifest = verifyCandidate(root, manifestPath);
    const buildId = uniqueId(`${manifest.candidateId.slice(0, 75)}-${channel}`);
    const buildDir = path.join(path.dirname(manifestPath), "builds", buildId);
    mkdirSync(buildDir, { recursive: true });
    const env = {
      ...process.env,
      PIXEL_CANDIDATE_MANIFEST: manifestPath,
      PIXEL_BUILD_ID: buildId,
      PIXEL_BUILD_CHANNEL: channel,
    };
    if (channel === "web") {
      // The output location is owned by this build. Native builds never touch it.
      env.PIXEL_WEB_OUT_DIR = path.join(buildDir, "dist");
      run("npm", ["run", "build"], env);
      verifyCandidate(root, manifestPath);
      const record = recordBuild(root, channel, [env.PIXEL_WEB_OUT_DIR], env);
      selectWebBuild(root, record);
      console.log(JSON.stringify(record, null, 2));
    } else {
      // Existing native staging records app/DMG hashes after validating the source again.
      run("bash", ["scripts/build-macos.sh", channel, ...args.slice(2)], env);
      verifyCandidate(root, manifestPath);
    }
  } else if (command === "preview") {
    const { directory, build } = frozenWebDirectory(
      root,
      args[0] && path.resolve(args[0]),
    );
    console.log(
      `Frozen ${build.baseVersion}-web · ${build.candidateId} · ${build.buildId}\n${directory}`,
    );
    // No current-source verification here: a new working tree must not invalidate an already frozen preview.
    const { preview } = await import("vite");
    const server = await preview({
      configFile: false,
      root,
      build: { outDir: directory },
      preview: { host: "localhost", port: 1422, strictPort: true, open: false },
    });
    server.printUrls();
  } else {
    throw new Error(
      "Commands: freeze [candidateId], check <manifest>, build <web|dev|beta> <manifest>, preview [build.json]",
    );
  }
}
main().catch((error) => {
  console.error(`Candidate error: ${error.message}`);
  process.exitCode = 1;
});
