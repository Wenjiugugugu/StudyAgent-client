<script setup lang="ts">
import { computed, onMounted, onBeforeUnmount, ref } from "vue";
import { useRouter } from "vue-router";
import { useDashboardStore } from "@/stores/dashboard";
import { useSettingsStore } from "@/stores/settings";
import { useTodayStore } from "@/stores/today";
import { todayString, currentHourShanghai, currentMinutesShanghai, timeStringToMinutes, daysBetween } from "@/utils/date";
import * as api from "@/api";
import Card from "@/components/ui/Card.vue";
import Badge from "@/components/ui/Badge.vue";
import Button from "@/components/ui/Button.vue";
import ProgressBar from "@/components/ui/ProgressBar.vue";
import LoadingSpinner from "@/components/ui/LoadingSpinner.vue";
import EmptyState from "@/components/ui/EmptyState.vue";
import {
  Calendar,
  Sparkles,
  TrendingUp,
  Target,
  RefreshCw,
  ChevronRight,
  Clock,
  Flag,
  Zap,
  Award,
} from "lucide-vue-next";
import type { DashboardSummary, PlanTask, PlanSummary, SubjectKey } from "@/types";

const router = useRouter();
const dashboardStore = useDashboardStore();
const settingsStore = useSettingsStore();
const todayStore = useTodayStore();

const summary = computed<DashboardSummary | null>(() => dashboardStore.summary);
const todayDateStr = todayString();

// 本周每日摘要（与周计划页同源，保证进度口径一致）
const weekSummaries = ref<PlanSummary[]>([]);

// ── Hero ──
const greeting = computed(() => {
  const h = currentHourShanghai();
  if (h >= 5 && h < 12) return "早上好";
  if (h >= 12 && h < 18) return "下午好";
  if (h >= 18 && h < 22) return "晚上好";
  return "夜深了";
});

const displayName = computed(() => {
  const name = settingsStore.settings?.user_name?.trim();
  return name || "";
});

const dateLabel = computed(() => {
  const d = new Date();
  return new Intl.DateTimeFormat("zh-CN", {
    timeZone: "Asia/Shanghai",
    year: "numeric",
    month: "long",
    day: "numeric",
    weekday: "long",
  }).format(d);
});

function computeDaysToExam(): number {
  const examDate = settingsStore.settings?.exam_date;
  if (!examDate) return 0;
  return daysBetween(examDate, todayDateStr);
}

const remainingDays = computed(() => computeDaysToExam());

// ── Today Focus ──
const focusTask = computed<PlanTask | null>(() => {
  const tasks = todayStore.allTasks;
  if (tasks.length === 0) return null;
  const active = tasks.find((t) => t.priority === "A" && t.status !== "done");
  return active ?? tasks.find((t) => t.status !== "done") ?? tasks[0];
});

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
  () => settingsStore.settings?.study_schedule?.start_time ?? "09:00",
);

async function generateTodayPlan() {
  await todayStore.generate();
}

// ── Week Progress ──
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

// 完成率口径与周计划页保持一致：已复盘日的 completionRate 平均值
const weekPercent = computed(() => {
  const reviewed = weekSummaries.value.filter((d) => d.has_review);
  if (!reviewed.length) return 0;
  const sum = reviewed.reduce((s, d) => s + d.completion_rate, 0);
  return Math.round(sum / reviewed.length);
});

// 整周计划完成进度：已学习天数 / 计划学习天数（推进度）
const weekPlanProgress = computed(() => {
  const planned = plannedDaysPerWeek.value;
  if (planned <= 0) return 0;
  return Math.min(100, Math.round((studiedDays.value / planned) * 100));
});

const remainingHours = computed(() => {
  const wp = summary.value?.week_progress;
  if (!wp) return 0;
  return Math.max(0, Math.round((wp.target_hours - wp.completed_hours) * 10) / 10);
});

const weekStart = computed(() => summary.value?.week_progress.week_start ?? todayDateStr);

const daysElapsed = computed(() => {
  const start = new Date(weekStart.value + "T00:00:00");
  const today = new Date(todayDateStr + "T00:00:00");
  const diff = Math.floor((today.getTime() - start.getTime()) / 86400000);
  return Math.max(0, Math.min(6, diff));
});

const expectedRate = computed(() =>
  Math.round(((daysElapsed.value + 1) / 7) * 100)
);

