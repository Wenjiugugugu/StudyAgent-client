<script setup lang="ts">
/**
 * 每日计划页（TodayView）— 「今天具体学什么」
 *
 * 信息架构（0.7.1 重构）：
 *   顶部：日期 + 星期 + 考研倒计时 / 今日计划 + 任务数 + 预计总时长
 *   左侧（~70%）：今日任务 → 按科目分组的紧凑两行任务列表
 *   右侧（~30%）：今日概览 + 今日科目分配 + 任务状态
 *
 * 设计约束：
 * - 不使用任务卡片；用科目标题（一级）、任务行（二级）、1px 细线与留白建立层级。
 * - 任务行只有两个操作：checkbox（切换完成状态）、计时按钮（启用计时时可用）。
 *   不提供任务细则展开/详情下钻，行内不做二次展开。
 * - 「截止」只在目标计划模式（该科目存在生效中的目标区间）下展示。
 * - 不展示长期目标 / 目标模式卡 / 计划依据 / AI 文案；所有统计均来自真实数据。
 */
import { computed, onMounted, onUnmounted, ref, watch } from "vue";
import { useRoute, useRouter } from "vue-router";
import { useTodayStore } from "@/stores/today";
import { useSettingsStore } from "@/stores/settings";
import * as api from "@/api";
import { todayString, yesterdayString, daysBetween, getWeekStart, prevDateString, nextDateString, currentMinutesShanghai, timeStringToMinutes, weekdayName } from "@/utils/date";
import Button from "@/components/ui/Button.vue";
import LoadingSpinner from "@/components/ui/LoadingSpinner.vue";
import EmptyState from "@/components/ui/EmptyState.vue";
import {
  ChevronLeft,
  ChevronRight,
  ChevronDown,
  ChevronUp,
  RefreshCw,
  RotateCcw,
  Coffee,
  Ban,
  AlertTriangle,
  Sparkles,
  Play,
  Pause,
} from "lucide-vue-next";
import type { PlanTask, SubjectKey, ExcludedReasonType, Goal } from "@/types";

const todayStore = useTodayStore();
const settingsStore = useSettingsStore();
const route = useRoute();
const router = useRouter();

// ────────────────────────────────────────────────────────────
// 基础：日期与导航
// ────────────────────────────────────────────────────────────

const currentDate = computed(() => {
  const q = route.query.date;
  return typeof q === "string" && /^\d{4}-\d{2}-\d{2}$/.test(q) ? q : todayString();
});

const isToday = computed(() => currentDate.value === todayString());
// 仅允许修改今天和昨天的任务完成情况，更早的任务只读
const canModifyTasks = computed(
  () => currentDate.value === todayString() || currentDate.value === yesterdayString()
);

/** 星期中文（如「星期五」） */
const weekdayLabel = computed(() => {
  const [y, m, d] = currentDate.value.split("-").map(Number);
  if (!y || !m || !d) return "";
  const w = new Date(y, m - 1, d, 12, 0, 0).getDay();
  return `星期${["日", "一", "二", "三", "四", "五", "六"][w] ?? ""}`;
});

const plan = computed(() => todayStore.plan);
const tasks = computed(() => todayStore.allTasks);

/** 日计划生成时的降级提示（后端 data.warnings：目标倒排失败、任务因预算裁剪等） */
const planWarnings = computed<string[]>(() => plan.value?.data?.warnings ?? []);

const source = computed(() => route.query.from as string | undefined);
const backLabel = computed(() => (source.value === "history" ? "返回历史计划" : "返回今天"));

function goBack() {
  if (source.value === "history") {
    router.push({ name: "history-plans" });
    return;
  }
  router.replace({ name: "plan" });
}

/** 跳转到指定日期（与 query.from 保持一致） */
function goToDate(date: string) {
  router.replace({
    name: "plan",
    query: { date, ...(source.value ? { from: source.value } : {}) },
  });
}

// 使用设置中的考试日期计算倒计时，避免 AI 生成内容解析错误
const computedRemainingDays = computed(() => {
  const examDate = settingsStore.settings?.exam_date;
  if (examDate && /^\d{4}-\d{2}-\d{2}$/.test(examDate)) {
    return daysBetween(examDate, currentDate.value);
  }
  return plan.value?.data?.remaining_days ?? 0;
});

// ────────────────────────────────────────────────────────────
// 科目：显示名与颜色
// ────────────────────────────────────────────────────────────

const FALLBACK_SUBJECT_LABEL: Record<SubjectKey, string> = {
  math: "数学",
  english: "英语",
  politics: "政治",
  professional: "专业课",
};
const SUBJECT_COLOR: Record<SubjectKey, string> = {
  math: "var(--color-math)",
  english: "var(--color-english)",
  politics: "var(--color-politics)",
  professional: "var(--color-professional)",
};

/**
 * 科目显示名：优先使用设置中的考试类型（如「数学二 / 英语二 / 政治 / 408计算机」），
 * 与引导页写入 exam_type 的口径一致；未配置时回退到通用科目名。
 */
const subjectLabels = computed<Record<SubjectKey, string>>(() => {
  const parts = (settingsStore.settings?.exam_type ?? "")
    .split("/")
    .map((s) => s.trim())
    .filter(Boolean);
  const pick = (prefix: string) => parts.find((p) => p.startsWith(prefix));
  const used = new Set<string>();
  const math = pick("数学");
  const english = pick("英语");
  const politics = pick("政治");
  if (math) used.add(math);
  if (english) used.add(english);
  if (politics) used.add(politics);
  return {
    math: math ?? FALLBACK_SUBJECT_LABEL.math,
    english: english ?? FALLBACK_SUBJECT_LABEL.english,
    politics: politics ?? FALLBACK_SUBJECT_LABEL.politics,
    // 专业课：取 exam_type 中未被前三科占用的部分（如「408计算机」）
    professional: parts.find((p) => !used.has(p)) ?? FALLBACK_SUBJECT_LABEL.professional,
  };
});

// ────────────────────────────────────────────────────────────
// 任务计时（仅当设置启用 enable_time_tracking 且查看今天/昨天时启用）
// ────────────────────────────────────────────────────────────

