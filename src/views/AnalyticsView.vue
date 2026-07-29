<script setup lang="ts">
import { computed, onMounted } from "vue";
import { useAnalyticsStore } from "@/stores/analytics";
import { useSettingsStore } from "@/stores/settings";
import Card from "@/components/ui/Card.vue";
import Button from "@/components/ui/Button.vue";
import LoadingSpinner from "@/components/ui/LoadingSpinner.vue";
import EmptyState from "@/components/ui/EmptyState.vue";
import {
  BarChart3,
  TrendingUp,
  TrendingDown,
  Minus,
  Target,
  Clock,
  CheckCircle2,
  AlertTriangle,
  RefreshCw,
  Calendar,
  Award,
  Activity,
} from "lucide-vue-next";
import type { AnalyticsRange, PeriodComparison } from "@/types";

import * as echarts from "echarts/core";
import { BarChart, LineChart, PieChart } from "echarts/charts";
import {
  GridComponent,
  TooltipComponent,
  LegendComponent,
  TitleComponent,
  DataZoomComponent,
} from "echarts/components";
import { CanvasRenderer } from "echarts/renderers";
import VChart from "vue-echarts";

echarts.use([
  BarChart,
  LineChart,
  PieChart,
  GridComponent,
  TooltipComponent,
  LegendComponent,
  TitleComponent,
  DataZoomComponent,
  CanvasRenderer,
]);

const store = useAnalyticsStore();
const settingsStore = useSettingsStore();

const rangeOptions: { value: AnalyticsRange; label: string }[] = [
  { value: "last_7_days", label: "近7天" },
  { value: "last_30_days", label: "近30天" },
  { value: "all", label: "全部" },
];

// 主题色
const isDark = computed(() => settingsStore.theme === "dark");
const textColor = computed(() => (isDark.value ? "#cbd5e1" : "#475569"));
const axisLineColor = computed(() => (isDark.value ? "#475569" : "#cbd5e1"));
const gridLineColor = computed(() => (isDark.value ? "#334155" : "#e2e8f0"));

// 主题色板
const palette = {
  primary: "#6366f1",
  success: "#10b981",
  warning: "#f59e0b",
  danger: "#ef4444",
  info: "#3b82f6",
  purple: "#a855f7",
  cyan: "#06b6d4",
};

// ── 图表配置 ──

// 1. 完成率与任务量趋势
const completionOption = computed(() => {
  const points = store.learningTrend?.points ?? [];
  return {
    tooltip: {
      trigger: "axis",
      axisPointer: { type: "cross" },
    },
    legend: {
      data: ["完成率", "计划任务", "已完成"],
      textStyle: { color: textColor.value },
    },
    grid: { left: 50, right: 50, bottom: 30, top: 40 },
    xAxis: {
      type: "category",
      data: points.map((p) => p.date.slice(5)),
      axisLine: { lineStyle: { color: axisLineColor.value } },
      axisLabel: { color: textColor.value, fontSize: 11 },
    },
    yAxis: [
      {
        type: "value",
        name: "完成率(%)",
        max: 100,
        axisLine: { lineStyle: { color: axisLineColor.value } },
        axisLabel: { color: textColor.value },
        splitLine: { lineStyle: { color: gridLineColor.value } },
      },
      {
        type: "value",
        name: "任务数",
        axisLine: { lineStyle: { color: axisLineColor.value } },
        axisLabel: { color: textColor.value },
        splitLine: { show: false },
      },
    ],
    series: [
      {
        name: "完成率",
        type: "line",
        smooth: true,
        data: points.map((p) => p.completion_rate.toFixed(1)),
        itemStyle: { color: palette.primary },
        areaStyle: { opacity: 0.15 },
      },
      {
        name: "计划任务",
        type: "bar",
        yAxisIndex: 1,
        data: points.map((p) => p.planned_tasks),
        itemStyle: { color: palette.info, opacity: 0.6 },
      },
      {
        name: "已完成",
        type: "bar",
        yAxisIndex: 1,
        data: points.map((p) => p.completed_tasks),
        itemStyle: { color: palette.success },
      },
    ],
  };
});

