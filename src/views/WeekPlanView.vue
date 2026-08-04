<script setup lang="ts">
import { computed, onMounted, ref, watch } from "vue";
import { useRoute, useRouter } from "vue-router";
import * as api from "@/api";
import { todayString } from "@/utils/date";
import { useSettingsStore } from "@/stores/settings";
import Card from "@/components/ui/Card.vue";
import Badge from "@/components/ui/Badge.vue";
import Button from "@/components/ui/Button.vue";
import LoadingSpinner from "@/components/ui/LoadingSpinner.vue";
import EmptyState from "@/components/ui/EmptyState.vue";
import Modal from "@/components/ui/Modal.vue";
import {
  CalendarDays,
  Sparkles,
  Target,
  CheckCircle2,
  ChevronRight,
  ChevronLeft,
  Plus,
  TrendingUp,
  Circle,
  Clock,
  Plane,
  HeartPulse,
  FileText,
  Ban,
  X,
} from "lucide-vue-next";
import type {
  WeekPlan,
  SubjectKey,
  PlanSummary,
  ExcludedDay,
  ExcludedReasonType,
  WorkloadDirection,
  WorkloadLevel,
} from "@/types";

const router = useRouter();
const route = useRoute();
const settingsStore = useSettingsStore();

const weekPlan = ref<WeekPlan | null>(null);
const weekSummaries = ref<PlanSummary[]>([]);
const loading = ref(false);
const generating = ref(false);
const error = ref<string | null>(null);

// ── 本周配置弹窗（生成前）──
const showConfigModal = ref(false);
const wlDirection = ref<WorkloadDirection>("unchanged");
const wlLevel = ref<WorkloadLevel>("small");
const wlNote = ref("");
// 生成前勾选的排除日（date -> 配置）
const configExcluded = ref<Record<string, { reason_type: ExcludedReasonType; note: string }>>({});
// 上周报告
const prevWeekSummaries = ref<PlanSummary[]>([]);
const prevWeekReportLoading = ref(false);

// ── 周中排除对话框 ──
const showExcludeDialog = ref(false);
const excludeDate = ref("");
const excludeType = ref<ExcludedReasonType>("travel");
const excludeNote = ref("");
const regenerating = ref(false);
const regenMessage = ref("");

// 今天的日期字符串（YYYY-MM-DD），用于区分「未开始」与「未复盘」
const today = todayDateStr();

/* ── Date helpers (timezone-safe, local date) ── */

function parseDate(dateStr: string): Date {
  const [y, m, d] = dateStr.split("-").map(Number);
  // 使用 12:00 避免时区边界导致日期偏移
  return new Date(y, m - 1, d, 12, 0, 0);
}

function addDays(dateStr: string, n: number): string {
  const dt = parseDate(dateStr);
  dt.setDate(dt.getDate() + n);
  const y = dt.getFullYear();
  const m = String(dt.getMonth() + 1).padStart(2, "0");
  const d = String(dt.getDate()).padStart(2, "0");
  return `${y}-${m}-${d}`;
}

function weekdayShort(dateStr: string): string {
  const weekdays = ["日", "一", "二", "三", "四", "五", "六"];
  const d = new Date(`${dateStr}T12:00:00`);
  return weekdays[d.getDay()];
}

function dayLabel(dateStr: string): string {
  // 直接从字符串取日，避免任何时区/Date 解析带来的偏移
  return dateStr.split("-")[2]?.replace(/^0/, "") ?? "";
}

function monthDay(dateStr: string): string {
  const [y, m, d] = dateStr.split("-");
  if (!y || !m || !d) return dateStr;
  return `${parseInt(m, 10)}/${parseInt(d, 10)}`;
}

function todayDateStr(): string {
  return todayString();
}

/** 中文星期名映射：根据日期字符串返回"周日"/"周一"等 */
function weekdayFullName(dateStr: string): string {
  const weekdays = ["周日", "周一", "周二", "周三", "周四", "周五", "周六"];
  const d = new Date(`${dateStr}T12:00:00`);
  return weekdays[d.getDay()];
}

/** 根据用户设置的休息日列表判断某日是否为休息日 */
function isUserRestDay(dateStr: string): boolean {
  const restDays = settingsStore.settings?.study_schedule?.rest_days ?? ["周日"];
  return restDays.includes(weekdayFullName(dateStr));
}

function getMonday(date: Date): string {
  const d = new Date(date.getFullYear(), date.getMonth(), date.getDate(), 12, 0, 0);
  const day = d.getDay();
  const diff = (day === 0 ? -6 : 1) - day;
  d.setDate(d.getDate() + diff);
  const y = d.getFullYear();
  const m = String(d.getMonth() + 1).padStart(2, "0");
  const dd = String(d.getDate()).padStart(2, "0");
  return `${y}-${m}-${dd}`;
}

const currentWeekStart = computed(() => {
  const q = route.query.week;
  if (typeof q === "string" && /^\d{4}-\d{2}-\d{2}$/.test(q)) {
    return q;
  }
  return getMonday(new Date());
});

const isCurrentWeek = computed(
  () => currentWeekStart.value === getMonday(new Date())
);

/** 最早可浏览的周一（当前周向前推 52 周） */
const earliestWeekStart = computed(() => addDays(getMonday(new Date()), -7 * 52));
/** 最晚可浏览的周一（当前周） */
const latestWeekStart = computed(() => getMonday(new Date()));

const canPrevWeek = computed(() => currentWeekStart.value > earliestWeekStart.value);
const canNextWeek = computed(() => currentWeekStart.value < latestWeekStart.value);

function prevWeek() {
  if (!canPrevWeek.value) return;
  router.replace({
    query: { ...route.query, week: addDays(currentWeekStart.value, -7) },
  });
}

function nextWeek() {
  if (!canNextWeek.value) return;
  router.replace({
    query: { ...route.query, week: addDays(currentWeekStart.value, 7) },
  });
}

function backToThisWeek() {
  const q = { ...route.query };
  delete q.week;
  router.replace({ query: q });
}

const dateRange = computed(() => {
  if (!weekPlan.value) return "";
  const start = monthDay(weekPlan.value.meta.week_start);
  const end = monthDay(addDays(weekPlan.value.meta.week_start, 6));
  return `${start} - ${end}`;
});