const timeTrackingEnabled = computed(
  () => !!settingsStore.settings?.study_schedule?.enable_time_tracking && isToday.value && canModifyTasks.value
);

/** 每个任务的计时状态：accumulated 已累计分钟，startedAt 为正在计时的开始时间戳 */
interface TaskTimerState {
  accumulated: number;
  startedAt: string | null;
}
const taskTimers = ref<Record<string, TaskTimerState>>({});
/** 用于触发正在计时的任务实时分钟数刷新的 tick（每秒 +1） */
const timerTick = ref(0);
let timerInterval: number | undefined;

/** 正在计时的任务实时显示的分钟数（含正在进行的时段） */
function taskLiveMinutes(taskId: string): number {
  const t = taskTimers.value[taskId];
  if (!t) return 0;
  let total = t.accumulated;
  if (t.startedAt) {
    // 用 timerTick 触发响应式重算
    void timerTick.value;
    const start = new Date(t.startedAt).getTime();
    const now = Date.now();
    if (!isNaN(start) && now > start) {
      total += Math.floor((now - start) / 60000);
    }
  }
  return total;
}

/** 格式化分钟为 "5h40m" / "50m" / "0h" */
function formatMin(min: number): string {
  const m = Math.max(0, Math.round(min));
  if (m === 0) return "0h";
  if (m < 60) return `${m}m`;
  const h = Math.floor(m / 60);
  const r = m % 60;
  return r > 0 ? `${h}h${r}m` : `${h}h`;
}

async function loadTaskTimers() {
  if (!timeTrackingEnabled.value) {
    taskTimers.value = {};
    return;
  }
  try {
    const state = await api.getState();
    const map: Record<string, TaskTimerState> = {};
    for (const st of state.current_task?.tasks ?? []) {
      if (!st.task_id) continue;
      map[st.task_id] = {
        accumulated: st.accumulated_minutes ?? 0,
        startedAt: st.started_at ?? null,
      };
    }
    taskTimers.value = map;
  } catch (e) {
    // 读取失败不影响主流程
    console.warn("加载任务计时状态失败", e);
  }
}

// ── 操作失败提示（计时 / 生成等即时动作，与 todayStore.error 的计划加载错误分开）──
const actionError = ref("");

function reportActionError(msg: string, e: unknown) {
  actionError.value = `${msg}：${e instanceof Error ? e.message : String(e)}`;
}

async function startTimer(taskId: string) {
  try {
    await api.startTaskTimer(taskId);
    if (!taskTimers.value[taskId]) {
      taskTimers.value[taskId] = { accumulated: 0, startedAt: null };
    }
    // startedAt 用本地时间近似（用于 UI 实时计算，后端存的权威值以 +0800 为准）
    taskTimers.value[taskId].startedAt = new Date().toISOString();
    actionError.value = "";
  } catch (e) {
    console.error("开始计时失败", e);
    reportActionError("开始计时失败", e);
  }
}

async function pauseTimer(taskId: string): Promise<boolean> {
  try {
    const added = await api.pauseTaskTimer(taskId);
    if (!taskTimers.value[taskId]) {
      taskTimers.value[taskId] = { accumulated: 0, startedAt: null };
    }
    taskTimers.value[taskId].accumulated += added;
    taskTimers.value[taskId].startedAt = null;
    actionError.value = "";
    return true;
  } catch (e) {
    console.error("暂停计时失败", e);
    reportActionError("暂停计时失败", e);
    return false;
  }
}

function isTaskRunning(taskId: string): boolean {
  return !!taskTimers.value[taskId]?.startedAt;
}

// ────────────────────────────────────────────────────────────
// 目标计划模式（用于任务行「截止」提示；不展示目标模式大卡）
// ────────────────────────────────────────────────────────────

const goalActive = ref<Goal[]>([]);

async function loadActiveGoals() {
  goalActive.value = [];
  try {
    const file = await api.listGoals();
    goalActive.value = (file?.data?.goals ?? []).filter(
      (g) => g.active && g.status === "active" && g.deadline >= currentDate.value,
    );
  } catch {
    // 获取失败不影响主流程
    goalActive.value = [];
  }
}

/**
 * 处于「目标计划模式」的科目 → 该科目的目标截止日（MM-DD）。
 *
 * 判定：goal.active 且 deadline ≥ 当前日期（与后端 `active_goals_for_subject` 的过滤一致）。
 * 目标模式的科目其当日任务由后端按章节顺序表倒排生成（见 core/goal_planner.rs），
 * 因此「截止日」只对这些科目下的任务有意义；非目标模式的科目不展示截止日。
 *
 * 多书并行（2026-09-11 起每科可有「每书一条」的多条生效目标）：这里取该科**最早的**
 * deadline 作为展示口径。同一科目的任务行无法区分所属书时，展示最早截止日更保守
 *（提醒用户先赶最紧的目标），因此是有意为之而非取错。
 */
const goalModeBySubject = computed<Partial<Record<SubjectKey, string>>>(() => {
  const out: Partial<Record<SubjectKey, string>> = {};
  for (const g of goalActive.value) {
    const cur = out[g.subject];
    if (!cur || g.deadline < cur) out[g.subject] = g.deadline.slice(5); // YYYY-MM-DD → MM-DD
  }
  return out;
});

/** 某科当前目标数（>1 表示多书并行；用于「截止」提示的 title 说明口径） */
const goalCountBySubject = computed<Partial<Record<SubjectKey, number>>>(() => {
  const out: Partial<Record<SubjectKey, number>> = {};
  for (const g of goalActive.value) {
    out[g.subject] = (out[g.subject] ?? 0) + 1;
  }
  return out;
});

// ────────────────────────────────────────────────────────────
// 任务视图模型（状态派生 + 统计，全部基于现有真实字段）
// ────────────────────────────────────────────────────────────

/** 界面状态：进行中同时兼容显式 in_progress 与正在计时（含已累计时长）的任务 */
type RowStatus = "pending" | "in_progress" | "done" | "abandoned";

