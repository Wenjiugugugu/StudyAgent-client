<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, watch } from "vue";
import { useRoute, useRouter } from "vue-router";
import { useTodayStore } from "@/stores/today";
import { useSettingsStore } from "@/stores/settings";
import { todayString, yesterdayString, daysBetween, getWeekStart, prevDateString, nextDateString, currentMinutesShanghai, timeStringToMinutes } from "@/utils/date";
import Card from "@/components/ui/Card.vue";
import Badge from "@/components/ui/Badge.vue";
import Button from "@/components/ui/Button.vue";
import ProgressBar from "@/components/ui/ProgressBar.vue";
import LoadingSpinner from "@/components/ui/LoadingSpinner.vue";
import EmptyState from "@/components/ui/EmptyState.vue";
import {
  CheckCircle2,
  Circle,
  AlertTriangle,
  Target,
  BookOpen,
  ChevronDown,
  ChevronLeft,
  ChevronRight,
  ListChecks,
  Lightbulb,
  RefreshCw,
  RotateCcw,
  Flag,
  ShieldAlert,
  ArrowRight,
  Sparkles,
} from "lucide-vue-next";
import type { PlanTask, PlanRisk, SubjectKey } from "@/types";

const todayStore = useTodayStore();
const settingsStore = useSettingsStore();
const route = useRoute();
const router = useRouter();

const currentDate = computed(() => {
  const q = route.query.date;
  return typeof q === "string" && /^\d{4}-\d{2}-\d{2}$/.test(q) ? q : todayString();
});

const isToday = computed(() => currentDate.value === todayString());
// 仅允许修改今天和昨天的任务完成情况，更早的任务只读
const yesterday = computed(() => yesterdayString());
const canModifyTasks = computed(() =>
  currentDate.value === todayString() || currentDate.value === yesterday.value
);
const plan = computed(() => todayStore.plan);
const planData = computed(() => plan.value?.data ?? null);
const priorityA = computed(() => todayStore.priorityATasks);
const priorityB = computed(() => todayStore.priorityBTasks);

const source = computed(() => route.query.from as string | undefined);

const backLabel = computed(() => {
  if (source.value === "history") return "返回历史计划";
  if (source.value === "week-plan") return "返回周计划";
  return "返回今天";
});

function goBack() {
  if (source.value === "history") {
    router.push({ name: "history-plans" });
    return;
  }
  if (source.value === "week-plan") {
    const week = getWeekStart(currentDate.value);
    router.push({ name: "week-plan", query: { week } });
    return;
  }
  router.replace({ name: "plan" });
}

// 使用设置中的考试日期计算倒计时，避免 AI 生成内容解析错误
const computedRemainingDays = computed(() => {
  const examDate = settingsStore.settings?.exam_date;
  if (examDate && /^\d{4}-\d{2}-\d{2}$/.test(examDate)) {
    return daysBetween(examDate, currentDate.value);
  }
  return plan.value?.data?.remaining_days ?? 0;
});
const expandedTaskId = ref<string | null>(null);

function toggleExpand(taskId: string) {
  expandedTaskId.value = expandedTaskId.value === taskId ? null : taskId;
}

function isExpanded(taskId: string): boolean {
  return expandedTaskId.value === taskId;
}

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

function priorityBadgeVariant(priority: string): "danger" | "warning" | "default" {
  if (priority === "A") return "danger";
  if (priority === "B") return "warning";
  return "default";
}

function riskBadgeVariant(level: PlanRisk["level"]): "info" | "warning" | "danger" {
  if (level === "high") return "danger";
  if (level === "medium") return "warning";
  return "info";
}

function riskLabel(level: PlanRisk["level"]): string {
  const map: Record<PlanRisk["level"], string> = {
    critical: "危急",
    high: "高风险",
    medium: "中风险",
    low: "低风险",
  };
  return map[level];
}

async function completeTask(task: PlanTask) {
  await todayStore.updateTaskStatus(task.id, "done");
  if (expandedTaskId.value === task.id) expandedTaskId.value = null;
}

async function reopenTask(task: PlanTask) {
  await todayStore.updateTaskStatus(task.id, "pending");
}

async function generatePlan() {
  await todayStore.generate(currentDate.value);
}

async function loadPlan() {
  await todayStore.loadByDate(currentDate.value);
}

