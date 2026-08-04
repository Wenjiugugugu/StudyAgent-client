<script setup lang="ts">
import { computed, onMounted, onBeforeUnmount, ref } from "vue";
import { useRouter } from "vue-router";
import { useDashboardStore } from "@/stores/dashboard";
import { useSettingsStore } from "@/stores/settings";
import { useTodayStore } from "@/stores/today";
import { useUpdateStore } from "@/stores/update";
import { todayString, prevDateString, currentHourShanghai, currentMinutesShanghai, timeStringToMinutes, daysBetween, weekdayName, getWeekStart } from "@/utils/date";
import * as api from "@/api";
import Card from "@/components/ui/Card.vue";
import Badge from "@/components/ui/Badge.vue";
import Button from "@/components/ui/Button.vue";
import ProgressBar from "@/components/ui/ProgressBar.vue";
import LoadingSpinner from "@/components/ui/LoadingSpinner.vue";
import EmptyState from "@/components/ui/EmptyState.vue";
import Modal from "@/components/ui/Modal.vue";
import MarkdownText from "@/components/MarkdownText.vue";
import {
  Calendar,
  Sparkles,
  TrendingUp,
  Target,
  RefreshCw,
  ChevronRight,
  Clock,
  Flag,
  Award,
  Download,
  HardDriveDownload,
  Package,
  CheckCircle2,
  Coffee,
  Ban,
} from "lucide-vue-next";
import type { DashboardSummary, PlanTask, PlanSummary, SubjectKey, ExcludedReasonType, BriefingFile, GetBriefingResult, ReviewFile } from "@/types";

const router = useRouter();
const dashboardStore = useDashboardStore();
const updateStore = useUpdateStore();
const settingsStore = useSettingsStore();
const todayStore = useTodayStore();

const summary = computed<DashboardSummary | null>(() => dashboardStore.summary);
const todayDateStr = todayString();

// ── 每日简报 ──
const briefingResult = ref<GetBriefingResult | null>(null);
const briefingLoading = ref(false);
const briefingRegenerating = ref(false);

const briefing = computed<BriefingFile | null>(() => briefingResult.value?.briefing ?? null);
const briefingExists = computed(() => briefingResult.value?.exists ?? false);
const yesterdayReviewExists = computed(() => briefingResult.value?.yesterday_review_exists ?? false);
const yesterdayExempt = computed(() => briefingResult.value?.yesterday_exempt ?? false);
const withinMakeupWindow = computed(() => briefingResult.value?.within_makeup_window ?? false);

// 昨日复盘缺失且非豁免：需提示用户先去复盘
const needYesterdayReview = computed(
  () => !yesterdayReviewExists.value && !yesterdayExempt.value,
);

// 错过补复盘窗口（非今日）：不提供 AI 建议
const missedMakeupWindow = computed(
  () => !yesterdayReviewExists.value && !yesterdayExempt.value && !withinMakeupWindow.value,
);

// 特殊状态：休息日/排除日/开始时间前 — 隐藏侧栏，居中提示
const isSpecialState = computed(
  () => isBeforeDailyStart.value || isTodayRestDay.value || isTodayExcluded.value,
);

// ── 昨日复盘摘要（侧栏数据） ──
const yesterdayReviewData = ref<ReviewFile | null>(null);

const yesterdayCompletionRate = computed(() => {
  if (!yesterdayReviewData.value) return 0;
  const review = yesterdayReviewData.value;
  // 优先从 task_reviews 聚合（与后端 compute_priority_a_completion 口径一致）
  if (review.task_reviews?.length) {
    const all = review.task_reviews;
    const aTasks = all.filter((t) => t.priority === "A");
    const allDone = all.filter((t) => t.status === "completed").length;
    const aDone = aTasks.filter((t) => t.status === "completed").length;
    if (aTasks.length > 0) return Math.round((aDone / aTasks.length) * 100);
    if (all.length > 0) return Math.round((allDone / all.length) * 100);
    return 0;
  }
  // 回退到 data.completion（旧版复盘文件）
  const c = review.data.completion;
  const total = c.priority_a_total + c.priority_b_total;
  if (total === 0) return 0;
  return Math.round(((c.priority_a_done + c.priority_b_done) / total) * 100);
});

const yesterdayFeeling = computed(() => {
  const f = yesterdayReviewData.value?.daily_review?.overall_feeling;
  return f === "smooth" ? "顺利" : f === "normal" ? "一般" : f === "hard" ? "困难" : "—";
});

const yesterdayFeelingVariant = computed(() => {
  const f = yesterdayReviewData.value?.daily_review?.overall_feeling;
  if (f === "smooth") return "success";
  if (f === "hard") return "danger";
  return "default";
});

const yesterdayDifficulty = computed(() => {
  const d = yesterdayReviewData.value?.daily_review?.main_difficulty;
  if (!d) return null;
  const map: Record<string, string> = {
    understanding: "理解困难",
    problems: "解题困难",
    memorization: "记忆困难",
    attention: "注意力不集中",
    time_management: "时间管理",
    environment: "环境干扰",
    other: "其他",
  };
  return map[d] ?? d;
});

const yesterdayActualHours = computed(() => {
  if (!yesterdayReviewData.value) return 0;
  return yesterdayReviewData.value.data.total_hours ?? 0;
});

