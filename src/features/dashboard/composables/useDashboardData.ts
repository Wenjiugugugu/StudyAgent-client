/**
 * 工作台 — 数据加载与计算（原 DashboardView 数据层）
 *
 * 承载首页数据加载编排、本周进度计算、今日状态、Hero 问候/倒计时，
 * 以及休息日/排除日/开始时间前的「特殊状态」判定。
 */
import { computed, ref } from "vue";
import { useDashboardStore } from "@/stores/dashboard";
import { useTodayStore } from "@/stores/today";
import { useSettingsStore } from "@/stores/settings";
import {
  todayString,
  getWeekStart,
  daysBetween,
  currentHourShanghai,
  timeStringToMinutes,
  currentMinutesShanghai,
  weekdayName,
} from "@/utils/date";
import { dateLabelShanghai } from "../utils/date-labels";
import {
  weekPlanProgress,
  daysElapsedFromStart,
  expectedRateFromElapsed,
  isOnTrack,
} from "../utils/progress";
import { dashboardApi } from "../api";
import type { DashboardSummary, PlanSummary, PlanTask, ExcludedReasonType } from "@/types";

export function useDashboardData() {
  const dashboardStore = useDashboardStore();
  const todayStore = useTodayStore();
  const settingsStore = useSettingsStore();

  const todayDateStr = todayString();
  const summary = computed<DashboardSummary | null>(() => dashboardStore.summary);

  // ── Hero ──
  const greeting = computed(() => {
    const h = currentHourShanghai();
    if (h >= 5 && h < 12) return "早上好";
    if (h >= 12 && h < 18) return "下午好";
    if (h >= 18 && h < 22) return "晚上好";
    return "夜深了";
  });

  const displayName = computed(() => settingsStore.settings?.user_name?.trim() ?? "");

  const showGreeting = computed(() => settingsStore.settings?.show_greeting !== false);

  const dateLabel = computed(() => dateLabelShanghai());

  const remainingDays = computed(() => {
    const examDate = settingsStore.settings?.exam_date;
    if (!examDate) return 0;
    return daysBetween(examDate, todayDateStr);
  });

  // ── Current Status ──
  const streakDays = computed(() => summary.value?.streak_days ?? 0);
  const totalStudyDays = computed(() => summary.value?.total_study_days ?? 0);

  // ── Today 状态（来自 today store） ──
  const todayTasks = computed<PlanTask[]>(() => todayStore.allTasks);
  const todayDoneCount = computed(() => todayStore.doneCount);
  const todayTotalCount = computed(() => todayStore.totalCount);

  // 今日计划是否已全部完成
  const allTasksCompleted = computed(() => {
    const tasks = todayStore.allTasks;
    return tasks.length > 0 && tasks.every((t) => t.status === "done");
  });

  // 是否启用「记录学习时长」：关闭时不展示估时
  const timeTrackingEnabled = computed(
    () => !!settingsStore.settings?.study_schedule?.enable_time_tracking
  );

  // ── 每日开始时间前不展示今日计划（与 TodayView 口径一致） ──
  const nowMinutes = ref(currentMinutesShanghai());
  let nowTimer: number | undefined;

  const dailyStartMinutes = computed(() => {
    const t = settingsStore.settings?.study_schedule?.start_time;
    if (!t) return -1;
    return timeStringToMinutes(t);
  });

  const isBeforeDailyStart = computed(() => {
    if (dailyStartMinutes.value < 0) return false;
    return nowMinutes.value < dailyStartMinutes.value;
  });

  const dailyStartTimeLabel = computed(
    () => settingsStore.settings?.study_schedule?.start_time ?? "09:00"
  );

  // ── 休息日判断：根据用户设置的 rest_days 判断今天是否为休息日 ──
  const isTodayRestDay = computed(() => {
    const restDays = settingsStore.settings?.study_schedule?.rest_days ?? ["周日"];
    return restDays.includes(weekdayName(todayDateStr));
  });

  // ── 排除日判断：检查今天是否为周计划中的特殊情况排除日 ──
  const isTodayExcluded = ref(false);
  const todayExcludedReason = ref<ExcludedReasonType | null>(null);
  const todayExcludedNote = ref<string | null>(null);

  const todayExcludedReasonLabel = computed(() => {
    if (!todayExcludedReason.value) return "今日为特殊情况排除日，不生成学习计划。";
    const label = { travel: "外出旅行", sick: "生病", exam: "考试", other: "其他" }[todayExcludedReason.value];
    return todayExcludedNote.value ? `${label}（${todayExcludedNote.value}）` : label;
  });

  async function checkTodayExcluded() {
    isTodayExcluded.value = false;
    todayExcludedReason.value = null;
    todayExcludedNote.value = null;
    try {
      const ws = getWeekStart(todayDateStr);
      const wp = await dashboardApi.getWeekPlan(ws);
      const ex = wp.data?.excluded_days?.find((d) => d.date === todayDateStr);
      if (ex) {
        isTodayExcluded.value = true;
        todayExcludedReason.value = ex.reason_type;
        todayExcludedNote.value = ex.note ?? null;
      }
    } catch {
      // 无周计划或获取失败，忽略
    }
  }

  // ── 特殊状态：休息日/排除日/开始时间前 — 隐藏侧栏，居中提示 ──
  const isSpecialState = computed(
    () => isBeforeDailyStart.value || isTodayRestDay.value || isTodayExcluded.value
  );

  // ── 本周每日摘要（与周计划页同源，保证进度口径一致） ──
  const weekSummaries = ref<PlanSummary[]>([]);

  const plannedDaysPerWeek = computed(
    () => settingsStore.settings?.study_schedule.study_days_per_week ?? 6
  );

  // 已学习天数：与周计划页口径保持一致，当日有复盘或已完成任务即算已学习
  const studiedDays = computed(() => {
    return weekSummaries.value.filter((d) => d.has_review || d.completed_tasks > 0).length;
  });

  // 根据日期从 weekSummaries 查找对应日期的学习状态
  function getDaySummary(dateStr: string): PlanSummary | undefined {
    return weekSummaries.value.find((d) => d.date === dateStr);
  }

  function isDayStudied(dateStr: string): boolean {
    const s = getDaySummary(dateStr);
    return !!s && (s.has_review || s.completed_tasks > 0);
  }

  // 整周计划完成进度：已学习天数 / 计划学习天数（推进度）
  const weekProgressValue = computed(() =>
    weekPlanProgress(studiedDays.value, plannedDaysPerWeek.value)
  );

  const remainingHours = computed(() => {
    const wp = summary.value?.week_progress;
    if (!wp) return 0;
    return Math.max(0, Math.round((wp.target_hours - wp.completed_hours) * 10) / 10);
  });

  const weekStart = computed(() => summary.value?.week_progress.week_start ?? todayDateStr);

  const daysElapsed = computed(() => daysElapsedFromStart(weekStart.value, todayDateStr));

  const expectedRate = computed(() => expectedRateFromElapsed(daysElapsed.value));

  // 进度状态基于整周计划完成进度（推进度）与时间进度比较
  const isOnTrackValue = computed(() => isOnTrack(weekProgressValue.value, expectedRate.value));
  const onTrackLabel = computed(() => (isOnTrackValue.value ? "按计划" : "需加快"));

  // 加载本周每日摘要（与周计划页同源）
  async function loadWeekSummaries() {
    const ws = summary.value?.week_progress.week_start;
    if (!ws) return;
    try {
      weekSummaries.value = await dashboardApi.getWeekSummaries(ws);
    } catch {
      weekSummaries.value = [];
    }
  }

  // ── 统一刷新（首页数据，不含简报；简报由页面编排） ──
  function refresh() {
    dashboardStore.loadSummary().then(loadWeekSummaries);
    todayStore.loadToday();
    checkTodayExcluded();
  }

  // ── 生命周期：每分钟刷新当前时间，确保到点后自动展示今日计划 ──
  function startClock() {
    nowTimer = window.setInterval(() => {
      nowMinutes.value = currentMinutesShanghai();
    }, 60_000);
  }

  function stopClock() {
    if (nowTimer !== undefined) {
      clearInterval(nowTimer);
      nowTimer = undefined;
    }
  }

  return {
    todayDateStr,
    summary,
    greeting,
    displayName,
    showGreeting,
    dateLabel,
    remainingDays,
    streakDays,
    totalStudyDays,
    todayTasks,
    todayDoneCount,
    todayTotalCount,
    allTasksCompleted,
    timeTrackingEnabled,
    isBeforeDailyStart,
    dailyStartTimeLabel,
    isTodayRestDay,
    isTodayExcluded,
    todayExcludedReasonLabel,
    isSpecialState,
    plannedDaysPerWeek,
    studiedDays,
    isDayStudied,
    weekProgressValue,
    remainingHours,
    isOnTrackValue,
    onTrackLabel,
    loadWeekSummaries,
    checkTodayExcluded,
    refresh,
    startClock,
    stopClock,
  };
}
