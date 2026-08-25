import { defineStore } from "pinia";
import { ref } from "vue";
import { dashboardApi } from "@/features/dashboard/api";
import type { DashboardSummary } from "@/types";
import { useAiRequest } from "@/composables/useAiRequest";

export const useDashboardStore = defineStore("dashboard", () => {
  const summary = ref<DashboardSummary | null>(null);
  const loading = ref(false);
  // 统一请求状态（阶段6：替代手写 loading/error 三件套）
  const ai = useAiRequest();
  const error = ai.error;

  async function loadSummary() {
    loading.value = true;
    ai.clearError();
    try {
      summary.value = await dashboardApi.getDashboardSummary();
    } catch (e) {
      ai.setError(e instanceof Error ? e.message : String(e));
    } finally {
      loading.value = false;
    }
  }

  return { summary, loading, error, loadSummary };
});