const STATUS_LABEL: Record<RowStatus, string> = {
  pending: "待完成",
  in_progress: "进行中",
  done: "已完成",
  abandoned: "已放弃",
};

interface TaskRow {
  task: PlanTask;
  /** 今日计划中的序号（1 起） */
  index: number;
  status: RowStatus;
  /** 预计用时（分钟） */
  estMin: number;
  /** 实际已计时（分钟，未启用计时或无记录时为 0） */
  actualMin: number;
  /** 进行中时的预计剩余时间（分钟） */
  remainMin: number | null;
  /** 第二行的辅助信息片段 */
  meta: string[];
}

function deriveStatus(t: PlanTask): RowStatus {
  if (t.status === "done") return "done";
  if (t.status === "abandoned") return "abandoned";
  if (t.status === "in_progress") return "in_progress";
  const tm = taskTimers.value[t.id];
  if (tm && (tm.startedAt || (tm.accumulated ?? 0) > 0)) return "in_progress";
  return "pending";
}

const rows = computed<TaskRow[]>(() =>
  tasks.value.map((task, i) => {
    const status = deriveStatus(task);
    const estMin = Math.max(0, Math.round((task.estimated_hours ?? 0) * 60));
    const actualMin = timeTrackingEnabled.value ? taskLiveMinutes(task.id) : 0;
    // 截止日只在目标计划模式下展示（该科目处于生效中的目标区间）
    const deadline = goalModeBySubject.value[task.subject];

    const meta: string[] = [];
    if (status === "in_progress") {
      meta.push("进行中");
      meta.push(`已学习 ${formatMin(actualMin)} / 预计 ${formatMin(estMin)}`);
    } else if (status === "done") {
      meta.push("已完成");
      meta.push(actualMin > 0 ? `实际 ${formatMin(actualMin)}` : `预计 ${formatMin(estMin)}`);
    } else if (status === "abandoned") {
      meta.push("已放弃");
    }
    if (task.textbook) meta.push(task.textbook);
    if (deadline) {
      // 多书并行时该科可能有多条生效目标，此处展示的是最早截止日（口径见 goalModeBySubject）
      const goalCount = goalCountBySubject.value[task.subject] ?? 0;
      meta.push(
        goalCount > 1 ? `截止 ${deadline}（该科 ${goalCount} 个目标中最早）` : `截止 ${deadline}`,
      );
    }
    // 任务不区分优先级（0.4 起产品已移除优先级口径），无辅助信息时第二行为空即可。

    return {
      task,
      index: i + 1,
      status,
      estMin,
      actualMin,
      remainMin: status === "in_progress" ? Math.max(0, estMin - actualMin) : null,
      meta,
    };
  })
);

/** 按科目分组，顺序沿用计划输出顺序（首次出现的科目在前） */
interface SubjectGroup {
  subject: SubjectKey;
  label: string;
  color: string;
  rows: TaskRow[];
  count: number;
  estMin: number;
}

const groups = computed<SubjectGroup[]>(() => {
  const map = new Map<SubjectKey, SubjectGroup>();
  for (const row of rows.value) {
    const key = row.task.subject;
    let g = map.get(key);
    if (!g) {
      g = {
        subject: key,
        label: subjectLabels.value[key],
        color: SUBJECT_COLOR[key],
        rows: [],
        count: 0,
        estMin: 0,
      };
      map.set(key, g);
    }
    g.rows.push(row);
    g.count += 1;
    g.estMin += row.estMin;
  }
  return [...map.values()];
});

// 序号：今日计划中的全局顺序
const CIRCLED = "①②③④⑤⑥⑦⑧⑨⑩⑪⑫⑬⑭⑮⑯⑰⑱⑲⑳";
function indexLabel(i: number): string {
  return i >= 1 && i <= 20 ? CIRCLED[i - 1] : String(i);
}

// ── 分组折叠（默认全部展开，保证进页面即可看到今天学什么）──
const collapsed = ref<Record<string, boolean>>({});
function isCollapsed(s: SubjectKey): boolean {
  return !!collapsed.value[s];
}
function toggleGroup(s: SubjectKey) {
  collapsed.value = { ...collapsed.value, [s]: !collapsed.value[s] };
}
const allCollapsed = computed(
  () => groups.value.length > 0 && groups.value.every((g) => isCollapsed(g.subject))
);
function toggleAllGroups() {
  const next = !allCollapsed.value;
  const m: Record<string, boolean> = {};
  for (const g of groups.value) m[g.subject] = next;
  collapsed.value = m;
}

// ── 统计（全部基于上面派生的真实数据）──
const totalEstMin = computed(() => rows.value.reduce((a, r) => a + r.estMin, 0));
const doneMin = computed(() =>
  rows.value
    .filter((r) => r.status === "done")
    .reduce((a, r) => a + (r.actualMin > 0 ? r.actualMin : r.estMin), 0)
);
const remainMin = computed(() => Math.max(0, totalEstMin.value - doneMin.value));

const statusCounts = computed(() => {
  const c = { done: 0, in_progress: 0, pending: 0, abandoned: 0 };
  for (const r of rows.value) c[r.status] += 1;
  return c;
});
/** 任务状态分段条（宽度的百分比） */
const statusSegments = computed(() => {
  const total = rows.value.length;
  if (total === 0) return [];
  const c = statusCounts.value;
  return [
    { key: "done", label: "已完成", count: c.done, pct: (c.done / total) * 100 },
    { key: "in_progress", label: "进行中", count: c.in_progress, pct: (c.in_progress / total) * 100 },
    { key: "pending", label: "待完成", count: c.pending, pct: (c.pending / total) * 100 },
    { key: "abandoned", label: "已放弃", count: c.abandoned, pct: (c.abandoned / total) * 100 },
  ].filter((s) => s.count > 0);
});

/** 今日科目分配（按预计时长降序） */
const allocations = computed(() =>
  groups.value
    .filter((g) => g.estMin > 0)
    .map((g) => ({
      subject: g.subject,
      label: g.label,
      color: g.color,
      min: g.estMin,
      pct: totalEstMin.value > 0 ? Math.round((g.estMin / totalEstMin.value) * 100) : 0,
    }))
    .sort((a, b) => b.min - a.min)
);

