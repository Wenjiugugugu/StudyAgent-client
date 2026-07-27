import { defineStore } from "pinia";
import { ref } from "vue";
import * as api from "@/api";
import type { DashboardSummary } from "@/types";

export const useDashboardStore = defineStore("dashboard", () => {
  const summary = ref<DashboardSummary | null>(null);
  const loading = ref(false);
  const error = ref<string | null>(null);

  async function loadSummary() {
    loading.value = true;
    error.value = null;
    try {
      summary.value = await api.getDashboardSummary();
    } catch (e) {
      error.value = e instanceof Error ? e.message : String(e);
    } finally {
      loading.value = false;
    }
  }

  return { summary, loading, error, loadSummary };
});
