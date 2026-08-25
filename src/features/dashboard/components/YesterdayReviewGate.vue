<script setup lang="ts">
/**
 * 工作台 — 昨日复盘摘要侧栏
 *
 * 由原 DashboardView 的右侧 `<aside>` 拆分而来：承载「昨日复盘」卡片与
 * 紧凑统计卡（连续天数 / 累计天数 / 本周进度）。数据通过 props 注入，
 * 交互（去复盘 / 查看复盘详情）通过 emit 通知页面编排。
 */
import Card from "@/components/ui/Card.vue";
import Badge from "@/components/ui/Badge.vue";
import Button from "@/components/ui/Button.vue";
import { Award, Flag, Coffee, ChevronRight } from "lucide-vue-next";

export interface ReviewSidebarView {
  /** 侧栏入场动画 */
  sidebarAnimated: boolean;
  yesterdayDateStr: string;
  /** 昨日复盘文件是否存在（由简报结果决定） */
  yesterdayReviewExists: boolean;
  /** 是否有可展示的复盘数据（exists && 已拉取） */
  hasReviewData: boolean;
  completionRate: number;
  feeling: string;
  feelingVariant: "default" | "success" | "danger";
  difficulty: string | null;
  actualHours: number;
  timeTrackingEnabled: boolean;
  needYesterdayReview: boolean;
  withinMakeupWindow: boolean;
  /** 紧凑统计卡 */
  streakDays: number;
  totalStudyDays: number;
  weekPlanProgress: number;
}

defineProps<{ view: ReviewSidebarView }>();

defineEmits<{ goReview: [] }>();
</script>

<template>
  <aside
    class="review-sidebar"
    :class="{ 'sidebar-enter': view.sidebarAnimated }"
  >
    <Card padding="md" class="card review-card" surface="1" hoverable>
      <div class="review-sidebar-header">
        <Award :size="16" class="review-sidebar-icon" />
        <h3 class="review-sidebar-title">昨日复盘</h3>
        <span class="review-sidebar-date">{{ view.yesterdayDateStr }}</span>
      </div>

      <!-- 有复盘数据 -->
      <div v-if="view.hasReviewData" class="review-sidebar-body">
        <div class="review-metric">
          <span class="review-metric-label">完成率</span>
          <span
            class="review-metric-value"
            :class="view.completionRate >= 80 ? 'good' : view.completionRate >= 50 ? 'warn' : 'bad'"
          >
            {{ view.completionRate }}%
          </span>
        </div>

        <div class="review-metric">
          <span class="review-metric-label">整体感受</span>
          <Badge :variant="view.feelingVariant" size="sm">
            {{ view.feeling }}
          </Badge>
        </div>

        <div v-if="view.difficulty" class="review-metric">
          <span class="review-metric-label">主要困难</span>
          <span class="review-metric-text">{{ view.difficulty }}</span>
        </div>

        <div v-if="view.timeTrackingEnabled && view.actualHours > 0" class="review-metric">
          <span class="review-metric-label">学习时长</span>
          <span class="review-metric-text">{{ view.actualHours.toFixed(1) }} 小时</span>
        </div>

        <Button variant="ghost" size="sm" class="review-sidebar-action" @click="$emit('goReview')">
          查看复盘详情
          <ChevronRight :size="14" />
        </Button>
      </div>

      <!-- 缺失复盘 -->
      <div v-else-if="view.needYesterdayReview" class="review-sidebar-missing">
        <Flag :size="20" class="review-missing-icon" />
        <span class="review-missing-title">昨日未复盘</span>
        <span class="review-missing-desc">
          {{ view.withinMakeupWindow ? '点击下方按钮补复盘' : '已错过补复盘窗口' }}
        </span>
        <Button v-if="view.withinMakeupWindow" variant="primary" size="sm" @click="$emit('goReview')">
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
          <span class="stats-value">{{ view.streakDays }}</span>
          <span class="stats-label">连续天数</span>
        </div>
        <div class="stats-divider" />
        <div class="stats-item">
          <span class="stats-value">{{ view.totalStudyDays }}</span>
          <span class="stats-label">累计天数</span>
        </div>
        <div class="stats-divider" />
        <div class="stats-item">
          <span class="stats-value">{{ view.weekPlanProgress }}%</span>
          <span class="stats-label">本周进度</span>
        </div>
      </div>
    </Card>
  </aside>
</template>