// ── 任务状态切换（唯一的完成入口）──
async function toggleTaskDone(row: TaskRow) {
  if (!canModifyTasks.value) return;
  if (row.status === "done") {
    await todayStore.updateTaskStatus(row.task.id, "pending");
    return;
  }
  // 完成任务前自动暂停计时（若正在计时中）。
  // 暂停失败时必须回读计时状态：否则前端仍显示「计时中」而后端仍在计时，
  // 用户只能刷新页面才能纠正。
  if (timeTrackingEnabled.value && isTaskRunning(row.task.id)) {
    const paused = await pauseTimer(row.task.id);
    if (!paused) {
      await loadTaskTimers();
    }
  }
  await todayStore.updateTaskStatus(row.task.id, "done");
}

// ────────────────────────────────────────────────────────────
// 加载 / 空状态
// ────────────────────────────────────────────────────────────

async function loadPlan() {
  await todayStore.loadByDate(currentDate.value);
  // 计时状态需要在 plan 加载后加载（依赖 task_id）
  await loadTaskTimers();
  // 检查当前日期是否为排除日
  await checkExcludedDay();
  // 加载目标区间（用于任务行「截止」提示）
  await loadActiveGoals();
}

// ── 无周计划时的快捷生成（周计划页已下线，作为内置数据供日计划切分）──
const generatingWeek = ref(false);
async function generateCurrentWeek() {
  if (generatingWeek.value) return;
  generatingWeek.value = true;
  try {
    await api.generateWeekPlan(getWeekStart(currentDate.value), [], undefined);
    await loadPlan();
    actionError.value = "";
  } catch (e) {
    console.error("生成周计划失败:", e);
    reportActionError("生成周计划失败", e);
  } finally {
    generatingWeek.value = false;
  }
}

// ── 每日开始时间前不展示今日计划 ──
const nowMinutes = ref(currentMinutesShanghai());
let nowTimer: number | undefined;

const dailyStartMinutes = computed(() => {
  const t = settingsStore.settings?.study_schedule?.start_time;
  if (!t) return -1;
  return timeStringToMinutes(t);
});

const isBeforeDailyStart = computed(() => {
  if (dailyStartMinutes.value < 0) return false;
  if (nowMinutes.value >= dailyStartMinutes.value) return false;
  return currentDate.value >= todayString();
});

const dailyStartTimeLabel = computed(() => settingsStore.settings?.study_schedule?.start_time ?? "09:00");

// ── 休息日 / 排除日 ──
const isCurrentDateRestDay = computed(() => {
  const restDays = settingsStore.settings?.study_schedule?.rest_days ?? ["周日"];
  return restDays.includes(weekdayName(currentDate.value));
});

const currentDateExcluded = ref(false);
const currentDateExcludedReason = ref<ExcludedReasonType | null>(null);
const currentDateExcludedNote = ref<string | null>(null);

function reasonTypeLabel(t: ExcludedReasonType): string {
  return { travel: "外出旅行", sick: "生病", exam: "考试", other: "其他" }[t];
}

async function checkExcludedDay() {
  currentDateExcluded.value = false;
  currentDateExcludedReason.value = null;
  currentDateExcludedNote.value = null;
  try {
    const ws = getWeekStart(currentDate.value);
    const wp = await api.getWeekPlan(ws);
    const ex = wp.data?.excluded_days?.find((d) => d.date === currentDate.value);
    if (ex) {
      currentDateExcluded.value = true;
      currentDateExcludedReason.value = ex.reason_type;
      currentDateExcludedNote.value = ex.note ?? null;
    }
  } catch {
    // 无周计划或获取失败，忽略
  }
}

function refreshNow() {
  nowMinutes.value = currentMinutesShanghai();
}

function goToReview() {
  router.push({ name: "review", query: { date: yesterdayString() } });
}

watch(currentDate, () => {
  loadPlan();
});

/** 全局键盘监听：左右键切换历史日期 */
function handleKeydown(e: KeyboardEvent) {
  if (route.name !== "plan") return;
  const target = e.target as HTMLElement | null;
  if (target) {
    const tag = target.tagName;
    if (tag === "INPUT" || tag === "TEXTAREA" || tag === "SELECT" || target.isContentEditable) {
      return;
    }
    if (target.closest('[contenteditable="true"]')) return;
  }
  if (e.metaKey || e.ctrlKey || e.altKey) return;

  if (e.key === "ArrowLeft") {
    e.preventDefault();
    goToDate(prevDateString(currentDate.value));
  } else if (e.key === "ArrowRight") {
    e.preventDefault();
    const next = nextDateString(currentDate.value);
    if (next > todayString()) return;
    goToDate(next);
  }
}

/** 是否存在正在计时的任务 —— 只有它需要每秒刷新 */
const hasRunningTimer = computed(() =>
  Object.values(taskTimers.value).some((t) => !!t.startedAt)
);

function startTimerTick() {
  if (timerInterval != null) return;
  timerInterval = window.setInterval(() => {
    timerTick.value++;
  }, 1000);
}

function stopTimerTick() {
  if (timerInterval != null) {
    window.clearInterval(timerInterval);
    timerInterval = undefined;
  }
}

// 仅在「有任务正在计时」时启动每秒 tick，避免无事时 rows computed 每秒全量重算
watch(hasRunningTimer, (running) => {
  if (running) startTimerTick();
  else stopTimerTick();
});

onMounted(() => {
  loadPlan();
  window.addEventListener("keydown", handleKeydown);
  // 每分钟刷新一次当前时间，确保到点后自动展示计划
  nowTimer = window.setInterval(refreshNow, 60_000);
});

onUnmounted(() => {
  window.removeEventListener("keydown", handleKeydown);
  if (nowTimer) window.clearInterval(nowTimer);
  stopTimerTick();
});
</script>

