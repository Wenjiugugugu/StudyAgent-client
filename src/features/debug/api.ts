/**
 * 调试页 — 领域化 API 层
 *
 * 仅 re-export 调试页用到的 API 函数，避免各面板/composable 直接耦合全局 @/api。
 * 命令名与后端保持一致，未做任何改动，仅做转发。
 */
import {
  getState,
  getDashboardSummary,
  getPlanByDate,
  getReview,
  getSettings,
  testAIProvider,
  getAiUsageLog,
  clearAiUsageLog,
  readAppLog,
  clearAppLog,
  debugListDir,
  debugReadFile,
  isTauri,
} from "@/api";

export { isTauri } from "@/api";
export type { DebugDirEntry } from "@/api";
export type {
  StudyState,
  DashboardSummary,
  AppSettings,
  AIProviderConfig,
  DailyPlan,
  ReviewRecord,
  AiUsageEntry,
} from "@/types";

export const debugApi = {
  getState,
  getDashboardSummary,
  getPlanByDate,
  getReview,
  getSettings,
  testAIProvider,
  getAiUsageLog,
  clearAiUsageLog,
  readAppLog,
  clearAppLog,
  debugListDir,
  debugReadFile,
  isTauri,
};