// 2. 学习时长趋势
const hoursOption = computed(() => {
  const points = store.learningTrend?.points ?? [];
  return {
    tooltip: { trigger: "axis" },
    legend: {
      data: ["计划时长", "实际时长"],
      textStyle: { color: textColor.value },
    },
    grid: { left: 50, right: 20, bottom: 30, top: 40 },
    xAxis: {
      type: "category",
      data: points.map((p) => p.date.slice(5)),
      axisLine: { lineStyle: { color: axisLineColor.value } },
      axisLabel: { color: textColor.value, fontSize: 11 },
    },
    yAxis: {
      type: "value",
      name: "小时",
      axisLine: { lineStyle: { color: axisLineColor.value } },
      axisLabel: { color: textColor.value },
      splitLine: { lineStyle: { color: gridLineColor.value } },
    },
    series: [
      {
        name: "计划时长",
        type: "line",
        smooth: true,
        data: points.map((p) => p.planned_hours.toFixed(2)),
        itemStyle: { color: palette.warning },
        lineStyle: { type: "dashed" as const },
      },
      {
        name: "实际时长",
        type: "line",
        smooth: true,
        data: points.map((p) => p.actual_hours.toFixed(2)),
        itemStyle: { color: palette.success },
        areaStyle: { opacity: 0.2 },
      },
    ],
  };
});

// 3. 掌握度分布饼图
const masteryOption = computed(() => {
  const m = store.reviewQuality?.mastery;
  if (!m) return {};
  return {
    tooltip: { trigger: "item", formatter: "{b}: {c} ({d}%)" },
    legend: {
      bottom: 0,
      textStyle: { color: textColor.value },
    },
    series: [
      {
        type: "pie",
        radius: ["40%", "70%"],
        center: ["50%", "45%"],
        avoidLabelOverlap: false,
        itemStyle: { borderRadius: 6, borderColor: isDark.value ? "#1e293b" : "#fff", borderWidth: 2 },
        label: { show: false, position: "center" },
        emphasis: { label: { show: true, fontSize: 14, fontWeight: "bold" } },
        data: [
          { value: m.mastered, name: "已掌握", itemStyle: { color: palette.success } },
          { value: m.basic, name: "基本掌握", itemStyle: { color: palette.info } },
          { value: m.weak, name: "掌握不足", itemStyle: { color: palette.warning } },
          { value: m.not_marked, name: "未标记", itemStyle: { color: palette.danger, opacity: 0.5 } },
        ],
      },
    ],
  };
});

// 4. 阻碍因素 Top 5
const blockersOption = computed(() => {
  const blockers = store.reviewQuality?.blockers ?? [];
  const top5 = blockers.slice(0, 5);
  return {
    tooltip: { trigger: "axis", axisPointer: { type: "shadow" } },
    grid: { left: 100, right: 30, bottom: 20, top: 20 },
    xAxis: {
      type: "value",
      axisLine: { lineStyle: { color: axisLineColor.value } },
      axisLabel: { color: textColor.value },
      splitLine: { lineStyle: { color: gridLineColor.value } },
    },
    yAxis: {
      type: "category",
      data: top5.map((b) => b.label).reverse(),
      axisLine: { lineStyle: { color: axisLineColor.value } },
      axisLabel: { color: textColor.value, fontSize: 12 },
    },
    series: [
      {
        type: "bar",
        data: top5.map((b) => b.count).reverse(),
        itemStyle: {
          color: new echarts.graphic.LinearGradient(0, 0, 1, 0, [
            { offset: 0, color: palette.warning },
            { offset: 1, color: palette.danger },
          ]),
          borderRadius: [0, 4, 4, 0],
        },
        label: { show: true, position: "right", color: textColor.value },
      },
    ],
  };
});