// ── 每日开始时间前不展示今日计划 ──
// 仅当查看今天且当前时间早于设置中的 start_time 时，隐藏计划并提示
const nowMinutes = ref(currentMinutesShanghai());
let nowTimer: number | undefined;

const dailyStartMinutes = computed(() => {
  const t = settingsStore.settings?.study_schedule?.start_time;
  if (!t) return -1;
  return timeStringToMinutes(t);
});

const isBeforeDailyStart = computed(() => {
  if (!isToday.value) return false;
  if (dailyStartMinutes.value < 0) return false;
  return nowMinutes.value < dailyStartMinutes.value;
});

const dailyStartTimeLabel = computed(() => {
  return settingsStore.settings?.study_schedule?.start_time ?? "09:00";
});

function refreshNow() {
  nowMinutes.value = currentMinutesShanghai();
}

watch(currentDate, () => {
  loadPlan();
});

/** 跳转到指定日期（与 query.from 保持一致） */
function goToDate(date: string) {
  router.replace({
    name: "plan",
    query: { date, ...(source.value ? { from: source.value } : {}) },
  });
}

/** 全局键盘监听：左右键切换历史日期 */
function handleKeydown(e: KeyboardEvent) {
  // 仅在 plan 路由生效
  if (route.name !== "plan") return;
  // 输入控件聚焦时不响应（避免与输入法/表单冲突）
  const target = e.target as HTMLElement | null;
  if (target) {
    const tag = target.tagName;
    if (
      tag === "INPUT" ||
      tag === "TEXTAREA" ||
      tag === "SELECT" ||
      target.isContentEditable
    ) {
      return;
    }
    // 如果事件来自任何可交互控件（如展开的任务卡按钮），也忽略
    if (target.closest('[contenteditable="true"]')) return;
  }
  if (e.metaKey || e.ctrlKey || e.altKey) return;

  if (e.key === "ArrowLeft") {
    e.preventDefault();
    goToDate(prevDateString(currentDate.value));
  } else if (e.key === "ArrowRight") {
    e.preventDefault();
    // 不允许超过今天
    const next = nextDateString(currentDate.value);
    if (next > todayString()) return;
    goToDate(next);
  }
}

onMounted(() => {
  loadPlan();
  window.addEventListener("keydown", handleKeydown);
  // 每分钟刷新一次当前时间，确保到点后自动展示计划
  nowTimer = window.setInterval(refreshNow, 60_000);
});

onUnmounted(() => {
  window.removeEventListener("keydown", handleKeydown);
  if (nowTimer) window.clearInterval(nowTimer);
});
</script>

