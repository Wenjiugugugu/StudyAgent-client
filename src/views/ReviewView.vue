<script setup lang="ts">
import { ref, computed, onMounted, watch } from "vue";
import { useRoute, useRouter } from "vue-router";
import { useTodayStore } from "@/stores/today";
import { useSettingsStore } from "@/stores/settings";
import { todayString, yesterdayString, prevDateString, daysBetween } from "@/utils/date";
import * as api from "@/api";
import Card from "@/components/ui/Card.vue";
import Badge from "@/components/ui/Badge.vue";
import Button from "@/components/ui/Button.vue";
import EmptyState from "@/components/ui/EmptyState.vue";
import {
  CheckCircle2,
  Circle,
  AlertTriangle,
  ArrowRight,
  ArrowLeft,
  Check,
  Sparkles,
  Clock,
  Calendar,
  ChevronLeft,
  ChevronRight,
  History,
} from "lucide-vue-next";
import type { TaskReviewEntry, DailyReviewInput, ReviewRecord, OvercompletionEntry } from "@/types";

const todayStore = useTodayStore();
const settingsStore = useSettingsStore();
const route = useRoute();
const router = useRouter();

// ── Date navigation ──
const selectedDate = ref<string>(todayString());
const todayDate = todayString();
const yesterdayDate = computed(() => yesterdayString());

// 已有复盘的日期列表（用于跳转）
const reviewDates = ref<string[]>([]);

// 日期切换方向：用于过渡动画（prev=向右滑入，next=向左滑入）
const dateTransitionName = ref("date-slide-left");

function setDate(date: string, direction: "prev" | "next" | "jump" = "jump") {
  if (date === selectedDate.value) return;
  dateTransitionName.value = direction === "prev" ? "date-slide-right" : "date-slide-left";
  selectedDate.value = date;
  // 同步 URL query
  router.replace({ name: "review", query: { date } });
}

function goPrevDay() {
  setDate(prevDateString(selectedDate.value), "prev");
}

function goNextDay() {
  const [y, m, d] = selectedDate.value.split("-").map(Number);
  const dt = new Date(y, m - 1, d, 12, 0, 0);
  dt.setDate(dt.getDate() + 1);
  const next = `${dt.getFullYear()}-${String(dt.getMonth() + 1).padStart(2, "0")}-${String(dt.getDate()).padStart(2, "0")}`;
  // 不允许超过今天
  if (next <= todayDate) {
    setDate(next, "next");
  }
}

function goToday() {
  setDate(todayDate);
}

const isToday = computed(() => selectedDate.value === todayDate);
const isYesterday = computed(() => selectedDate.value === yesterdayDate.value);
const isFuture = computed(() => selectedDate.value > todayDate);
const isOlderThanYesterday = computed(() =>
  selectedDate.value < yesterdayDate.value && selectedDate.value < todayDate
);

// ── State ──
const step = ref(0);
const totalSteps = 6;
const submitting = ref(false);
const loading = ref(true);
const existingReview = ref<ReviewRecord | null>(null);
const submitted = ref(false);

// 重排状态：复盘提交后若需要 AI 重新生成剩余天数计划
const regenerating = ref(false);
const regenMessage = ref("");

// 超量完成：用户实际进度领先计划时填写
const hasOvercompletion = ref(false);
const overcompletions = ref<OvercompletionEntry[]>([]);

// ── Time gate ──
const endTime = computed(() => settingsStore.settings?.study_schedule?.end_time ?? "22:00");
const startTime = computed(() => settingsStore.settings?.study_schedule?.start_time ?? "09:00");
const beforeEndTime = computed(() => {
  if (!isToday.value) return false;
  const now = new Date();
  const [h, m] = endTime.value.split(":").map(Number);
  const end = new Date(now);
  end.setHours(h, m, 0, 0);
  return now < end;
});
// 当前时间是否在今天的学习开始时间之前（用于补复盘时间窗）
const beforeStartTimeToday = computed(() => {
  const now = new Date();
  const [h, m] = startTime.value.split(":").map(Number);
  const start = new Date(now);
  start.setHours(h, m, 0, 0);
  return now < start;
});

// ── 补复盘条件 ──
// 仅允许对「昨天」补复盘，且昨天没有复盘 + 当前时间在今天的学习开始时间之前
const canBackfill = computed(() =>
  isYesterday.value &&
  !existingReview.value &&
  beforeStartTimeToday.value
);

// 是否允许填写复盘表单
const canFillReview = computed(() => {
  if (isToday.value) {
    // 今天：需要在结束时间之后且无已有复盘
    return !beforeEndTime.value && !submitted.value;
  }
  if (canBackfill.value) {
    return true;
  }
  return false;
});

// 是否只读（查看历史复盘）
const isReadOnly = computed(() =>
  !!existingReview.value && submitted.value
);

// ── Tasks (from state, for the selected date) ──
const plan = computed(() => todayStore.plan);
const allTasks = computed(() => todayStore.allTasks);
const priorityATasks = computed(() => todayStore.priorityATasks);

// Step 1: task completion (read from state, initialized from task.status)
const taskCompleted = ref<Record<string, boolean>>({});

// Step 2: blockers (per incomplete Priority A task)
const taskBlockers = ref<Record<string, string[]>>({});
const blockerNotes = ref<Record<string, string>>({});

// Step 3: mastery (per completed task)
const taskMastery = ref<Record<string, string>>({});

// Step 4: overall feeling
const overallFeeling = ref("normal");

// Step 5: main difficulty
const mainDifficulty = ref("");

// ── Computed ──
const incompletePriorityA = computed(() => {
  return priorityATasks.value.filter(t => !taskCompleted.value[t.id]);
});

const doneTasks = computed(() => {
  return allTasks.value.filter(t => taskCompleted.value[t.id]);
});

// ── Labels ──
const blockerOptions = [
  { value: "time", label: "时间不足" },
  { value: "understanding", label: "理解困难" },
  { value: "practice", label: "练习不足" },
  { value: "memorization", label: "遗忘较多" },
  { value: "overload", label: "工作量安排过多" },
  { value: "interruption", label: "临时事务" },
  { value: "energy", label: "今天状态不好" },
  { value: "resource", label: "资源不足" },
  { value: "other", label: "其它" },
];

const masteryOptions = [
  { value: "mastered", label: "已掌握，可以继续下一部分" },
  { value: "basic", label: "基本掌握，建议简单复习" },
  { value: "weak", label: "掌握不足，希望继续巩固" },
];

