import { defineStore } from "pinia";
import { ref, computed } from "vue";
import * as api from "@/api";
import { todayString, yesterdayString, weekdayName, getWeekStart } from "@/utils/date";
import { useAiRequest } from "@/composables/useAiRequest";
import type { DailyPlan, PlanTask } from "@/types";

export const useTodayStore = defineStore("today", () => {
  const plan = ref<DailyPlan | null>(null);
  const loading = ref(false);
  // 统一 AI 请求状态（H21：替代手写 withTimeout + generating 三件套）
  const ai = useAiRequest();
  const generating = ai.pending;
  const error = ai.error;
  /** 昨日复盘是否缺失（仅今天加载时检查） */
  const missingYesterdayReview = ref(false);

  const allTasks = computed<PlanTask[]>(() => {
    if (!plan.value?.data?.tasks) return [];
    return plan.value.data.tasks;
  });

  const doneCount = computed(() => allTasks.value.filter((t) => t.status === "done").length);
  const totalCount = computed(() => allTasks.value.length);
  const completionRate = computed(() =>
    totalCount.value > 0 ? Math.round((doneCount.value / totalCount.value) * 100) : 0
  );

  async function loadToday() {
    await loadByDate(todayString());
  }

  /** M33：判断昨日是否为休息日或排除日（免复盘），内部封装三层检查 */
  async function isYesterdayExempt(yesterday: string): Promise<boolean> {
    // 1) 休息日判断（依据用户设置的 rest_days）
    try {
      const settings = await api.getSettings();
      const restDays = settings.study_schedule?.rest_days ?? ["周日"];
      if (restDays.includes(weekdayName(yesterday))) {
        return true;
      }
    } catch {
      // 设置读取失败，继续后续检查
    }
    // 2) 排除日判断（依据周计划中的 excluded_days）
    try {
      const wp = await api.getWeekPlan(getWeekStart(yesterday));
      return !!wp.data?.excluded_days?.some((d) => d.date === yesterday);
    } catch {
      // 无周计划，按正常流程检查复盘
    }
    return false;
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
        const exempt = await isYesterdayExempt(yesterday);
        if (!exempt) {
          try {
            await api.getReview(yesterday);
          } catch {
            // 昨日复盘不存在
            missingYesterdayReview.value = true;
          }
        }
      }
    } catch (e) {
      error.value = e instanceof Error ? e.message : String(e);
      // 加载失败（如访问无日计划文件的日期）时清空缓存的计划，
      // 避免顶部报错但下方仍显示上一次/其他日期的旧计划。
      plan.value = null;
    } finally {
      loading.value = false;
    }
  }

  async function generate(date?: string) {
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

      // 统一 AI 调用：api.generateDailyPlan 内部已含 60s 超时 + 自动取消（aiInvoke）
      await ai.run(
        () => api.generateDailyPlan(targetDate).then((p) => { plan.value = p; }),
        "生成日计划失败"
      );
    } catch (e) {
      // useAiRequest 已记录 error，此处不再重复赋值
    }
  }

  async function updateTaskStatus(taskId: string, status: PlanTask["status"]) {
    if (!plan.value) return;
    const task = allTasks.value.find((t) => t.id === taskId);
    if (task) {
      const oldStatus = task.status;
      task.status = status;
      try {
        await api.updateTaskStatus(taskId, status);
      } catch (e) {
        task.status = oldStatus;
        throw e;
      }
    }
  }

  return {
    plan,
    loading,
    generating,
    error,
    missingYesterdayReview,
    allTasks,
    doneCount,
    totalCount,
    completionRate,
    loadToday,
    loadByDate,
    generate,
    updateTaskStatus,
  };
});