// 进度状态基于整周计划完成进度（推进度）与时间进度比较
const isOnTrack = computed(() => weekPlanProgress.value >= expectedRate.value * 0.9);
const onTrackLabel = computed(() => (isOnTrack.value ? "按计划" : "需加快"));

// 跳转到周计划页
function goWeekPlan() {
  router.push({ name: "week-plan" });
}

// 加载本周每日摘要（与周计划页同源）
async function loadWeekSummaries() {
  const ws = summary.value?.week_progress.week_start;
  if (!ws) return;
  try {
    weekSummaries.value = await api.getWeekSummaries(ws);
  } catch {
    weekSummaries.value = [];
  }
}

// ── Current Status ──
const currentPhase = computed(() => summary.value?.current_phase ?? "—");
const streakDays = computed(() => summary.value?.streak_days ?? 0);
const totalStudyDays = computed(() => summary.value?.total_study_days ?? 0);
const targetScore = computed(() => settingsStore.settings?.target_score ?? 0);
const subjectProgressList = computed(() => summary.value?.subject_progress ?? []);

// ── Utilities ──
function weekdayShort(dateStr: string): string {
  const d = new Date(dateStr);
  const weekdays = ["日", "一", "二", "三", "四", "五", "六"];
  return weekdays[d.getDay()];
}

function dayOfMonth(dateStr: string): string {
  const d = new Date(dateStr);
  return `${d.getMonth() + 1}/${d.getDate()}`;
}

function isToday(dateStr: string): boolean {
  return dateStr === todayDateStr;
}

function subjectBadgeVariant(
  subject: string
): "default" | "math" | "english" | "politics" | "professional" {
  const map: Record<string, "default" | "math" | "english" | "politics" | "professional"> = {
    math: "math",
    english: "english",
    politics: "politics",
    professional: "professional",
  };
  return map[subject] ?? "default";
}

function priorityBadgeVariant(priority: string): "danger" | "warning" | "default" {
  if (priority === "A") return "danger";
  if (priority === "B") return "warning";
  return "default";
}

function subjectLabel(subject: SubjectKey | string): string {
  const map: Record<string, string> = {
    math: "数学",
    english: "英语",
    politics: "政治",
    professional: "专业课",
  };
  return map[subject] ?? subject;
}

function goToday() {
  router.push("/today");
}

function refresh() {
  dashboardStore.loadSummary().then(loadWeekSummaries);
  todayStore.loadToday();
}

onMounted(() => {
  dashboardStore.loadSummary().then(loadWeekSummaries);
  todayStore.loadToday();
  // 每分钟刷新当前时间，确保到点后自动展示今日计划
  nowTimer = window.setInterval(() => {
    nowMinutes.value = currentMinutesShanghai();
  }, 60_000);
});

onBeforeUnmount(() => {
  if (nowTimer !== undefined) {
    clearInterval(nowTimer);
    nowTimer = undefined;
  }
});
</script>

