<script setup lang="ts">
/**
 * 工作台 — 主容器
 *
 * 由原 DashboardView.vue 拆分而来：只负责页面装配、数据编排与生命周期。
 * 各数据层见 composables/，UI 见 components/，共享样式见 dashboard-base.css。
 */
import { computed, onMounted, onBeforeUnmount } from "vue";
import { useRouter } from "vue-router";
import { useDashboardStore } from "@/stores/dashboard";
import LoadingSpinner from "@/components/ui/LoadingSpinner.vue";
import EmptyState from "@/components/ui/EmptyState.vue";
import Button from "@/components/ui/Button.vue";
import { RefreshCw } from "lucide-vue-next";
import { useDashboardData } from "./composables/useDashboardData";
import { useBriefing } from "./composables/useBriefing";
import { useDashboardAnimation } from "./composables/useDashboardAnimation";
import { weekdayShort, isToday } from "./utils/date-labels";
import DashboardHero from "./components/DashboardHero.vue";
import UpdateModal from "./components/UpdateModal.vue";
import BriefingCard, {
  type BriefingView,
  type SpecialView,
  type TodayView,
  type WeekView,
} from "./components/BriefingCard.vue";
import YesterdayReviewGate, {
  type ReviewSidebarView,
} from "./components/YesterdayReviewGate.vue";
import "./dashboard-base.css";

const router = useRouter();
const dashboardStore = useDashboardStore();

const data = useDashboardData();
const briefing = useBriefing();
const animation = useDashboardAnimation();

// ── 导航 ──
function goToday() {
  router.push("/today");
}
function goReview() {
  router.push("/review");
}
function goWeekPlan() {
  router.push({ name: "week-plan" });
}

// ── 简报加载（含首次查看的闪烁提示与入场动画） ──
async function loadBriefing() {
  await briefing.loadBriefing();
  // 如果简报存在且非特殊状态（开始前/休息日/排除日），触发入场动画
  if (briefing.briefingExists.value && !data.isSpecialState.value) {
    animation.playEntranceAnimation();
  }
}

// 统一刷新：首页数据 + 简报
function refresh() {
  data.refresh();
  loadBriefing();
}

// ── BriefingCard 分组视图 ──
const briefingView = computed<BriefingView>(() => ({
  exists: briefing.briefingExists.value,
  yesterdayReviewExists: briefing.yesterdayReviewExists.value,
  loading: briefing.briefingLoading.value,
  regenerating: briefing.briefingRegenerating.value,
  needYesterdayReview: briefing.needYesterdayReview.value,
  withinMakeupWindow: briefing.withinMakeupWindow.value,
  greeting: briefing.briefingGreeting.value,
  estimations: briefing.estimationList.value,
  animated: animation.briefingAnimated.value,
}));

const specialView = computed<SpecialView>(() => ({
  active: data.isSpecialState.value,
  beforeStart: data.isBeforeDailyStart.value,
  dailyStartTimeLabel: data.dailyStartTimeLabel.value,
  isRestDay: data.isTodayRestDay.value,
  isExcluded: data.isTodayExcluded.value,
  excludedReasonLabel: data.todayExcludedReasonLabel.value,
}));

const todayView = computed<TodayView>(() => ({
  tasks: data.todayTasks.value,
  allCompleted: data.allTasksCompleted.value,
  timeTrackingEnabled: data.timeTrackingEnabled.value,
  doneCount: data.todayDoneCount.value,
  totalCount: data.todayTotalCount.value,
}));

const weekView = computed<WeekView>(() => ({
  hasWeekProgress: !!data.summary.value?.week_progress,
  progress: data.weekProgressValue.value,
  studiedDays: data.studiedDays.value,
  plannedDays: data.plannedDaysPerWeek.value,
  remainingHours: data.remainingHours.value,
  isOnTrack: data.isOnTrackValue.value,
  onTrackLabel: data.onTrackLabel.value,
  dailyBreakdown: data.summary.value?.week_progress.daily_breakdown ?? [],
  isDayStudied: data.isDayStudied,
  isToday: (d: string) => isToday(d, data.todayDateStr),
  weekdayShort,
}));

// ── 昨日复盘侧栏视图 ──
const reviewSidebarView = computed<ReviewSidebarView>(() => ({
  sidebarAnimated: animation.sidebarAnimated.value,
  yesterdayDateStr: briefing.yesterdayDateStr.value,
  yesterdayReviewExists: briefing.yesterdayReviewExists.value,
  hasReviewData: briefing.yesterdayReviewExists.value && !!briefing.yesterdayReviewData.value,
  completionRate: briefing.yesterdayCompletionRate.value,
  feeling: briefing.yesterdayFeeling.value,
  feelingVariant: briefing.yesterdayFeelingVariant.value,
  difficulty: briefing.yesterdayDifficulty.value,
  actualHours: briefing.yesterdayActualHours.value,
  timeTrackingEnabled: data.timeTrackingEnabled.value,
  needYesterdayReview: briefing.needYesterdayReview.value,
  withinMakeupWindow: briefing.withinMakeupWindow.value,
  streakDays: data.streakDays.value,
  totalStudyDays: data.totalStudyDays.value,
  weekPlanProgress: data.weekProgressValue.value,
}));

onMounted(() => {
  data.refresh();
  loadBriefing();
  // 每分钟刷新当前时间，确保到点后自动展示今日计划
  data.startClock();
});

onBeforeUnmount(() => {
  // H30：组件卸载后不再更新 state / 操作 DOM
  briefing.dispose();
  animation.markUnmounted();
  data.stopClock();
});
</script>

<template>
  <div class="dashboard-view">
    <!-- Loading -->
    <LoadingSpinner
      v-if="dashboardStore.loading && !data.summary.value"
      :size="32"
      label="加载工作台数据…"
    />

    <!-- Empty / Error -->
    <EmptyState
      v-else-if="!data.summary.value"
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
      <UpdateModal />

      <!-- Hero -->
      <DashboardHero
        :greeting="data.greeting.value"
        :display-name="data.displayName.value"
        :date-label="data.dateLabel.value"
        :remaining-days="data.remainingDays.value"
        :show-greeting="data.showGreeting.value"
      />

      <!-- 主区域：简报 + 侧栏 -->
      <div class="dashboard-main" :class="{ 'no-sidebar': data.isSpecialState.value }">
        <!-- 左侧：每日简报 -->
        <BriefingCard
          :briefing="briefingView"
          :special="specialView"
          :today="todayView"
          :week="weekView"
          @regenerate="briefing.regenerateBriefing"
          @go-today="goToday"
          @go-review="goReview"
          @go-week-plan="goWeekPlan"
        />

        <!-- 右侧：昨日复盘摘要侧栏 -->
        <YesterdayReviewGate
          v-if="!data.isSpecialState.value"
          :view="reviewSidebarView"
          @go-review="goReview"
        />
      </div>
    </template>
  </div>
</template>