<template>
  <div class="today-view">
    <!-- Loading -->
    <LoadingSpinner
      v-if="todayStore.loading && !plan"
      :size="32"
      label="加载计划…"
    />

    <!-- 今日计划尚未到开始时间 -->
    <EmptyState
      v-else-if="isBeforeDailyStart"
      :title="`今天的学习时间还没开始`"
      :description="`每日开始时间为 ${dailyStartTimeLabel}，到点后这里会展示今日学习计划。`"
    >
      <template #actions>
        <Button variant="secondary" @click="refreshNow">
          <RefreshCw :size="15" />
          刷新时间
        </Button>
      </template>
    </EmptyState>

    <!-- Empty / Error -->
    <EmptyState
      v-else-if="!plan"
      :title="`${currentDate} 还没有学习计划`"
      :description="todayStore.error || '生成一份基于当前状态与用户画像的个性化计划'"
    >
      <template #actions>
        <Button v-if="isToday" variant="primary" :loading="todayStore.generating" @click="generatePlan">
          <Sparkles :size="15" />
          生成今日计划
        </Button>
        <Button v-if="!isToday" variant="secondary" @click="goBack">
          {{ backLabel }}
        </Button>
      </template>
    </EmptyState>

    <!-- Content -->
    <template v-else>
      <!-- Top action -->
      <div class="top-bar">
        <div class="top-meta">
          <span class="meta-date">{{ plan.meta.date }}</span>
          <span v-if="!isToday" class="meta-tag">历史计划</span>
          <span class="meta-dot">·</span>
          <span class="meta-remain">距考研 {{ computedRemainingDays }} 天</span>
        </div>
        <div class="top-actions">
          <Button
            v-if="!isToday"
            variant="ghost"
            size="sm"
            icon
            title="前一天（←）"
            :disabled="todayStore.loading"
            @click="goToDate(prevDateString(currentDate))"
          >
            <ChevronLeft :size="16" />
          </Button>
          <Button
            v-if="!isToday"
            variant="ghost"
            size="sm"
            icon
            title="后一天（→）"
            :disabled="todayStore.loading || nextDateString(currentDate) > todayString()"
            @click="goToDate(nextDateString(currentDate))"
          >
            <ChevronRight :size="16" />
          </Button>
          <Button
            v-if="!isToday"
            variant="ghost"
            size="sm"
            @click="goBack"
          >
            {{ backLabel }}
          </Button>
          <Button
            variant="ghost"
            size="sm"
            :loading="todayStore.loading"
            :disabled="todayStore.loading || todayStore.generating"
            @click="loadPlan"
          >
            <RotateCcw :size="14" />
            刷新
          </Button>
          <Button
            v-if="isToday"
            variant="ghost"
            size="sm"
            :loading="todayStore.generating"
            :disabled="todayStore.generating"
            @click="generatePlan"
          >
            <RefreshCw :size="14" />
            重新生成
          </Button>
        </div>
      </div>

      <!-- Error banner -->
      <div v-if="todayStore.error" class="error-banner">
        <AlertTriangle :size="16" />
        <span>{{ todayStore.error }}</span>
      </div>

      <!-- Strategy area -->
      <Card padding="lg" class="strategy-card">
        <div class="strategy-head">
          <Target :size="18" class="strategy-icon" />
          <div class="strategy-target">
            <span class="st-label">目标</span>
            <span class="st-value">{{ planData?.target }}</span>
          </div>
        </div>

        <p class="strategy-text">{{ planData?.strategy }}</p>

        <div class="strategy-stats">
          <div class="strat-stat">
            <ListChecks :size="14" />
            <span class="strat-val">{{ planData?.total_tasks }}</span>
            <span class="strat-key">总任务</span>
          </div>
        </div>
      </Card>

      <!-- Completion rate -->
      <Card padding="md" class="rate-card">
        <div class="rate-row">
          <div class="rate-info">
            <span class="rate-label">完成率</span>
            <span class="rate-num">
              {{ todayStore.doneCount }} / {{ todayStore.totalCount }}
            </span>
          </div>
          <span class="rate-percent">{{ todayStore.completionRate }}%</span>
        </div>
        <ProgressBar
          :value="todayStore.doneCount"
          :max="todayStore.totalCount || 1"
          :variant="todayStore.completionRate >= 100 ? 'success' : 'default'"
          size="lg"
        />
      </Card>

      <!-- Priority A -->
      <div class="section-heading">
        <h2 class="section-title">
          <Flag :size="15" class="title-icon" />
          Priority A · 核心任务
        </h2>
        <span class="section-count">{{ priorityA.length }}</span>
      </div>

      <div v-if="priorityA.length" class="task-list">
        <Card
          v-for="task in priorityA"
          :key="task.id"
          padding="md"
          class="task-card"
          :class="{ done: task.status === 'done', expanded: isExpanded(task.id) }"
        >
          <!-- Task header -->
          <div class="task-header" @click="toggleExpand(task.id)">
            <div class="task-status-icon" :class="task.status">
              <CheckCircle2 v-if="task.status === 'done'" :size="18" />
              <Circle v-else :size="18" />
            </div>

            <div class="task-main">
              <div class="task-badges">
                <Badge :variant="subjectBadgeVariant(task.subject)" size="sm">
                  {{ task.subject }}
                </Badge>
                <Badge :variant="priorityBadgeVariant(task.priority)" size="sm">
                  P{{ task.priority }}
                </Badge>
              </div>
              <h3 class="task-title" :class="{ 'done-text': task.status === 'done' }">
                {{ task.title }}
              </h3>
            </div>

            <div v-if="canModifyTasks" class="task-actions" @click.stop>
              <Button
                v-if="task.status !== 'done'"
                variant="primary"
                size="sm"
                @click="completeTask(task)"
              >
                <CheckCircle2 :size="13" />
                完成
              </Button>
              <Button
                v-else
                variant="ghost"
                size="sm"
                @click="reopenTask(task)"
              >
                <RotateCcw :size="13" />
                重开
              </Button>
            </div>
            <div v-else class="task-status-text" :class="task.status">
              {{ task.status === 'done' ? '已完成' : task.status === 'abandoned' ? '已放弃' : '未完成' }}
            </div>

            <ChevronDown :size="18" class="expand-chevron" :class="{ open: isExpanded(task.id) }" />
          </div>

          <!-- Task detail (expandable) -->
          <transition name="expand">
            <div v-if="isExpanded(task.id)" class="task-detail">
              <div class="detail-block">
                <div class="detail-label">
                  <Target :size="13" />
                  目标
                </div>
                <p class="detail-text">{{ task.goal }}</p>
              </div>

              <div class="detail-block">
                <div class="detail-label">
                  <ListChecks :size="13" />
                  完成标准 (DoD)
                </div>
                <ul class="dod-list">
                  <li v-for="(c, i) in task.completion_criteria" :key="i" class="dod-item">
                    <CheckCircle2 :size="14" class="dod-check" />
                    <span>{{ c }}</span>
                  </li>
                </ul>
              </div>

              <div v-if="task.textbook" class="detail-block">
                <div class="detail-label">
                  <BookOpen :size="13" />
                  教材
                </div>
                <p class="detail-text">{{ task.textbook }}</p>
              </div>

              <div v-if="task.style_tips" class="detail-block">
                <div class="detail-label">
                  <Lightbulb :size="13" />
                  风格提示
                </div>
                <p class="detail-text muted">{{ task.style_tips }}</p>
              </div>

              <div v-if="task.fallback_plan" class="detail-block fallback">
                <div class="detail-label">
                  <ShieldAlert :size="13" />
                  失败回退
                </div>
                <p class="detail-text">{{ task.fallback_plan }}</p>
              </div>
            </div>
          </transition>
        </Card>
      </div>

      <!-- Priority B -->
      <template v-if="priorityB.length">
        <div class="section-heading">
          <h2 class="section-title">
            <Flag :size="15" class="title-icon" />
            Priority B · 次要任务
          </h2>
          <span class="section-count">{{ priorityB.length }}</span>
        </div>

        <div class="task-list">
          <Card
            v-for="task in priorityB"
            :key="task.id"
            padding="md"
            class="task-card"
            :class="{ done: task.status === 'done', expanded: isExpanded(task.id) }"
          >
            <div class="task-header" @click="toggleExpand(task.id)">
              <div class="task-status-icon" :class="task.status">
                <CheckCircle2 v-if="task.status === 'done'" :size="18" />
                <Circle v-else :size="18" />
              </div>

              <div class="task-main">
                <div class="task-badges">
                  <Badge :variant="subjectBadgeVariant(task.subject)" size="sm">
                    {{ task.subject }}
                  </Badge>
                  <Badge :variant="priorityBadgeVariant(task.priority)" size="sm">
                    P{{ task.priority }}
                  </Badge>
                </div>
                <h3 class="task-title" :class="{ 'done-text': task.status === 'done' }">
                  {{ task.title }}
                </h3>
              </div>

              <div v-if="canModifyTasks" class="task-actions" @click.stop>
                <Button
                  v-if="task.status !== 'done'"
                  variant="primary"
                  size="sm"
                  @click="completeTask(task)"
                >
                  <CheckCircle2 :size="13" />
                  完成
                </Button>
                <Button
                  v-else
                  variant="ghost"
                  size="sm"
                  @click="reopenTask(task)"
                >
                  <RotateCcw :size="13" />
                  重开
                </Button>
              </div>
              <div v-else class="task-status-text" :class="task.status">
                {{ task.status === 'done' ? '已完成' : task.status === 'abandoned' ? '已放弃' : '未完成' }}
              </div>

              <ChevronDown :size="18" class="expand-chevron" :class="{ open: isExpanded(task.id) }" />
            </div>

            <transition name="expand">
              <div v-if="isExpanded(task.id)" class="task-detail">
                <div class="detail-block">
                  <div class="detail-label">
                    <Target :size="13" />
                    目标
                  </div>
                  <p class="detail-text">{{ task.goal }}</p>
                </div>

                <div class="detail-block">
                  <div class="detail-label">
                    <ListChecks :size="13" />
                    完成标准 (DoD)
                  </div>
                  <ul class="dod-list">
                    <li v-for="(c, i) in task.completion_criteria" :key="i" class="dod-item">
                      <CheckCircle2 :size="14" class="dod-check" />
                      <span>{{ c }}</span>
                    </li>
                  </ul>
                </div>

                <div v-if="task.textbook" class="detail-block">
                  <div class="detail-label">
                    <BookOpen :size="13" />
                    教材
                  </div>
                  <p class="detail-text">{{ task.textbook }}</p>
                </div>

                <div v-if="task.style_tips" class="detail-block">
                  <div class="detail-label">
                    <Lightbulb :size="13" />
                    风格提示
                  </div>
                  <p class="detail-text muted">{{ task.style_tips }}</p>
                </div>

                <div v-if="task.fallback_plan" class="detail-block fallback">
                  <div class="detail-label">
                    <ShieldAlert :size="13" />
                    失败回退
                  </div>
                  <p class="detail-text">{{ task.fallback_plan }}</p>
                </div>
              </div>
            </transition>
          </Card>
        </div>
      </template>

      <!-- Risks -->
      <template v-if="planData?.risks?.length">
        <div class="section-heading">
          <h2 class="section-title">
            <AlertTriangle :size="15" class="title-icon" />
            风险提示
          </h2>
        </div>

        <Card padding="md" class="risk-card">
          <div
            v-for="(risk, i) in planData.risks"
            :key="i"
            class="risk-item"
            :class="risk.level"
          >
            <div class="risk-head">
              <span class="risk-item-text">{{ risk.item }}</span>
              <Badge :variant="riskBadgeVariant(risk.level)" size="sm">
                {{ riskLabel(risk.level) }}
              </Badge>
            </div>
            <p class="risk-suggestion">{{ risk.suggestion }}</p>
          </div>
        </Card>
      </template>

      <!-- Reminders -->
      <template v-if="planData?.reminders?.length">
        <div class="section-heading">
          <h2 class="section-title">
            <Lightbulb :size="15" class="title-icon" />
            今日提醒
          </h2>
        </div>

        <Card padding="md" class="reminder-card">
          <ul class="reminder-list">
            <li v-for="(r, i) in planData.reminders" :key="i" class="reminder-item">
              <ArrowRight :size="14" class="reminder-bullet" />
              <span>{{ r }}</span>
            </li>
          </ul>
        </Card>
      </template>

      <!-- After today -->
      <Card v-if="planData?.after_today" padding="md" class="after-card">
        <div class="after-label">完成今日后</div>
        <p class="after-text">{{ planData.after_today }}</p>
      </Card>
    </template>
  </div>
