<script setup lang="ts">
/**
 * 工作台 — 每日简报卡片（主区域）
 *
 * 由原 DashboardView 的简报区块拆分而来：承载特殊状态（开始前/休息日/排除日/
 * 全部完成）、AI 寄语、今日任务清单、各科估时与迷你本周进度等展示与交互。
 * 数据通过分组 props 注入，交互通过 emit 通知页面编排。
 */
import Card from "@/components/ui/Card.vue";
import Badge from "@/components/ui/Badge.vue";
import Button from "@/components/ui/Button.vue";
import ProgressBar from "@/components/ui/ProgressBar.vue";
import LoadingSpinner from "@/components/ui/LoadingSpinner.vue";
import {
  Sparkles,
  RefreshCw,
  ChevronRight,
  Clock,
  Coffee,
  Ban,
  CheckCircle2,
  Target,
  TrendingUp,
  Calendar,
  Flag,
} from "lucide-vue-next";
import type { PlanTask, SubjectEstimation } from "@/types";
import { subjectLabel, subjectBadgeVariant } from "../utils/subject-labels";

export interface BriefingView {
  exists: boolean;
  /** 昨日复盘是否存在（决定能否提供 AI 建议 / 显示重新生成按钮） */
  yesterdayReviewExists: boolean;
  loading: boolean;
  regenerating: boolean;
  needYesterdayReview: boolean;
  withinMakeupWindow: boolean;
  greeting: string;
  estimations: (SubjectEstimation & { subjectLabel: string })[];
  animated: boolean;
}

export interface SpecialView {
  active: boolean;
  beforeStart: boolean;
  dailyStartTimeLabel: string;
  isRestDay: boolean;
  isExcluded: boolean;
  excludedReasonLabel: string;
}

export interface TodayView {
  tasks: PlanTask[];
  allCompleted: boolean;
  timeTrackingEnabled: boolean;
  doneCount: number;
  totalCount: number;
}

export interface WeekView {
  hasWeekProgress: boolean;
  progress: number;
  studiedDays: number;
  plannedDays: number;
  remainingHours: number;
  isOnTrack: boolean;
  onTrackLabel: string;
  dailyBreakdown: { date: string; hours: number; tasks_done: number }[];
  isDayStudied: (date: string) => boolean;
  isToday: (date: string) => boolean;
  weekdayShort: (date: string) => string;
}

defineProps<{
  briefing: BriefingView;
  special: SpecialView;
  today: TodayView;
  week: WeekView;
}>();

defineEmits<{
  regenerate: [];
  goToday: [];
  goReview: [];
  goWeekPlan: [];
}>();
</script>