const feelingOptions = [
  { value: "smooth", label: "很顺利", icon: "😊" },
  { value: "normal", label: "一般", icon: "😐" },
  { value: "hard", label: "比较困难", icon: "😣" },
];

const difficultyOptions = [
  { value: "understanding", label: "理解概念" },
  { value: "problems", label: "做题" },
  { value: "memorization", label: "记忆" },
  { value: "attention", label: "注意力" },
  { value: "time_management", label: "时间安排" },
  { value: "environment", label: "学习环境" },
  { value: "other", label: "其它" },
];

function subjectLabel(s: string): string {
  const m: Record<string, string> = { math: "数学", english: "英语", politics: "政治", professional: "专业课" };
  return m[s] ?? s;
}

// 反查任务标题（用于复盘记录展示，优先用 task_reviews 自带的 title，再回退到当日计划）
function findTaskTitle(taskId: string, tr?: TaskReviewEntry): string {
  if (tr?.title) return tr.title;
  return allTasks.value.find(t => t.id === taskId)?.title ?? "(任务已删除)";
}

function findTaskSubject(taskId: string, tr?: TaskReviewEntry): string {
  if (tr?.subject) return tr.subject;
  return allTasks.value.find(t => t.id === taskId)?.subject ?? "";
}

function findTaskPriority(taskId: string, tr?: TaskReviewEntry): string {
  if (tr?.priority) return tr.priority;
  return allTasks.value.find(t => t.id === taskId)?.priority ?? "";
}

// 超量完成：可选科目（基于今日计划中出现的科目）
const overcompletionSubjectOptions = computed(() => {
  const subjects = new Set<string>();
  for (const t of allTasks.value) subjects.add(t.subject);
  return Array.from(subjects).map(s => ({ value: s, label: subjectLabel(s) }));
});

function addOvercompletion() {
  const firstSubject = overcompletionSubjectOptions.value[0]?.value ?? "math";
  overcompletions.value.push({ subject: firstSubject, chapter_reached: "", note: undefined });
}

function removeOvercompletion(idx: number) {
  overcompletions.value.splice(idx, 1);
}

function subjectBadgeVariant(s: string): "math" | "english" | "politics" | "professional" | "default" {
  const set = new Set(["math", "english", "politics", "professional"]);
  return set.has(s) ? (s as any) : "default";
}

function statusLabel(s: string): string {
  const m: Record<string, string> = { completed: "已完成", partial: "部分完成", incomplete: "未完成", abandoned: "放弃" };
  return m[s] ?? s;
}

function feelingLabel(s: string): string {
  const m: Record<string, string> = { smooth: "😊 很顺利", normal: "😐 一般", hard: "😣 比较困难" };
  return m[s] ?? s;
}

function difficultyLabel(s: string): string {
  const m: Record<string, string> = {
    understanding: "理解概念",
    problems: "做题",
    memorization: "记忆",
    attention: "注意力",
    time_management: "时间安排",
    environment: "学习环境",
    other: "其它",
  };
  return m[s] ?? (s || "—");
}

function toggleBlocker(taskId: string, value: string) {
  const current = taskBlockers.value[taskId] ?? [];
  if (current.includes(value)) {
    taskBlockers.value[taskId] = current.filter(v => v !== value);
  } else {
    taskBlockers.value[taskId] = [...current, value];
  }
}

// ── Init from state ──
function initFromState() {
  if (!plan.value) return;
  for (const task of allTasks.value) {
    taskCompleted.value[task.id] = task.status === "done";
  }
}

// 从已有复盘初始化（用于查看模式展示，兼容旧版与新版）
function initFromReview(review: ReviewRecord) {
  // 新版：优先使用 task_reviews
  if (review.task_reviews?.length) {
    for (const tr of review.task_reviews) {
      taskCompleted.value[tr.task_id] = tr.status === "completed";
      if (tr.mastery) taskMastery.value[tr.task_id] = tr.mastery;
      if (tr.blockers?.length) taskBlockers.value[tr.task_id] = [...tr.blockers];
      if (tr.blocker_note) blockerNotes.value[tr.task_id] = tr.blocker_note;
    }
  } else if (review.data?.completed_tasks?.length) {
    // 旧版：从 data.completed_tasks 回填完成状态
    for (const ct of review.data.completed_tasks) {
      const tid = ct.task_id ?? ct.title;
      if (tid) taskCompleted.value[tid] = ct.completed;
    }
  }
  if (review.daily_review) {
    overallFeeling.value = review.daily_review.overall_feeling || "normal";
    mainDifficulty.value = review.daily_review.main_difficulty || "";
  }
  // 超量完成记录（仅用于只读展示）
  if (review.overcompletion?.length) {
    hasOvercompletion.value = true;
    overcompletions.value = review.overcompletion.map(oc => ({ ...oc }));
  }
}

function resetForm() {
  step.value = 0;
  taskCompleted.value = {};
  taskBlockers.value = {};
  blockerNotes.value = {};
  taskMastery.value = {};
  overallFeeling.value = "normal";
  mainDifficulty.value = "";
  hasOvercompletion.value = false;
  overcompletions.value = [];
}

// ── Navigation ──
function canNext(): boolean {
  switch (step.value) {
    case 0: return allTasks.value.length > 0;
    case 3: return !!overallFeeling.value;
    default: return true;
  }
}

function goNext() { if (step.value < totalSteps - 1) step.value++; }
function goPrev() { if (step.value > 0) step.value--; }