// 5. 感受曲线
const feelingOption = computed(() => {
  const feelings = store.reviewQuality?.feelings ?? [];
  return {
    tooltip: {
      trigger: "axis",
      formatter: (params: any) => {
        const p = params[0];
        const point = feelings[p.dataIndex];
        return `${point.date}<br/>感受: ${point.label}`;
      },
    },
    grid: { left: 40, right: 20, bottom: 30, top: 20 },
    xAxis: {
      type: "category",
      data: feelings.map((f) => f.date.slice(5)),
      axisLine: { lineStyle: { color: axisLineColor.value } },
      axisLabel: { color: textColor.value, fontSize: 11 },
    },
    yAxis: {
      type: "value",
      min: 0,
      max: 3,
      interval: 1,
      axisLine: { lineStyle: { color: axisLineColor.value } },
      axisLabel: {
        color: textColor.value,
        formatter: (v: number) => {
          if (v === 3) return "顺利";
          if (v === 2) return "一般";
          if (v === 1) return "困难";
          return "";
        },
      },
      splitLine: { lineStyle: { color: gridLineColor.value } },
    },
    series: [
      {
        type: "line",
        smooth: true,
        data: feelings.map((f) => f.score),
        itemStyle: { color: palette.purple },
        areaStyle: {
          color: new echarts.graphic.LinearGradient(0, 0, 0, 1, [
            { offset: 0, color: "rgba(168, 85, 247, 0.3)" },
            { offset: 1, color: "rgba(168, 85, 247, 0.02)" },
          ]),
        },
        markLine: {
          silent: true,
          data: [{ yAxis: 2 }],
          lineStyle: { color: gridLineColor.value, type: "dashed" as const },
        },
      },
    ],
  };
});

// 6. 困难类型分布饼图
const difficultyOption = computed(() => {
  const difficulties = store.reviewQuality?.difficulties ?? [];
  if (difficulties.length === 0) return {};
  return {
    tooltip: { trigger: "item", formatter: "{b}: {c} ({d}%)" },
    legend: {
      bottom: 0,
      textStyle: { color: textColor.value },
      type: "scroll",
    },
    series: [
      {
        type: "pie",
        radius: "60%",
        center: ["50%", "45%"],
        data: difficulties.map((d) => ({
          value: d.count,
          name: d.label,
        })),
        itemStyle: { borderRadius: 4, borderColor: isDark.value ? "#1e293b" : "#fff", borderWidth: 2 },
        label: { color: textColor.value },
      },
    ],
  };
});

// ── 周期对比图 ──
function buildComparisonOption(cmp: PeriodComparison | undefined) {
  if (!cmp) return {};
  return {
    tooltip: { trigger: "axis", axisPointer: { type: "shadow" } },
    legend: {
      data: [cmp.current_label, cmp.previous_label],
      textStyle: { color: textColor.value },
    },
    grid: { left: 50, right: 30, bottom: 30, top: 40 },
    xAxis: {
      type: "category",
      data: ["完成率(%)", "学习时长(h)", "任务总数", "学习天数"],
      axisLine: { lineStyle: { color: axisLineColor.value } },
      axisLabel: { color: textColor.value },
    },
    yAxis: {
      type: "value",
      axisLine: { lineStyle: { color: axisLineColor.value } },
      axisLabel: { color: textColor.value },
      splitLine: { lineStyle: { color: gridLineColor.value } },
    },
    series: [
      {
        name: cmp.current_label,
        type: "bar",
        data: [
          cmp.current.avg_completion_rate.toFixed(1),
          cmp.current.total_hours.toFixed(1),
          cmp.current.total_tasks,
          cmp.current.study_days,
        ],
        itemStyle: { color: palette.primary, borderRadius: [4, 4, 0, 0] },
      },
      {
        name: cmp.previous_label,
        type: "bar",
        data: [
          cmp.previous.avg_completion_rate.toFixed(1),
          cmp.previous.total_hours.toFixed(1),
          cmp.previous.total_tasks,
          cmp.previous.study_days,
        ],
        itemStyle: { color: palette.cyan, borderRadius: [4, 4, 0, 0] },
      },
    ],
  };
}

const weekCompareOption = computed(() =>
  buildComparisonOption(store.comparison?.week_comparison)
);
const monthCompareOption = computed(() =>
  buildComparisonOption(store.comparison?.month_comparison)
);

// ── 统计卡片 ──
const trendStats = computed(() => {
  const t = store.learningTrend;
  if (!t) return null;
  return [
    { label: "平均完成率", value: `${t.avg_completion_rate.toFixed(1)}%`, icon: Target, color: palette.primary },
    { label: "累计学习时长", value: `${t.total_actual_hours.toFixed(1)}h`, icon: Clock, color: palette.success },
    { label: "累计完成任务", value: `${t.total_completed_tasks}/${t.total_planned_tasks}`, icon: CheckCircle2, color: palette.info },
    { label: "学习天数", value: `${t.study_days}`, icon: Award, color: palette.warning },
  ];
});