<template>
  <div class="dashboard-view">
    <!-- Loading -->
    <LoadingSpinner
      v-if="dashboardStore.loading && !summary"
      :size="32"
      label="加载工作台数据…"
    />

    <!-- Empty / Error -->
    <EmptyState
      v-else-if="!summary"
      title="暂无工作台数据"
      :description="dashboardStore.error || '点击下方按钮重新加载'"
    >
      <template #actions>
        <Button variant="primary" :icon="false" @click="refresh">
          <RefreshCw :size="15" />
          重新加载
        </Button>
      </template>
    </EmptyState>

    <!-- Content -->
    <template v-else>
      <!-- Hero -->
      <header class="hero">
        <div class="hero-text">
          <h1 v-if="settingsStore.settings?.show_greeting !== false" class="hero-name">
            {{ displayName ? `${greeting}，${displayName}` : greeting }}
          </h1>
          <h1 v-else class="hero-name">{{ displayName }}</h1>
          <span class="hero-date">
            <Calendar :size="12" />
            {{ dateLabel }}
          </span>
        </div>
        <div class="hero-countdown">
          <span class="countdown-number">{{ remainingDays }}</span>
          <span class="countdown-unit">天后考研</span>
        </div>
      </header>

      <!-- Today Focus -->
      <Card padding="lg" class="card focus-card" surface="1" hoverable>
        <div class="focus-header">
          <div class="focus-title-row">
            <span class="focus-indicator" />
            <Sparkles :size="18" class="focus-icon" />
            <h2 class="focus-heading">今日焦点</h2>
          </div>
          <Button v-if="focusTask && !isBeforeDailyStart" variant="ghost" size="sm" @click="goToday">
            查看详情
            <ChevronRight :size="14" />
          </Button>
        </div>

        <!-- 每日开始时间前：展示提示，不展示计划 -->
        <div v-if="isBeforeDailyStart" class="focus-before-start">
          <div class="focus-empty-icon">
            <Clock :size="22" />
          </div>
          <div class="focus-empty-text">
            <span class="focus-empty-title">今天的学习时间还没开始</span>
            <span class="focus-empty-desc">
              每日开始时间为 {{ dailyStartTimeLabel }}，到点后这里会展示今日学习计划。
            </span>
          </div>
        </div>

        <div v-else-if="focusTask" class="focus-body" @click="goToday">
          <div class="focus-meta">
            <div class="focus-badges">
              <Badge :variant="subjectBadgeVariant(focusTask.subject)" size="sm">
                {{ subjectLabel(focusTask.subject) }}
              </Badge>
              <Badge :variant="priorityBadgeVariant(focusTask.priority)" size="sm">
                <Flag :size="10" />
                P{{ focusTask.priority }}
              </Badge>
            </div>
            <span v-if="focusTask.estimated_hours" class="focus-time">
              <Clock :size="12" />
              {{ focusTask.estimated_hours }} 小时
            </span>
          </div>

          <h3 class="focus-task-title">{{ focusTask.title }}</h3>
          <p v-if="focusTask.goal" class="focus-goal">{{ focusTask.goal }}</p>

          <div class="focus-footer">
            <Button variant="primary" size="md" @click.stop="goToday">
              开始学习
              <ChevronRight :size="16" />
            </Button>
          </div>
        </div>

        <button
          v-else
          class="focus-empty"
          :disabled="todayStore.generating"
          @click="generateTodayPlan"
        >
          <div class="focus-empty-icon">
            <Sparkles v-if="!todayStore.generating" :size="22" />
            <RefreshCw v-else :size="18" class="spin" />
          </div>
          <div class="focus-empty-text">
            <span class="focus-empty-title">
              {{ todayStore.generating ? "正在生成今日计划…" : "今日暂无计划" }}
            </span>
            <span class="focus-empty-desc">点击生成基于当前状态的个性化学习计划</span>
          </div>
        </button>
      </Card>

      <!-- Week Progress -->
      <Card padding="md" class="card week-card clickable" hoverable @click="goWeekPlan">
        <div class="card-header">
          <div class="card-title-row">
            <TrendingUp :size="18" class="card-icon" />
            <h2 class="card-title">本周进度</h2>
          </div>
          <span class="week-range">
            {{ dayOfMonth(summary.week_progress.week_start) }} - {{ dayOfMonth(summary.week_progress.week_end) }}
          </span>
        </div>

        <div class="week-stats">
          <div class="week-rates">
            <div class="week-rate">
              <span class="rate-number">{{ weekPlanProgress }}%</span>
              <span class="rate-label">整周进度</span>
            </div>
            <div class="rate-divider" />
            <div class="week-rate">
              <span class="rate-number">{{ weekPercent }}%</span>
              <span class="rate-label">平均完成率</span>
            </div>
          </div>
          <div class="week-details">
            <div class="detail-item">
              <span class="detail-value">{{ studiedDays }}<small> / {{ plannedDaysPerWeek }} 天</small></span>
              <span class="detail-label">本周已学习</span>
            </div>
            <div class="detail-item">
              <span class="detail-value">{{ remainingHours }} 小时</span>
              <span class="detail-label">剩余目标</span>
            </div>
            <div class="detail-item">
              <span class="detail-value" :class="{ 'status-good': isOnTrack, 'status-warn': !isOnTrack }">
                {{ onTrackLabel }}
              </span>
              <span class="detail-label">进度状态</span>
            </div>
          </div>
        </div>

        <ProgressBar
          :value="weekPlanProgress"
          :max="100"
          :variant="weekPlanProgress >= 100 ? 'success' : 'default'"
          size="md"
        />

        <div class="week-dots">
          <div
            v-for="day in summary.week_progress.daily_breakdown"
            :key="day.date"
            class="day-dot"
            :class="{ studied: isDayStudied(day.date), today: isToday(day.date) }"
          >
            <span class="dot" />
            <span class="day-label">{{ weekdayShort(day.date) }}</span>
            <span class="day-date">{{ dayOfMonth(day.date) }}</span>
          </div>
        </div>

        <div class="card-footer-hint">
          <span>点击查看周计划详情</span>
          <ChevronRight :size="14" />
        </div>
      </Card>

      <!-- Current Status / Subject Progress -->
      <Card padding="md" class="card status-card" hoverable>
        <div class="card-header">
          <div class="card-title-row">
            <Target :size="18" class="card-icon" />
            <h2 class="card-title">学科进度</h2>
          </div>
        </div>

        <div v-if="subjectProgressList.length > 0" class="subject-grid">
          <div
            v-for="sp in subjectProgressList"
            :key="sp.subject"
            class="subject-cell"
          >
            <div class="subject-head">
              <Badge :variant="subjectBadgeVariant(sp.subject)" size="sm">
                {{ subjectLabel(sp.subject) }}
              </Badge>
              <span class="subject-hours">{{ sp.weekly_hours }} 小时/周</span>
            </div>
            <span class="subject-name" :title="subjectLabel(sp.subject)">{{ subjectLabel(sp.subject) }}</span>
            <span class="subject-phase">{{ sp.current_topic || '—' }}</span>
          </div>
        </div>

        <div class="status-footer">
          <span class="status-foot-item">
            <span class="status-foot-label">阶段</span>
            <span class="status-foot-value">{{ currentPhase }}</span>
          </span>
          <span class="status-foot-item">
            <span class="status-foot-label">连续</span>
            <span class="status-foot-value">{{ streakDays }} 天</span>
          </span>
          <span class="status-foot-item">
            <span class="status-foot-label">累计</span>
            <span class="status-foot-value">{{ totalStudyDays }} 天</span>
          </span>
          <span class="status-foot-item">
            <span class="status-foot-label">目标</span>
            <span class="status-foot-value">{{ targetScore > 0 ? targetScore : "—" }}</span>
          </span>
        </div>
      </Card>
    </template>
  </div>