<template>
  <div class="today-view">
    <!-- Loading -->
    <LoadingSpinner v-if="todayStore.loading && !plan" :size="32" label="加载计划…" />

    <!-- 今日计划尚未到开始时间 -->
    <EmptyState
      v-else-if="isBeforeDailyStart"
      title="今天的学习时间还没开始"
      :description="`每日开始时间为 ${dailyStartTimeLabel}，到点后这里会展示今日学习计划。`"
    >
      <template #actions>
        <Button variant="secondary" @click="refreshNow">
          <RefreshCw :size="15" />
          刷新时间
        </Button>
      </template>
    </EmptyState>

    <!-- 休息日 -->
    <EmptyState
      v-else-if="isCurrentDateRestDay"
      :title="`${currentDate} 是休息日`"
      :description="`今日为设定的休息日（${weekdayName(currentDate)}），好好放松一下吧。`"
    >
      <template #icon>
        <Coffee :size="48" />
      </template>
      <template #actions>
        <Button v-if="!isToday" variant="secondary" @click="goBack">{{ backLabel }}</Button>
      </template>
    </EmptyState>

    <!-- 排除日 -->
    <EmptyState
      v-else-if="currentDateExcluded"
      :title="`${currentDate} 是排除日`"
      :description="
        currentDateExcludedReason
          ? `${reasonTypeLabel(currentDateExcludedReason)}${
              currentDateExcludedNote ? `（${currentDateExcludedNote}）` : ''
            }，今日不生成学习计划。`
          : '今日为特殊情况排除日，不生成学习计划。'
      "
    >
      <template #icon>
        <Ban :size="48" />
      </template>
      <template #actions>
        <Button v-if="!isToday" variant="secondary" @click="goBack">{{ backLabel }}</Button>
      </template>
    </EmptyState>

    <!-- 无计划 -->
    <EmptyState
      v-else-if="!plan"
      :title="`${currentDate} 还没有学习计划`"
      :description="todayStore.error || '请先生成周计划，日计划将自动从周计划中拆分生成'"
    >
      <template #actions>
        <Button
          v-if="isToday"
          variant="primary"
          :loading="generatingWeek"
          :disabled="generatingWeek"
          @click="generateCurrentWeek"
        >
          <Sparkles :size="15" />
          生成本周计划
        </Button>
        <Button v-if="!isToday" variant="secondary" @click="goBack">{{ backLabel }}</Button>
      </template>
    </EmptyState>

    <!-- Content -->
    <template v-else>
      <!-- ── 顶部：日期 + 倒计时 / 今日计划 概览 ── -->
      <header class="page-head">
        <div class="head-row">
          <div class="head-date">
            <span class="head-date-text">{{ currentDate }}</span>
            <span class="head-weekday">{{ weekdayLabel }}</span>
            <span v-if="!isToday" class="head-tag">历史计划</span>
          </div>
          <div class="head-right">
            <span class="head-countdown">距考研 {{ computedRemainingDays }} 天</span>
            <span class="head-divider" aria-hidden="true"></span>
            <div class="head-actions">
              <template v-if="!isToday">
                <button
                  class="icon-btn"
                  type="button"
                  title="前一天（←）"
                  :disabled="todayStore.loading"
                  @click="goToDate(prevDateString(currentDate))"
                >
                  <ChevronLeft :size="16" />
                </button>
                <button
                  class="icon-btn"
                  type="button"
                  title="后一天（→）"
                  :disabled="todayStore.loading || nextDateString(currentDate) > todayString()"
                  @click="goToDate(nextDateString(currentDate))"
                >
                  <ChevronRight :size="16" />
                </button>
                <button class="text-btn" type="button" @click="goBack">{{ backLabel }}</button>
              </template>
              <button
                class="icon-btn"
                type="button"
                title="刷新"
                :disabled="todayStore.loading"
                @click="loadPlan"
              >
                <RotateCcw :size="15" />
              </button>
            </div>
          </div>
        </div>
        <div class="head-plan">
          <h1 class="plan-title">今日计划</h1>
          <span class="plan-brief">
            {{ rows.length }} 项 · 预计 {{ formatMin(totalEstMin) }}
          </span>
        </div>
      </header>

      <!-- 错误 / 提醒（保留既有必要提示，非装饰性文案） -->
      <div v-if="todayStore.error" class="error-banner">
        <AlertTriangle :size="16" />
        <span>{{ todayStore.error }}</span>
      </div>
      <div v-if="actionError" class="error-banner">
        <AlertTriangle :size="16" />
        <span>{{ actionError }}</span>
        <button class="text-btn" type="button" @click="actionError = ''">知道了</button>
      </div>
      <!-- 日计划生成时的降级提示（如某本书目标倒排失败、任务因预算被裁剪） -->
      <div v-if="planWarnings.length" class="warn-banner">
        <AlertTriangle :size="16" />
        <div class="warn-list">
          <span v-for="(w, i) in planWarnings" :key="i">{{ w }}</span>
        </div>
      </div>
      <div v-if="todayStore.missingYesterdayReview" class="review-banner">
        <AlertTriangle :size="16" />
        <span>昨日复盘尚未完成，建议先完成复盘再开始今日学习。</span>
        <button class="text-btn" type="button" @click="goToReview">去复盘</button>
      </div>

      <div class="plan-layout">
        <!-- ══════════ 左：今日任务（按科目分组） ══════════ -->
        <section class="plan-main">
          <div class="list-head">
            <h2 class="list-title">今日任务</h2>
            <button
              v-if="groups.length > 1"
              class="text-btn muted"
              type="button"
              @click="toggleAllGroups"
            >
              {{ allCollapsed ? "全部展开" : "全部折叠" }}
            </button>
          </div>

          <EmptyState
            v-if="groups.length === 0"
            title="今日暂无任务"
            description="该日期的计划中没有任务条目。"
          />

          <div v-else class="group-list">
            <section v-for="g in groups" :key="g.subject" class="subject-group">
              <button class="group-head" type="button" @click="toggleGroup(g.subject)">
                <span class="group-dot" :style="{ background: g.color }" aria-hidden="true"></span>
                <span class="group-name">{{ g.label }}</span>
                <span class="group-meta">{{ g.count }} 项 · {{ formatMin(g.estMin) }}</span>
                <ChevronUp v-if="!isCollapsed(g.subject)" :size="15" class="group-caret" />
                <ChevronDown v-else :size="15" class="group-caret" />
              </button>

              <div v-show="!isCollapsed(g.subject)" class="group-rows">
                <div
                  v-for="row in g.rows"
                  :key="row.task.id"
                  class="task-row"
                  :class="row.status"
                >
                  <button
                    class="task-check"
                    :class="row.status"
                    type="button"
                    role="checkbox"
                    :aria-checked="row.status === 'done'"
                    :disabled="!canModifyTasks"
                    :title="row.status === 'done' ? '标记为未完成' : '标记为已完成'"
                    :aria-label="`${row.task.title} — ${STATUS_LABEL[row.status]}，点击切换完成状态`"
                    @click="toggleTaskDone(row)"
                  ></button>

                  <div class="row-body" :class="{ 'has-timer': timeTrackingEnabled }">
                    <span class="row-index">{{ indexLabel(row.index) }}</span>
                    <span class="row-title">{{ row.task.title }}</span>
                    <span class="row-time">
                      {{ row.remainMin !== null ? formatMin(row.remainMin) : row.estMin > 0 ? formatMin(row.estMin) : "" }}
                    </span>
                    <button
                      v-if="timeTrackingEnabled"
                      class="timer-btn"
                      :class="{ running: isTaskRunning(row.task.id) }"
                      type="button"
                      :title="isTaskRunning(row.task.id) ? '暂停计时' : '开始计时'"
                      :aria-label="`${row.task.title} — ${isTaskRunning(row.task.id) ? '暂停计时' : '开始计时'}`"
                      @click="isTaskRunning(row.task.id) ? pauseTimer(row.task.id) : startTimer(row.task.id)"
                    >
                      <Pause v-if="isTaskRunning(row.task.id)" :size="12" />
                      <Play v-else :size="12" />
                    </button>
                    <span class="row-meta">{{ row.meta.join(" · ") }}</span>
                  </div>
                </div>
              </div>
            </section>
          </div>
        </section>

        <!-- ══════════ 右：今日概览 / 科目分配 / 任务状态 ══════════ -->
        <aside class="side-panel">
          <section class="panel-section">
            <h3 class="panel-title">今日概览</h3>
            <dl class="stat-list">
              <div class="stat-row">
                <dt>任务</dt>
                <dd>{{ rows.length }} 项</dd>
              </div>
              <div class="stat-row">
                <dt>预计</dt>
                <dd>{{ formatMin(totalEstMin) }}</dd>
              </div>
              <div class="stat-row">
                <dt>已完成</dt>
                <dd>{{ formatMin(doneMin) }}</dd>
              </div>
              <div class="stat-row">
                <dt>剩余</dt>
                <dd class="strong">{{ formatMin(remainMin) }}</dd>
              </div>
            </dl>
          </section>

          <section v-if="allocations.length" class="panel-section">
            <h3 class="panel-title">今日科目分配</h3>
            <ul class="alloc-list">
              <li v-for="a in allocations" :key="a.subject" class="alloc-item">
                <div class="alloc-head">
                  <span class="alloc-name">{{ a.label }}</span>
                  <span class="alloc-time">{{ formatMin(a.min) }}</span>
                  <span class="alloc-pct">{{ a.pct }}%</span>
                </div>
                <div class="alloc-track">
                  <span class="alloc-fill" :style="{ width: `${a.pct}%`, background: a.color }"></span>
                </div>
              </li>
            </ul>
          </section>

          <section class="panel-section">
            <h3 class="panel-title">任务状态</h3>
            <ul class="status-list">
              <li v-for="s in statusSegments" :key="s.key" class="status-row">
                <span class="status-dot" :class="s.key" aria-hidden="true"></span>
                <span class="status-name">{{ s.label }}</span>
                <span class="status-count">{{ s.count }}</span>
              </li>
            </ul>
            <div v-if="statusSegments.length" class="seg-track" aria-hidden="true">
              <span
                v-for="s in statusSegments"
                :key="s.key"
                class="seg-fill"
                :class="s.key"
                :style="{ width: `${s.pct}%` }"
              ></span>
            </div>
          </section>
        </aside>
      </div>
    </template>
  </div>