/* ── Week days grid ── */

interface DayInfo {
  date: string;
  weekday: string;
  dayNum: string;
  hasPlan: boolean;
  isRestDay: boolean;
  isExcluded: boolean;
  excludedType?: ExcludedReasonType;
  excludedNote?: string;
  hours: number;
  taskCount: number;
  isToday: boolean;
  completionRate: number;
  actualHours: number;
  tasksDone: number;
  hasReview: boolean;
}

/** 排除日类型 → 中文标签 */
function reasonTypeLabel(t: ExcludedReasonType): string {
  return { travel: "外出旅行", sick: "生病", exam: "考试", other: "其他" }[t];
}

/** 排除日类型 → 图标组件 */
function reasonTypeIconComp(t: ExcludedReasonType) {
  return { travel: Plane, sick: HeartPulse, exam: FileText, other: Ban }[t];
}

/** 任务量方向 → 中文标签 */
function wlDirectionLabel(d: WorkloadDirection): string {
  return { increase: "增加", unchanged: "不变", decrease: "减少" }[d];
}

/** 是否可标记为排除（今天及之后、非休息日、非已排除） */
function canExcludeDate(dateStr: string): boolean {
  if (dateStr < today) return false;
  if (isUserRestDay(dateStr)) return false;
  const ex = weekPlan.value?.data?.excluded_days ?? [];
  if (ex.some((d) => d.date === dateStr)) return false;
  return true;
}

const weekDays = computed<DayInfo[]>(() => {
  const wp = weekPlan.value;
  if (!wp) return [];
  const start = wp.meta.week_start;
  const summaries = weekSummaries.value;
  const excludedList = wp.data.excluded_days ?? [];
  const days: DayInfo[] = [];
  for (let i = 0; i < 7; i++) {
    const date = addDays(start, i);
    const dayPlan = wp.data.days.find((d) => d.date === date);
    const summary = summaries.find((s) => s.date === date);
    const allocations = dayPlan?.subject_allocations ?? [];
    const excludedEntry = excludedList.find((d) => d.date === date);

    // 优先使用 week_plan 中的 allocations；若无则回退到 summary 的真实计划数据
    const wpTaskCount = allocations.reduce(
      (sum, a) => sum + (a.task_templates?.length ?? 0),
      0
    );
    const wpHours = allocations.reduce((sum, a) => sum + (a.hours ?? 0), 0);

    const summaryHasPlan = summary?.has_plan ?? false;
    const summaryTaskCount = summary?.planned_tasks ?? 0;
    const summaryHours = summary?.planned_hours ?? 0;

    // 综合判断是否有计划：周计划有分配 OR 存在日计划文件
    const hasPlan = (wpTaskCount > 0) || summaryHasPlan;
    const taskCount = wpTaskCount > 0 ? wpTaskCount : summaryTaskCount;
    const hours = wpTaskCount > 0 ? wpHours : summaryHours;

    // 休息日以用户设置为准（防御性：即使 AI 生成的 is_rest_day 不正确也遵循用户配置）
    const userRest = isUserRestDay(date);
    // 若用户设置为休息日，则强制为休息日；若用户设置为学习日，则不为休息日
    const isRestDay = userRest;

    days.push({
      date,
      weekday: weekdayShort(date),
      dayNum: dayLabel(date),
      hasPlan,
      isRestDay,
      isExcluded: !!excludedEntry,
      excludedType: excludedEntry?.reason_type,
      excludedNote: excludedEntry?.note,
      hours,
      taskCount,
      isToday: date === today,
      completionRate: summary?.completion_rate ?? 0,
      actualHours: summary?.actual_hours ?? 0,
      tasksDone: summary?.completed_tasks ?? 0,
      hasReview: summary?.has_review ?? false,
    });
  }
  return days;
});

/** 周完成度汇总 */
const weekCompletionRate = computed(() => {
  const days = weekDays.value.filter((d) => d.hasPlan);
  if (!days.length) return 0;
  const reviewed = days.filter((d) => d.hasReview);
  if (!reviewed.length) return 0;
  const sum = reviewed.reduce((s, d) => s + d.completionRate, 0);
  return Math.round(sum / reviewed.length);
});

const weekActualHours = computed(() =>
  weekDays.value.reduce((sum, d) => sum + d.actualHours, 0)
);

const weekCompletedTasks = computed(() =>
  weekDays.value.reduce((sum, d) => sum + d.tasksDone, 0)
);

const goals = computed(() => weekPlan.value?.data?.goals ?? []);

// 是否启用「记录学习时长」：关闭时隐藏计划学时相关展示
const timeTrackingEnabled = computed(
  () => !!settingsStore.settings?.study_schedule?.enable_time_tracking
);

const weekPlannedDays = computed(() =>
  weekDays.value.filter((d) => d.hasPlan).length
);

const weekTotalTasks = computed(() =>
  weekDays.value.reduce((sum, d) => sum + d.taskCount, 0)
);

const weekTotalHours = computed(() =>
  weekDays.value.reduce((sum, d) => sum + d.hours, 0)
);

/* ── Subject styling ── */

function subjectBadgeVariant(
  subject: SubjectKey
): "math" | "english" | "politics" | "professional" {
  const map: Record<SubjectKey, "math" | "english" | "politics" | "professional"> = {
    math: "math",
    english: "english",
    politics: "politics",
    professional: "professional",
  };
  return map[subject];
}

function completionVariant(rate: number): string {
  if (rate >= 100) return "success";
  if (rate >= 50) return "warning";
  return "danger";
}

/** 未复盘时的展示文本：未来日期显示「未开始」，今天及之前显示「未复盘」 */
function pendingLabel(dateStr: string): string {
  return dateStr > today ? "未开始" : "未复盘";
}

/* ── Actions ── */

async function loadWeek() {
  loading.value = true;
  error.value = null;
  try {
    const [plan, summaries] = await Promise.all([
      api.getWeekPlan(currentWeekStart.value),
      api.getWeekSummaries(currentWeekStart.value).catch(() => [] as PlanSummary[]),
    ]);
    weekPlan.value = plan;
    weekSummaries.value = summaries;
  } catch (e) {
    error.value = e instanceof Error ? e.message : String(e);
  } finally {
    loading.value = false;
  }
}