</template>

<style scoped>
.today-view {
  max-width: 880px;
  margin: 0 auto;
  padding: var(--space-8);
  display: flex;
  flex-direction: column;
  gap: var(--space-4);
}

/* Top bar */
.top-bar {
  position: sticky;
  top: 0;
  z-index: 10;
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--space-3);
  background: var(--bg-primary);
  padding: var(--space-4) 0;
  margin: 0 calc(-1 * var(--space-8));
  padding-left: var(--space-8);
  padding-right: var(--space-8);
}

.top-meta {
  display: inline-flex;
  align-items: center;
  gap: var(--space-2);
  font-size: var(--text-sm);
  color: var(--text-secondary);
  flex-wrap: wrap;
}

.meta-date {
  font-weight: var(--font-semibold);
  color: var(--text-primary);
}

.meta-tag {
  font-size: var(--text-xs);
  padding: 1px 6px;
  background: var(--accent-subtle);
  color: var(--accent);
  border-radius: var(--radius-full);
  font-weight: var(--font-medium);
}

.meta-dot {
  color: var(--text-quaternary);
}

.meta-remain {
  color: var(--accent);
  font-weight: var(--font-medium);
}

.top-actions {
  display: inline-flex;
  align-items: center;
  gap: var(--space-2);
  flex-shrink: 0;
}