</template>

<style scoped>
/* ════════ 页面骨架：桌面端优先，充分利用横向空间 ════════ */
.today-view {
  padding: 0 var(--space-8) var(--space-10);
  display: flex;
  flex-direction: column;
}

/* ════════ 顶部 ════════ */
.page-head {
  position: sticky;
  top: 0;
  z-index: 20;
  margin: 0 calc(-1 * var(--space-8));
  padding: var(--space-5) var(--space-8) var(--space-3);
  background: var(--header-bg);
  backdrop-filter: blur(12px);
  -webkit-backdrop-filter: blur(12px);
  border-bottom: 1px solid var(--divider-color);
}

.head-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--space-4);
  min-height: 24px;
}

.head-date {
  display: inline-flex;
  align-items: baseline;
  gap: var(--space-2);
  min-width: 0;
}

.head-date-text {
  font-size: var(--text-base);
  font-weight: var(--font-semibold);
  color: var(--text-primary);
  font-variant-numeric: tabular-nums;
  letter-spacing: -0.01em;
}

.head-weekday {
  font-size: var(--text-sm);
  color: var(--text-secondary);
}

.head-tag {
  font-size: var(--text-xs);
  padding: 1px 8px;
  border-radius: var(--radius-full);
  background: var(--bg-tertiary);
  color: var(--text-secondary);
  font-weight: var(--font-medium);
}

.head-right {
  display: inline-flex;
  align-items: center;
  gap: var(--space-3);
  flex-shrink: 0;
}

.head-countdown {
  font-size: var(--text-sm);
  font-weight: var(--font-medium);
  color: var(--accent);
  font-variant-numeric: tabular-nums;
}

.head-divider {
  width: 1px;
  height: 14px;
  background: var(--divider-color);
}

.head-actions {
  display: inline-flex;
  align-items: center;
  gap: var(--space-1);
}