async function loadYesterdayReview() {
  if (!yesterdayReviewExists.value) {
    yesterdayReviewData.value = null;
    return;
  }
  try {
    yesterdayReviewData.value = await api.getReview(yesterdayDateStr.value);
  } catch {
    yesterdayReviewData.value = null;
  }
}

// ── 简报闪烁提示 + 引入动画 ──
const showBriefingHint = ref(false);
// 默认显示内容，避免首次加载时卡片 opacity 为 0 导致空白
const briefingAnimated = ref(true);
const sidebarAnimated = ref(true);
let hintTimer: number | undefined;

function triggerBriefingHint() {
  const hintKey = "studyagent.briefing_hint_viewed";
  if (sessionStorage.getItem(hintKey) === todayDateStr) return;
  showBriefingHint.value = true;
  sessionStorage.setItem(hintKey, todayDateStr);
  hintTimer = window.setTimeout(() => {
    showBriefingHint.value = false;
  }, 4000);
}

function playEntranceAnimation() {
  briefingAnimated.value = false;
  sidebarAnimated.value = false;
  // 触发重排后添加动画 class
  requestAnimationFrame(() => {
    briefingAnimated.value = true;
    setTimeout(() => { sidebarAnimated.value = true; }, 150);
  });
}

async function loadBriefing() {
  briefingLoading.value = true;
  try {
    briefingResult.value = await api.getBriefing(todayDateStr);
    // 加载昨日复盘数据（侧栏用）
    await loadYesterdayReview();
    // 如果简报存在且是今日首次查看，触发闪烁提示
    if (briefingExists.value && !isSpecialState.value) {
      triggerBriefingHint();
      playEntranceAnimation();
    }
  } catch (e) {
    console.warn("[Briefing] 加载简报失败:", e);
    briefingResult.value = null;
  } finally {
    briefingLoading.value = false;
  }
}

async function regenerateBriefing() {
  if (briefingRegenerating.value) return;
  briefingRegenerating.value = true;
  try {
    const fresh = await api.regenerateBriefing(todayDateStr);
    // 重新拉取完整状态（包含 exists 等字段）
    await loadBriefing();
    briefingResult.value = briefingResult.value ?? {
      briefing: fresh,
      exists: true,
      yesterday_review_exists: true,
      is_rest_day: false,
      is_excluded_day: false,
      yesterday_exempt: false,
      within_makeup_window: true,
    };
  } catch (e) {
    console.error("[Briefing] 重新生成简报失败:", e);
    alert(e instanceof Error ? e.message : "重新生成简报失败");
  } finally {
    briefingRegenerating.value = false;
  }
}

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
  () => settingsStore.settings?.study_schedule?.start_time ?? "09:00",
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
    const wp = await api.getWeekPlan(ws);
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

function goReview() {
  router.push("/review");
}

function refresh() {
  dashboardStore.loadSummary().then(loadWeekSummaries);
  todayStore.loadToday();
  checkTodayExcluded();
  loadBriefing();
}

// ── 简报展示辅助 ──
// 今日任务清单（来自 today store）
const todayTasks = computed<PlanTask[]>(() => todayStore.allTasks);

// 昨日复盘摘要：从 dashboard summary 读取（review_reminder 字段）
const yesterdayDateStr = computed(() => prevDateString(todayDateStr));

// 科目估算展示：附中文科目名
const estimationList = computed(() => {
  if (!briefing.value?.data?.estimations) return [];
  return briefing.value.data.estimations.map((e) => ({
    ...e,
    subjectLabel: subjectLabel(e.subject),
  }));
});

// 简报寄语
const briefingGreeting = computed(() => briefing.value?.data?.greeting?.trim() ?? "");

