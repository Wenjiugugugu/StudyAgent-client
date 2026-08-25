/**
 * 调试页 — 格式化工具
 *
 * 纯函数：耗时/时间戳格式化、Agent 中文标签、路径拼接。
 */

/** 格式化调用耗时（毫秒 → 可读字符串） */
export function formatDuration(ms: number | null): string {
  if (ms === null) return "—";
  if (ms < 1000) return `${ms} ms`;
  return `${(ms / 1000).toFixed(2)} s`;
}

/** 格式化用量耗时（毫秒 → 可读字符串，入参为数值） */
export function formatUsageDuration(ms: number): string {
  if (ms < 1000) return `${ms} ms`;
  return `${(ms / 1000).toFixed(2)} s`;
}

/** 格式化时间戳（ISO → 本地时间 HH:mm:ss） */
export function formatTimestamp(iso: string): string {
  try {
    const d = new Date(iso);
    return d.toLocaleTimeString("zh-CN", { hour12: false });
  } catch {
    return iso;
  }
}

/** 格式化用量时间戳（ISO → 本地完整时间） */
export function formatUsageTimestamp(iso: string): string {
  try {
    const d = new Date(iso);
    return d.toLocaleString("zh-CN", { hour12: false });
  } catch {
    return iso;
  }
}

/** Agent 类型中文标签 */
export function agentLabel(agent: string): string {
  const map: Record<string, string> = {
    planner: "计划生成",
    reviewer: "复盘",
    assistant: "助手",
    teacher: "教学",
    unknown: "未知",
  };
  return map[agent] ?? agent;
}

/** 路径拼接（统一为正斜杠，压缩连续斜杠） */
export function joinPath(...parts: string[]): string {
  return parts.filter(Boolean).join("/").replace(/\\/g, "/").replace(/\/+/g, "/");
}