// ── 预测状态 ──
const predictionStatus = computed(() => {
  const p = store.comparison?.prediction;
  if (!p) return null;
  const map: Record<string, { icon: any; color: string; label: string }> = {
    on_track: { icon: CheckCircle2, color: palette.success, label: "进度健康" },
    at_risk: { icon: AlertTriangle, color: palette.warning, label: "存在风险" },
    off_track: { icon: AlertTriangle, color: palette.danger, label: "明显偏离" },
    no_data: { icon: Minus, color: palette.info, label: "数据不足" },
  };
  return { ...p, ...map[p.status] };
});

// ── 变化趋势图标 ──
function deltaIcon(delta: number) {
  if (delta > 0) return TrendingUp;
  if (delta < 0) return TrendingDown;
  return Minus;
}
function deltaColor(delta: number, higherIsBetter = true) {
  if (delta === 0) return palette.info;
  const positive = higherIsBetter ? delta > 0 : delta < 0;
  return positive ? palette.success : palette.danger;
}
function deltaText(delta: number, suffix = "", decimals = 1) {
  const sign = delta > 0 ? "+" : "";
  return `${sign}${delta.toFixed(decimals)}${suffix}`;
}

// ── 数据加载 ──
const showEmpty = computed(
  () => !store.loading && !store.error && !store.summary
);
const hasNoData = computed(() => {
  const t = store.learningTrend;
  return !t || t.points.length === 0 || t.study_days === 0;
});

async function refresh() {
  await store.load();
}

onMounted(() => {
  store.load();
});
</script>