/** 上周报告数据（用于生成前弹窗展示） */
const prevWeekReport = computed(() => {
  const sums = prevWeekSummaries.value;
  if (!sums.length) return null;
  const studyDays = sums.filter((s) => s.has_plan || s.has_review);
  const totalPlanned = sums.reduce((a, s) => a + (s.planned_tasks ?? 0), 0);
  const totalCompleted = sums.reduce((a, s) => a + (s.completed_tasks ?? 0), 0);
  const totalActualHours = sums.reduce((a, s) => a + (s.actual_hours ?? 0), 0);
  const reviewedDays = sums.filter((s) => s.has_review);
  const avgCompletion = reviewedDays.length
    ? Math.round(reviewedDays.reduce((a, s) => a + (s.completion_rate ?? 0), 0) / reviewedDays.length)
    : 0;
  return {
    studyDays: studyDays.length,
    totalPlanned,
    totalCompleted,
    totalActualHours,
    avgCompletion,
    reviewedDays: reviewedDays.length,
    days: sums,
  };
});

/** 上周已学天数展示标签（如 "5/7"） */
const prevReportDayLabel = computed(() => {
  if (!prevWeekReport.value) return "0/7";
  return `${prevWeekReport.value.studyDays}/7`;
});

/** 配置弹窗中可勾选的排除日候选（今天及之后、非休息日） */
const configExcludeCandidates = computed(() => {
  const start = currentWeekStart.value;
  const candidates: { date: string; weekday: string; isToday: boolean }[] = [];
  for (let i = 0; i < 7; i++) {
    const date = addDays(start, i);
    if (date < today) continue;
    if (isUserRestDay(date)) continue;
    candidates.push({
      date,
      weekday: weekdayShort(date),
      isToday: date === today,
    });
  }
  return candidates;
});

/** 加载上周 7 天摘要（用于配置弹窗中的上周报告） */
async function loadPrevWeekReport() {
  prevWeekReportLoading.value = true;
  try {
    const prevStart = addDays(currentWeekStart.value, -7);
    prevWeekSummaries.value = await api
      .getWeekSummaries(prevStart)
      .catch(() => [] as PlanSummary[]);
  } finally {
    prevWeekReportLoading.value = false;
  }
}

/** 打开"本周配置"弹窗（生成前） */
async function openConfigModal() {
  // 重置表单
  wlDirection.value = "unchanged";
  wlLevel.value = "small";
  wlNote.value = "";
  configExcluded.value = {};
  showConfigModal.value = true;
  // 异步加载上周报告
  loadPrevWeekReport();
}

/** 切换某日是否在生成前排除 */
function toggleConfigExclude(date: string) {
  if (configExcluded.value[date]) {
    const next = { ...configExcluded.value };
    delete next[date];
    configExcluded.value = next;
  } else {
    configExcluded.value = {
      ...configExcluded.value,
      [date]: { reason_type: "travel", note: "" },
    };
  }
}

/** 确认生成周计划（携带排除日 + 任务量调整） */
async function confirmGenerate() {
  showConfigModal.value = false;
  generating.value = true;
  error.value = null;
  try {
    const excludedDays: ExcludedDay[] = Object.entries(configExcluded.value).map(
      ([date, cfg]) => ({
        date,
        reason_type: cfg.reason_type,
        note: cfg.note.trim() || undefined,
      })
    );
    const workloadAdjustment =
      wlDirection.value === "unchanged"
        ? undefined
        : {
            direction: wlDirection.value,
            level: wlLevel.value,
            note: wlNote.value.trim() || undefined,
          };
    weekPlan.value = await api.generateWeekPlan(
      currentWeekStart.value,
      excludedDays,
      workloadAdjustment
    );
    // 重新拉取完成度数据
    weekSummaries.value = await api
      .getWeekSummaries(currentWeekStart.value)
      .catch(() => [] as PlanSummary[]);
  } catch (e) {
    error.value = e instanceof Error ? e.message : String(e);
  } finally {
    generating.value = false;
  }
}

/** 打开周中"标记为排除"对话框 */
function openExcludeDialog(date: string) {
  excludeDate.value = date;
  excludeType.value = "travel";
  excludeNote.value = "";
  showExcludeDialog.value = true;
}

/** 确认排除某日并触发重排 */
async function confirmExclude() {
  showExcludeDialog.value = false;
  regenerating.value = true;
  regenMessage.value = "正在调用 AI 调整后续计划，请勿关闭应用…";
  try {
    const excludedDay: ExcludedDay = {
      date: excludeDate.value,
      reason_type: excludeType.value,
      note: excludeNote.value.trim() || undefined,
    };
    const result = await api.addExcludedDayAndRegenerate(
      currentWeekStart.value,
      excludedDay
    );
    if (result.regenerated) {
      // 重新加载周计划与摘要以反映重排结果
      await loadWeek();
    }
    regenMessage.value = result.regenerated
      ? `已排除 ${excludeDate.value}，并重排了 ${result.affected_dates.length} 天的计划。`
      : `已排除 ${excludeDate.value}（该日已是排除日或无需重排）。`;
  } catch (e) {
    regenMessage.value = e instanceof Error ? e.message : String(e);
  } finally {
    regenerating.value = false;
  }
}

/** 取消 AI 重排（M9：超时过长且无取消机制） */
async function cancelRegeneration() {
  try {
    const found = await api.cancelAiRequest(api.AI_CANCEL_KEYS.planner);
    regenMessage.value = found
      ? "正在取消 AI 调整，请稍候…"
      : "未找到进行中的 AI 调整请求";
  } catch {
    regenMessage.value = "取消失败，请稍后再试";
  }
}

async function generateWeek() {
  await openConfigModal();
}

function openDay(date: string) {
  router.push({ name: "plan", query: { date, from: "week-plan" } });
}

watch(
  () => route.query.week,
  () => {
    loadWeek();
  }
);

onMounted(async () => {
  // 确保设置已加载（用于判断用户配置的休息日）
  if (!settingsStore.settings) {
    await settingsStore.load();
  }
  loadWeek();
});
</script>