.icon-btn {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 26px;
  height: 26px;
  border: none;
  border-radius: var(--radius-xs);
  background: transparent;
  color: var(--text-tertiary);
  cursor: pointer;
  transition: background var(--transition-fast), color var(--transition-fast);
}
.icon-btn:hover:not(:disabled) {
  background: var(--bg-overlay);
  color: var(--text-primary);
}
.icon-btn:disabled {
  opacity: 0.45;
  cursor: default;
}

.text-btn {
  border: none;
  background: transparent;
  padding: 2px 6px;
  border-radius: var(--radius-xs);
  font-size: var(--text-sm);
  color: var(--accent);
  cursor: pointer;
  transition: background var(--transition-fast);
}
.text-btn:hover {
  background: var(--accent-subtle);
}
.text-btn.muted {
  color: var(--text-tertiary);
}
.text-btn.muted:hover {
  background: var(--bg-overlay);
  color: var(--text-secondary);
}

.head-plan {
  display: flex;
  align-items: baseline;
  gap: var(--space-3);
  margin-top: var(--space-3);
}

.plan-title {
  font-size: var(--text-xl);
  font-weight: var(--font-semibold);
  color: var(--text-primary);
  letter-spacing: -0.02em;
}

.plan-brief {
  font-size: var(--text-sm);
  color: var(--text-tertiary);
  font-variant-numeric: tabular-nums;
}

/* ════════ 提示条 ════════ */
.error-banner,
.review-banner,
.warn-banner {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  margin-top: var(--space-4);
  padding: var(--space-3) var(--space-4);
  border-radius: var(--radius-md);
  font-size: var(--text-sm);
}
.error-banner {
  background: var(--color-danger-subtle);
  color: var(--color-danger);
}
.review-banner {
  background: var(--color-warning-subtle);
  color: var(--color-warning);
}
.warn-banner {
  align-items: flex-start;
  background: var(--color-warning-subtle);
  color: var(--color-warning);
}
.warn-list {
  display: flex;
  flex-direction: column;
  gap: 2px;
}

/* ════════ 两栏布局 ════════ */
.plan-layout {
  display: grid;
  grid-template-columns: minmax(0, 1fr) clamp(280px, 26%, 360px);
  gap: var(--space-8);
  align-items: start;
  margin-top: var(--space-5);
}

/* ════════ 左：任务列表 ════════ */
.plan-main {
  min-width: 0;
}

.list-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--space-3);
  padding-bottom: var(--space-2);
}

.list-title {
  font-size: var(--text-sm);
  font-weight: var(--font-semibold);
  color: var(--text-secondary);
  letter-spacing: 0.02em;
}

.group-list {
  display: flex;
  flex-direction: column;
  gap: var(--space-6);
}

/* 科目标题 = 一级层级：细线 + 留白划界，不用卡片 */
.group-head {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  width: 100%;
  padding: var(--space-2) 0;
  border: none;
  border-bottom: 1px solid var(--divider-color);
  background: transparent;
  /* button 默认 color 为系统 buttontext，不继承主题文字色，深色主题下会变成黑字 */
  color: inherit;
  cursor: pointer;
  text-align: left;
}

.group-dot {
  width: 6px;
  height: 6px;
  border-radius: var(--radius-full);
  flex-shrink: 0;
}

.group-name {
  font-size: var(--text-base);
  font-weight: var(--font-semibold);
  color: var(--text-primary);
  letter-spacing: -0.01em;
}

.group-meta {
  font-size: var(--text-xs);
  color: var(--text-tertiary);
  font-variant-numeric: tabular-nums;
}

.group-caret {
  margin-left: auto;
  color: var(--text-quaternary);
  flex-shrink: 0;
}
.group-head:hover .group-caret {
  color: var(--text-secondary);
}

.group-rows {
  padding-top: var(--space-1);
}

/* 任务 = 二级层级：紧凑两行行式，无背景、无阴影 */
.task-row {
  display: flex;
  align-items: flex-start;
  gap: var(--space-2);
  padding: var(--space-2) var(--space-2) var(--space-2) 0;
  border-radius: var(--radius-sm);
  transition: background var(--transition-fast);
}
.task-row:hover {
  background: var(--bg-overlay);
}
/* 进行中：极淡背景 + 左侧状态细线，不做成大卡片 */
.task-row.in_progress {
  background: var(--accent-subtle);
  box-shadow: inset 2px 0 0 var(--accent);
}

/* 状态图标（唯一完成入口） */
.task-check {
  position: relative;
  width: 18px;
  height: 18px;
  margin-top: 1px;
  border-radius: var(--radius-full);
  border: 1.5px solid var(--border-color-strong);
  background: transparent;
  flex-shrink: 0;
  cursor: pointer;
  padding: 0;
  transition: border-color var(--transition-fast), background var(--transition-fast);
}
.task-check:hover:not(:disabled) {
  border-color: var(--accent);
}
.task-check:disabled {
  cursor: default;
}
.task-check.in_progress {
  border-color: var(--accent);
  background: linear-gradient(90deg, var(--accent) 0 50%, transparent 50% 100%);
}
.task-check.done {
  border-color: var(--color-success);
  background: var(--color-success-subtle);
}
.task-check.done::after {
  content: "";
  position: absolute;
  left: 5.5px;
  top: 3px;
  width: 4px;
  height: 8px;
  border-right: 1.6px solid var(--color-success);
  border-bottom: 1.6px solid var(--color-success);
  transform: rotate(42deg);
}
.task-check.abandoned {
  border-style: dashed;
}

/* 任务行内容（两行结构，时间列固定在右） */
.row-body {
  flex: 1;
  min-width: 0;
  display: grid;
  grid-template-columns: 22px minmax(0, 1fr) minmax(56px, auto);
  column-gap: var(--space-3);
  row-gap: 2px;
  align-items: baseline;
}
/* 启用计时时追加一个计时按钮列 */
.row-body.has-timer {
  grid-template-columns: 22px minmax(0, 1fr) minmax(56px, auto) 20px;
}

.row-index {
  font-size: var(--text-xs);
  color: var(--text-tertiary);
  font-variant-numeric: tabular-nums;
  text-align: left;
}

