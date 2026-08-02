<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import { useRouter } from "vue-router";
import * as api from "@/api";
import { todayString } from "@/utils/date";
import { useSettingsStore } from "@/stores/settings";
import Button from "@/components/ui/Button.vue";
import LoadingSpinner from "@/components/ui/LoadingSpinner.vue";
import EmptyState from "@/components/ui/EmptyState.vue";
import { CalendarClock, ChevronRight, CheckCircle2, Circle, Clock } from "lucide-vue-next";
import type { PlanSummary } from "@/types";

const router = useRouter();
const settingsStore = useSettingsStore();

const summaries = ref<PlanSummary[]>([]);
const loading = ref(false);
const error = ref<string | null>(null);

const today = todayString();

// 是否启用「记录学习时长」：关闭时隐藏实际学时展示
const timeTrackingEnabled = computed(
  () => !!settingsStore.settings?.study_schedule?.enable_time_tracking
);

const weekdayLabels = ["周一", "周二", "周三", "周四", "周五", "周六", "周日"];

interface DayCell {
  date: string;
  item: PlanSummary | null;
  isPadding: boolean; // 是否属于相邻月份，用于灰显
}

interface WeekGroup {
  weekStart: string; // 周一日期，用作 key
  days: DayCell[];
}

interface MonthGroup {
  month: string;
  weeks: WeekGroup[];
}

/** 根据日期字符串返回所在周的周一日期（YYYY-MM-DD） */
function getWeekStart(dateStr: string): string {
  const [y, m, d] = dateStr.split("-").map(Number);
  const date = new Date(y, m - 1, d, 12, 0, 0);
  const day = date.getDay();
  // 周一作为一周起点：周日(0) -> -6，其他(1-6) -> 1-day
  const diff = (day === 0 ? -6 : 1) - day;
  date.setDate(date.getDate() + diff);
  const yy = date.getFullYear();
  const mm = String(date.getMonth() + 1).padStart(2, "0");
  const dd = String(date.getDate()).padStart(2, "0");
  return `${yy}-${mm}-${dd}`;
}

function parseDate(dateStr: string): Date {
  const [y, m, d] = dateStr.split("-").map(Number);
  return new Date(y, m - 1, d, 12, 0, 0);
}

function addDays(date: Date, n: number): Date {
  const copy = new Date(date);
  copy.setDate(copy.getDate() + n);
  return copy;
}

function formatDate(d: Date): string {
  const yy = d.getFullYear();
  const mm = String(d.getMonth() + 1).padStart(2, "0");
  const dd = String(d.getDate()).padStart(2, "0");
  return `${yy}-${mm}-${dd}`;
}

const groupedSummaries = computed<MonthGroup[]>(() => {
  const dateMap = new Map<string, PlanSummary>();
  for (const s of summaries.value) {
    dateMap.set(s.date, s);
  }

  // 1. 按月份分组
  const monthMap = new Map<string, PlanSummary[]>();
  for (const s of summaries.value) {
    const month = s.date.slice(0, 7); // YYYY-MM
    if (!monthMap.has(month)) monthMap.set(month, []);
    monthMap.get(month)!.push(s);
  }

  const result: MonthGroup[] = [];
  // 月份降序：最新月份在上
  const sortedMonths = Array.from(monthMap.keys()).sort().reverse();
  for (const month of sortedMonths) {
    const [y, m] = month.split("-").map(Number);
    const firstDay = new Date(y, m - 1, 1, 12, 0, 0);
    const lastDay = new Date(y, m, 0, 12, 0, 0);

    const firstMonday = parseDate(getWeekStart(formatDate(firstDay)));
    const lastMonday = parseDate(getWeekStart(formatDate(lastDay)));

    const weeks: WeekGroup[] = [];
    let currentMonday = firstMonday;
    while (currentMonday <= lastMonday) {
      const days: DayCell[] = [];
      for (let i = 0; i < 7; i++) {
        const d = addDays(currentMonday, i);
        const dateStr = formatDate(d);
        const item = dateMap.get(dateStr) ?? null;
        days.push({
          date: dateStr,
          item,
          isPadding: dateStr.slice(0, 7) !== month,
        });
      }
      weeks.push({
        weekStart: formatDate(currentMonday),
        days,
      });
      currentMonday = addDays(currentMonday, 7);
    }

    // 周降序：最新周在上；但每行内部日期已经是周一→周日升序
    weeks.reverse();

    result.push({ month, weeks });
  }
  return result;
});