// ── Submit ──
async function doSubmit() {
  submitting.value = true;
  try {
    const taskReviews: TaskReviewEntry[] = allTasks.value.map(t => ({
      task_id: t.id,
      status: taskCompleted.value[t.id] ? "completed" : "incomplete",
      completion: taskCompleted.value[t.id] ? 1.0 : 0.0,
      mastery: taskMastery.value[t.id] || "",
      blockers: taskBlockers.value[t.id] || [],
      blocker_note: blockerNotes.value[t.id] || undefined,
      title: t.title,
      subject: t.subject,
      priority: t.priority,
    }));

    const dailyReview: DailyReviewInput = {
      overall_feeling: overallFeeling.value,
      main_difficulty: mainDifficulty.value,
    };

    // 仅在用户勾选超量完成且填写了有效章节时提交
    const validOvercompletions = hasOvercompletion.value
      ? overcompletions.value.filter(oc => oc.subject && oc.chapter_reached.trim())
      : [];

    const result = await api.submitReview({
      date: selectedDate.value,
      task_reviews: taskReviews,
      daily_review: dailyReview,
      overcompletion: validOvercompletions.length > 0 ? validOvercompletions : undefined,
    });
    submitted.value = true;

    // 若需要 AI 重排剩余天数，提示用户并调用
    if (result.needs_regeneration) {
      regenerating.value = true;
      regenMessage.value = "正在调整后续计划，请勿关闭应用…";
      try {
        const regenResult = await api.regenerateRemainingDays(selectedDate.value);
        if (regenResult.regenerated) {
          regenMessage.value = `已调整后续 ${regenResult.affected_dates.length} 天的计划安排`;
        } else {
          regenMessage.value = "";
        }
      } catch (e) {
        console.error("调整后续计划失败:", e);
        regenMessage.value = "调整失败，不影响复盘结果。";
      } finally {
        regenerating.value = false;
      }
    }

    // 重新加载复盘
    await loadReviewData();
    // 刷新复盘日期列表
    await loadReviewDates();
  } catch (e) {
    console.error("提交复盘失败:", e);
  } finally {
    submitting.value = false;
  }
}

// ── Data loading ──
async function loadReviewData() {
  loading.value = true;
  resetForm();
  existingReview.value = null;
  submitted.value = false;

  try {
    // 加载选中日期的计划（用于显示任务列表）
    await todayStore.loadByDate(selectedDate.value);

    // 检查是否已有复盘
    try {
      existingReview.value = await api.getReview(selectedDate.value);
      if (existingReview.value) {
        submitted.value = true;
        // 从已有复盘初始化表单数据（用于只读展示）
        initFromReview(existingReview.value);
      }
    } catch {
      // 无已有复盘
    }

    // 如果不是只读模式，从 state 初始化任务完成状态
    if (!submitted.value) {
      initFromState();
    }
  } finally {
    loading.value = false;
  }
}

async function loadReviewDates() {
  try {
    reviewDates.value = await api.listReviewDates();
  } catch {
    reviewDates.value = [];
  }
}

// 跳转到指定复盘日期
function jumpToReviewDate(date: string) {
  // 根据目标日期与当前日期的关系决定方向
  const dir = date < selectedDate.value ? "prev" : "next";
  setDate(date, dir);
}

// ── Watch date changes ──
watch(selectedDate, () => {
  loadReviewData();
});

// 从 URL query 初始化日期
function initDateFromQuery() {
  const q = route.query.date;
  if (typeof q === "string" && /^\d{4}-\d{2}-\d{2}$/.test(q)) {
    selectedDate.value = q;
  }
}

onMounted(async () => {
  initDateFromQuery();
  await Promise.all([loadReviewData(), loadReviewDates()]);
});

// 历史复盘日期下拉
const showHistoryDropdown = ref(false);
const sortedReviewDates = computed(() => [...reviewDates.value].reverse());
</script>

