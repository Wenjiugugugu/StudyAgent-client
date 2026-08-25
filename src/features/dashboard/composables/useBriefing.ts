/**
 * 工作台 — 每日简报加载与状态
 *
 * 负责拉取 get_briefing 结果、重新生成简报，并内聚昨日复盘摘要门控
 * （useYesterdayReviewGate）。简报 / 复盘相关的展示状态全部由此派生。
 */
import { computed, ref } from "vue";
import { todayString } from "@/utils/date";
import { dashboardApi } from "../api";
import { subjectLabel } from "../utils/subject-labels";
import { useYesterdayReviewGate } from "./useYesterdayReviewGate";
import type { GetBriefingResult, BriefingFile } from "@/types";

export function useBriefing() {
  const todayDateStr = todayString();

  const briefingResult = ref<GetBriefingResult | null>(null);
  const briefingLoading = ref(false);
  const briefingRegenerating = ref(false);

  const briefing = computed<BriefingFile | null>(() => briefingResult.value?.briefing ?? null);
  const briefingExists = computed(() => briefingResult.value?.exists ?? false);
  const yesterdayReviewExists = computed(() => briefingResult.value?.yesterday_review_exists ?? false);
  const yesterdayExempt = computed(() => briefingResult.value?.yesterday_exempt ?? false);
  const withinMakeupWindow = computed(() => briefingResult.value?.within_makeup_window ?? false);

  // 昨日复盘缺失且非豁免：需提示用户先去复盘
  const needYesterdayReview = computed(() => !yesterdayReviewExists.value && !yesterdayExempt.value);

  // 昨日复盘摘要（侧栏数据）
  const gate = useYesterdayReviewGate(todayDateStr);

  // H30：组件卸载后不再更新 state / 操作 DOM
  let disposed = false;

  async function loadYesterdayReview() {
    await gate.loadYesterdayReview(yesterdayReviewExists.value);
  }

  async function loadBriefing() {
    if (disposed) return;
    briefingLoading.value = true;
    try {
      briefingResult.value = await dashboardApi.getBriefing(todayDateStr);
      if (disposed) return;
      // 加载昨日复盘数据（侧栏用）
      await loadYesterdayReview();
    } catch (e) {
      if (disposed) return;
      console.warn("[Briefing] 加载简报失败:", e);
      briefingResult.value = null;
    } finally {
      if (!disposed) briefingLoading.value = false;
    }
  }

  async function regenerateBriefing() {
    if (briefingRegenerating.value) return;
    briefingRegenerating.value = true;
    try {
      const fresh = await dashboardApi.regenerateBriefing(todayDateStr);
      // 重新拉取完整状态（包含 exists 等字段）
      await loadBriefing();
      briefingResult.value = briefingResult.value ?? {
        briefing: fresh,
        exists: true,
        yesterday_review_exists: true,
        is_rest_day: false,
        is_excluded_day: false,
        yesterday_exempt: false,
        within_makeup_window: true,
      };
    } catch (e) {
      console.error("[Briefing] 重新生成简报失败:", e);
      alert(e instanceof Error ? e.message : "重新生成简报失败");
    } finally {
      briefingRegenerating.value = false;
    }
  }

  function dispose() {
    disposed = true;
  }

  // ── 简报展示辅助 ──
  const briefingGreeting = computed(() => briefing.value?.data?.greeting?.trim() ?? "");

  // 科目估算展示：附中文科目名
  const estimationList = computed(() => {
    if (!briefing.value?.data?.estimations) return [];
    return briefing.value.data.estimations.map((e) => ({
      ...e,
      subjectLabel: subjectLabel(e.subject),
    }));
  });

  return {
    briefing,
    briefingExists,
    briefingLoading,
    briefingRegenerating,
    yesterdayReviewExists,
    needYesterdayReview,
    withinMakeupWindow,
    briefingGreeting,
    estimationList,
    loadBriefing,
    regenerateBriefing,
    dispose,
    // 昨日复盘摘要（透传门控状态）
    ...gate,
  };
}