onMounted(() => {
  dashboardStore.loadSummary().then(loadWeekSummaries);
  todayStore.loadToday();
  checkTodayExcluded();
  loadBriefing();
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
  if (hintTimer !== undefined) {
    clearTimeout(hintTimer);
    hintTimer = undefined;
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
      <!-- 发现新版本弹窗 -->
      <Modal
        :open="updateStore.showUpdateModal"
        title="发现新版本"
        :close-on-overlay="false"
        :width="520"
        @close="updateStore.dismissUpdate()"
      >
        <div v-if="updateStore.updateResult" class="update-modal-body">
          <p class="update-modal-version">
            新版本：<strong>v{{ updateStore.updateResult.latest_version }}</strong>
            <span v-if="updateStore.updateResult.release_name" class="update-modal-name">
              — {{ updateStore.updateResult.release_name }}
            </span>
          </p>

          <!-- Release notes -->
          <div v-if="updateStore.updateResult.release_notes" class="update-modal-notes">
            <MarkdownText :content="updateStore.updateResult.release_notes" />
          </div>

          <!-- 安装包选择 + 下载 -->
          <div class="update-modal-actions">
            <div v-if="updateStore.updateResult.assets.length > 1" class="update-modal-assets">
              <button
                v-for="asset in updateStore.updateResult.assets"
                :key="asset.download_url"
                class="update-asset-btn"
                :class="{ active: updateStore.selectedAsset?.download_url === asset.download_url }"
                @click="updateStore.selectedAsset = asset"
              >
                <Package :size="13" />
                <span>{{ updateStore.assetLabel(asset.kind) }}</span>
                <span class="update-asset-size">{{ updateStore.formatSize(asset.size) }}</span>
              </button>
            </div>

            <!-- 下载进度条 -->
            <div
              v-if="updateStore.downloadState === 'downloading' && updateStore.downloadProgress"
              class="update-download-progress"
            >
              <ProgressBar
                :value="updateStore.downloadProgress.percent || 0"
                :max="100"
              />
              <span class="update-progress-text">
                {{ updateStore.downloadProgress.percent?.toFixed(0) ?? 0 }}%
              </span>
            </div>

            <p v-if="updateStore.downloadError" class="update-download-error">
              {{ updateStore.downloadError }}
            </p>
          </div>
        </div>

        <template #footer>
          <Button variant="ghost" size="sm" @click="router.push('/settings#settings-update')">
            查看详情
          </Button>

          <Button
            v-if="updateStore.downloadState === 'idle' || updateStore.downloadState === 'error'"
            variant="primary"
            size="sm"
            :disabled="!updateStore.selectedAsset"
            @click="updateStore.handleDownload()"
          >
            <Download :size="13" />
            <span>下载安装包</span>
          </Button>

          <Button
            v-if="updateStore.downloadState === 'downloaded'"
            variant="primary"
            size="sm"
            :loading="updateStore.installing"
            @click="updateStore.handleInstall()"
          >
            <HardDriveDownload :size="13" />
            <span>立即安装</span>
          </Button>

          <Button
            v-if="updateStore.downloadState === 'downloading'"
            variant="secondary"
            size="sm"
            disabled
          >
            <span>下载中…</span>
          </Button>

          <Button
            v-if="updateStore.downloadState === 'installing'"
            variant="secondary"
            size="sm"
            disabled
          >
            <span>安装中…</span>
          </Button>
        </template>
      </Modal>

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

      <!-- 简报闪烁提示 -->
      <Transition name="hint-fade">
        <div v-if="showBriefingHint" class="briefing-flash-hint">
          <Sparkles :size="14" />
          <span>有新的每日简报可供查看</span>
        </div>
      </Transition>

      <!-- 主区域：简报 + 侧栏 -->
      <div class="dashboard-main" :class="{ 'no-sidebar': isSpecialState }">
        <!-- 左侧：每日简报 -->
        <Card
          padding="lg"
          class="card briefing-card"
          :class="{ 'briefing-enter': briefingAnimated }"
          surface="1"
          hoverable
        >
          <div class="briefing-header">
            <div class="briefing-title-row">
              <span class="briefing-indicator" />
              <Sparkles :size="18" class="briefing-icon" />
              <h2 class="briefing-heading">每日简报</h2>
            </div>
            <div class="briefing-actions">
              <Button
                v-if="!isSpecialState && briefingExists && yesterdayReviewExists"
                variant="ghost"
                size="sm"
                :loading="briefingRegenerating"
                @click="regenerateBriefing"
              >
                <RefreshCw :size="13" />
                重新生成
              </Button>
              <Button
                v-if="(todayTasks.length > 0 || allTasksCompleted) && !isSpecialState"
                variant="ghost"
                size="sm"
                @click="goToday"
              >
                查看详情
                <ChevronRight :size="14" />
              </Button>
            </div>
          </div>

          <!-- 特殊状态：居中提示 -->
          <div v-if="isBeforeDailyStart" class="briefing-center-prompt">
            <div class="briefing-empty-icon"><Clock :size="28" /></div>
            <span class="briefing-empty-title">今天的学习时间还没开始</span>
            <span class="briefing-empty-desc">每日开始时间为 {{ dailyStartTimeLabel }}，到点后这里会展示今日简报。</span>
          </div>

          <div v-else-if="isTodayRestDay" class="briefing-center-prompt">
            <div class="briefing-empty-icon briefing-rest-icon"><Coffee :size="28" /></div>
            <span class="briefing-empty-title">今日是休息日</span>
            <span class="briefing-empty-desc">好好放松一下，明天继续。</span>
          </div>

          <div v-else-if="isTodayExcluded" class="briefing-center-prompt">
            <div class="briefing-empty-icon briefing-rest-icon"><Ban :size="28" /></div>
            <span class="briefing-empty-title">今日是排除日</span>
            <span class="briefing-empty-desc">{{ todayExcludedReasonLabel }}</span>
          </div>

          <div v-else-if="allTasksCompleted" class="briefing-center-prompt clickable" @click="goToday">
            <div class="briefing-empty-icon briefing-done-icon"><CheckCircle2 :size="28" /></div>
            <span class="briefing-empty-title">今日计划已全部完成</span>
            <span class="briefing-empty-desc">辛苦了！可前往复盘记录今日学习情况。</span>
          </div>

          <!-- 简报加载中 -->
          <div v-else-if="briefingLoading && !briefingExists && todayTasks.length === 0" class="briefing-loading">
            <LoadingSpinner :size="24" label="正在生成今日简报…" />
          </div>

          <!-- 简报内容 -->
          <div v-else class="briefing-body">
            <!-- 缺失昨日复盘提示横幅 -->
            <div v-if="needYesterdayReview" class="briefing-missing-review">
              <div class="briefing-empty-icon briefing-warn-icon">
                <Flag :size="18" />
              </div>
              <div class="briefing-empty-text">
                <span class="briefing-empty-title">昨日复盘缺失</span>
                <span class="briefing-empty-desc">
                  {{ withinMakeupWindow
                    ? '完成昨日复盘后即可生成今日 AI 简报与建议'
                    : '已错过补复盘窗口，今日不提供 AI 建议' }}
                </span>
              </div>
              <Button v-if="withinMakeupWindow" variant="primary" size="sm" @click="goReview">
                去补复盘
                <ChevronRight :size="14" />
              </Button>
            </div>

            <!-- AI 寄语（大字引言式） -->
            <div v-if="briefingGreeting" class="briefing-quote">
              <span class="briefing-quote-mark">"</span>
              <p class="briefing-quote-text">{{ briefingGreeting }}</p>
              <span class="briefing-quote-mark closing">"</span>
            </div>

            <!-- 今日任务清单 -->
            <div v-if="todayTasks.length > 0" class="briefing-section" @click="goToday">
              <div class="briefing-section-title clickable-title">
                <Target :size="13" />
                <span>今日任务（{{ todayStore.doneCount }}/{{ todayStore.totalCount }}）</span>
                <ChevronRight :size="12" class="briefing-section-arrow" />
              </div>
              <ul class="briefing-task-list">
                <li
                  v-for="task in todayTasks"
                  :key="task.id"
                  class="briefing-task-item"
                  :class="{ done: task.status === 'done' }"
                >
                  <span class="briefing-task-priority" :class="`prio-${task.priority.toLowerCase()}`">
                    {{ task.priority }}
                  </span>
                  <Badge :variant="subjectBadgeVariant(task.subject)" size="sm">
                    {{ subjectLabel(task.subject) }}
                  </Badge>
                  <span class="briefing-task-title">{{ task.title }}</span>
                  <span
                    v-if="timeTrackingEnabled && task.estimated_hours"
                    class="briefing-task-time"
                  >
                    <Clock :size="11" />
                    {{ task.estimated_hours }}h
                  </span>
                </li>
              </ul>
            </div>

            <!-- AI 各科估时 -->
            <div v-if="estimationList.length > 0 && yesterdayReviewExists" class="briefing-section">
              <div class="briefing-section-title">
                <TrendingUp :size="13" />
                <span>阶段估时</span>
              </div>
              <div class="briefing-estimation-grid">
                <div
                  v-for="est in estimationList"
                  :key="est.subject"
                  class="briefing-estimation-cell"
                >
                  <div class="briefing-estimation-head">
                    <Badge :variant="subjectBadgeVariant(est.subject)" size="sm">
                      {{ est.subjectLabel }}
                    </Badge>
                    <span class="briefing-estimation-days">
                      约 {{ est.estimated_days_to_finish }} 天
                    </span>
                  </div>
                  <span v-if="est.current_chapter" class="briefing-estimation-chapter">
                    {{ est.current_chapter }}
                  </span>
                  <span v-if="est.note" class="briefing-estimation-note">{{ est.note }}</span>
                </div>
              </div>
            </div>

            <!-- 迷你本周进度 -->
            <div v-if="summary?.week_progress" class="briefing-section mini-week-progress" @click="goWeekPlan">
              <div class="briefing-section-title clickable-title">
                <Calendar :size="13" />
                <span>本周进度</span>
                <ChevronRight :size="12" class="briefing-section-arrow" />
              </div>
              <div class="mini-week-stats">
                <span class="mini-week-percent">{{ weekPlanProgress }}%</span>
                <span class="mini-week-detail">{{ studiedDays }}/{{ plannedDaysPerWeek }} 天 · 剩余 {{ remainingHours }} 小时 · {{ onTrackLabel }}</span>
              </div>
              <ProgressBar
                :value="weekPlanProgress"
                :max="100"
                :variant="weekPlanProgress >= 100 ? 'success' : isOnTrack ? 'default' : 'warning'"
                size="sm"
              />
              <div class="mini-week-dots">
                <div
                  v-for="day in summary.week_progress.daily_breakdown"
                  :key="day.date"
                  class="mini-dot"
                  :class="{ studied: isDayStudied(day.date), today: isToday(day.date) }"
                  :title="day.date"
                >
                  <span class="dot-label">{{ weekdayShort(day.date) }}</span>
                </div>
              </div>
            </div>

            <!-- 底部操作 -->
            <div v-if="todayTasks.length > 0" class="briefing-footer">
              <Button variant="primary" size="md" @click="goToday">
                开始学习
                <ChevronRight :size="16" />
              </Button>
            </div>
          </div>

          <!-- 无简报且无任务：空状态 -->
          <div
            v-if="!isSpecialState && !allTasksCompleted && !briefingExists && todayTasks.length === 0 && !needYesterdayReview && !briefingLoading"
            class="briefing-center-prompt"
          >
            <div class="briefing-empty-icon"><Sparkles :size="28" /></div>
            <span class="briefing-empty-title">今日暂无计划</span>
            <span class="briefing-empty-desc">请先生成周计划，日计划将自动从周计划中拆分生成</span>
          </div>
        </Card>

        <!-- 右侧：昨日复盘摘要侧栏 -->
        <aside
          v-if="!isSpecialState"
          class="review-sidebar"
          :class="{ 'sidebar-enter': sidebarAnimated }"
        >
          <Card padding="md" class="card review-card" surface="1" hoverable>
            <div class="review-sidebar-header">
              <Award :size="16" class="review-sidebar-icon" />
              <h3 class="review-sidebar-title">昨日复盘</h3>
              <span class="review-sidebar-date">{{ yesterdayDateStr }}</span>
            </div>

            <!-- 有复盘数据 -->
            <div v-if="yesterdayReviewExists && yesterdayReviewData" class="review-sidebar-body">
              <div class="review-metric">
                <span class="review-metric-label">完成率</span>
                <span class="review-metric-value" :class="yesterdayCompletionRate >= 80 ? 'good' : yesterdayCompletionRate >= 50 ? 'warn' : 'bad'">
                  {{ yesterdayCompletionRate }}%
                </span>
              </div>

              <div class="review-metric">
                <span class="review-metric-label">整体感受</span>
                <Badge :variant="yesterdayFeelingVariant as 'default' | 'success' | 'danger'" size="sm">
                  {{ yesterdayFeeling }}
                </Badge>
              </div>

              <div v-if="yesterdayDifficulty" class="review-metric">
                <span class="review-metric-label">主要困难</span>
                <span class="review-metric-text">{{ yesterdayDifficulty }}</span>
              </div>

              <div v-if="timeTrackingEnabled && yesterdayActualHours > 0" class="review-metric">
                <span class="review-metric-label">学习时长</span>
                <span class="review-metric-text">{{ yesterdayActualHours.toFixed(1) }} 小时</span>
              </div>

              <Button variant="ghost" size="sm" class="review-sidebar-action" @click="goReview">
                查看复盘详情
                <ChevronRight :size="14" />
              </Button>
            </div>

            <!-- 缺失复盘 -->
            <div v-else-if="needYesterdayReview" class="review-sidebar-missing">
              <Flag :size="20" class="review-missing-icon" />
              <span class="review-missing-title">昨日未复盘</span>
              <span class="review-missing-desc">
                {{ withinMakeupWindow ? '点击下方按钮补复盘' : '已错过补复盘窗口' }}
              </span>
              <Button v-if="withinMakeupWindow" variant="primary" size="sm" @click="goReview">
                去补复盘
                <ChevronRight :size="14" />
              </Button>
            </div>

            <!-- 昨日豁免（休息日/排除日） -->
            <div v-else class="review-sidebar-exempt">
              <Coffee :size="20" class="review-exempt-icon" />
              <span class="review-missing-title">昨日为休息日</span>
              <span class="review-missing-desc">无需复盘</span>
            </div>
          </Card>

          <!-- 紧凑统计卡 -->
          <Card padding="sm" class="card stats-card" surface="1">
            <div class="stats-row">
              <div class="stats-item">
                <span class="stats-value">{{ streakDays }}</span>
                <span class="stats-label">连续天数</span>
              </div>
              <div class="stats-divider" />
              <div class="stats-item">
                <span class="stats-value">{{ totalStudyDays }}</span>
                <span class="stats-label">累计天数</span>
              </div>
              <div class="stats-divider" />
              <div class="stats-item">
                <span class="stats-value">{{ weekPlanProgress }}%</span>
                <span class="stats-label">本周进度</span>
              </div>
            </div>
          </Card>
        </aside>
      </div>
    </template>
  </div>
</template>

<style scoped>
.dashboard-view {
  max-width: 1120px;
  margin: 0 auto;
  padding: var(--space-8) var(--space-8) var(--space-10);
  display: flex;
  flex-direction: column;
  gap: var(--space-6);
  overflow: hidden;
}

/* ── 主区域网格：简报 2fr + 侧栏 1fr ── */
.dashboard-main {
  display: grid;
  grid-template-columns: 2fr 1fr;
  gap: var(--space-5);
  align-items: start;
}

.dashboard-main.no-sidebar {
  grid-template-columns: 1fr;
  max-width: 640px;
  margin: 0 auto;
  width: 100%;
}

/* ── 闪烁提示 ── */
.briefing-flash-hint {
  display: inline-flex;
  align-items: center;
  gap: var(--space-2);
  padding: var(--space-2) var(--space-4);
  background: linear-gradient(135deg, var(--accent-subtle) 0%, var(--bg-tertiary) 100%);
  border: 1px solid var(--accent);
  border-radius: var(--radius-full);
  font-size: var(--text-sm);
  color: var(--accent);
  font-weight: var(--font-medium);
  align-self: flex-start;
  animation: hint-pulse 1.5s ease-in-out infinite;
}

@keyframes hint-pulse {
  0%, 100% { opacity: 1; box-shadow: 0 0 0 0 var(--accent-subtle); }
  50% { opacity: 0.85; box-shadow: 0 0 0 6px transparent; }
}

.hint-fade-enter-active, .hint-fade-leave-active {
  transition: opacity 0.4s ease, transform 0.4s ease;
}
.hint-fade-enter-from, .hint-fade-leave-to {
  opacity: 0;
  transform: translateY(-8px);
}

/* ── 入场动画 ── */
.briefing-card {
  opacity: 0;
  transform: translateY(12px);
  transition: opacity 0.5s ease, transform 0.5s ease;
}
.briefing-card.briefing-enter {
  opacity: 1;
  transform: translateY(0);
}

.review-sidebar {
  opacity: 0;
  transform: translateX(12px);
  transition: opacity 0.5s ease, transform 0.5s ease;
  display: flex;
  flex-direction: column;
  gap: var(--space-3);
}
.review-sidebar.sidebar-enter {
  opacity: 1;
  transform: translateX(0);
}

/* ── 发现新版本弹窗 ── */
.update-modal-body {
  display: flex;
  flex-direction: column;
  gap: var(--space-3);
}

.update-modal-version {
  margin: 0;
  font-size: var(--text-sm);
  color: var(--text-secondary);
}

.update-modal-version strong {
  color: var(--accent);
  font-weight: var(--font-semibold);
}

.update-modal-name {
  color: var(--text-secondary);
}

.update-modal-notes {
  padding: var(--space-3);
  background: var(--bg-elevated);
  border-radius: var(--radius-md);
  border: 1px solid var(--border-color);
  font-size: var(--text-sm);
  max-height: 240px;
  overflow-y: auto;
}

.update-modal-actions {
  display: flex;
  flex-direction: column;
  gap: var(--space-2);
}

.update-modal-assets {
  display: flex;
  flex-wrap: wrap;
  gap: var(--space-2);
}

.update-asset-btn {
  display: flex;
  align-items: center;
  gap: var(--space-1);
  padding: var(--space-1) var(--space-3);
  border: 1px solid var(--border-color);
  background: var(--bg-elevated);
  border-radius: var(--radius-sm);
  font-size: var(--text-xs);
  color: var(--text-secondary);
  cursor: pointer;
  font-family: inherit;
  transition: all var(--transition-fast);
}

.update-asset-btn:hover {
  border-color: var(--border-color-strong);
}

.update-asset-btn.active {
  border-color: var(--accent);
  background: var(--accent-subtle);
  color: var(--accent);
}

.update-asset-size {
  color: var(--text-tertiary);
  font-size: 10px;
}

.update-download-progress {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  flex: 1;
  min-width: 150px;
}

.update-progress-text {
  font-size: var(--text-xs);
  font-family: var(--font-mono);
  color: var(--text-secondary);
  white-space: nowrap;
}

.update-download-error {
  margin: 0;
  font-size: var(--text-xs);
  color: var(--color-danger, #ef4444);
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

/* Daily Briefing — Apple-style: quiet accent, no heavy border decoration */
.briefing-card {
  background: var(--bg-elevated);
  position: relative;
}

.briefing-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--space-2);
}

.briefing-title-row {
  display: flex;
  align-items: center;
  gap: var(--space-2);
}

.briefing-indicator {
  width: 6px;
  height: 6px;
  border-radius: var(--radius-full);
  background: var(--accent);
  flex-shrink: 0;
}

.briefing-icon {
  color: var(--accent);
}

.briefing-heading {
  font-size: var(--text-base);
  font-weight: var(--font-heading);
  color: var(--text-primary);
}

.briefing-actions {
  display: flex;
  align-items: center;
  gap: var(--space-2);
}

.briefing-body {
  display: flex;
  flex-direction: column;
  gap: var(--space-4);
}

/* AI 寄语（大字引言式） */
.briefing-quote {
  position: relative;
  display: flex;
  align-items: flex-start;
  gap: var(--space-1);
  padding: var(--space-4) var(--space-5);
  background: linear-gradient(135deg, var(--accent-subtle) 0%, transparent 100%);
  border-left: 3px solid var(--accent);
  border-radius: var(--radius-md);
}

.briefing-quote-mark {
  font-size: 36px;
  line-height: 1;
  color: var(--accent);
  font-weight: var(--font-bold);
  font-family: Georgia, serif;
  flex-shrink: 0;
  opacity: 0.4;
  margin-top: -4px;
}

.briefing-quote-mark.closing {
  align-self: flex-end;
  margin-top: auto;
  margin-bottom: -8px;
}

.briefing-quote-text {
  margin: 0;
  font-size: var(--text-lg);
  line-height: var(--leading-relaxed);
  color: var(--text-primary);
  font-weight: var(--font-medium);
  flex: 1;
  letter-spacing: -0.01em;
}

/* 居中提示（特殊状态） */
.briefing-center-prompt {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: var(--space-2);
  padding: var(--space-8) var(--space-4);
  text-align: center;
}

.briefing-center-prompt.clickable {
  cursor: pointer;
  border-radius: var(--radius-md);
  transition: background var(--transition-fast);
}

.briefing-center-prompt.clickable:hover {
  background: var(--accent-subtle);
}

.briefing-center-prompt .briefing-empty-icon {
  width: 56px;
  height: 56px;
}

.briefing-center-prompt .briefing-empty-title {
  font-size: var(--text-lg);
}

.briefing-center-prompt .briefing-empty-desc {
  max-width: 360px;
  text-align: center;
}

/* 简报区块 */
.briefing-section {
  display: flex;
  flex-direction: column;
  gap: var(--space-2);
}

.briefing-section-title {
  display: flex;
  align-items: center;
  gap: var(--space-1);
  font-size: var(--text-xs);
  font-weight: var(--font-label);
  color: var(--text-tertiary);
}

.briefing-section-title svg {
  color: var(--text-tertiary);
}

/* 今日任务清单 */
.briefing-task-list {
  list-style: none;
  margin: 0;
  padding: 0;
  display: flex;
  flex-direction: column;
  gap: var(--space-2);
}

.briefing-task-item {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  padding: var(--space-2) var(--space-3);
  background: var(--bg-tertiary);
  border-radius: var(--radius-sm);
  min-width: 0;
}

.briefing-task-item.done {
  opacity: 0.55;
}

.briefing-task-item.done .briefing-task-title {
  text-decoration: line-through;
}

.briefing-task-priority {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 18px;
  height: 18px;
  border-radius: var(--radius-sm);
  font-size: 10px;
  font-weight: var(--font-bold);
  flex-shrink: 0;
}

.briefing-task-priority.prio-a {
  background: var(--color-danger-subtle, rgba(239, 68, 68, 0.12));
  color: var(--color-danger, #ef4444);
}

.briefing-task-priority.prio-b {
  background: var(--color-warning-subtle, rgba(245, 158, 11, 0.12));
  color: var(--color-warning, #f59e0b);
}

.briefing-task-priority.prio-c {
  background: var(--bg-elevated);
  color: var(--text-tertiary);
}

.briefing-task-title {
  font-size: var(--text-sm);
  color: var(--text-primary);
  flex: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.briefing-task-time {
  display: inline-flex;
  align-items: center;
  gap: 2px;
  font-size: var(--text-xs);
  color: var(--text-tertiary);
  flex-shrink: 0;
}

/* AI 各科估时 */
.briefing-estimation-grid {
  display: grid;
  grid-template-columns: repeat(2, 1fr);
  gap: var(--space-2);
}

.briefing-estimation-cell {
  display: flex;
  flex-direction: column;
  gap: 4px;
  padding: var(--space-2) var(--space-3);
  background: var(--bg-tertiary);
  border-radius: var(--radius-sm);
  min-width: 0;
}

.briefing-estimation-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--space-2);
  min-width: 0;
}

.briefing-estimation-days {
  font-size: var(--text-xs);
  font-weight: var(--font-semibold);
  color: var(--accent);
  flex-shrink: 0;
}

.briefing-estimation-chapter {
  font-size: var(--text-xs);
  font-weight: var(--font-medium);
  color: var(--text-secondary);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.briefing-estimation-note {
  font-size: 10px;
  color: var(--text-tertiary);
  line-height: var(--leading-normal);
  overflow: hidden;
  text-overflow: ellipsis;
  display: -webkit-box;
  -webkit-line-clamp: 2;
  -webkit-box-orient: vertical;
}

/* ── 迷你本周进度 ── */
.mini-week-progress {
  cursor: pointer;
  border: 1px solid var(--border-color);
  border-radius: var(--radius-md);
  padding: var(--space-3);
  background: var(--bg-secondary);
  transition: background var(--transition-fast), border-color var(--transition-fast);
}

.mini-week-progress:hover {
  background: var(--accent-subtle);
  border-color: var(--accent);
}

.mini-week-stats {
  display: flex;
  align-items: baseline;
  gap: var(--space-2);
  margin: var(--space-2) 0;
}

.mini-week-percent {
  font-size: var(--text-xl);
  font-weight: var(--font-bold);
  color: var(--text-primary);
}

.mini-week-detail {
  font-size: var(--text-xs);
  color: var(--text-secondary);
}

.mini-week-dots {
  display: flex;
  align-items: center;
  gap: var(--space-1);
  margin-top: var(--space-2);
}

.mini-dot {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 2px;
  flex: 1;
}

.mini-dot::before {
  content: "";
  width: 6px;
  height: 6px;
  border-radius: var(--radius-full);
  background: var(--bg-tertiary);
  border: 1px solid var(--border-color);
}

.mini-dot.studied::before {
  background: var(--color-success, #22c55e);
  border-color: var(--color-success, #22c55e);
}

.mini-dot.today::before {
  background: var(--accent);
  border-color: var(--accent);
  box-shadow: 0 0 0 2px var(--accent-subtle);
}

.dot-label {
  font-size: 10px;
  color: var(--text-tertiary);
}

/* 昨日复盘入口 */
.briefing-yesterday {
  padding: var(--space-2) var(--space-3);
  background: var(--bg-tertiary);
  border-radius: var(--radius-sm);
  cursor: pointer;
  transition: background var(--transition-fast);
}

.briefing-yesterday:hover {
  background: var(--accent-subtle);
}

.briefing-yesterday-arrow {
  margin-left: auto;
  color: var(--text-tertiary);
}

.briefing-yesterday-desc {
  margin: 0;
  font-size: var(--text-xs);
  color: var(--text-tertiary);
}

/* 错过窗口提示 */
.briefing-missed-hint {
  padding: var(--space-2) var(--space-3);
  background: var(--color-warning-subtle, var(--bg-tertiary));
  border-radius: var(--radius-sm);
  font-size: var(--text-xs);
  color: var(--color-warning, var(--text-secondary));
}

/* 缺失复盘提示块 */
.briefing-missing-review {
  display: flex;
  align-items: center;
  gap: var(--space-4);
  padding: var(--space-4);
  background: var(--color-warning-subtle, var(--bg-tertiary));
  border: 1.5px solid var(--color-warning, var(--border-color));
  border-radius: var(--radius-md);
  text-align: left;
  width: 100%;
}

.briefing-warn-icon {
  background: var(--color-warning-subtle, var(--accent-subtle));
  color: var(--color-warning, var(--accent));
}

.briefing-loading {
  display: flex;
  justify-content: center;
  padding: var(--space-4);
}

.briefing-footer {
  display: flex;
  align-items: center;
  gap: var(--space-3);
  margin-top: var(--space-1);
}

/* 可点击区块标题 */
.clickable-title {
  cursor: pointer;
  transition: color var(--transition-fast);
}

.clickable-title:hover {
  color: var(--accent);
}

.briefing-section-arrow {
  margin-left: auto;
  color: var(--text-tertiary);
  transition: transform var(--transition-fast);
}

.clickable-title:hover .briefing-section-arrow {
  transform: translateX(2px);
  color: var(--accent);
}

/* ── 昨日复盘侧栏 ── */
.review-sidebar-header {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  margin-bottom: var(--space-3);
}

.review-sidebar-icon {
  color: var(--accent);
}

.review-sidebar-title {
  font-size: var(--text-base);
  font-weight: var(--font-heading);
  color: var(--text-primary);
  margin: 0;
}

.review-sidebar-date {
  margin-left: auto;
  font-size: var(--text-xs);
  color: var(--text-tertiary);
}

.review-sidebar-body {
  display: flex;
  flex-direction: column;
  gap: var(--space-3);
}

.review-metric {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--space-2);
}

.review-metric-label {
  font-size: var(--text-xs);
  color: var(--text-tertiary);
  font-weight: var(--font-label);
}

.review-metric-value {
  font-size: var(--text-xl);
  font-weight: var(--font-bold);
}

.review-metric-value.good { color: var(--color-success, #22c55e); }
.review-metric-value.warn { color: var(--color-warning, #f59e0b); }
.review-metric-value.bad { color: var(--color-danger, #ef4444); }

.review-metric-text {
  font-size: var(--text-sm);
  color: var(--text-primary);
  text-align: right;
}

.review-sidebar-action {
  align-self: flex-start;
  margin-top: var(--space-1);
}

/* 缺失复盘 */
.review-sidebar-missing,
.review-sidebar-exempt {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: var(--space-2);
  padding: var(--space-4) var(--space-2);
  text-align: center;
}

.review-missing-icon,
.review-exempt-icon {
  color: var(--color-warning, var(--text-tertiary));
}

.review-exempt-icon {
  color: var(--accent);
}

.review-missing-title {
  font-size: var(--text-sm);
  font-weight: var(--font-semibold);
  color: var(--text-primary);
}

.review-missing-desc {
  font-size: var(--text-xs);
  color: var(--text-tertiary);
}

/* ── 紧凑统计卡 ── */
.stats-card {
  padding: var(--space-3) var(--space-4) !important;
}

.stats-row {
  display: flex;
  align-items: center;
  justify-content: space-around;
  gap: var(--space-2);
}

.stats-item {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 2px;
  flex: 1;
}

.stats-value {
  font-size: var(--text-lg);
  font-weight: var(--font-bold);
  color: var(--text-primary);
}

.stats-label {
  font-size: 10px;
  color: var(--text-tertiary);
  font-weight: var(--font-label);
}

.stats-divider {
  width: 1px;
  height: 28px;
  background: var(--border-color);
  flex-shrink: 0;
}

.briefing-empty {
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

.briefing-empty:hover:not(:disabled) {
  border-color: var(--accent);
  background: var(--accent-subtle);
}

/* 每日开始时间前的提示块（非交互） */
.briefing-before-start {
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

/* 休息日提示块（非交互） */
.briefing-rest-day {
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

.briefing-rest-icon {
  background: var(--accent-subtle);
  color: var(--accent);
}

/* 今日计划全部完成提示块（可点击跳转） */
.briefing-all-done {
  display: flex;
  align-items: center;
  gap: var(--space-4);
  padding: var(--space-4);
  background: var(--color-success-subtle, var(--bg-tertiary));
  border: 1.5px solid var(--color-success, var(--border-color));
  border-radius: var(--radius-md);
  cursor: pointer;
  transition: transform var(--transition-fast), box-shadow var(--transition-fast);
  text-align: left;
  width: 100%;
}

.briefing-all-done:hover {
  transform: translateY(-1px);
  box-shadow: var(--shadow-sm);
}

.briefing-done-icon {
  background: var(--color-success-subtle, var(--accent-subtle));
  color: var(--color-success, var(--accent));
}

.briefing-empty-icon {
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

.briefing-empty-text {
  display: flex;
  flex-direction: column;
  gap: 2px;
  min-width: 0;
  flex: 1;
}

.briefing-empty-title {
  font-size: var(--text-base);
  font-weight: var(--font-semibold);
  color: var(--text-primary);
}

.briefing-empty-desc {
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

@media (max-width: 860px) {
  .dashboard-main {
    grid-template-columns: 1fr;
  }

  .review-sidebar {
    transform: translateX(0) translateY(12px);
  }

  .review-sidebar.sidebar-enter {
    transform: translateX(0) translateY(0);
  }
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

  .briefing-quote-text {
    font-size: var(--text-base);
  }

  .briefing-estimation-grid {
    grid-template-columns: 1fr;
  }

  .briefing-missing-review {
    flex-wrap: wrap;
  }

  .stats-row {
    gap: var(--space-1);
  }

  .stats-value {
    font-size: var(--text-base);
  }

  .status-footer {
    flex-wrap: wrap;
    gap: var(--space-2) var(--space-4);
  }
}
</style>