<template>
  <div class="week-view">
    <!-- Top action bar -->
    <div class="action-bar">
      <div class="bar-left">
        <CalendarDays :size="18" class="bar-icon" />
        <div class="bar-title-wrap">
          <h1 class="bar-title">本周计划</h1>
          <span v-if="weekPlan" class="bar-range">
            {{ dateRange }}
            <span class="bar-week">· 第 {{ weekPlan.meta.week_number }} 周</span>
          </span>
          <span v-else class="bar-range">加载中…</span>
        </div>
      </div>
      <div class="bar-actions">
        <Button
          v-if="!isCurrentWeek"
          variant="ghost"
          size="sm"
          @click="backToThisWeek"
        >
          回到本周
        </Button>
        <Button variant="ghost" size="sm" icon :disabled="!canPrevWeek" :title="canPrevWeek ? '上一周' : '已到最早可浏览周'" @click="prevWeek">
          <ChevronLeft :size="16" />
        </Button>
        <Button variant="ghost" size="sm" icon :disabled="!canNextWeek" :title="canNextWeek ? '下一周' : '已到当前周'" @click="nextWeek">
          <ChevronRight :size="16" />
        </Button>
        <Button
          variant="primary"
          :loading="generating"
          :disabled="generating"
          @click="generateWeek"
        >
          <Sparkles :size="15" />
          生成本周计划
        </Button>
      </div>
    </div>

    <!-- Loading -->
    <LoadingSpinner
      v-if="loading && !weekPlan"
      :size="32"
      label="加载本周计划…"
    />

    <!-- Empty / Error -->
    <EmptyState
      v-else-if="!weekPlan"
      title="暂无周计划"
      :description="error || '生成一份本周的学习计划'"
    >
      <template #actions>
        <Button variant="primary" :loading="generating" @click="generateWeek">
          <Sparkles :size="15" />
          生成本周计划
        </Button>
      </template>
    </EmptyState>

    <!-- Content -->
    <template v-else>
      <!-- Week summary -->
      <Card padding="md" class="summary-card">
        <div class="summary-grid">
          <div class="sum-item">
            <CheckCircle2 :size="15" class="sum-icon" />
            <div class="sum-text">
              <span class="sum-value">{{ weekTotalTasks }}</span>
              <span class="sum-label">计划任务</span>
            </div>
          </div>
          <div v-if="timeTrackingEnabled" class="sum-item">
            <TrendingUp :size="15" class="sum-icon" />
            <div class="sum-text">
              <span class="sum-value">{{ weekTotalHours }}h</span>
              <span class="sum-label">计划学时</span>
            </div>
          </div>
          <div class="sum-item">
            <CalendarDays :size="15" class="sum-icon" />
            <div class="sum-text">
              <span class="sum-value">{{ weekPlannedDays }}</span>
              <span class="sum-label">已计划天数</span>
            </div>
          </div>
          <div class="sum-item">
            <CheckCircle2 :size="15" class="sum-icon" :class="completionVariant(weekCompletionRate)" />
            <div class="sum-text">
              <span class="sum-value" :class="completionVariant(weekCompletionRate)">
                {{ weekCompletionRate }}%
              </span>
              <span class="sum-label">周完成率</span>
            </div>
          </div>
          <div v-if="timeTrackingEnabled" class="sum-item">
            <Clock :size="15" class="sum-icon" />
            <div class="sum-text">
              <span class="sum-value">{{ weekActualHours.toFixed(1) }}h</span>
              <span class="sum-label">实际学时</span>
            </div>
          </div>
          <div class="sum-item">
            <Circle :size="15" class="sum-icon" />
            <div class="sum-text">
              <span class="sum-value">{{ weekCompletedTasks }}/{{ weekTotalTasks }}</span>
              <span class="sum-label">已完成任务</span>
            </div>
          </div>
        </div>
      </Card>

      <!-- Goals -->
      <div class="section-heading">
        <h2 class="section-title">
          <Target :size="15" class="title-icon" />
          本周目标
        </h2>
      </div>
      <Card padding="md" class="goals-card">
        <EmptyState
          v-if="!goals.length"
          title="暂无目标"
          description="生成周计划后将显示本周目标"
        />
        <ul v-else class="goals-list">
          <li v-for="(g, i) in goals" :key="i" class="goal-item">
            <span class="goal-index">{{ i + 1 }}</span>
            <span class="goal-text">{{ g }}</span>
          </li>
        </ul>
      </Card>

      <!-- Daily plan overview -->
      <div class="section-heading">
        <h2 class="section-title">
          <CalendarDays :size="15" class="title-icon" />
          每日计划概览
        </h2>
        <span class="section-hint">点击查看当日详情</span>
      </div>

      <div class="day-grid">
        <button
          v-for="day in weekDays"
          :key="day.date"
          type="button"
          class="day-card"
          :class="{
            today: day.isToday,
            rest: day.isRestDay,
            excluded: day.isExcluded,
            planned: day.hasPlan,
            done: day.hasReview && day.completionRate >= 100,
            empty: !day.hasPlan && !day.isRestDay && !day.isExcluded,
          }"
          @click="openDay(day.date)"
        >
          <div class="day-head">
            <span class="day-weekday">{{ day.weekday }}</span>
            <span v-if="day.isToday" class="day-today-tag">今天</span>
            <span
              v-if="canExcludeDate(day.date)"
              class="day-exclude-btn"
              role="button"
              tabindex="0"
              title="标记为特殊情况排除日"
              @click.stop="openExcludeDialog(day.date)"
              @keydown.enter.stop.prevent="openExcludeDialog(day.date)"
            >
              <Ban :size="11" />
            </span>
          </div>
          <span class="day-num">{{ day.dayNum }}</span>

          <div v-if="day.isExcluded" class="day-body">
            <span class="day-excluded-label">{{ day.excludedType ? reasonTypeLabel(day.excludedType) : '已排除' }}</span>
            <span v-if="day.excludedNote" class="day-excluded-note">{{ day.excludedNote }}</span>
          </div>

          <div v-else-if="day.isRestDay" class="day-body">
            <span class="day-rest-label">休息日</span>
          </div>

          <div v-else-if="day.hasPlan" class="day-body">
            <div class="day-stat">
              <CheckCircle2 :size="12" />
              <span>{{ day.tasksDone }}/{{ day.taskCount }}</span>
            </div>
            <div v-if="timeTrackingEnabled" class="day-hours">{{ day.hours }}h</div>
            <div v-if="day.hasReview" class="day-rate">
              <div class="rate-bar">
                <div
                  class="rate-fill"
                  :class="completionVariant(day.completionRate)"
                  :style="{ width: `${Math.min(day.completionRate, 100)}%` }"
                />
              </div>
              <span class="rate-text" :class="completionVariant(day.completionRate)">
                {{ Math.round(day.completionRate) }}%
              </span>
            </div>
            <div v-else class="day-status">
              <span class="status-pending" :class="{ 'status-future': day.date > today }">
                {{ pendingLabel(day.date) }}
              </span>
            </div>
          </div>

          <div v-else class="day-empty">
            <Plus :size="14" />
            <span>无计划</span>
          </div>
        </button>
      </div>

      <p v-if="weekPlan" class="week-foot">
        本周 {{ weekPlannedDays }} 天有计划 · 已完成任务 {{ weekCompletedTasks }}/{{ weekTotalTasks }}<template v-if="timeTrackingEnabled"> · 实际学时 {{ weekActualHours.toFixed(1) }}h</template>
        <ChevronRight :size="13" />
        点击日期卡片查看详情
      </p>
    </template>

    <!-- 重排中提示 -->
    <Card v-if="regenerating || regenMessage" padding="md" class="regen-banner" :class="{ 'regen-active': regenerating }">
      <div class="regen-content">
        <LoadingSpinner v-if="regenerating" :size="18" />
        <CheckCircle2 v-else :size="18" class="regen-done-icon" />
        <span class="regen-text">{{ regenMessage }}</span>
        <Button v-if="regenerating" variant="ghost" size="sm" class="regen-cancel-btn" @click="cancelRegeneration">
          取消
        </Button>
      </div>
    </Card>

    <!-- 周中"标记为排除"对话框 -->
    <Modal :open="showExcludeDialog" title="标记为特殊情况排除日" :width="420" @close="showExcludeDialog = false">
      <div class="exclude-form">
        <p class="exclude-hint">
          将 <strong>{{ excludeDate }}</strong>（{{ weekdayFullName(excludeDate) }}）标记为排除日后：
        </p>
        <ul class="exclude-effects">
          <li>该日不生成学习计划，AI 会把任务量分摊到本周其他学习日</li>
          <li>该日自动免复盘</li>
          <li>将触发本周剩余天数计划重新生成（AI 驱动，约需 1-3 分钟）</li>
        </ul>
        <div class="form-row">
          <label class="form-label">类型</label>
          <div class="reason-type-grid">
            <button
              v-for="opt in (['travel','sick','exam','other'] as ExcludedReasonType[])"
              :key="opt"
              type="button"
              class="reason-type-btn"
              :class="{ active: excludeType === opt }"
              @click="excludeType = opt"
            >
              <component :is="reasonTypeIconComp(opt)" :size="14" />
              <span>{{ reasonTypeLabel(opt) }}</span>
            </button>
          </div>
        </div>
        <div class="form-row">
          <label class="form-label">备注（可选）</label>
          <input
            v-model="excludeNote"
            type="text"
            class="form-input"
            placeholder="如：去XX玩、感冒发烧…"
            maxlength="100"
          />
        </div>
      </div>
      <template #footer>
        <Button variant="ghost" @click="showExcludeDialog = false">取消</Button>
        <Button variant="primary" :loading="regenerating" @click="confirmExclude">
          <Ban :size="14" />
          确认排除并重排
        </Button>
      </template>
    </Modal>

    <!-- 生成前"本周配置"弹窗 -->
    <Modal
      :open="showConfigModal"
      title="本周计划配置"
      :width="560"
      :close-on-overlay="!generating"
      :close-on-esc="!generating"
      :show-close="!generating"
      @close="!generating && (showConfigModal = false)"
    >
      <div class="config-form">
        <!-- 上周报告 -->
        <section class="config-section">
          <h4 class="config-section-title">
            <TrendingUp :size="14" />
            上周报告
          </h4>
          <LoadingSpinner v-if="prevWeekReportLoading" :size="20" label="加载上周数据…" />
          <div v-else-if="prevWeekReport" class="prev-report">
            <div class="prev-report-grid">
              <div class="prev-stat">
                <span class="prev-stat-value">{{ prevReportDayLabel }}</span>
                <span class="prev-stat-label">已学天数</span>
              </div>
              <div class="prev-stat">
                <span class="prev-stat-value">{{ prevWeekReport.totalCompleted }}/{{ prevWeekReport.totalPlanned }}</span>
                <span class="prev-stat-label">完成任务</span>
              </div>
              <div class="prev-stat">
                <span class="prev-stat-value" :class="completionVariant(prevWeekReport.avgCompletion)">{{ prevWeekReport.avgCompletion }}%</span>
                <span class="prev-stat-label">平均完成率</span>
              </div>
              <div v-if="timeTrackingEnabled" class="prev-stat">
                <span class="prev-stat-value">{{ prevWeekReport.totalActualHours.toFixed(1) }}h</span>
                <span class="prev-stat-label">实际学时</span>
              </div>
            </div>
            <div class="prev-report-days">
              <span
                v-for="d in prevWeekReport.days"
                :key="d.date"
                class="prev-day-chip"
                :class="{
                  studied: d.has_plan || d.has_review,
                  done: d.has_review && d.completion_rate >= 100,
                  rest: d.is_rest_day,
                }"
                :title="`${d.date}：${d.completed_tasks}/${d.planned_tasks} 任务${d.has_review ? '（已复盘）' : ''}`"
              >
                {{ d.date.slice(8) }}
              </span>
            </div>
          </div>
          <p v-else class="prev-report-empty">暂无上周数据（首次使用或上周未生成计划）</p>
        </section>

        <!-- 任务量调整 -->
        <section class="config-section">
          <h4 class="config-section-title">
            <Sparkles :size="14" />
            本周任务量调整（相对上周）
          </h4>
          <div class="wl-direction-grid">
            <button
              v-for="opt in (['increase','unchanged','decrease'] as WorkloadDirection[])"
              :key="opt"
              type="button"
              class="wl-direction-btn"
              :class="{ active: wlDirection === opt }"
              @click="wlDirection = opt"
            >
              {{ wlDirectionLabel(opt) }}
            </button>
          </div>
          <div v-if="wlDirection !== 'unchanged'" class="wl-level-row">
            <span class="form-label">幅度：</span>
            <button
              v-for="opt in (['small','large'] as WorkloadLevel[])"
              :key="opt"
              type="button"
              class="wl-level-btn"
              :class="{ active: wlLevel === opt }"
              @click="wlLevel = opt"
            >
              {{ opt === 'small' ? '小幅（约 20%）' : '大幅（约 40%）' }}
            </button>
          </div>
          <input
            v-model="wlNote"
            type="text"
            class="form-input"
            placeholder="备注（可选）：如上周太累、本周状态好…"
            maxlength="100"
          />
        </section>

        <!-- 排除日期 -->
        <section class="config-section">
          <h4 class="config-section-title">
            <CalendarDays :size="14" />
            特殊情况排除日期（本周不学习的日子）
          </h4>
          <p class="config-hint">勾选本周不学习的日期，AI 会把任务量分摊到其他学习日。排除日自动免复盘。</p>
          <div class="exclude-day-grid">
            <div
              v-for="day in configExcludeCandidates"
              :key="day.date"
              class="exclude-day-item"
              :class="{ active: !!configExcluded[day.date] }"
            >
              <label class="exclude-day-toggle">
                <input
                  type="checkbox"
                  :checked="!!configExcluded[day.date]"
                  @change="toggleConfigExclude(day.date)"
                />
                <span class="exclude-day-label">
                  <span class="exclude-day-date">{{ day.date.slice(5) }}</span>
                  <span class="exclude-day-weekday">{{ day.weekday }}</span>
                  <span v-if="day.isToday" class="exclude-day-today">今天</span>
                </span>
              </label>
              <template v-if="configExcluded[day.date]">
                <select
                  v-model="configExcluded[day.date].reason_type"
                  class="form-select"
                >
                  <option value="travel">外出旅行</option>
                  <option value="sick">生病</option>
                  <option value="exam">考试</option>
                  <option value="other">其他</option>
                </select>
                <input
                  v-model="configExcluded[day.date].note"
                  type="text"
                  class="form-input"
                  placeholder="备注（可选）"
                  maxlength="100"
                />
              </template>
            </div>
          </div>
        </section>
      </div>
      <template #footer>
        <Button variant="ghost" :disabled="generating" @click="showConfigModal = false">取消</Button>
        <Button variant="primary" :loading="generating" @click="confirmGenerate">
          <Sparkles :size="14" />
          生成周计划
        </Button>
      </template>
    </Modal>
  </div>