function monthLabel(monthStr: string): string {
  const [y, m] = monthStr.split("-");
  return `${y}年${parseInt(m, 10)}月`;
}

function weekday(dateStr: string): string {
  const weekdays = ["周日", "周一", "周二", "周三", "周四", "周五", "周六"];
  const d = new Date(`${dateStr}T12:00:00`);
  return weekdays[d.getDay()];
}

function dayNum(dateStr: string): string {
  return dateStr.split("-")[2]?.replace(/^0/, "") ?? "";
}

function completionVariant(rate: number): string {
  if (rate >= 100) return "success";
  if (rate >= 50) return "warning";
  return "danger";
}

/** 排除日类型 → 中文标签 */
function excludedTypeLabel(type?: string): string {
  switch (type) {
    case "travel":
      return "出差/出行";
    case "sick":
      return "生病";
    case "exam":
      return "考试";
    case "other":
      return "其它";
    default:
      return "特殊情况";
  }
}

/** 排除日完整描述（类型 + 备注） */
function excludedDescription(item: PlanSummary): string {
  const typeLabel = excludedTypeLabel(item.excluded_type);
  const note = item.excluded_note?.trim();
  return note ? `${typeLabel}：${note}` : typeLabel;
}

/** 根据日期与复盘状态返回展示文本与样式类。
 *  - 已复盘：显示完成率
 *  - 未复盘且日期在今天及之前：显示「未复盘」（danger 样式）
 *  - 未复盘且日期在今天之后：显示「未开始」（neutral 样式） */
function reviewStatus(dateStr: string, hasReview: boolean, rate: number): { text: string; cls: string } {
  if (hasReview) {
    return { text: `${Math.round(rate)}%`, cls: completionVariant(rate) };
  }
  if (dateStr > today) {
    return { text: "未开始", cls: "neutral" };
  }
  return { text: "未复盘", cls: "danger" };
}

function openPlan(date: string) {
  router.push({ name: "plan", query: { date, from: "history" } });
}

async function load() {
  loading.value = true;
  error.value = null;
  try {
    summaries.value = await api.listPlanSummaries();
  } catch (e) {
    error.value = e instanceof Error ? e.message : String(e);
  } finally {
    loading.value = false;
  }
}

onMounted(() => {
  load();
});
</script>

