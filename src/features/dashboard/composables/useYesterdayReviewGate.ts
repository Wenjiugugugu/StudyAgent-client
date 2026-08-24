/**
 * 工作台 — 昨日复盘摘要（侧栏数据）门控
 *
 * 负责加载昨日复盘文件并派生完成率 / 整体感受 / 主要困难 / 实际时长等展示数据。
 * 由 useBriefing 在简报加载流程中调用（是否拉取由简报结果的昨日复盘存在标志决定）。
 */
import { computed, ref } from "vue";
import { prevDateString } from "@/utils/date";
import { dashboardApi } from "../api";
import { completionRateFromReview } from "../utils/progress";
import type { ReviewFile } from "@/types";

export function useYesterdayReviewGate(todayDateStr: string) {
  const yesterdayDateStr = computed(() => prevDateString(todayDateStr));
  const yesterdayReviewData = ref<ReviewFile | null>(null);

  const yesterdayCompletionRate = computed(() => completionRateFromReview(yesterdayReviewData.value));

  const yesterdayFeeling = computed(() => {
    const f = yesterdayReviewData.value?.daily_review?.overall_feeling;
    return f === "smooth" ? "顺利" : f === "normal" ? "一般" : f === "hard" ? "困难" : "—";
  });

  const yesterdayFeelingVariant = computed(() => {
    const f = yesterdayReviewData.value?.daily_review?.overall_feeling;
    if (f === "smooth") return "success";
    if (f === "hard") return "danger";
    return "default";
  });

  const yesterdayDifficulty = computed(() => {
    const d = yesterdayReviewData.value?.daily_review?.main_difficulty;
    if (!d) return null;
    const map: Record<string, string> = {
      understanding: "理解困难",
      problems: "解题困难",
      memorization: "记忆困难",
      attention: "注意力不集中",
      time_management: "时间管理",
      environment: "环境干扰",
      other: "其他",
    };
    return map[d] ?? d;
  });

  const yesterdayActualHours = computed(() => yesterdayReviewData.value?.data.total_hours ?? 0);

  /** 仅当昨日复盘存在时拉取（exists 由简报结果决定） */
  async function loadYesterdayReview(exists: boolean) {
    if (!exists) {
      yesterdayReviewData.value = null;
      return;
    }
    try {
      yesterdayReviewData.value = await dashboardApi.getReview(yesterdayDateStr.value);
    } catch {
      yesterdayReviewData.value = null;
    }
  }

  return {
    yesterdayDateStr,
    yesterdayReviewData,
    yesterdayCompletionRate,
    yesterdayFeeling,
    yesterdayFeelingVariant,
    yesterdayDifficulty,
    yesterdayActualHours,
    loadYesterdayReview,
  };
}