</template>

<style scoped>
.week-view {
  max-width: 920px;
  margin: 0 auto;
  padding: var(--space-8);
  display: flex;
  flex-direction: column;
  gap: var(--space-4);
}

/* Action bar */
.action-bar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--space-4);
  padding: var(--space-2) 0 var(--space-2);
}

.bar-left {
  display: flex;
  align-items: center;
  gap: var(--space-3);
}

.bar-icon {
  color: var(--accent);
  flex-shrink: 0;
}

.bar-title-wrap {
  display: flex;
  flex-direction: column;
  line-height: 1.3;
}

.bar-title {
  font-size: var(--text-xl);
  font-weight: var(--font-bold);
  color: var(--text-primary);
  letter-spacing: -0.01em;
}

.bar-range {
  font-size: var(--text-sm);
  color: var(--text-tertiary);
}

.bar-week {
  color: var(--text-quaternary);
}

.bar-actions {
  display: inline-flex;
  align-items: center;
  gap: var(--space-2);
  flex-shrink: 0;
}

/* Summary card */
.summary-card {
  display: flex;
}

.summary-grid {
  display: flex;
  align-items: center;
  gap: var(--space-4);
  width: 100%;
}

.sum-item {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  flex: 1;
}

.sum-icon {
  color: var(--accent);
  flex-shrink: 0;
}

