import type { AgentDescriptor, AgentProvider, WorkspacePolicy } from "./types";

export const providers: Record<
  AgentProvider,
  {
    name: string;
    command: string;
    args: string[];
    host: string;
    profile: string;
  }
> = {
  mock: {
    name: "Mock Agent",
    command: "",
    args: [],
    host: "内置演示",
    profile: "只使用浏览器预览数据，不启动本机 Agent。",
  },
  hermes: {
    name: "Hermes",
    command: "hermes",
    args: ["acp"],
    host: "hermes",
    profile:
      "Hermes 使用本机配置；隔离 profile 由 Host 启动配置决定。可能加载其已配置的记忆与资料。",
  },
  claude_code: {
    name: "Claude Code",
    command: "claude-agent-acp",
    args: [],
    host: "claude",
    profile:
      "通过 Claude ACP adapter 使用本机 Claude 认证和配置；可能读取 Claude 配置及项目指引。",
  },
  codex: {
    name: "Codex",
    command: "codex-acp",
    args: [],
    host: "codex",
    profile:
      "应用仅内置 Codex ACP 适配器和 Node 运行时；Codex CLI 使用本机已安装且已登录的官方版本。连接时自动检查，Atrio 不保存账号或密钥；未登录时请先在 Codex 中登录。可能读取 Codex 配置及项目指引。",
  },
};
export function agentProvider(agent: AgentDescriptor): AgentProvider {
  if (agent.transport === "mock") return "mock";
  if (
    agent.provider &&
    agent.provider in providers &&
    agent.provider !== "mock"
  )
    return agent.provider;
  if (agent.id === "claude_code" || agent.command.endsWith("claude-agent-acp"))
    return "claude_code";
  if (agent.id === "codex" || agent.command.endsWith("codex-acp"))
    return "codex";
  return "hermes"; // Legacy native descriptors were Hermes only.
}
export function providerName(agent: AgentDescriptor) {
  return providers[agentProvider(agent)].name;
}
export function scopeAccepted(
  policy: WorkspacePolicy,
  agent: AgentDescriptor,
): boolean {
  if (policy.scopeProvider && policy.scopeProvider !== agentProvider(agent))
    return false;
  return (
    policy.agentScopeAccepted ??
    (agentProvider(agent) === "hermes" && policy.hermesScopeAccepted)
  );
}
export function providerDescriptor(
  provider: AgentProvider,
  current?: AgentDescriptor,
): AgentDescriptor {
  const defaults = providers[provider];
  if (current && agentProvider(current) === provider)
    return { ...current, provider };
  return {
    provider,
    id: provider,
    name: defaults.name,
    transport: provider === "mock" ? "mock" : "stdio",
    command: defaults.command,
    args: [...defaults.args],
    env: {},
    cwd: "",
    probeStatus: "not_connected",
  };
}