<template>
  <div class="review-view">
    <!-- Date navigation bar -->
    <div class="date-bar">
      <div class="date-nav">
        <Button
          variant="ghost"
          size="sm"
          :disabled="selectedDate <= '2020-01-01'"
          @click="goPrevDay"
        >
          <ChevronLeft :size="16" />
        </Button>
        <div class="date-display">
          <Calendar :size="14" />
          <span class="date-text">{{ selectedDate }}</span>
          <span v-if="isToday" class="date-tag today-tag">今天</span>
          <span v-else-if="isYesterday" class="date-tag yesterday-tag">昨天</span>
          <span v-else-if="isFuture" class="date-tag future-tag">未来</span>
          <span v-else class="date-tag past-tag">历史</span>
        </div>
        <Button
          variant="ghost"
          size="sm"
          :disabled="isToday"
          @click="goNextDay"
        >
          <ChevronRight :size="16" />
        </Button>
      </div>
      <div class="date-actions">
        <!-- 历史复盘下拉 -->
        <div class="history-dropdown-wrapper">
          <Button
            variant="ghost"
            size="sm"
            @click="showHistoryDropdown = !showHistoryDropdown"
          >
            <History :size="14" />
            历史复盘
          </Button>
          <div v-if="showHistoryDropdown" class="history-dropdown" @click.stop>
            <div class="dropdown-header">选择日期查看复盘</div>
            <div v-if="sortedReviewDates.length === 0" class="dropdown-empty">
              暂无复盘记录
            </div>
            <button
              v-for="date in sortedReviewDates"
              :key="date"
              type="button"
              class="dropdown-item"
              :class="{ active: date === selectedDate }"
              @click="jumpToReviewDate(date); showHistoryDropdown = false"
            >
              {{ date }}
              <span v-if="date === todayDate" class="item-tag">今天</span>
              <span v-else-if="date === yesterdayDate" class="item-tag">昨天</span>
            </button>
          </div>
        </div>
        <Button v-if="!isToday" variant="ghost" size="sm" @click="goToday">
          回到今天
        </Button>
      </div>
    </div>

    <!-- Loading -->
    <transition :name="dateTransitionName" mode="out-in">
    <div :key="selectedDate" class="review-content">
    <div v-if="loading" class="loading-msg">加载中…</div>

    <!-- Future date -->
    <Card v-else-if="isFuture" padding="lg" class="gate-card">
      <div class="gate-hero">
        <div class="gate-icon"><Calendar :size="40" /></div>
        <h1 class="gate-title">未来日期</h1>
        <p class="gate-desc">无法为未来日期创建复盘。</p>
        <Button variant="primary" size="sm" @click="goToday">回到今天</Button>
      </div>
    </Card>

    <!-- Today: before end time -->
    <Card v-else-if="isToday && beforeEndTime" padding="lg" class="gate-card">
      <div class="gate-hero">
        <div class="gate-icon"><Clock :size="40" /></div>
        <h1 class="gate-title">今日尚未结束</h1>
        <p class="gate-desc">每日复盘需在 {{ endTime }} 之后进行。</p>
        <p class="gate-hint">请在学习结束后再来复盘。</p>
      </div>
    </Card>

    <!-- No plan -->
    <EmptyState
      v-else-if="!plan"
      :title="`${selectedDate} 没有学习计划`"
      :description="isToday ? '请先生成今日计划后，再进行复盘' : '该日无学习计划，无法复盘'"
    >
      <template #actions>
        <Button v-if="isToday" variant="primary" @click="router.push('/today')">
          生成今日计划
        </Button>
        <Button v-if="!isToday" variant="secondary" @click="goToday">回到今天</Button>
      </template>
    </EmptyState>

    <!-- Yesterday without review and past start_time (cannot backfill) -->
    <Card v-else-if="isYesterday && !existingReview && !beforeStartTimeToday" padding="lg" class="gate-card">
      <div class="gate-hero">
        <div class="gate-icon"><AlertTriangle :size="40" /></div>
        <h1 class="gate-title">无法补复盘</h1>
        <p class="gate-desc">昨天未进行复盘，但今日学习已开始。</p>
        <p class="gate-hint">补复盘仅可在今日学习开始时间（{{ startTime }}）之前进行。</p>
      </div>
    </Card>

    <!-- Older than yesterday without review -->
    <Card v-else-if="isOlderThanYesterday && !existingReview" padding="lg" class="gate-card">
      <div class="gate-hero">
        <div class="gate-icon"><Calendar :size="40" /></div>
        <h1 class="gate-title">无复盘记录</h1>
        <p class="gate-desc">{{ selectedDate }} 没有复盘记录。</p>
        <p class="gate-hint">仅可对昨天补复盘，更早的日期无法补录。</p>
      </div>
    </Card>

    <!-- Already submitted / read-only review -->
    <template v-else-if="submitted && existingReview">
      <Card padding="lg" class="done-card">
        <div class="done-hero">
          <div class="done-badge"><Check :size="32" /></div>
          <h1 class="done-title">
            {{ isToday ? '今日复盘已完成' : `${selectedDate} 复盘记录` }}
          </h1>
          <p class="done-desc">{{ isToday ? '今天的结构化复盘已保存。' : '查看历史复盘记录。' }}</p>

          <!-- 重排提示 -->
          <div v-if="regenMessage" class="regen-banner" :class="{ 'regen-loading': regenerating }">
            <AlertTriangle :size="18" v-if="regenerating" />
            <CheckCircle2 :size="18" v-else />
            <span>{{ regenMessage }}</span>
          </div>

          <!-- Review summary -->
          <div v-if="existingReview.task_reviews?.length || existingReview.data?.completed_tasks?.length" class="review-summary">
            <div class="summary-row">
              <span class="summary-label">完成率</span>
              <span class="summary-value">
                <template v-if="existingReview.task_reviews?.length">
                  {{ existingReview.task_reviews.filter(t => t.status === 'completed').length }} / {{ existingReview.task_reviews.length }}
                </template>
                <template v-else>
                  {{ existingReview.data.completed_tasks.filter(t => t.completed).length }} / {{ existingReview.data.completed_tasks.length }}
                </template>
              </span>
            </div>
            <div v-if="existingReview.data?.total_hours" class="summary-row">
              <span class="summary-label">学习时长</span>
              <span class="summary-value">{{ existingReview.data.total_hours.toFixed(1) }} 小时</span>
            </div>
            <div v-if="existingReview.daily_review?.overall_feeling" class="summary-row">
              <span class="summary-label">整体感受</span>
              <span class="summary-value">{{ feelingLabel(existingReview.daily_review.overall_feeling) }}</span>
            </div>
            <div v-if="existingReview.daily_review?.main_difficulty" class="summary-row">
              <span class="summary-label">最大困难</span>
              <span class="summary-value">{{ difficultyLabel(existingReview.daily_review.main_difficulty) }}</span>
            </div>
          </div>

          <!-- Task-level review details (new version) -->
          <div v-if="existingReview.task_reviews?.length" class="task-reviews-list">
            <div class="task-reviews-title">任务复盘详情</div>
            <div
              v-for="tr in existingReview.task_reviews"
              :key="tr.task_id"
              class="task-review-row"
              :class="tr.status"
            >
              <div class="trr-left">
                <div class="trr-status-icon">
                  <CheckCircle2 v-if="tr.status === 'completed'" :size="16" />
                  <Circle v-else :size="16" />
                </div>
                <div class="trr-title-wrap">
                  <span class="trr-title">{{ findTaskTitle(tr.task_id, tr) }}</span>
                  <div class="trr-meta">
                    <span v-if="findTaskSubject(tr.task_id, tr)" class="trr-subject">{{ subjectLabel(findTaskSubject(tr.task_id, tr)) }}</span>
                    <span class="trr-status">{{ statusLabel(tr.status) }}</span>
                  </div>
                </div>
              </div>
              <div class="trr-right">
                <span v-if="tr.mastery" class="trr-chip mastery">
                  {{ tr.mastery === 'mastered' ? '已掌握' : tr.mastery === 'basic' ? '基本掌握' : '需巩固' }}
                </span>
                <span v-for="b in tr.blockers" :key="b" class="trr-chip blocker">
                  {{ blockerOptions.find(o => o.value === b)?.label ?? b }}
                </span>
                <span v-if="tr.blocker_note" class="trr-note">{{ tr.blocker_note }}</span>
              </div>
            </div>
          </div>

          <!-- Old version: completed_tasks fallback -->
          <div v-else-if="existingReview.data?.completed_tasks?.length" class="task-reviews-list">
            <div class="task-reviews-title">任务完成情况（旧版记录）</div>
            <div
              v-for="ct in existingReview.data.completed_tasks"
              :key="ct.task_id ?? ct.title"
              class="task-review-row"
              :class="ct.completed ? 'completed' : 'incomplete'"
            >
              <div class="trr-left">
                <div class="trr-status-icon">
                  <CheckCircle2 v-if="ct.completed" :size="16" />
                  <Circle v-else :size="16" />
                </div>
                <div class="trr-title-wrap">
                  <span class="trr-title">{{ ct.title }}</span>
                  <div class="trr-meta">
                    <span class="trr-subject">{{ subjectLabel(ct.subject) }}</span>
                    <span class="trr-status">{{ ct.completed ? '已完成' : '未完成' }}</span>
                  </div>
                </div>
              </div>
            </div>
          </div>

          <!-- Overcompletion records -->
          <div v-if="existingReview.overcompletion?.length" class="task-reviews-list">
            <div class="task-reviews-title">超量完成记录</div>
            <div
              v-for="(oc, idx) in existingReview.overcompletion"
              :key="idx"
              class="task-review-row overcompletion"
            >
              <div class="trr-left">
                <div class="trr-status-icon"><Sparkles :size="16" /></div>
                <div class="trr-title-wrap">
                  <span class="trr-title">{{ subjectLabel(oc.subject) }}：{{ oc.chapter_reached }}</span>
                  <div class="trr-meta">
                    <span class="trr-subject">{{ subjectLabel(oc.subject) }}</span>
                    <span class="trr-status">实际进度</span>
                  </div>
                </div>
              </div>
              <div class="trr-right">
                <span v-if="oc.note" class="trr-note">{{ oc.note }}</span>
              </div>
            </div>
          </div>

          <p class="done-hint">复盘数据将在下一次 Planner 中自动生效。</p>
        </div>
      </Card>
    </template>

    <!-- Steps (fill review) -->
    <template v-else-if="canFillReview">
      <div class="backfill-banner" v-if="isYesterday">
        <AlertTriangle :size="14" />
        <span>补复盘模式：为昨天（{{ selectedDate }}）补录复盘</span>
      </div>

      <div class="step-bar">
        <div class="step-dots">
          <span v-for="i in totalSteps" :key="i" class="step-dot"
            :class="{ active: i - 1 === step, done: i - 1 < step }" />
        </div>
        <span class="step-label">{{ step + 1 }} / {{ totalSteps }}</span>
      </div>

      <!-- Step 1: Task Completion -->
      <Card v-if="step === 0" padding="lg" class="step-card">
        <h2 class="step-title">任务完成情况</h2>
        <p class="step-desc">勾选{{ isYesterday ? '昨天' : '今天' }}已完成的任务（自动读取 State 中的完成状态）</p>
        <div class="task-review-list">
          <div v-for="task in allTasks" :key="task.id" class="task-review-item"
            :class="{ done: taskCompleted[task.id] }">
            <div class="tri-left">
              <div class="tri-badges">
                <Badge :variant="subjectBadgeVariant(task.subject)" size="sm">{{ subjectLabel(task.subject) }}</Badge>
                <Badge :variant="task.priority === 'A' ? 'danger' : 'warning'" size="sm">P{{ task.priority }}</Badge>
              </div>
              <span class="tri-title">{{ task.title }}</span>
            </div>
            <button type="button" class="check-btn" :class="{ checked: taskCompleted[task.id] }"
              @click="taskCompleted[task.id] = !taskCompleted[task.id]">
              <CheckCircle2 v-if="taskCompleted[task.id]" :size="18" />
              <Circle v-else :size="18" />
              {{ taskCompleted[task.id] ? '已完成' : '未完成' }}
            </button>
          </div>
        </div>
      </Card>

      <!-- Step 2: Blockers -->
      <Card v-if="step === 1 && incompletePriorityA.length > 0" padding="lg" class="step-card">
        <h2 class="step-title">未完成原因</h2>
        <p class="step-desc">以下 Priority A 任务未能完成，请选择原因</p>
        <div v-for="task in incompletePriorityA" :key="task.id" class="blocker-item">
          <div class="blocker-task">
            <Badge variant="danger" size="sm">P{{ task.priority }}</Badge>
            <span class="blocker-title">{{ task.title }}</span>
          </div>
          <div class="blocker-chips">
            <button v-for="opt in blockerOptions" :key="opt.value" type="button" class="blocker-chip"
              :class="{ active: (taskBlockers[task.id] ?? []).includes(opt.value) }"
              @click="toggleBlocker(task.id, opt.value)">{{ opt.label }}</button>
          </div>
          <input v-if="(taskBlockers[task.id] ?? []).includes('other')"
            v-model="blockerNotes[task.id]" type="text" class="field-input" placeholder="请说明具体原因..." />
        </div>
      </Card>

      <Card v-if="step === 1 && incompletePriorityA.length === 0" padding="lg" class="step-card">
        <div class="empty-step">
          <CheckCircle2 :size="32" class="empty-icon" />
          <p>所有 Priority A 任务均已完成，无需填写原因。</p>
        </div>
      </Card>

      <!-- Step 3: Mastery -->
      <Card v-if="step === 2 && doneTasks.length > 0" padding="lg" class="step-card">
        <h2 class="step-title">掌握情况</h2>
        <p class="step-desc">针对已完成的任务，评估你的掌握程度</p>
        <div v-for="task in doneTasks" :key="task.id" class="mastery-item">
          <div class="mastery-task">
            <Badge :variant="subjectBadgeVariant(task.subject)" size="sm">{{ subjectLabel(task.subject) }}</Badge>
            <span class="mastery-title">{{ task.title }}</span>
          </div>
          <div class="mastery-chips">
            <button v-for="opt in masteryOptions" :key="opt.value" type="button" class="mastery-chip"
              :class="{ active: taskMastery[task.id] === opt.value }"
              @click="taskMastery[task.id] = opt.value">{{ opt.label }}</button>
          </div>
        </div>
      </Card>

      <Card v-if="step === 2 && doneTasks.length === 0" padding="lg" class="step-card">
        <div class="empty-step">
          <AlertTriangle :size="32" class="empty-icon warn" />
          <p>没有已完成的任务需要评估掌握程度。</p>
        </div>
      </Card>

      <!-- Step 4: Overall Feeling -->
      <Card v-if="step === 3" padding="lg" class="step-card">
        <h2 class="step-title">整体学习感受</h2>
        <p class="step-desc">{{ isYesterday ? '昨天' : '今天' }}整体学习感觉如何？</p>
        <div class="feeling-grid">
          <button v-for="opt in feelingOptions" :key="opt.value" type="button" class="feeling-chip"
            :class="{ active: overallFeeling === opt.value }" @click="overallFeeling = opt.value">
            <span class="feeling-emoji">{{ opt.icon }}</span>
            <span>{{ opt.label }}</span>
          </button>
        </div>
      </Card>

      <!-- Step 5: Main Difficulty -->
      <Card v-if="step === 4" padding="lg" class="step-card">
        <h2 class="step-title">最大困难（可选）</h2>
        <p class="step-desc">{{ isYesterday ? '昨天' : '今天' }}最大的困难是什么？用于 Analytics 分析。</p>
        <div class="difficulty-grid">
          <button v-for="opt in difficultyOptions" :key="opt.value" type="button" class="difficulty-chip"
            :class="{ active: mainDifficulty === opt.value }"
            @click="mainDifficulty = mainDifficulty === opt.value ? '' : opt.value">{{ opt.label }}</button>
        </div>
      </Card>

      <!-- Step 6: Overcompletion (extra, optional) -->
      <Card v-if="step === 5" padding="lg" class="step-card">
        <h2 class="step-title">超量完成（可选）</h2>
        <p class="step-desc">如果{{ isYesterday ? '昨天' : '今天' }}实际学习进度领先于计划安排，请在此记录实际到达的章节，避免下次生成计划时进度落后于实际。</p>
        <div class="overcompletion-toggle">
          <button type="button" class="oc-switch" :class="{ active: hasOvercompletion }"
            @click="hasOvercompletion = !hasOvercompletion">
            <CheckCircle2 v-if="hasOvercompletion" :size="18" />
            <Circle v-else :size="18" />
            {{ hasOvercompletion ? '已开启超量完成记录' : '我今天超量完成了任务' }}
          </button>
        </div>
        <div v-if="hasOvercompletion" class="overcompletion-list">
          <div v-for="(oc, idx) in overcompletions" :key="idx" class="oc-item">
            <div class="oc-row">
              <select v-model="oc.subject" class="oc-select">
                <option v-for="opt in overcompletionSubjectOptions" :key="opt.value" :value="opt.value">{{ opt.label }}</option>
              </select>
              <input v-model="oc.chapter_reached" type="text" class="field-input oc-input"
                placeholder="实际已学习到的章节（如：多元函数微分学）" />
              <Button variant="ghost" size="sm" @click="removeOvercompletion(idx)">
                <AlertTriangle :size="14" />
              </Button>
            </div>
            <input v-model="oc.note" type="text" class="field-input" placeholder="备注（可选）" />
          </div>
          <Button variant="secondary" size="sm" @click="addOvercompletion">
            <Sparkles :size="14" /> 添加一条
          </Button>
        </div>
        <div v-if="hasOvercompletion && overcompletions.length === 0" class="empty-step">
          <Sparkles :size="32" class="empty-icon" />
          <p>点击「添加一条」记录实际进度领先的科目与章节。</p>
        </div>
      </Card>

      <!-- Navigation -->
      <div class="step-nav">
        <Button v-if="step > 0" variant="ghost" @click="goPrev"><ArrowLeft :size="16" /> 上一步</Button>
        <div class="nav-right">
          <Button v-if="step < totalSteps - 1" variant="primary" :disabled="!canNext()" @click="goNext">
            下一步 <ArrowRight :size="16" />
          </Button>
          <Button v-else variant="primary" @click="doSubmit" :loading="submitting">
            <Sparkles :size="16" /> 提交复盘
          </Button>
        </div>
      </div>
    </template>
    </div>
    </transition>
  </div>