.sum-text {
  display: flex;
  flex-direction: column;
  line-height: 1.2;
}

.sum-value {
  font-size: var(--text-lg);
  font-weight: var(--font-bold);
  color: var(--text-primary);
}

.sum-label {
  font-size: var(--text-xs);
  color: var(--text-tertiary);
}

/* Section heading */
.section-heading {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  padding: var(--space-2) var(--space-1) 0;
}

.section-title {
  display: inline-flex;
  align-items: center;
  gap: var(--space-2);
  font-size: var(--text-sm);
  font-weight: var(--font-semibold);
  color: var(--text-secondary);
  text-transform: uppercase;
  letter-spacing: 0.04em;
}

.title-icon {
  color: var(--text-tertiary);
}

.section-hint {
  font-size: var(--text-xs);
  color: var(--text-quaternary);
}

/* Goals */
.goals-card {
  display: flex;
  flex-direction: column;
}

.goals-list {
  list-style: none;
  margin: 0;
  padding: 0;
  display: flex;
  flex-direction: column;
  gap: var(--space-3);
}

.goal-item {
  display: flex;
  align-items: center;
  gap: var(--space-3);
}

.goal-index {
  width: 22px;
  height: 22px;
  display: flex;
  align-items: center;
  justify-content: center;
  background: var(--accent-subtle);
  color: var(--accent);
  border-radius: var(--radius-full);
  font-size: var(--text-xs);
  font-weight: var(--font-semibold);
  flex-shrink: 0;
}

.goal-text {
  font-size: var(--text-sm);
  color: var(--text-primary);
  line-height: var(--leading-normal);
}

/* Day grid */
.day-grid {
  display: grid;
  grid-template-columns: repeat(7, 1fr);
  gap: var(--space-2);
}

.day-card {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: var(--space-2);
  padding: var(--space-3) var(--space-2);
  background: var(--bg-elevated);
  border: 1px solid transparent;
  border-radius: var(--radius-md);
  box-shadow: var(--shadow-xs);
  cursor: pointer;
  transition: all var(--transition-fast);
  font-family: inherit;
  min-width: 0;
}

.day-card:hover {
  box-shadow: var(--shadow-md);
  transform: translateY(-1px);
  border-color: var(--border-color);
}

.day-card.today {
  border-color: var(--accent);
  background: var(--accent-subtle);
}