.error-banner {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  padding: var(--space-3) var(--space-4);
  background: var(--color-danger-subtle);
  color: var(--color-danger);
  border-radius: var(--radius-md);
  font-size: var(--text-sm);
}

/* Strategy card */
.strategy-card {
  display: flex;
  flex-direction: column;
  gap: var(--space-4);
}

.strategy-head {
  display: flex;
  align-items: center;
  gap: var(--space-3);
}

.strategy-icon {
  color: var(--accent);
  flex-shrink: 0;
}

.strategy-target {
  display: flex;
  flex-direction: column;
  line-height: 1.3;
}

.st-label {
  font-size: var(--text-xs);
  color: var(--text-tertiary);
}

.st-value {
  font-size: var(--text-base);
  font-weight: var(--font-semibold);
  color: var(--text-primary);
}

.strategy-text {
  font-size: var(--text-base);
  color: var(--text-primary);
  line-height: var(--leading-relaxed);
  margin: 0;
}

.strategy-stats {
  display: flex;
  gap: var(--space-6);
}

.strat-stat {
  display: inline-flex;
  align-items: center;
  gap: var(--space-1);
  font-size: var(--text-sm);
  color: var(--text-secondary);
}

.strat-stat svg {
  color: var(--text-tertiary);
}

.strat-val {
  font-weight: var(--font-semibold);
  color: var(--text-primary);
}