<template>
  <div class="analytics-view">
    <!-- Header -->
    <div class="header">
      <div class="header-left">
        <BarChart3 :size="20" class="header-icon" />
        <h1 class="title">学习分析</h1>
      </div>
      <div class="header-actions">
        <div class="range-tabs">
          <button
            v-for="opt in rangeOptions"
            :key="opt.value"
            class="range-tab"
            :class="{ active: store.currentRange === opt.value }"
            @click="store.setRange(opt.value)"
          >
            {{ opt.label }}
          </button>
        </div>
        <Button variant="ghost" size="sm" :loading="store.loading" @click="refresh">
          <RefreshCw :size="14" />
          刷新
        </Button>
      </div>
    </div>

    <!-- Loading -->
    <LoadingSpinner v-if="store.loading && !store.summary" :size="32" label="加载分析数据…" />

    <!-- Error -->
    <EmptyState
      v-else-if="store.error"
      title="加载失败"
      :description="store.error"
    >
      <template #actions>
        <Button variant="primary" @click="refresh">重试</Button>
      </template>
    </EmptyState>

    <!-- Empty -->
    <EmptyState
      v-else-if="showEmpty"
      title="暂无分析数据"
      description="完成几天的学习并提交复盘后，这里会展示你的学习数据分析"
    />

    <!-- Content -->
    <template v-else>
      <!-- 无数据但已加载 -->
      <EmptyState
        v-if="hasNoData"
        title="所选范围内暂无学习数据"
        description="尝试切换时间范围，或开始学习并提交复盘后再来查看"
      />

      <template v-else>
        <!-- 区块1：学习量趋势 -->
        <section class="section">
          <div class="section-head">
            <TrendingUp :size="16" class="section-icon" />
            <h2 class="section-title">学习量趋势</h2>
          </div>

          <!-- 统计卡片 -->
          <div v-if="trendStats" class="stats-grid">
            <Card v-for="stat in trendStats" :key="stat.label" padding="md" class="stat-card">
              <div class="stat-row">
                <div class="stat-icon" :style="{ color: stat.color }">
                  <component :is="stat.icon" :size="18" />
                </div>
                <div class="stat-info">
                  <div class="stat-value">{{ stat.value }}</div>
                  <div class="stat-label">{{ stat.label }}</div>
                </div>
              </div>
            </Card>
          </div>

          <!-- 完成率图表 -->
          <Card padding="md" class="chart-card">
            <div class="chart-title">完成率与任务量</div>
            <v-chart :option="completionOption" autoresize class="chart" />
          </Card>

          <!-- 学习时长图表 -->
          <Card padding="md" class="chart-card">
            <div class="chart-title">学习时长（计划 vs 实际）</div>
            <v-chart :option="hoursOption" autoresize class="chart" />
          </Card>
        </section>

        <!-- 区块2：复盘质量分析 -->
        <section v-if="store.reviewQuality && store.reviewQuality.review_count > 0" class="section">
          <div class="section-head">
            <Activity :size="16" class="section-icon" />
            <h2 class="section-title">复盘质量分析</h2>
            <span class="section-count">基于 {{ store.reviewQuality.review_count }} 次复盘</span>
          </div>

          <div class="charts-row two-col">
            <!-- 掌握度分布 -->
            <Card padding="md" class="chart-card">
              <div class="chart-title">任务掌握度分布</div>
              <v-chart :option="masteryOption" autoresize class="chart" />
            </Card>

            <!-- 困难类型分布 -->
            <Card v-if="store.reviewQuality.difficulties.length > 0" padding="md" class="chart-card">
              <div class="chart-title">主要困难类型分布</div>
              <v-chart :option="difficultyOption" autoresize class="chart" />
            </Card>
          </div>

          <!-- 阻碍因素 Top 5 -->
          <Card v-if="store.reviewQuality.blockers.length > 0" padding="md" class="chart-card">
            <div class="chart-title">阻碍因素 Top 5</div>
            <v-chart :option="blockersOption" autoresize class="chart" />
          </Card>

          <!-- 感受曲线 -->
          <Card v-if="store.reviewQuality.feelings.length > 0" padding="md" class="chart-card">
            <div class="chart-title">学习感受曲线</div>
            <v-chart :option="feelingOption" autoresize class="chart" />
          </Card>
        </section>

        <!-- 区块3：周期对比与预测 -->
        <section v-if="store.comparison" class="section">
          <div class="section-head">
            <Calendar :size="16" class="section-icon" />
            <h2 class="section-title">周期对比与预测</h2>
          </div>

          <!-- 预测卡片 -->
          <Card v-if="predictionStatus" padding="md" class="prediction-card" :class="predictionStatus.status">
            <div class="pred-row">
              <div class="pred-icon" :style="{ color: predictionStatus.color }">
                <component :is="predictionStatus.icon" :size="22" />
              </div>
              <div class="pred-info">
                <div class="pred-title">{{ predictionStatus.label }}</div>
                <div class="pred-desc">{{ predictionStatus.description }}</div>
                <div class="pred-stats">
                  <span>近7天平均完成率: {{ predictionStatus.recent_avg_completion_rate.toFixed(1) }}%</span>
                  <span>近7天日均学习: {{ predictionStatus.recent_avg_daily_hours.toFixed(1) }}h</span>
                </div>
              </div>
            </div>
          </Card>

          <!-- 周对比 -->
          <Card padding="md" class="chart-card">
            <div class="chart-title">
              {{ store.comparison.week_comparison.current_label }} vs
              {{ store.comparison.week_comparison.previous_label }}
              <span class="delta-info">
                <component
                  :is="deltaIcon(store.comparison.week_comparison.completion_rate_delta)"
                  :size="13"
                  :style="{ color: deltaColor(store.comparison.week_comparison.completion_rate_delta) }"
                />
                <span :style="{ color: deltaColor(store.comparison.week_comparison.completion_rate_delta) }">
                  {{ deltaText(store.comparison.week_comparison.completion_rate_delta, '%') }}
                </span>
              </span>
            </div>
            <v-chart :option="weekCompareOption" autoresize class="chart" />
          </Card>

          <!-- 月对比 -->
          <Card padding="md" class="chart-card">
            <div class="chart-title">
              {{ store.comparison.month_comparison.current_label }} vs
              {{ store.comparison.month_comparison.previous_label }}
              <span class="delta-info">
                <component
                  :is="deltaIcon(store.comparison.month_comparison.completion_rate_delta)"
                  :size="13"
                  :style="{ color: deltaColor(store.comparison.month_comparison.completion_rate_delta) }"
                />
                <span :style="{ color: deltaColor(store.comparison.month_comparison.completion_rate_delta) }">
                  {{ deltaText(store.comparison.month_comparison.completion_rate_delta, '%') }}
                </span>
              </span>
            </div>
            <v-chart :option="monthCompareOption" autoresize class="chart" />
          </Card>
        </section>
      </template>
    </template>
  </div>
</template>

<style scoped>
.analytics-view {
  padding: var(--space-6) var(--space-6) var(--space-8);
  display: flex;
  flex-direction: column;
  gap: var(--space-6);
  max-width: 1200px;
  margin: 0 auto;
  width: 100%;
}

