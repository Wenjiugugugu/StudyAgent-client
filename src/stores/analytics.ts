import { defineStore } from "pinia";
import { ref, computed } from "vue";
import * as api from "@/api";
import type { AnalyticsSummary, AnalyticsRange } from "@/types";

export const useAnalyticsStore = defineStore("analytics", () => {
  const summary = ref<AnalyticsSummary | null>(null);
  const loading = ref(false);
  const error = ref<string | null>(null);
  const currentRange = ref<AnalyticsRange>("last_30_days");
  // 是否在分析中排除休息日和特殊情况排除日（默认开启）
  const excludeExemptDates = ref(true);

  const learningTrend = computed(() => summary.value?.learning_trend ?? null);
  const reviewQuality = computed(() => summary.value?.review_quality ?? null);
  const comparison = computed(() => summary.value?.comparison ?? null);

  async function load(
    range: AnalyticsRange = currentRange.value,
    exclude: boolean = excludeExemptDates.value,
  ) {
    loading.value = true;
    error.value = null;
    currentRange.value = range;
    excludeExemptDates.value = exclude;
    try {
      summary.value = await api.getAnalytics(range, exclude);
    } catch (e) {
      error.value = e instanceof Error ? e.message : String(e);
    } finally {
      loading.value = false;
    }
  }

  function setRange(range: AnalyticsRange) {
    if (range === currentRange.value) return;
    load(range);
  }

  function setExcludeExemptDates(exclude: boolean) {
    if (exclude === excludeExemptDates.value) return;
    load(currentRange.value, exclude);
  }

  return {
    summary,
    loading,
    error,
    currentRange,
    excludeExemptDates,
    learningTrend,
    reviewQuality,
    comparison,
    load,
    setRange,
    setExcludeExemptDates,
  };
});
