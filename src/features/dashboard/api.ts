/**
 * 工作台（Dashboard）— 领域化 API 层
 *
 * 仅 re-export 工作台用到的 API 函数，避免页面/组件/composable 直接耦合全局 @/api。
 * 命令名与后端保持一致，未做任何改动，仅做转发。
 */
import {
  getDashboardSummary,
  getBriefing,
  regenerateBriefing,
  getWeekSummaries,
  getWeekPlan,
  getReview,
} from "@/api";

export type {
  DashboardSummary,
  PlanSummary,
  PlanTask,
  BriefingFile,
  GetBriefingResult,
  ReviewFile,
  SubjectKey,
  ExcludedReasonType,
} from "@/types";

export const dashboardApi = {
  getDashboardSummary,
  getBriefing,
  regenerateBriefing,
  getWeekSummaries,
  getWeekPlan,
  getReview,
};