.strat-key {
  color: var(--text-tertiary);
}

.overview-chips {
  display: flex;
  flex-wrap: wrap;
  gap: var(--space-2);
}

.ov-chip {
  display: inline-flex;
  align-items: center;
  gap: var(--space-2);
  padding: var(--space-1) var(--space-3);
  background: var(--bg-tertiary);
  border-radius: var(--radius-full);
  font-size: var(--text-xs);
}

.ov-subject {
  font-weight: var(--font-medium);
  color: var(--text-primary);
}

.ov-hours {
  color: var(--text-tertiary);
}

.ov-priority {
  color: var(--accent);
  font-weight: var(--font-medium);
}

/* Rate card */
.rate-card {
  display: flex;
  flex-direction: column;
  gap: var(--space-3);
}

.rate-row {
  display: flex;
  align-items: baseline;
  justify-content: space-between;
}

.rate-info {
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.rate-label {
  font-size: var(--text-xs);
  color: var(--text-tertiary);
}

.rate-num {
  font-size: var(--text-base);
  font-weight: var(--font-semibold);
  color: var(--text-primary);
}

.rate-percent {
  font-size: var(--text-2xl);
  font-weight: var(--font-bold);
  color: var(--accent);
  letter-spacing: -0.02em;
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

.section-count {
  font-size: var(--text-xs);
  color: var(--text-tertiary);
  background: var(--bg-tertiary);
  padding: 1px 8px;
  border-radius: var(--radius-full);
}

/* Task list */
.task-list {
  display: flex;
  flex-direction: column;
  gap: var(--space-3);
}

.task-card {
  transition: box-shadow var(--transition-fast);
}

.task-card.done {
  opacity: 0.72;
}

.task-card.expanded {
  box-shadow: var(--shadow-md);
}

/* Task header */
.task-header {
  display: flex;
  align-items: center;
  gap: var(--space-3);
  cursor: pointer;
}

.task-status-icon {
  width: 32px;
  height: 32px;
  display: flex;
  align-items: center;
  justify-content: center;
  border-radius: var(--radius-full);
  flex-shrink: 0;
  color: var(--text-tertiary);
  transition: all var(--transition-fast);
}

.task-status-icon.pending {
  color: var(--text-quaternary);
}

.task-status-icon.in_progress {
  color: var(--text-on-accent);
  background: var(--accent);
}

.task-status-icon.done {
  color: var(--color-success);
}

.task-main {
  flex: 1;
  display: flex;
  flex-direction: column;
  gap: var(--space-1);
  min-width: 0;
}

.task-badges {
  display: flex;
  gap: var(--space-2);
}

.task-title {
  font-size: var(--text-base);
  font-weight: var(--font-medium);
  color: var(--text-primary);
  line-height: var(--leading-tight);
}

.task-title.done-text {
  text-decoration: line-through;
  color: var(--text-tertiary);
}

.task-actions {
  display: flex;
  gap: var(--space-2);
  flex-shrink: 0;
}

.task-status-text {
  font-size: var(--text-xs);
  font-weight: var(--font-medium);
  color: var(--text-tertiary);
  padding: var(--space-1) var(--space-3);
  border-radius: var(--radius-md);
  background: var(--bg-tertiary);
  flex-shrink: 0;
  white-space: nowrap;
}

.task-status-text.done {
  color: var(--color-success);
  background: var(--color-success-subtle);
}

.task-status-text.abandoned {
  color: var(--text-quaternary);
  background: var(--bg-overlay);
}

.expand-chevron {
  color: var(--text-quaternary);
  flex-shrink: 0;
  transition: transform var(--transition-normal);
}

.expand-chevron.open {
  transform: rotate(180deg);
}

/* Task detail */
.task-detail {
  margin-top: var(--space-4);
  padding-top: var(--space-4);
  border-top: 1px solid var(--divider-color);
  display: flex;
  flex-direction: column;
  gap: var(--space-4);
}

.detail-block {
  display: flex;
  flex-direction: column;
  gap: var(--space-2);
}

.detail-label {
  display: inline-flex;
  align-items: center;
  gap: var(--space-1);
  font-size: var(--text-xs);
  font-weight: var(--font-semibold);
  color: var(--text-tertiary);
  text-transform: uppercase;
  letter-spacing: 0.04em;
}

.detail-text {
  font-size: var(--text-sm);
  color: var(--text-primary);
  line-height: var(--leading-relaxed);
  margin: 0;
}

.detail-text.muted {
  color: var(--text-secondary);
}

.detail-block.fallback {
  padding: var(--space-3);
  background: var(--color-warning-subtle);
  border-radius: var(--radius-md);
}

.dod-list {
  list-style: none;
  margin: 0;
  padding: 0;
  display: flex;
  flex-direction: column;
  gap: var(--space-2);
}

.dod-item {
  display: flex;
  align-items: flex-start;
  gap: var(--space-2);
  font-size: var(--text-sm);
  color: var(--text-primary);
  line-height: var(--leading-normal);
}

.dod-check {
  color: var(--color-success);
  flex-shrink: 0;
  margin-top: 1px;
}

/* Risk */
.risk-card {
  display: flex;
  flex-direction: column;
  gap: var(--space-3);
}

.risk-item {
  display: flex;
  flex-direction: column;
  gap: var(--space-1);
  padding: var(--space-3);
  border-radius: var(--radius-md);
  background: var(--bg-overlay);
}

.risk-item.high {
  background: var(--color-danger-subtle);
}

.risk-item.medium {
  background: var(--color-warning-subtle);
}

.risk-item.low {
  background: var(--color-info-subtle);
}

.risk-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--space-2);
}

