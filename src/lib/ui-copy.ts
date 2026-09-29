import type { AgentConnection, ModuleProposal, PolicyMode } from "./types";

/** Shared short labels for user-visible states. Keep detailed diagnostics in panels. */
export const policyModeLabels: Record<PolicyMode, string> = {
  disabled: "禁止运行",
  restricted: "受限 / 只读",
  ask: "询问",
  full: "完全访问",
};

export const connectionStatusLabels: Record<AgentConnection["status"], string> =
  {
    not_installed: "未找到",
    not_authenticated: "待认证",
    disconnected: "未连接",
    connecting: "正在连接",
    connected: "已连接",
    stopping: "正在停止",
    error: "连接错误",
  };

export const runStatusLabels = {
  idle: "就绪",
  running: "Agent 正在工作",
  waiting_approval: "等待审批",
  completed: "任务已完成",
  failed: "任务失败",
  cancelled: "任务已取消",
} as const;

export const proposalStatusLabels: Record<ModuleProposal["status"], string> = {
  pending: "待审批 · 未写入",
  applied: "已应用",
  rejected: "已拒绝 · 未应用",
  conflict: "版本冲突 · 未应用",
  cancelled: "已取消 · 未应用",
};

/** Convert Host protection details into one actionable UI status. */
export function userFacingError(error: unknown): string {
  const message = error instanceof Error ? error.message : String(error);
  if (
    /(revision|版本).*(冲突|changed|change)|stale|过期|外部修改/i.test(message)
  )
    return "版本冲突 · 重新读取后再试";
  if (/(permission|权限).*(denied|拒绝|禁止)|权限.*不允许/i.test(message))
    return "权限已变化 · 请重新确认权限后再试";
  return message;
}
