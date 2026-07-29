import { defineStore } from "pinia";
import { ref, computed } from "vue";
import * as api from "@/api";
import { todayString, yesterdayString, weekdayName } from "@/utils/date";
import type { DailyPlan, PlanTask } from "@/types";

export const useTodayStore = defineStore("today", () => {
  const plan = ref<DailyPlan | null>(null);
  const loading = ref(false);
  const generating = ref(false);
  const error = ref<string | null>(null);
  /** 昨日复盘是否缺失（仅今天加载时检查） */
  const missingYesterdayReview = ref(false);

  const allTasks = computed<PlanTask[]>(() => {
    if (!plan.value?.data?.tasks) return [];
    return plan.value.data.tasks;
  });

  const priorityATasks = computed(() =>
    allTasks.value.filter((t) => t.priority === "A")
  );
  const priorityBTasks = computed(() =>
    allTasks.value.filter((t) => t.priority === "B")
  );

  const doneCount = computed(() => allTasks.value.filter((t) => t.status === "done").length);
  const totalCount = computed(() => allTasks.value.length);
  const completionRate = computed(() =>
    totalCount.value > 0 ? Math.round((doneCount.value / totalCount.value) * 100) : 0
  );

  async function loadToday() {
    await loadByDate(todayString());
  }

  async function loadByDate(date: string) {
    loading.value = true;
    error.value = null;
    missingYesterdayReview.value = false;
    try {
      plan.value = await api.getPlanByDate(date);

      // 仅今天加载时检查昨日复盘是否存在
      if (date === todayString()) {
        const yesterday = yesterdayString();
        try {
          await api.getReview(yesterday);
        } catch {
          // 昨日复盘不存在
          missingYesterdayReview.value = true;
        }
      }
    } catch (e) {
      error.value = e instanceof Error ? e.message : String(e);
    } finally {
      loading.value = false;
    }
  }

  async function generate(date?: string) {
    generating.value = true;
    error.value = null;
    const targetDate = date ?? todayString();
    try {
      // 仅允许生成今天的计划
      if (targetDate !== todayString()) {
        throw new Error("只能生成今天的学习计划，不支持生成过去或未来的计划");
      }

      // 检测是否为休息日
      const settings = await api.getSettings();
      const weekday = weekdayName(targetDate);
      if (settings.study_schedule?.rest_days?.includes(weekday)) {
        throw new Error(`${targetDate}（${weekday}）是休息日，不生成学习计划`);
      }

      // 增加 60 秒 UI 层超时保护，避免 AI 请求挂起时一直转圈
      plan.value = await withTimeout(
        api.generateDailyPlan(targetDate),
        60000,
        `生成日计划超时（超过 60 秒）。请检查 AI Provider 配置或网络连接。`
      );
    } catch (e) {
      error.value = e instanceof Error ? e.message : String(e);
    } finally {
      generating.value = false;
    }
  }

  function withTimeout<T>(promise: Promise<T>, ms: number, timeoutMessage: string): Promise<T> {
    return Promise.race([
      promise,
      new Promise<never>((_, reject) =>
        setTimeout(() => reject(new Error(timeoutMessage)), ms)
      ),
    ]);
  }

  async function updateTaskStatus(taskId: string, status: PlanTask["status"]) {
    if (!plan.value) return;
    const task = allTasks.value.find((t) => t.id === taskId);
    if (task) {
      task.status = status;
    }
    await api.updateTaskStatus(taskId, status);
  }

  return {
    plan,
    loading,
    generating,
    error,
    missingYesterdayReview,
    allTasks,
    priorityATasks,
    priorityBTasks,
    doneCount,
    totalCount,
    completionRate,
    loadToday,
    loadByDate,
    generate,
    updateTaskStatus,
  };
});