</template>

<style scoped>
.review-view {
  max-width: 720px;
  margin: 0 auto;
  padding: var(--space-8);
  display: flex;
  flex-direction: column;
  gap: var(--space-4);
}

/* Date bar */
.date-bar {
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
  flex-wrap: wrap;
}

.date-nav {
  display: inline-flex;
  align-items: center;
  gap: var(--space-2);
}

.date-display {
  display: inline-flex;
  align-items: center;
  gap: var(--space-2);
  font-size: var(--text-sm);
  font-weight: var(--font-semibold);
  color: var(--text-primary);
  min-width: 160px;
  justify-content: center;
}

.date-tag {
  font-size: var(--text-xs);
  padding: 1px 8px;
  border-radius: var(--radius-full);
  font-weight: var(--font-medium);
}

.today-tag { background: var(--accent-subtle); color: var(--accent); }
.yesterday-tag { background: var(--color-warning-subtle); color: var(--color-warning); }
.past-tag { background: var(--bg-tertiary); color: var(--text-tertiary); }
.future-tag { background: var(--bg-overlay); color: var(--text-quaternary); }

.date-actions {
  display: inline-flex;
  align-items: center;
  gap: var(--space-2);
  position: relative;
}

.history-dropdown-wrapper {
  position: relative;
}