<template>
  <div class="history-view">
    <div class="page-header">
      <div class="header-main">
        <CalendarClock :size="20" class="header-icon" />
        <div>
          <h1 class="page-title">历史计划</h1>
          <p class="page-subtitle">浏览并查看以往生成的学习计划</p>
        </div>
      </div>
      <Button variant="ghost" size="sm" :loading="loading" @click="load">
        刷新
      </Button>
    </div>

    <LoadingSpinner v-if="loading && groupedSummaries.length === 0" :size="32" label="加载历史计划…" />

    <EmptyState
      v-else-if="groupedSummaries.length === 0"
      title="暂无历史计划"
      :description="error || '生成计划后将在这里集中展示'"
    >
      <template #actions>
        <Button variant="primary" @click="router.push({ name: 'plan' })">
          去生成计划
        </Button>
      </template>
    </EmptyState>

    <template v-else>
      <div v-for="group in groupedSummaries" :key="group.month" class="month-group">
        <h2 class="month-title">{{ monthLabel(group.month) }}</h2>

        <!-- 星期表头 -->
        <div class="weekday-header">
          <div v-for="label in weekdayLabels" :key="label" class="weekday-label">
            {{ label }}
          </div>
        </div>

        <div class="month-weeks">
          <div
            v-for="week in group.weeks"
            :key="week.weekStart"
            class="week-row"
          >
            <div class="date-grid">
              <template v-for="day in week.days" :key="day.date">
                <button
                  v-if="day.item && !day.isPadding"
                  type="button"
                  class="date-card"
                  :class="{
                    today: day.date === today,
                    rest: day.item.is_rest_day,
                    excluded: day.item.is_excluded && !day.item.is_rest_day,
                    done: !day.item.is_rest_day && !day.item.is_excluded && day.item.has_review && day.item.completion_rate >= 100,
                    pending: !day.item.is_rest_day && !day.item.is_excluded && day.item.has_review && day.item.completion_rate < 100,
                    padding: day.isPadding,
                  }"
                  :title="day.item.is_excluded ? excludedDescription(day.item) : undefined"
                  @click="openPlan(day.date)"
                >
                  <!-- 完成角标 -->
                  <span
                    v-if="!day.item.is_rest_day && !day.item.is_excluded && day.item.has_review && day.item.completion_rate >= 100"
                    class="done-badge"
                  >
                    <CheckCircle2 :size="14" />
                  </span>

                  <div class="date-main">
                    <span class="date-day">{{ dayNum(day.date) }}</span>
                    <span class="date-weekday">{{ weekday(day.date) }}</span>
                  </div>

                  <div class="date-stats">
                    <span v-if="day.item.is_rest_day" class="rest-badge">休息日</span>
                    <template v-else-if="day.item.is_excluded">
                      <span class="excluded-badge">{{ excludedTypeLabel(day.item.excluded_type) }}</span>
                      <span v-if="day.item.excluded_note?.trim()" class="excluded-note">{{ day.item.excluded_note }}</span>
                    </template>
                    <template v-else>
                      <div class="stat-row">
                        <span
                          class="stat-value rate-text"
                          :class="reviewStatus(day.date, day.item.has_review, day.item.completion_rate).cls"
                        >
                          {{ reviewStatus(day.date, day.item.has_review, day.item.completion_rate).text }}
                        </span>
                      </div>
                      <div class="stat-row">
                        <Circle :size="11" class="stat-icon" />
                        <span class="stat-value">{{ day.item.completed_tasks }}/{{ day.item.planned_tasks }}</span>
                      </div>
                      <div v-if="timeTrackingEnabled" class="stat-row">
                        <Clock :size="11" class="stat-icon" />
                        <span class="stat-value">{{ day.item.actual_hours.toFixed(1) }}h</span>
                      </div>
                    </template>
                  </div>

                  <div class="date-meta">
                    <span v-if="day.date === today" class="today-badge">今天</span>
                    <ChevronRight :size="14" class="date-arrow" />
                  </div>
                </button>

                <div
                  v-else
                  class="date-cell-empty"
                  :class="{ padding: day.isPadding, today: day.date === today && !day.isPadding }"
                >
                  <span class="empty-day">{{ dayNum(day.date) }}</span>
                </div>
              </template>
            </div>
          </div>
        </div>
      </div>
    </template>
  </div>
</template>

<style scoped>
.history-view {
  max-width: 900px;
  margin: 0 auto;
  padding: var(--space-8);
  display: flex;
  flex-direction: column;
  gap: var(--space-6);
}

.page-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--space-4);
}

.header-main {
  display: flex;
  align-items: center;
  gap: var(--space-3);
}

.header-icon {
  color: var(--accent);
  flex-shrink: 0;
}

.page-title {
  font-size: var(--text-xl);
  font-weight: var(--font-bold);
  color: var(--text-primary);
  letter-spacing: -0.01em;
}

.page-subtitle {
  font-size: var(--text-sm);
  color: var(--text-secondary);
  margin: 0;
}

.month-group {
  display: flex;
  flex-direction: column;
  gap: var(--space-3);
}

.month-title {
  font-size: var(--text-base);
  font-weight: var(--font-semibold);
  color: var(--text-primary);
  padding: 0 var(--space-1);
  text-align: center;
}

/* ── 星期表头 ── */
.weekday-header {
  display: grid;
  grid-template-columns: repeat(7, 1fr);
  gap: var(--space-2);
  margin-bottom: var(--space-2);
}

.weekday-label {
  text-align: center;
  font-size: var(--text-xs);
  font-weight: var(--font-semibold);
  color: var(--text-tertiary);
  padding: var(--space-1);
}

.month-weeks {
  display: flex;
  flex-direction: column;
  gap: var(--space-2);
}

.week-row {
  /* 每周一行，行内日期卡片横向排列 */
}

.date-grid {
  display: grid;
  /* 固定 7 列，像日历一样 */
  grid-template-columns: repeat(7, 1fr);
  gap: var(--space-2);
}