.day-card.done {
  border-color: var(--color-success);
}

.day-card.rest {
  border-color: var(--border-color);
  background: var(--bg-tertiary);
}

.day-card.planned {
  border-color: var(--accent);
}

.day-card.empty {
  opacity: 0.6;
}

.day-head {
  display: flex;
  align-items: center;
  gap: var(--space-1);
  width: 100%;
  justify-content: center;
}

.day-weekday {
  font-size: var(--text-xs);
  color: var(--text-tertiary);
  font-weight: var(--font-medium);
}

.day-today-tag {
  font-size: 9px;
  color: var(--accent);
  background: var(--bg-elevated);
  padding: 1px 5px;
  border-radius: var(--radius-full);
  font-weight: var(--font-semibold);
}

.day-num {
  font-size: var(--text-lg);
  font-weight: var(--font-bold);
  color: var(--text-primary);
  letter-spacing: -0.02em;
}

.day-body {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: var(--space-1);
  width: 100%;
}

.day-stat {
  display: inline-flex;
  align-items: center;
  gap: 2px;
  font-size: var(--text-xs);
  color: var(--text-secondary);
}

.day-stat svg {
  color: var(--text-tertiary);
}

.day-rate {
  width: 100%;
  margin-top: 2px;
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 2px;
}

.rate-bar {
  width: 100%;
  height: 3px;
  background: var(--bg-tertiary);
  border-radius: var(--radius-full);
  overflow: hidden;
}

.rate-fill {
  height: 100%;
  border-radius: var(--radius-full);
  transition: width var(--transition-base);
}

