// @vitest-environment jsdom
import { beforeEach, describe, expect, it, vi } from "vitest";
import { version } from "../../package.json";
import { appVersionLabel, initialAppInfo, loadAppInfo } from "./app-info";

const runtime = vi.hoisted(() => ({ desktop: false, invoke: vi.fn() }));
vi.mock("./api", () => ({
  get isDesktop() {
    return runtime.desktop;
  },
}));
vi.mock("@tauri-apps/api/core", () => ({ invoke: runtime.invoke }));

describe("application identity source", () => {
  beforeEach(() => {
    runtime.desktop = false;
    runtime.invoke.mockReset();
  });

  it("uses the Web build version and the current origin without invoking native APIs", async () => {
    const state = initialAppInfo();
    expect(appVersionLabel(state)).toBe(`${version}-web`);
    const info = await loadAppInfo();
    expect(info).toMatchObject({
      version,
      channel: "web",
      source: "web",
      identifier: window.location.origin,
      candidateId: null,
      buildId: "local-web",
      sourceFingerprint: null,
    });
    expect(info.dataDir).toContain(window.location.origin);
    expect(runtime.invoke).not.toHaveBeenCalled();
  });

  it("rejects a malformed Host response rather than claiming the build identity", async () => {
    runtime.desktop = true;
    runtime.invoke.mockResolvedValue({ version, channel: "web" });
    expect(initialAppInfo()).toEqual({ status: "loading" });
    await expect(loadAppInfo()).rejects.toThrow("无法核实运行版本");
    runtime.invoke.mockResolvedValue({
      version,
      channel: "dev",
      appName: "Atrio WorkSpace Dev",
      identifier: "dev.pixel.workspace.dev",
      executablePath: "",
      dataDir: "/data",
    });
    await expect(loadAppInfo()).rejects.toThrow("无法核实运行版本");
  });
  it("accepts truthful unfrozen native identity and rejects partial or mismatched provenance", async () => {
    runtime.desktop = true;
    const value = {
      version,
      channel: "dev",
      appName: "Atrio WorkSpace Dev",
      identifier: "dev.pixel.workspace.dev",
      executablePath: "/test/app",
      dataDir: "/test/data",
      candidateId: null,
      sourceFingerprint: null,
      buildId: "local-native",
    };
    runtime.invoke.mockResolvedValue(value);
    expect(await loadAppInfo()).toMatchObject({ ...value, source: "native" });
    runtime.invoke.mockResolvedValue({ ...value, candidateId: "candidate-1" });
    await expect(loadAppInfo()).rejects.toThrow("无法核实运行版本");
    runtime.invoke.mockResolvedValue({
      ...value,
      candidateId: "candidate-1",
      sourceFingerprint: `sha256:${"a".repeat(64)}`,
    });
    await expect(loadAppInfo()).rejects.toThrow("候选身份不一致");
  });
});
