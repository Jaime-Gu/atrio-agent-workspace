import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import { version } from "./package.json";
import { viteBuildContext } from "./scripts/candidate-lib.mjs";

export default defineConfig(({ mode }) => {
  const channel =
    mode === "native-dev" ? "dev" : mode === "native-beta" ? "beta" : "web";
  const { identity, webOutDir } = viteBuildContext(channel);
  return {
    plugins: [react()],
    define: {
      __APP_VERSION__: JSON.stringify(version),
      __APP_CHANNEL__: JSON.stringify(channel),
      __APP_CANDIDATE_ID__: JSON.stringify(identity.candidateId),
      __APP_BUILD_ID__: JSON.stringify(identity.buildId),
      __APP_SOURCE_FINGERPRINT__: JSON.stringify(identity.sourceFingerprint),
    },
    server: {
      host: "localhost",
      port: channel === "dev" ? 1420 : 1421,
      strictPort: true,
      watch: { ignored: ["**/src-tauri/**"] },
    },
    preview: { host: "localhost", port: 1422, strictPort: true },
    clearScreen: false,
    envPrefix: ["VITE_", "TAURI_ENV_*"],
    build: {
      target: "safari15",
      outDir: webOutDir ?? `dist/${channel}`,
      emptyOutDir: true,
    },
  };
});