.rate-fill.success {
  background: var(--color-success, #10b981);
}

.rate-fill.warning {
  background: var(--color-warning, #f59e0b);
}

.rate-fill.danger {
  background: var(--color-danger, #ef4444);
}

.rate-text {
  font-size: 10px;
  font-weight: var(--font-semibold);
  line-height: 1;
}

.rate-text.success {
  color: var(--color-success, #10b981);
}

.rate-text.warning {
  color: var(--color-warning, #f59e0b);
}

.rate-text.danger {
  color: var(--color-danger, #ef4444);
}

.day-hours {
  font-size: var(--text-xs);
  color: var(--accent);
  font-weight: var(--font-medium);
}

.day-rest-label {
  font-size: var(--text-xs);
  color: var(--text-quaternary);
  font-weight: var(--font-medium);
}

.day-status {
  margin-top: 2px;
  font-size: 10px;
}

.status-done {
  color: var(--color-success);
}

.status-pending {
  color: var(--text-quaternary);
}

.status-pending.status-future {
  color: var(--text-tertiary);
}

/* Completion variant colors (used by summary icons/values too) */
.sum-icon.success,
.sum-value.success {
  color: var(--color-success, #10b981);
}

.sum-icon.warning,
.sum-value.warning {
  color: var(--color-warning, #f59e0b);
}

.sum-icon.danger,
.sum-value.danger {
  color: var(--color-danger, #ef4444);
}

.day-empty {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 2px;
  font-size: var(--text-xs);
  color: var(--text-quaternary);
  padding: var(--space-2) 0;
}

.day-empty svg {
  color: var(--text-quaternary);
}

/* Week foot */
.week-foot {
  display: flex;
  align-items: center;
  justify-content: center;
  gap: var(--space-1);
  font-size: var(--text-xs);
  color: var(--text-tertiary);
  margin: var(--space-2) 0 0;
}

@media (max-width: 760px) {
  .day-grid {
    grid-template-columns: repeat(4, 1fr);
  }
}

@media (max-width: 480px) {
  .day-grid {
    grid-template-columns: repeat(2, 1fr);
  }
  .summary-grid {
    flex-direction: column;
    align-items: flex-start;
    gap: var(--space-3);
  }
}

/* ── Day exclude button ── */
.day-exclude-btn {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 16px;
  height: 16px;
  border-radius: var(--radius-full);
  color: var(--text-quaternary);
  cursor: pointer;
  transition: all var(--transition-fast);
  flex-shrink: 0;
}

.day-exclude-btn:hover {
  color: var(--color-danger, #ef4444);
  background: var(--bg-tertiary);
}

/* ── Day excluded state ── */
.day-excluded-label {
  font-size: var(--text-xs);
  color: var(--color-danger, #ef4444);
  font-weight: var(--font-medium);
}

.day-excluded-note {
  font-size: 10px;
  color: var(--text-quaternary);
  line-height: 1.2;
  text-align: center;
  word-break: break-all;
}

/* ── Regen banner ── */
.regen-banner {
  position: sticky;
  bottom: var(--space-4);
  z-index: 10;
  border-color: var(--accent);
}

.regen-banner.regen-active {
  border-color: var(--accent);
  background: var(--accent-subtle);
}

.regen-content {
  display: flex;
  align-items: center;
  gap: var(--space-3);
}

.regen-done-icon {
  color: var(--color-success, #10b981);
  flex-shrink: 0;
}

.regen-text {
  font-size: var(--text-sm);
  color: var(--text-primary);
  line-height: var(--leading-normal);
}

.regen-cancel-btn {
  margin-left: auto;
  flex-shrink: 0;
}

/* ── Form elements (shared) ── */
.form-row {
  display: flex;
  flex-direction: column;
  gap: var(--space-2);
  margin-bottom: var(--space-3);
}

.form-label {
  font-size: var(--text-xs);
  font-weight: var(--font-medium);
  color: var(--text-secondary);
}

.form-input,
.form-select {
  width: 100%;
  padding: var(--space-2) var(--space-3);
  border: 1px solid var(--border-color);
  border-radius: var(--radius-sm);
  background: var(--bg-primary);
  color: var(--text-primary);
  font-size: var(--text-sm);
  font-family: inherit;
  transition: border-color var(--transition-fast);
}

.form-input:focus,
.form-select:focus {
  outline: none;
  border-color: var(--accent);
}

.form-input::placeholder {
  color: var(--text-quaternary);
}

/* ── Exclude dialog ── */
.exclude-form {
  display: flex;
  flex-direction: column;
  gap: var(--space-3);
}

.exclude-hint {
  font-size: var(--text-sm);
  color: var(--text-secondary);
  margin: 0;
}

.exclude-effects {
  margin: 0;
  padding-left: var(--space-5);
  font-size: var(--text-xs);
  color: var(--text-tertiary);
  line-height: var(--leading-relaxed);
}

.exclude-effects li {
  margin-bottom: 2px;
}

.reason-type-grid {
  display: grid;
  grid-template-columns: repeat(2, 1fr);
  gap: var(--space-2);
}

.reason-type-btn {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  gap: var(--space-2);
  padding: var(--space-2) var(--space-3);
  border: 1px solid var(--border-color);
  border-radius: var(--radius-sm);
  background: var(--bg-primary);
  color: var(--text-secondary);
  font-size: var(--text-sm);
  font-family: inherit;
  cursor: pointer;
  transition: all var(--transition-fast);
}

.reason-type-btn:hover {
  border-color: var(--accent);
  color: var(--accent);
}

.reason-type-btn.active {
  border-color: var(--accent);
  background: var(--accent-subtle);
  color: var(--accent);
  font-weight: var(--font-semibold);
}

/* ── Config modal ── */
.config-form {
  display: flex;
  flex-direction: column;
  gap: var(--space-4);
}

.config-section {
  display: flex;
  flex-direction: column;
  gap: var(--space-2);
}

.config-section-title {
  display: inline-flex;
  align-items: center;
  gap: var(--space-2);
  font-size: var(--text-sm);
  font-weight: var(--font-semibold);
  color: var(--text-primary);
  margin: 0;
}

.config-section-title svg {
  color: var(--accent);
}

.config-hint {
  font-size: var(--text-xs);
  color: var(--text-tertiary);
  margin: 0;
}

/* ── Prev week report ── */
.prev-report {
  display: flex;
  flex-direction: column;
  gap: var(--space-3);
}

.prev-report-grid {
  display: grid;
  grid-template-columns: repeat(4, 1fr);
  gap: var(--space-2);
}

.prev-stat {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 2px;
  padding: var(--space-2);
  background: var(--bg-secondary);
  border-radius: var(--radius-sm);
}

.prev-stat-value {
  font-size: var(--text-base);
  font-weight: var(--font-bold);
  color: var(--text-primary);
}

.prev-stat-value.success {
  color: var(--color-success, #10b981);
}

.prev-stat-value.warning {
  color: var(--color-warning, #f59e0b);
}

.prev-stat-value.danger {
  color: var(--color-danger, #ef4444);
}

.prev-stat-label {
  font-size: 10px;
  color: var(--text-tertiary);
}

.prev-report-days {
  display: flex;
  gap: 4px;
  flex-wrap: wrap;
}

.prev-day-chip {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  min-width: 28px;
  height: 24px;
  padding: 0 6px;
  border-radius: var(--radius-sm);
  font-size: var(--text-xs);
  font-weight: var(--font-medium);
  background: var(--bg-tertiary);
  color: var(--text-quaternary);
}

.prev-day-chip.studied {
  background: var(--accent-subtle);
  color: var(--accent);
}

.prev-day-chip.done {
  background: var(--color-success, #10b981);
  color: #fff;
}

.prev-day-chip.rest {
  opacity: 0.5;
}

.prev-report-empty {
  font-size: var(--text-xs);
  color: var(--text-quaternary);
  margin: 0;
  text-align: center;
  padding: var(--space-3) 0;
}

/* ── Workload adjustment ── */
.wl-direction-grid {
  display: grid;
  grid-template-columns: repeat(3, 1fr);
  gap: var(--space-2);
}

.wl-direction-btn {
  padding: var(--space-2) var(--space-3);
  border: 1px solid var(--border-color);
  border-radius: var(--radius-sm);
  background: var(--bg-primary);
  color: var(--text-secondary);
  font-size: var(--text-sm);
  font-family: inherit;
  cursor: pointer;
  transition: all var(--transition-fast);
  text-align: center;
}

.wl-direction-btn:hover {
  border-color: var(--accent);
  color: var(--accent);
}

.wl-direction-btn.active {
  border-color: var(--accent);
  background: var(--accent-subtle);
  color: var(--accent);
  font-weight: var(--font-semibold);
}

.wl-level-row {
  display: inline-flex;
  align-items: center;
  gap: var(--space-2);
}

.wl-level-btn {
  padding: var(--space-1) var(--space-3);
  border: 1px solid var(--border-color);
  border-radius: var(--radius-sm);
  background: var(--bg-primary);
  color: var(--text-secondary);
  font-size: var(--text-xs);
  font-family: inherit;
  cursor: pointer;
  transition: all var(--transition-fast);
}

.wl-level-btn:hover {
  border-color: var(--accent);
  color: var(--accent);
}

.wl-level-btn.active {
  border-color: var(--accent);
  background: var(--accent-subtle);
  color: var(--accent);
  font-weight: var(--font-semibold);
}

/* ── Exclude day grid (config modal) ── */
.exclude-day-grid {
  display: flex;
  flex-direction: column;
  gap: var(--space-2);
}

.exclude-day-item {
  display: flex;
  flex-direction: column;
  gap: var(--space-2);
  padding: var(--space-2) var(--space-3);
  border: 1px solid var(--border-color);
  border-radius: var(--radius-sm);
  transition: border-color var(--transition-fast);
}

.exclude-day-item.active {
  border-color: var(--color-danger, #ef4444);
  background: var(--bg-secondary);
}

.exclude-day-toggle {
  display: inline-flex;
  align-items: center;
  gap: var(--space-2);
  cursor: pointer;
}

.exclude-day-toggle input[type="checkbox"] {
  width: 16px;
  height: 16px;
  cursor: pointer;
  accent-color: var(--color-danger, #ef4444);
}

.exclude-day-label {
  display: inline-flex;
  align-items: center;
  gap: var(--space-2);
  font-size: var(--text-sm);
  color: var(--text-primary);
}

.exclude-day-date {
  font-weight: var(--font-medium);
}

.exclude-day-weekday {
  font-size: var(--text-xs);
  color: var(--text-tertiary);
}

.exclude-day-today {
  font-size: 9px;
  color: var(--accent);
  background: var(--accent-subtle);
  padding: 1px 5px;
  border-radius: var(--radius-full);
  font-weight: var(--font-semibold);
}
</style>