</template>

<style scoped>
.dashboard-view {
  max-width: 840px;
  margin: 0 auto;
  padding: var(--space-8) var(--space-8) var(--space-10);
  display: flex;
  flex-direction: column;
  gap: var(--space-6);
  overflow: hidden;
}

/* Hero */
.hero {
  display: flex;
  align-items: flex-end;
  justify-content: space-between;
  gap: var(--space-4);
  padding-bottom: var(--space-4);
}

.hero-text {
  display: flex;
  flex-direction: column;
  gap: var(--space-1);
  min-width: 0;
}

.hero-name {
  font-size: var(--text-3xl);
  font-weight: var(--font-display);
  color: var(--text-primary);
  letter-spacing: -0.025em;
  line-height: var(--leading-tight);
}

.hero-date {
  display: inline-flex;
  align-items: center;
  gap: var(--space-1);
  font-size: var(--text-xs);
  color: var(--text-tertiary);
  font-weight: var(--font-label);
}

/* Countdown — soft gradient accent block, premium and quiet */
.hero-countdown {
  display: flex;
  flex-direction: column;
  align-items: flex-end;
  line-height: 1.1;
  padding: var(--space-3) var(--space-5);
  background: linear-gradient(135deg, var(--surface-accent) 0%, var(--surface-3) 100%);
  border: 1px solid var(--border-color);
  border-radius: var(--radius-lg);
  box-shadow: var(--shadow-sm);
  flex-shrink: 0;
}

.countdown-number {
  font-size: var(--text-3xl);
  font-weight: var(--font-display);
  color: var(--accent);
  letter-spacing: -0.03em;
}

.countdown-unit {
  font-size: var(--text-xs);
  color: var(--accent);
  font-weight: var(--font-label);
  opacity: 0.8;
}

/* Common card */
.card {
  display: flex;
  flex-direction: column;
  gap: var(--space-5);
}

.card-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--space-2);
}

.card-title-row {
  display: flex;
  align-items: center;
  gap: var(--space-2);
}

