/**
 * 工作台进度计算（纯函数）
 *
 * 完成率 / 周推进度 / 时间进度等口径与原 DashboardView 保持一致。
 */

import type { ReviewFile } from "@/types";

/**
 * 昨日复盘完成率（%）。
 * 优先从 task_reviews 聚合（与后端 compute_priority_a_completion 口径一致），
 * 回退到 data.completion（旧版复盘文件）。
 */
export function completionRateFromReview(review: ReviewFile | null): number {
  if (!review) return 0;
  if (review.task_reviews?.length) {
    const all = review.task_reviews;
    const aTasks = all.filter((t) => t.priority === "A");
    const allDone = all.filter((t) => t.status === "completed").length;
    const aDone = aTasks.filter((t) => t.status === "completed").length;
    if (aTasks.length > 0) return Math.round((aDone / aTasks.length) * 100);
    if (all.length > 0) return Math.round((allDone / all.length) * 100);
    return 0;
  }
  const c = review.data.completion;
  const total = c.priority_a_total + c.priority_b_total;
  if (total === 0) return 0;
  return Math.round(((c.priority_a_done + c.priority_b_done) / total) * 100);
}

/** 整周计划完成进度：已学习天数 / 计划学习天数（推进度，0-100） */
export function weekPlanProgress(studiedDays: number, plannedDays: number): number {
  if (plannedDays <= 0) return 0;
  return Math.min(100, Math.round((studiedDays / plannedDays) * 100));
}

/** 距本周开始已过的天数（0-6） */
export function daysElapsedFromStart(weekStart: string, todayDateStr: string): number {
  const start = new Date(weekStart + "T00:00:00");
  const today = new Date(todayDateStr + "T00:00:00");
  const diff = Math.floor((today.getTime() - start.getTime()) / 86400000);
  return Math.max(0, Math.min(6, diff));
}

/** 时间进度（按 7 天推进） */
export function expectedRateFromElapsed(daysElapsed: number): number {
  return Math.round(((daysElapsed + 1) / 7) * 100);
}

/** 进度状态：周推进度是否追上时间进度的 90% */
export function isOnTrack(progress: number, expectedRate: number): boolean {
  return progress >= expectedRate * 0.9;
}
