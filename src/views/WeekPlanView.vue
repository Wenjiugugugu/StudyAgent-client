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
} from "lucide-vue-next";
import type { WeekPlan, SubjectKey, PlanSummary } from "@/types";

const router = useRouter();
const route = useRoute();
const settingsStore = useSettingsStore();

const weekPlan = ref<WeekPlan | null>(null);
const weekSummaries = ref<PlanSummary[]>([]);
const loading = ref(false);
const generating = ref(false);
const error = ref<string | null>(null);

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
  hours: number;
  taskCount: number;
  isToday: boolean;
  completionRate: number;
  actualHours: number;
  tasksDone: number;
  hasReview: boolean;
}

const weekDays = computed<DayInfo[]>(() => {
  const wp = weekPlan.value;
  if (!wp) return [];
  const start = wp.meta.week_start;
  const summaries = weekSummaries.value;
  const days: DayInfo[] = [];
  for (let i = 0; i < 7; i++) {
    const date = addDays(start, i);
    const dayPlan = wp.data.days.find((d) => d.date === date);
    const summary = summaries.find((s) => s.date === date);
    const allocations = dayPlan?.subject_allocations ?? [];

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

async function generateWeek() {
  generating.value = true;
  error.value = null;
  try {
    weekPlan.value = await api.generateWeekPlan(currentWeekStart.value);
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
            planned: day.hasPlan,
            done: day.hasReview && day.completionRate >= 100,
            empty: !day.hasPlan && !day.isRestDay,
          }"
          @click="openDay(day.date)"
        >
          <div class="day-head">
            <span class="day-weekday">{{ day.weekday }}</span>
            <span v-if="day.isToday" class="day-today-tag">今天</span>
          </div>
          <span class="day-num">{{ day.dayNum }}</span>

          <div v-if="day.isRestDay" class="day-body">
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
</style>