.card-icon {
  color: var(--accent);
}

.card-title {
  font-size: var(--text-base);
  font-weight: var(--font-heading);
  color: var(--text-primary);
}

/* Today Focus — Apple-style: quiet accent, no heavy border decoration */
.focus-card {
  background: var(--bg-elevated);
  position: relative;
}

.focus-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--space-2);
}

.focus-title-row {
  display: flex;
  align-items: center;
  gap: var(--space-2);
}

.focus-indicator {
  width: 6px;
  height: 6px;
  border-radius: var(--radius-full);
  background: var(--accent);
  flex-shrink: 0;
}

.focus-icon {
  color: var(--accent);
}

.focus-heading {
  font-size: var(--text-base);
  font-weight: var(--font-heading);
  color: var(--text-primary);
}

.focus-body {
  display: flex;
  flex-direction: column;
  gap: var(--space-3);
  cursor: pointer;
}

.focus-meta {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--space-3);
  flex-wrap: wrap;
}

.focus-badges {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  flex-wrap: wrap;
}

.focus-time {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  font-size: var(--text-xs);
  color: var(--text-tertiary);
  font-weight: var(--font-label);
  flex-shrink: 0;
}

.focus-task-title {
  font-size: var(--text-xl);
  font-weight: var(--font-heading);
  color: var(--text-primary);
  letter-spacing: -0.02em;
  line-height: var(--leading-tight);
}

.focus-goal {
  font-size: var(--text-sm);
  color: var(--text-secondary);
  line-height: var(--leading-normal);
  margin: 0;
}

.focus-footer {
  display: flex;
  align-items: center;
  gap: var(--space-3);
  margin-top: var(--space-1);
}

.focus-empty {
  display: flex;
  align-items: center;
  gap: var(--space-4);
  padding: var(--space-4);
  background: var(--bg-tertiary);
  border: 1.5px dashed var(--border-color);
  border-radius: var(--radius-md);
  cursor: pointer;
  transition: border-color var(--transition-fast), background var(--transition-fast);
  font-family: inherit;
  text-align: left;
  width: 100%;
}

/* 每日开始时间前的提示块（非交互） */
.focus-before-start {
  display: flex;
  align-items: center;
  gap: var(--space-4);
  padding: var(--space-4);
  background: var(--bg-tertiary);
  border: 1.5px solid var(--border-color);
  border-radius: var(--radius-md);
  text-align: left;
  width: 100%;
}

.focus-empty:hover:not(:disabled) {
  border-color: var(--accent);
  background: var(--accent-subtle);
}

.focus-empty:disabled {
  cursor: progress;
  opacity: 0.7;
}

.focus-empty-icon {
  width: 44px;
  height: 44px;
  display: flex;
  align-items: center;
  justify-content: center;
  background: var(--accent-subtle);
  color: var(--accent);
  border-radius: var(--radius-md);
  flex-shrink: 0;
}

.focus-empty-text {
  display: flex;
  flex-direction: column;
  gap: 2px;
  min-width: 0;
}

.focus-empty-title {
  font-size: var(--text-base);
  font-weight: var(--font-semibold);
  color: var(--text-primary);
}

.focus-empty-desc {
  font-size: var(--text-xs);
  color: var(--text-tertiary);
}

.spin {
  animation: spin 0.8s linear infinite;
}

@keyframes spin {
  to {
    transform: rotate(360deg);
  }
}

/* Week Progress */
.week-range {
  font-size: var(--text-xs);
  color: var(--text-tertiary);
  font-weight: var(--font-label);
}

.week-stats {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--space-6);
}

.week-rates {
  display: flex;
  align-items: center;
  gap: var(--space-4);
  flex-shrink: 0;
}

.week-rate {
  display: flex;
  flex-direction: column;
  gap: var(--space-1);
}

.rate-divider {
  width: 1px;
  height: 32px;
  background: var(--divider-color);
  flex-shrink: 0;
}

.rate-number {
  font-size: var(--text-2xl);
  font-weight: var(--font-heading);
  color: var(--accent);
  letter-spacing: -0.02em;
  line-height: var(--leading-tight);
}

.rate-label {
  font-size: var(--text-xs);
  color: var(--text-tertiary);
  font-weight: var(--font-label);
}

.week-details {
  display: flex;
  align-items: center;
  gap: var(--space-6);
}