.history-dropdown {
  position: absolute;
  top: calc(100% + 4px);
  right: 0;
  min-width: 200px;
  max-height: 320px;
  overflow-y: auto;
  background: var(--bg-elevated);
  border: 1px solid var(--border-color);
  border-radius: var(--radius-md);
  box-shadow: var(--shadow-lg);
  z-index: 20;
  padding: var(--space-1);
}

.dropdown-header {
  font-size: var(--text-xs);
  color: var(--text-tertiary);
  padding: var(--space-2) var(--space-3);
  font-weight: var(--font-medium);
  border-bottom: 1px solid var(--divider-color);
  margin-bottom: var(--space-1);
}

.dropdown-empty {
  font-size: var(--text-sm);
  color: var(--text-tertiary);
  padding: var(--space-3);
  text-align: center;
}

.dropdown-item {
  display: flex;
  align-items: center;
  justify-content: space-between;
  width: 100%;
  padding: var(--space-2) var(--space-3);
  background: transparent;
  border: none;
  border-radius: var(--radius-sm);
  font-size: var(--text-sm);
  color: var(--text-primary);
  cursor: pointer;
  font-family: inherit;
  transition: background var(--transition-fast);
}

.dropdown-item:hover { background: var(--bg-overlay); }
.dropdown-item.active { background: var(--accent-subtle); color: var(--accent); font-weight: var(--font-semibold); }

.item-tag {
  font-size: 10px;
  color: var(--text-tertiary);
  background: var(--bg-tertiary);
  padding: 1px 6px;
  border-radius: var(--radius-full);
}

/* Backfill banner */
.backfill-banner {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  padding: var(--space-2) var(--space-3);
  background: var(--color-warning-subtle);
  color: var(--color-warning);
  border-radius: var(--radius-md);
  font-size: var(--text-sm);
  font-weight: var(--font-medium);
}

.loading-msg {
  text-align: center;
  color: var(--text-tertiary);
  padding: var(--space-8);
}

/* Gate */
.gate-card { text-align: center; }
.gate-hero {
  display: flex; flex-direction: column; align-items: center;
  gap: var(--space-4); padding: var(--space-8) var(--space-4);
}
.gate-icon { color: var(--text-tertiary); }
.gate-title { font-size: var(--text-xl); font-weight: var(--font-bold); color: var(--text-primary); }
.gate-desc { font-size: var(--text-base); color: var(--text-secondary); margin: 0; }
.gate-hint { font-size: var(--text-sm); color: var(--text-tertiary); margin: 0; }