.row-title {
  font-size: var(--text-base);
  font-weight: var(--font-medium);
  color: var(--text-primary);
  line-height: var(--leading-tight);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

/* 预计用时列右对齐，形成整齐的垂直时间列 */
.row-time {
  font-size: var(--text-sm);
  color: var(--text-tertiary);
  font-variant-numeric: tabular-nums;
  text-align: right;
  white-space: nowrap;
}

/* 计时按钮：静止时不出现，hover 行或正在计时时显示（不干扰任务标题的视觉中心） */
.timer-btn {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 20px;
  height: 20px;
  align-self: center;
  padding: 0;
  border: none;
  border-radius: var(--radius-full);
  background: transparent;
  color: var(--text-quaternary);
  cursor: pointer;
  opacity: 0;
  transition: opacity var(--transition-fast), color var(--transition-fast),
    background var(--transition-fast);
}
.task-row:hover .timer-btn,
.timer-btn:focus-visible {
  opacity: 1;
}
.timer-btn:hover {
  background: var(--bg-tertiary);
  color: var(--text-secondary);
}
.timer-btn.running {
  opacity: 1;
  color: var(--accent);
}

/* 第二行：状态 / 章节 / 截止 */
.row-meta {
  grid-column: 2 / -1;
  font-size: var(--text-xs);
  color: var(--text-tertiary);
  line-height: 1.45;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

/* 已完成：降低视觉权重 */
.task-row.done .row-title {
  color: var(--text-tertiary);
  font-weight: var(--font-normal);
}
.task-row.done .row-time {
  color: var(--text-quaternary);
}
.task-row.abandoned .row-title {
  color: var(--text-quaternary);
  text-decoration: line-through;
}
.task-row.in_progress .row-title {
  font-weight: var(--font-semibold);
}

/* ════════ 右：概览（无容器，纯信息列 + 细线分区）════════
   刻意不做成卡片：右侧只是对今日任务的整体说明，
   不应与任务列表争夺视觉重心——没有底色、描边、圆角与阴影。 */
.side-panel {
  position: sticky;
  top: 96px;
}

.panel-section {
  padding: var(--space-5) 0;
}
.panel-section:first-child {
  padding-top: 0;
}
.panel-section + .panel-section {
  border-top: 1px solid var(--divider-color);
}

.panel-title {
  font-size: var(--text-xs);
  font-weight: var(--font-medium);
  color: var(--text-tertiary);
  letter-spacing: 0.04em;
  margin-bottom: var(--space-3);
}

/* 今日概览 */
.stat-list {
  display: flex;
  flex-direction: column;
  gap: var(--space-2);
  margin: 0;
}
.stat-row {
  display: flex;
  align-items: baseline;
  justify-content: space-between;
  gap: var(--space-3);
}
.stat-row dt {
  font-size: var(--text-sm);
  color: var(--text-tertiary);
}
.stat-row dd {
  margin: 0;
  font-size: var(--text-sm);
  font-weight: var(--font-medium);
  color: var(--text-secondary);
  font-variant-numeric: tabular-nums;
}
/* 仅「剩余」保留一层强调，其余数值与标签同级 */
.stat-row dd.strong {
  font-weight: var(--font-semibold);
  color: var(--text-primary);
}
.stat-row dd.accent {
  color: var(--accent);
}

/* 今日科目分配 */
.alloc-list {
  list-style: none;
  margin: 0;
  padding: 0;
  display: flex;
  flex-direction: column;
  gap: var(--space-4);
}
.alloc-head {
  display: flex;
  align-items: baseline;
  gap: var(--space-2);
  margin-bottom: var(--space-2);
}
.alloc-name {
  font-size: var(--text-sm);
  font-weight: var(--font-medium);
  color: var(--text-secondary);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.alloc-time {
  margin-left: auto;
  font-size: var(--text-xs);
  color: var(--text-secondary);
  font-variant-numeric: tabular-nums;
}
.alloc-pct {
  font-size: var(--text-xs);
  color: var(--text-tertiary);
  font-variant-numeric: tabular-nums;
  min-width: 32px;
  text-align: right;
}
.alloc-track {
  height: 4px;
  border-radius: var(--radius-full);
  background: var(--bg-tertiary);
  overflow: hidden;
}
.alloc-fill {
  display: block;
  height: 100%;
  border-radius: var(--radius-full);
  transition: width var(--transition-normal);
}

/* 任务状态 */
.status-list {
  list-style: none;
  margin: 0 0 var(--space-3);
  padding: 0;
  display: flex;
  flex-direction: column;
  gap: var(--space-2);
}
.status-row {
  display: flex;
  align-items: center;
  gap: var(--space-2);
}
.status-dot {
  width: 7px;
  height: 7px;
  border-radius: var(--radius-full);
  flex-shrink: 0;
}
.status-dot.done {
  background: var(--color-success);
}
.status-dot.in_progress {
  background: var(--accent);
}
.status-dot.pending {
  border: 1.5px solid var(--text-quaternary);
}
.status-dot.abandoned {
  background: var(--text-quaternary);
}
.status-name {
  font-size: var(--text-sm);
  color: var(--text-secondary);
}
.status-count {
  margin-left: auto;
  font-size: var(--text-sm);
  font-weight: var(--font-medium);
  color: var(--text-secondary);
  font-variant-numeric: tabular-nums;
}

.seg-track {
  display: flex;
  gap: 2px;
  height: 4px;
  border-radius: var(--radius-full);
  overflow: hidden;
  background: var(--bg-tertiary);
}
.seg-fill {
  display: block;
  height: 100%;
  border-radius: var(--radius-full);
}
.seg-fill.done {
  background: var(--color-success);
}
.seg-fill.in_progress {
  background: var(--accent);
}
.seg-fill.pending {
  background: var(--text-quaternary);
}
.seg-fill.abandoned {
  background: var(--text-quaternary);
}
/* ════════ 响应式：窄窗口收成单列（桌面端优先） ════════ */
@media (max-width: 1180px) {
  .plan-layout {
    grid-template-columns: minmax(0, 1fr);
    gap: var(--space-6);
  }
  .side-panel {
    position: static;
  }
}
</style>