.detail-item {
  display: flex;
  flex-direction: column;
  align-items: flex-end;
  gap: 2px;
}

.detail-value {
  font-size: var(--text-base);
  font-weight: var(--font-heading);
  color: var(--text-primary);
  letter-spacing: -0.01em;
}

.detail-value small {
  font-size: var(--text-xs);
  font-weight: var(--font-label);
  color: var(--text-tertiary);
}

.detail-label {
  font-size: var(--text-xs);
  color: var(--text-tertiary);
  font-weight: var(--font-label);
}

.status-good {
  color: var(--color-success);
}

.status-warn {
  color: var(--color-warning);
}

.week-dots {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: var(--space-2);
}

/* ── 可点击卡片 ── */
.card.clickable {
  cursor: pointer;
  transition: transform var(--transition-fast), box-shadow var(--transition-fast);
}

.card.clickable:hover {
  transform: translateY(-1px);
}

.card-footer-hint {
  display: flex;
  align-items: center;
  justify-content: center;
  gap: var(--space-1);
  margin-top: var(--space-3);
  padding-top: var(--space-3);
  border-top: 1px solid var(--divider-color);
  font-size: var(--text-xs);
  color: var(--text-tertiary);
  transition: color var(--transition-fast);
}

.card.clickable:hover .card-footer-hint {
  color: var(--accent);
}

.day-dot {
  flex: 1;
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: var(--space-1);
  min-width: 0;
}

.dot {
  width: 8px;
  height: 8px;
  border-radius: var(--radius-full);
  background: var(--bg-tertiary);
  transition: background-color var(--transition-fast), box-shadow var(--transition-fast);
}

.day-dot.studied .dot {
  background: var(--accent);
}

.day-dot.today .dot {
  box-shadow: 0 0 0 3px var(--accent-subtle);
}

.day-dot.today.studied .dot {
  background: var(--color-success);
}

.day-label {
  font-size: var(--text-xs);
  color: var(--text-secondary);
  font-weight: var(--font-medium);
}

.day-dot.today .day-label {
  color: var(--accent);
  font-weight: var(--font-semibold);
}

.day-date {
  font-size: 10px;
  color: var(--text-quaternary);
}

/* Subject Progress / Current Status */
.subject-grid {
  display: grid;
  grid-template-columns: repeat(2, 1fr);
  gap: var(--space-3);
}

.subject-cell {
  display: flex;
  flex-direction: column;
  gap: var(--space-2);
  padding: var(--space-3);
  background: var(--bg-tertiary);
  border-radius: var(--radius-md);
  min-width: 0;
}

.subject-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--space-2);
  min-width: 0;
}

.subject-hours {
  font-size: var(--text-xs);
  font-weight: var(--font-medium);
  color: var(--text-tertiary);
  flex-shrink: 0;
}

.subject-name {
  font-size: var(--text-lg);
  font-weight: var(--font-bold);
  color: var(--text-primary);
  letter-spacing: -0.01em;
  line-height: var(--leading-tight);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.subject-phase {
  font-size: var(--text-xs);
  font-weight: var(--font-medium);
  color: var(--text-secondary);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.status-footer {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--space-4);
  padding-top: var(--space-3);
  border-top: 1px solid var(--divider-color);
}

.status-foot-item {
  display: flex;
  align-items: center;
  gap: var(--space-1);
  font-size: var(--text-xs);
  color: var(--text-secondary);
}

.status-foot-label {
  color: var(--text-tertiary);
  font-weight: var(--font-label);
}

.status-foot-value {
  color: var(--text-primary);
  font-weight: var(--font-heading);
}

@media (max-width: 720px) {
  .dashboard-view {
    padding: var(--space-4);
  }

  .hero {
    align-items: flex-start;
    flex-direction: column;
  }

  .hero-countdown {
    align-items: flex-start;
  }

  .week-stats {
    flex-direction: column;
    align-items: flex-start;
  }

  .week-details {
    width: 100%;
    justify-content: space-between;
    gap: var(--space-3);
  }

  .detail-item {
    align-items: flex-start;
  }

  .subject-grid {
    grid-template-columns: 1fr;
  }

  .status-footer {
    flex-wrap: wrap;
    gap: var(--space-2) var(--space-4);
  }
}
</style>
