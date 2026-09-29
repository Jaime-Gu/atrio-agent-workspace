import { isDesktop } from "./api";

declare const __APP_VERSION__: string;
declare const __APP_CANDIDATE_ID__: string | null;
declare const __APP_BUILD_ID__: string;
declare const __APP_SOURCE_FINGERPRINT__: string | null;
declare const __APP_CHANNEL__: "web" | "dev" | "beta";

export const PRODUCT_NAME = "Atrio WorkSpace";

export interface AppInfo {
  version: string;
  candidateId: string | null;
  buildId: string;
  sourceFingerprint: string | null;
  channel: "web" | "dev" | "beta";
  appName: string;
  identifier: string;
  executablePath: string;
  dataDir: string;
  source: "web" | "native";
}

export type AppInfoState =
  | { status: "loading" }
  | { status: "ready"; info: AppInfo }
  | { status: "error"; message: string };

function webAppInfo(): AppInfo {
  if (__APP_CHANNEL__ !== "web")
    throw new Error("当前页面不是 Web 渠道构建，请使用 Web 预览入口。");
  return {
    version: __APP_VERSION__,
    candidateId: __APP_CANDIDATE_ID__,
    buildId: __APP_BUILD_ID__,
    sourceFingerprint: __APP_SOURCE_FINGERPRINT__,
    channel: __APP_CHANNEL__,
    appName: "Atrio WorkSpace Web",
    identifier: window.location.origin,
    executablePath: window.location.href,
    dataDir: `浏览器 localStorage · ${window.location.origin}`,
    source: "web",
  };
}

export function initialAppInfo(): AppInfoState {
  if (isDesktop) return { status: "loading" };
  try {
    return { status: "ready", info: webAppInfo() };
  } catch (error) {
    return appInfoError(error);
  }
}

export function appInfoError(error: unknown): AppInfoState {
  return {
    status: "error",
    message: error instanceof Error ? error.message : String(error),
  };
}

export async function loadAppInfo(): Promise<AppInfo> {
  if (!isDesktop) return webAppInfo();
  const value: unknown = await (
    await import("@tauri-apps/api/core")
  ).invoke("get_app_info");
  if (
    !value ||
    typeof value !== "object" ||
    !("channel" in value) ||
    (value.channel !== "dev" && value.channel !== "beta") ||
    ![
      "version",
      "appName",
      "identifier",
      "executablePath",
      "dataDir",
      "buildId",
    ].every(
      (key) =>
        key in value &&
        typeof (value as Record<string, unknown>)[key] === "string" &&
        (value as Record<string, string>)[key].trim().length > 0,
    ) ||
    !("candidateId" in value) ||
    !("sourceFingerprint" in value) ||
    !(
      (value.candidateId === null && value.sourceFingerprint === null) ||
      (typeof value.candidateId === "string" &&
        value.candidateId.length > 0 &&
        typeof value.sourceFingerprint === "string" &&
        /^sha256:[a-f0-9]{64}$/.test(value.sourceFingerprint))
    )
  )
    throw new Error("本地 Host 返回的应用身份不完整，无法核实运行版本。");
  const info = value as Omit<AppInfo, "source">;
  if (
    (info.candidateId !== null || __APP_CANDIDATE_ID__ !== null) &&
    (info.candidateId !== __APP_CANDIDATE_ID__ ||
      info.buildId !== __APP_BUILD_ID__ ||
      info.sourceFingerprint !== __APP_SOURCE_FINGERPRINT__ ||
      info.version !== __APP_VERSION__ ||
      info.channel !== __APP_CHANNEL__)
  )
    throw new Error("前端与本地 Host 的候选身份不一致，请重新构建同一候选。");
  return { ...info, source: "native" };
}

export function appVersionLabel(state: AppInfoState): string {
  if (state.status === "loading") return "版本核实中…";
  if (state.status === "error") return "版本未确认";
  return `${state.info.version}-${state.info.channel}`;
}