.step-bar {
  display: flex; align-items: center; gap: var(--space-3); padding-bottom: var(--space-2);
}
.step-dots { display: flex; gap: var(--space-2); flex: 1; }
.step-dot {
  width: 100%; height: 4px; background: var(--bg-tertiary);
  border-radius: var(--radius-full); transition: background var(--transition-fast);
}
.step-dot.active { background: var(--accent); }
.step-dot.done { background: var(--color-success); }
.step-label {
  font-size: var(--text-xs); color: var(--text-tertiary);
  font-weight: var(--font-medium); flex-shrink: 0;
}

.step-card { display: flex; flex-direction: column; gap: var(--space-5); }
.step-title {
  font-size: var(--text-xl); font-weight: var(--font-bold);
  color: var(--text-primary); letter-spacing: -0.01em;
}
.step-desc { font-size: var(--text-sm); color: var(--text-secondary); margin: 0; margin-top: -12px; }

/* Task list */
.task-review-list { display: flex; flex-direction: column; gap: var(--space-3); }
.task-review-item {
  display: flex; align-items: center; justify-content: space-between; gap: var(--space-3);
  padding: var(--space-3) var(--space-4); background: var(--bg-tertiary);
  border-radius: var(--radius-md); flex-wrap: wrap;
}
.task-review-item.done { opacity: 0.65; background: var(--bg-overlay); }
.tri-left { display: flex; flex-direction: column; gap: var(--space-1); min-width: 0; flex: 1; }
.tri-badges { display: flex; gap: var(--space-1); }
.tri-title {
  font-size: var(--text-sm); font-weight: var(--font-medium);
  color: var(--text-primary); overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
}

.check-btn {
  display: flex; align-items: center; gap: var(--space-2);
  padding: var(--space-2) var(--space-3);
  border: 1.5px solid var(--border-color); border-radius: var(--radius-md);
  background: var(--bg-elevated); color: var(--text-secondary);
  font-size: var(--text-sm); font-weight: var(--font-medium);
  cursor: pointer; transition: all var(--transition-fast);
  font-family: inherit; white-space: nowrap; flex-shrink: 0;
}
.check-btn:hover { border-color: var(--accent); color: var(--accent); }
.check-btn.checked { border-color: var(--color-success); background: var(--color-success-subtle); color: var(--color-success); font-weight: var(--font-semibold); }

/* Blockers */
.blocker-item {
  display: flex; flex-direction: column; gap: var(--space-3);
  padding: var(--space-4); background: var(--bg-tertiary); border-radius: var(--radius-md);
}
.blocker-task { display: flex; align-items: center; gap: var(--space-2); flex-wrap: wrap; }
.blocker-title { font-size: var(--text-base); font-weight: var(--font-semibold); color: var(--text-primary); }
.blocker-chips { display: flex; flex-wrap: wrap; gap: var(--space-2); }
.blocker-chip {
  padding: var(--space-1) var(--space-3); border: 1.5px solid var(--border-color);
  border-radius: var(--radius-full); background: var(--bg-elevated);
  color: var(--text-secondary); font-size: var(--text-xs); font-weight: var(--font-medium);
  cursor: pointer; transition: all var(--transition-fast); font-family: inherit;
}
.blocker-chip:hover { border-color: var(--accent); }
.blocker-chip.active { border-color: var(--color-danger); background: var(--color-danger-subtle); color: var(--color-danger); font-weight: var(--font-semibold); }

.field-input {
  background: var(--bg-elevated); border: 1px solid var(--border-color);
  border-radius: var(--radius-md); padding: var(--space-2) var(--space-3);
  font-size: var(--text-sm); font-family: inherit; color: var(--text-primary);
  width: 100%; outline: none;
}
.field-input:focus { border-color: var(--accent); }

/* Mastery */
.mastery-item {
  display: flex; flex-direction: column; gap: var(--space-3);
  padding: var(--space-4); background: var(--bg-tertiary); border-radius: var(--radius-md);
}
.mastery-task { display: flex; align-items: center; gap: var(--space-2); }
.mastery-title { font-size: var(--text-base); font-weight: var(--font-semibold); color: var(--text-primary); }
.mastery-chips { display: flex; flex-direction: column; gap: var(--space-2); }
.mastery-chip {
  padding: var(--space-3) var(--space-4); border: 1.5px solid var(--border-color);
  border-radius: var(--radius-md); background: var(--bg-elevated);
  color: var(--text-secondary); font-size: var(--text-sm);
  cursor: pointer; transition: all var(--transition-fast); font-family: inherit; text-align: left;
}
.mastery-chip:hover { border-color: var(--accent); }
.mastery-chip.active { border-color: var(--accent); background: var(--accent-subtle); color: var(--accent); font-weight: var(--font-semibold); }

/* Empty step */
.empty-step {
  display: flex; flex-direction: column; align-items: center; gap: var(--space-3);
  padding: var(--space-8) var(--space-4); color: var(--text-secondary);
  font-size: var(--text-sm); text-align: center;
}
.empty-icon { color: var(--color-success); }
.empty-icon.warn { color: var(--color-warning); }

/* Feeling */
.feeling-grid { display: grid; grid-template-columns: repeat(3, 1fr); gap: var(--space-3); }
.feeling-chip {
  display: flex; flex-direction: column; align-items: center; gap: var(--space-2);
  padding: var(--space-5) var(--space-3); border: 1.5px solid var(--border-color);
  border-radius: var(--radius-lg); background: var(--bg-elevated);
  cursor: pointer; transition: all var(--transition-fast);
  font-family: inherit; font-size: var(--text-sm); color: var(--text-secondary);
}
.feeling-chip:hover { border-color: var(--accent); color: var(--accent); }
.feeling-chip.active { border-color: var(--accent); background: var(--accent-subtle); color: var(--accent); font-weight: var(--font-semibold); }
.feeling-emoji { font-size: 24px; }

/* Difficulty */
.difficulty-grid { display: flex; flex-wrap: wrap; gap: var(--space-2); }
.difficulty-chip {
  padding: var(--space-2) var(--space-4); border: 1.5px solid var(--border-color);
  border-radius: var(--radius-full); background: var(--bg-elevated);
  color: var(--text-secondary); font-size: var(--text-sm); font-weight: var(--font-medium);
  cursor: pointer; transition: all var(--transition-fast); font-family: inherit;
}
.difficulty-chip:hover { border-color: var(--accent); }
.difficulty-chip.active { border-color: var(--accent); background: var(--accent-subtle); color: var(--accent); }

