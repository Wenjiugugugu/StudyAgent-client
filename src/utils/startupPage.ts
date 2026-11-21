export const startupPages = [
  { value: "/dashboard", label: "工作台" },
  { value: "/today", label: "今日计划" },
  { value: "/goal-plan", label: "目标计划" },
  { value: "/history-plans", label: "历史计划" },
  { value: "/progress", label: "进度" },
  { value: "/review", label: "复盘" },
  { value: "/focus", label: "专注" },
  { value: "/analytics", label: "分析" },
  { value: "/settings", label: "设置" },
];

export function resolveStartupPage(value: unknown): string {
  return startupPages.some((page) => page.value === value) ? String(value) : "/dashboard";
}