.date-cell-empty {
  display: flex;
  align-items: flex-start;
  justify-content: center;
  padding: var(--space-3);
  min-height: 80px;
  border-radius: var(--radius-md);
  background: transparent;
  border: 1px solid transparent;
  color: var(--text-quaternary);
}

.date-cell-empty.padding {
  color: var(--text-quaternary);
  opacity: 0.6;
}

.date-cell-empty.today {
  border-color: var(--accent);
  background: var(--accent-subtle);
}

.empty-day {
  font-size: var(--text-sm);
  font-weight: var(--font-medium);
}

.date-card {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: var(--space-2);
  padding: var(--space-3);
  background: var(--bg-elevated);
  border: 1px solid transparent;
  border-radius: var(--radius-md);
  box-shadow: var(--shadow-xs);
  cursor: pointer;
  transition: all var(--transition-fast);
  font-family: inherit;
  text-align: center;
  min-width: 0;
}

.date-card:hover {
  box-shadow: var(--shadow-md);
  transform: translateY(-1px);
  border-color: var(--border-color);
}

.date-card.today {
  border-color: var(--accent);
  background: var(--accent-subtle);
}

.date-card.rest {
  background: var(--bg-secondary);
}

.date-card.excluded {
  background: var(--bg-secondary);
  border-color: var(--color-warning, #f59e0b);
  border-style: dashed;
}

.date-card.done {
  border-color: var(--color-success, #10b981);
  background: linear-gradient(135deg, rgba(16, 185, 129, 0.08), rgba(16, 185, 129, 0.02));
  box-shadow: 0 0 0 1px var(--color-success, #10b981), var(--shadow-sm);
}

.date-card.pending {
  border-color: var(--border-color);
}

/* 完成角标 */
.done-badge {
  position: absolute;
  top: 6px;
  right: 6px;
  display: flex;
  align-items: center;
  justify-content: center;
  width: 20px;
  height: 20px;
  border-radius: 50%;
  background: var(--color-success, #10b981);
  color: #fff;
  box-shadow: 0 1px 3px rgba(16, 185, 129, 0.4);
}

.rate-text {
  font-size: var(--text-sm);
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

.rate-text.neutral {
  color: var(--text-tertiary);
}

.date-card {
  position: relative;
}

.date-main {
  display: flex;
  align-items: baseline;
  justify-content: center;
  gap: var(--space-2);
}

.date-day {
  font-size: var(--text-2xl);
  font-weight: var(--font-bold);
  color: var(--text-primary);
  letter-spacing: -0.02em;
  line-height: 1;
}

.date-weekday {
  font-size: var(--text-xs);
  color: var(--text-tertiary);
  font-weight: var(--font-medium);
}

.date-stats {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 3px;
  font-size: var(--text-xs);
}

.stat-row {
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 4px;
  color: var(--text-tertiary);
}

.stat-icon {
  color: var(--text-quaternary);
  flex-shrink: 0;
}

.stat-value {
  color: var(--text-secondary);
  font-weight: var(--font-medium);
}

.stat-value.success {
  color: var(--color-success, #10b981);
}

.stat-value.warning {
  color: var(--color-warning, #f59e0b);
}

.stat-value.danger {
  color: var(--color-danger, #ef4444);
}

.rest-badge {
  display: inline-block;
  font-size: 10px;
  font-weight: var(--font-semibold);
  color: var(--text-tertiary);
  background: var(--bg-tertiary);
  padding: 2px 6px;
  border-radius: var(--radius-full);
  align-self: center;
}

.excluded-badge {
  display: inline-block;
  font-size: 10px;
  font-weight: var(--font-semibold);
  color: #fff;
  background: var(--color-warning, #f59e0b);
  padding: 2px 6px;
  border-radius: var(--radius-full);
  align-self: center;
}

.excluded-note {
  display: block;
  margin-top: 2px;
  font-size: 10px;
  color: var(--text-tertiary);
  max-width: 90px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  line-height: 1.2;
}

.date-meta {
  display: flex;
  align-items: center;
  justify-content: center;
  gap: var(--space-1);
  margin-top: var(--space-1);
}

.today-badge {
  font-size: 10px;
  font-weight: var(--font-semibold);
  color: var(--accent);
  background: var(--bg-elevated);
  padding: 2px 6px;
  border-radius: var(--radius-full);
}

.date-arrow {
  color: var(--text-quaternary);
  margin-left: auto;
}
</style>
