import { defineStore } from "pinia";
import { ref, computed } from "vue";
import * as api from "@/api";
import type { AnalyticsSummary, AnalyticsRange } from "@/types";

export const useAnalyticsStore = defineStore("analytics", () => {
  const summary = ref<AnalyticsSummary | null>(null);
  const loading = ref(false);
  const error = ref<string | null>(null);
  const currentRange = ref<AnalyticsRange>("last_30_days");

  const learningTrend = computed(() => summary.value?.learning_trend ?? null);
  const reviewQuality = computed(() => summary.value?.review_quality ?? null);
  const comparison = computed(() => summary.value?.comparison ?? null);

  async function load(range: AnalyticsRange = currentRange.value) {
    loading.value = true;
    error.value = null;
    currentRange.value = range;
    try {
      summary.value = await api.getAnalytics(range);
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

  return {
    summary,
    loading,
    error,
    currentRange,
    learningTrend,
    reviewQuality,
    comparison,
    load,
    setRange,
  };
});