.risk-item-text {
  font-size: var(--text-sm);
  font-weight: var(--font-semibold);
  color: var(--text-primary);
}

.risk-suggestion {
  font-size: var(--text-xs);
  color: var(--text-secondary);
  line-height: var(--leading-normal);
  margin: 0;
}

/* Reminders */
.reminder-card {
  display: flex;
  flex-direction: column;
}

.reminder-list {
  list-style: none;
  margin: 0;
  padding: 0;
  display: flex;
  flex-direction: column;
  gap: var(--space-3);
}

.reminder-item {
  display: flex;
  align-items: flex-start;
  gap: var(--space-2);
  font-size: var(--text-sm);
  color: var(--text-primary);
  line-height: var(--leading-normal);
}

.reminder-bullet {
  color: var(--accent);
  flex-shrink: 0;
  margin-top: 2px;
}

/* After today */
.after-card {
  display: flex;
  flex-direction: column;
  gap: var(--space-2);
  background: var(--accent-subtle);
}

.after-label {
  font-size: var(--text-xs);
  font-weight: var(--font-semibold);
  color: var(--accent);
  text-transform: uppercase;
  letter-spacing: 0.04em;
}

.after-text {
  font-size: var(--text-sm);
  color: var(--text-primary);
  line-height: var(--leading-relaxed);
  margin: 0;
}

/* Expand transition */
.expand-enter-active,
.expand-leave-active {
  transition: opacity var(--transition-fast), transform var(--transition-fast);
  overflow: hidden;
}

.expand-enter-from,
.expand-leave-to {
  opacity: 0;
  transform: translateY(-4px);
}

@media (max-width: 640px) {
  .task-actions {
    display: none;
  }
}
</style>