<template>
  <Card
    padding="lg"
    class="card briefing-card"
    :class="{ 'briefing-enter': briefing.animated }"
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
          v-if="!special.active && briefing.exists && briefing.yesterdayReviewExists"
          variant="ghost"
          size="sm"
          :loading="briefing.regenerating"
          @click="$emit('regenerate')"
        >
          <RefreshCw :size="13" />
          重新生成
        </Button>
        <Button
          v-if="(today.tasks.length > 0 || today.allCompleted) && !special.active"
          variant="ghost"
          size="sm"
          @click="$emit('goToday')"
        >
          查看详情
          <ChevronRight :size="14" />
        </Button>
      </div>
    </div>

    <!-- 特殊状态：居中提示 -->
    <div v-if="special.beforeStart" class="briefing-center-prompt">
      <div class="briefing-empty-icon"><Clock :size="28" /></div>
      <span class="briefing-empty-title">今天的学习时间还没开始</span>
      <span class="briefing-empty-desc">每日开始时间为 {{ special.dailyStartTimeLabel }}，到点后这里会展示今日简报。</span>
    </div>

    <div v-else-if="special.isRestDay" class="briefing-center-prompt">
      <div class="briefing-empty-icon briefing-rest-icon"><Coffee :size="28" /></div>
      <span class="briefing-empty-title">今日是休息日</span>
      <span class="briefing-empty-desc">好好放松一下，明天继续。</span>
    </div>

    <div v-else-if="special.isExcluded" class="briefing-center-prompt">
      <div class="briefing-empty-icon briefing-rest-icon"><Ban :size="28" /></div>
      <span class="briefing-empty-title">今日是排除日</span>
      <span class="briefing-empty-desc">{{ special.excludedReasonLabel }}</span>
    </div>

    <div v-else-if="today.allCompleted" class="briefing-center-prompt clickable" @click="$emit('goToday')">
      <div class="briefing-empty-icon briefing-done-icon"><CheckCircle2 :size="28" /></div>
      <span class="briefing-empty-title">今日计划已全部完成</span>
      <span class="briefing-empty-desc">辛苦了！可前往复盘记录今日学习情况。</span>
    </div>

    <!-- 简报加载中 -->
    <div v-else-if="briefing.loading && !briefing.exists && today.tasks.length === 0" class="briefing-loading">
      <LoadingSpinner :size="24" label="正在生成今日简报…" />
    </div>

    <!-- 简报内容 -->
    <div v-else class="briefing-body">
      <!-- 缺失昨日复盘提示横幅 -->
      <div v-if="briefing.needYesterdayReview" class="briefing-missing-review">
        <div class="briefing-empty-icon briefing-warn-icon">
          <Flag :size="18" />
        </div>
        <div class="briefing-empty-text">
          <span class="briefing-empty-title">昨日复盘缺失</span>
          <span class="briefing-empty-desc">
            {{ briefing.withinMakeupWindow
              ? '完成昨日复盘后即可生成今日 AI 简报与建议'
              : '已错过补复盘窗口，今日不提供 AI 建议' }}
          </span>
        </div>
        <Button v-if="briefing.withinMakeupWindow" variant="primary" size="sm" @click="$emit('goReview')">
          去补复盘
          <ChevronRight :size="14" />
        </Button>
      </div>

      <!-- AI 寄语（大字引言式） -->
      <div v-if="briefing.greeting" class="briefing-quote">
        <span class="briefing-quote-mark">"</span>
        <p class="briefing-quote-text">{{ briefing.greeting }}</p>
        <span class="briefing-quote-mark closing">"</span>
      </div>

      <!-- 今日任务清单 -->
      <div v-if="today.tasks.length > 0" class="briefing-section" @click="$emit('goToday')">
        <div class="briefing-section-title clickable-title">
          <Target :size="13" />
          <span>今日任务（{{ today.doneCount }}/{{ today.totalCount }}）</span>
          <ChevronRight :size="12" class="briefing-section-arrow" />
        </div>
        <ul class="briefing-task-list">
          <li
            v-for="task in today.tasks"
            :key="task.id"
            class="briefing-task-item"
            :class="{ done: task.status === 'done' }"
          >
            <Badge :variant="subjectBadgeVariant(task.subject)" size="sm">
              {{ subjectLabel(task.subject) }}
            </Badge>
            <span class="briefing-task-title">{{ task.title }}</span>
            <span
              v-if="today.timeTrackingEnabled && task.estimated_hours"
              class="briefing-task-time"
            >
              <Clock :size="11" />
              {{ task.estimated_hours }}h
            </span>
          </li>
        </ul>
      </div>

      <!-- AI 各科估时 -->
      <div v-if="briefing.estimations.length > 0 && briefing.yesterdayReviewExists" class="briefing-section">
        <div class="briefing-section-title">
          <TrendingUp :size="13" />
          <span>阶段估时</span>
        </div>
        <div class="briefing-estimation-grid">
          <div
            v-for="est in briefing.estimations"
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
      <div v-if="week.hasWeekProgress" class="briefing-section mini-week-progress" @click="$emit('goWeekPlan')">
        <div class="briefing-section-title clickable-title">
          <Calendar :size="13" />
          <span>本周进度</span>
          <ChevronRight :size="12" class="briefing-section-arrow" />
        </div>
        <div class="mini-week-stats">
          <span class="mini-week-percent">{{ week.progress }}%</span>
          <span class="mini-week-detail">{{ week.studiedDays }}/{{ week.plannedDays }} 天 · 剩余 {{ week.remainingHours }} 小时 · {{ week.onTrackLabel }}</span>
        </div>
        <ProgressBar
          :value="week.progress"
          :max="100"
          :variant="week.progress >= 100 ? 'success' : week.isOnTrack ? 'default' : 'warning'"
          size="sm"
        />
        <div class="mini-week-dots">
          <div
            v-for="day in week.dailyBreakdown"
            :key="day.date"
            class="mini-dot"
            :class="{ studied: week.isDayStudied(day.date), today: week.isToday(day.date) }"
            :title="day.date"
          >
            <span class="dot-label">{{ week.weekdayShort(day.date) }}</span>
          </div>
        </div>
      </div>

      <!-- 底部操作 -->
      <div v-if="today.tasks.length > 0" class="briefing-footer">
        <Button variant="primary" size="md" @click="$emit('goToday')">
          开始学习
          <ChevronRight :size="16" />
        </Button>
      </div>
    </div>

    <!-- 无简报且无任务：空状态 -->
    <div
      v-if="!special.active && !today.allCompleted && !briefing.exists && today.tasks.length === 0 && !briefing.needYesterdayReview && !briefing.loading"
      class="briefing-center-prompt"
    >
      <div class="briefing-empty-icon"><Sparkles :size="28" /></div>
      <span class="briefing-empty-title">今日暂无计划</span>
      <span class="briefing-empty-desc">请先生成周计划，日计划将自动从周计划中拆分生成</span>
    </div>
  </Card>
</template>