/* Nav */
.step-nav {
  display: flex; align-items: center; justify-content: space-between;
  gap: var(--space-3); padding-top: var(--space-4);
}
.nav-right { margin-left: auto; }

/* Done / existing review */
.done-card { text-align: center; }
.done-hero {
  display: flex; flex-direction: column; align-items: center; gap: var(--space-4);
  padding: var(--space-8) var(--space-4);
}
.done-badge {
  width: 72px; height: 72px; display: flex; align-items: center; justify-content: center;
  background: var(--color-success-subtle); color: var(--color-success); border-radius: var(--radius-lg);
}
.done-title { font-size: var(--text-2xl); font-weight: var(--font-bold); color: var(--text-primary); letter-spacing: -0.02em; }
.done-desc { font-size: var(--text-base); color: var(--text-secondary); margin: 0; }
.done-hint { font-size: var(--text-sm); color: var(--text-tertiary); margin: 0; }

.regen-banner {
  display: flex; align-items: center; gap: var(--space-2);
  padding: var(--space-3) var(--space-4);
  border-radius: var(--radius-md);
  font-size: var(--text-sm);
  margin-top: var(--space-3);
  background: var(--color-info-subtle, var(--bg-tertiary));
  color: var(--color-info, var(--text-secondary));
}
.regen-banner.regen-loading {
  background: var(--color-warning-subtle, var(--bg-tertiary));
  color: var(--color-warning, var(--text-primary));
}

.review-summary {
  display: flex; flex-direction: column; gap: var(--space-2);
  padding: var(--space-4); background: var(--bg-tertiary);
  border-radius: var(--radius-md); width: 100%; max-width: 320px;
}
.summary-row {
  display: flex; justify-content: space-between; align-items: center;
  font-size: var(--text-sm);
}
.summary-label { color: var(--text-tertiary); }
.summary-value { color: var(--text-primary); font-weight: var(--font-semibold); }

/* Task review details (read-only) */
.task-reviews-list {
  width: 100%;
  max-width: 560px;
  display: flex; flex-direction: column; gap: var(--space-3);
  text-align: left;
}
.task-reviews-title {
  font-size: var(--text-sm); font-weight: var(--font-semibold); color: var(--text-secondary);
  text-transform: uppercase; letter-spacing: 0.04em;
}
.task-review-row {
  display: flex; align-items: center; gap: var(--space-3);
  padding: var(--space-3); background: var(--bg-tertiary);
  border-radius: var(--radius-md); flex-wrap: wrap;
}
.task-review-row.incomplete { opacity: 0.7; }
.task-review-row.abandoned { opacity: 0.5; }
.trr-left {
  display: flex; align-items: center; gap: var(--space-2);
  flex-shrink: 0; min-width: 120px;
}
.trr-status-icon { color: var(--text-tertiary); }
.task-review-row.completed .trr-status-icon { color: var(--color-success); }
.task-review-row.overcompletion .trr-status-icon { color: var(--accent); }
.trr-title-wrap { display: flex; flex-direction: column; gap: 2px; min-width: 0; }
.trr-title {
  font-size: var(--text-sm); font-weight: var(--font-semibold);
  color: var(--text-primary); overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
  max-width: 320px;
}
.trr-meta { display: flex; align-items: center; gap: var(--space-2); }
.trr-subject {
  font-size: var(--text-xs); color: var(--text-tertiary);
  background: var(--bg-overlay); padding: 1px 6px; border-radius: var(--radius-full);
}
.trr-status { font-size: var(--text-sm); font-weight: var(--font-medium); color: var(--text-secondary); }
.trr-right {
  display: flex; align-items: center; gap: var(--space-2); flex-wrap: wrap;
  flex: 1;
}
.trr-chip {
  font-size: var(--text-xs);
  padding: 2px 8px;
  border-radius: var(--radius-full);
  font-weight: var(--font-medium);
}
.trr-chip.mastery { background: var(--accent-subtle); color: var(--accent); }
.trr-chip.blocker { background: var(--color-danger-subtle); color: var(--color-danger); }
.trr-note { font-size: var(--text-xs); color: var(--text-tertiary); font-style: italic; }

/* Overcompletion */
.overcompletion-toggle { margin-bottom: var(--space-3); }
.oc-switch {
  display: flex; align-items: center; gap: var(--space-2);
  padding: var(--space-3) var(--space-4); border: 1.5px solid var(--border-color);
  border-radius: var(--radius-md); background: var(--bg-elevated);
  color: var(--text-secondary); font-size: var(--text-sm); font-weight: var(--font-medium);
  cursor: pointer; transition: all var(--transition-fast); font-family: inherit; width: 100%;
}
.oc-switch:hover { border-color: var(--accent); }
.oc-switch.active { border-color: var(--accent); background: var(--accent-subtle); color: var(--accent); font-weight: var(--font-semibold); }
.overcompletion-list { display: flex; flex-direction: column; gap: var(--space-3); }
.oc-item {
  display: flex; flex-direction: column; gap: var(--space-2);
  padding: var(--space-3); background: var(--bg-tertiary); border-radius: var(--radius-md);
}
.oc-row { display: flex; align-items: center; gap: var(--space-2); }
.oc-select {
  background: var(--bg-elevated); border: 1px solid var(--border-color);
  border-radius: var(--radius-md); padding: var(--space-2) var(--space-3);
  font-size: var(--text-sm); font-family: inherit; color: var(--text-primary);
  outline: none; min-width: 110px;
}
.oc-input { flex: 1; }

/* ── 日期切换过渡动画 ── */
.date-slide-left-enter-active,
.date-slide-left-leave-active,
.date-slide-right-enter-active,
.date-slide-right-leave-active {
  transition: transform 0.22s ease, opacity 0.22s ease;
}

.date-slide-left-enter-from {
  transform: translateX(12px);
  opacity: 0;
}
.date-slide-left-leave-to {
  transform: translateX(-12px);
  opacity: 0;
}

.date-slide-right-enter-from {
  transform: translateX(-12px);
  opacity: 0;
}
.date-slide-right-leave-to {
  transform: translateX(12px);
  opacity: 0;
}
</style>