/* Header */
.header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  flex-wrap: wrap;
  gap: var(--space-3);
}
.header-left {
  display: flex;
  align-items: center;
  gap: var(--space-2);
}
.header-icon {
  color: var(--color-primary, var(--text-secondary));
}
.title {
  font-size: var(--text-xl);
  font-weight: 600;
  margin: 0;
}
.header-actions {
  display: flex;
  align-items: center;
  gap: var(--space-3);
}
.range-tabs {
  display: flex;
  background: var(--bg-secondary);
  border-radius: var(--radius-md);
  padding: 2px;
}
.range-tab {
  padding: 4px 12px;
  font-size: var(--text-sm);
  color: var(--text-tertiary);
  background: transparent;
  border: none;
  border-radius: calc(var(--radius-md) - 2px);
  cursor: pointer;
  transition: all 0.15s;
}
.range-tab:hover {
  color: var(--text-primary);
}
.range-tab.active {
  background: var(--bg-primary);
  color: var(--text-primary);
  box-shadow: 0 1px 2px rgba(0, 0, 0, 0.08);
}

/* Section */
.section {
  display: flex;
  flex-direction: column;
  gap: var(--space-3);
}
.section-head {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  padding: 0 var(--space-1);
}
.section-icon {
  color: var(--color-primary, var(--text-secondary));
}
.section-title {
  font-size: var(--text-base);
  font-weight: 600;
  margin: 0;
}
.section-count {
  font-size: var(--text-xs);
  color: var(--text-tertiary);
}

/* 统计卡片 */
.stats-grid {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(180px, 1fr));
  gap: var(--space-3);
}
.stat-card {
  transition: transform 0.15s;
}
.stat-card:hover {
  transform: translateY(-2px);
}
.stat-row {
  display: flex;
  align-items: center;
  gap: var(--space-3);
}
.stat-icon {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 36px;
  height: 36px;
  border-radius: var(--radius-md);
  background: var(--bg-tertiary);
}
.stat-info {
  display: flex;
  flex-direction: column;
}
.stat-value {
  font-size: var(--text-lg);
  font-weight: 600;
  color: var(--text-primary);
  line-height: 1.2;
}
.stat-label {
  font-size: var(--text-xs);
  color: var(--text-tertiary);
  margin-top: 2px;
}

/* 图表卡片 */
.chart-card {
  display: flex;
  flex-direction: column;
}
.chart-title {
  font-size: var(--text-sm);
  font-weight: 500;
  color: var(--text-secondary);
  margin-bottom: var(--space-2);
  display: flex;
  align-items: center;
  gap: var(--space-2);
}
.delta-info {
  display: inline-flex;
  align-items: center;
  gap: 2px;
  font-size: var(--text-xs);
  font-weight: 500;
}
.chart {
  width: 100%;
  height: 280px;
}

/* 双列布局 */
.charts-row.two-col {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(320px, 1fr));
  gap: var(--space-3);
}

/* 预测卡片 */
.prediction-card {
  border-left: 3px solid var(--color-info, var(--text-tertiary));
}
.prediction-card.on_track {
  border-left-color: var(--color-success, #10b981);
}
.prediction-card.at_risk {
  border-left-color: var(--color-warning, #f59e0b);
}
.prediction-card.off_track {
  border-left-color: var(--color-danger, #ef4444);
}
.pred-row {
  display: flex;
  align-items: flex-start;
  gap: var(--space-3);
}
.pred-icon {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 40px;
  height: 40px;
  border-radius: var(--radius-md);
  background: var(--bg-tertiary);
  flex-shrink: 0;
}
.pred-info {
  flex: 1;
}
.pred-title {
  font-size: var(--text-base);
  font-weight: 600;
  color: var(--text-primary);
}
.pred-desc {
  font-size: var(--text-sm);
  color: var(--text-secondary);
  margin-top: 4px;
}
.pred-stats {
  display: flex;
  gap: var(--space-4);
  margin-top: var(--space-2);
  font-size: var(--text-xs);
  color: var(--text-tertiary);
}

/* 响应式 */
@media (max-width: 768px) {
  .analytics-view {
    padding: var(--space-4);
  }
  .charts-row.two-col {
    grid-template-columns: 1fr;
  }
  .chart {
    height: 240px;
  }
  .pred-stats {
    flex-direction: column;
    gap: var(--space-1);
  }
}
</style>
