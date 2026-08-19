<script setup lang="ts">
/**
 * 专注（番茄钟）页
 *
 * - 支持倒计时 / 正计时两种计时方式
 * - 倒计时：学习 + 休息两段，默认 25 分钟学习 / 5 分钟休息
 * - 学习结束后可手动或自动进入休息；手动模式下先转正计时，等待用户点击开始休息
 * - 长休循环：每 N 个番茄后进入长休息（可配置开关）
 * - 计时状态由全局 store 管理，离开本页 / 切换页面不中断计时
 * - 可关联今日具体任务：学习番茄完成时把专注分钟累加到任务，勾选完成后同步任务状态
 * - 圆环进度条：中间显示剩余时间，圆环随时间逆时针减少
 * - 今日番茄数 / 历史记录由后端持久化
 */
import { computed, onMounted } from "vue";
import { useTodayStore } from "@/stores/today";
import { useFocusStore } from "@/stores/focus";
import Button from "@/components/ui/Button.vue";
import Card from "@/components/ui/Card.vue";
import Badge from "@/components/ui/Badge.vue";
import Select from "@/components/ui/Select.vue";
import {
  Play,
  Pause,
  RotateCcw,
  Coffee,
  BookOpen,
  Check,
  SkipForward,
  ListChecks,
  Timer,
  Hourglass,
  History,
} from "lucide-vue-next";
import type { PlanTask } from "@/types";

const todayStore = useTodayStore();
const focus = useFocusStore();

// ── 今日任务关联 ──
const todayTasks = computed<PlanTask[]>(() =>
  todayStore.allTasks.filter((t) => t.status !== "done")
);
const linkedTask = computed(() =>
  todayTasks.value.find((t) => t.id === focus.linkedTaskId) ?? null
);

/** 关联任务勾选完成：同步任务状态为 done */
async function completeLinkedTask() {
  if (!focus.linkedTaskId) return;
  try {
    await todayStore.updateTaskStatus(focus.linkedTaskId, "done");
  } catch (e) {
    console.warn("同步任务完成状态失败", e);
  }
}

// ── 专注记录展示 ──
const todayRecordList = computed(() =>
  [...focus.todaySessions].sort((a, b) => (a.ended_at < b.ended_at ? 1 : -1))
);

function sessionLabel(type: string): string {
  if (type === "focus") return "学习";
  if (type === "stopwatch") return "正计时";
  return type === "long_break" ? "长休息" : "短休息";
}
function sessionStatusLabel(status: string): string {
  return status === "completed" ? "完成" : "打断";
}
function formatTime(iso: string): string {
  const d = new Date(iso);
  return `${String(d.getHours()).padStart(2, "0")}:${String(d.getMinutes()).padStart(2, "0")}`;
}

// ── 生命周期 ──
onMounted(async () => {
  await focus.loadStats();
  await todayStore.loadToday();
  // M9：校验关联任务是否仍存在于今日未完成任务中，失效（已完成/跨天）则清空，
  // 避免后续番茄继续累加到已失效任务
  if (focus.linkedTaskId && !todayTasks.value.some((t) => t.id === focus.linkedTaskId)) {
    focus.linkedTaskId = null;
  }
});
</script>

<template>
  <div class="focus-page">
    <div class="focus-head">
      <h1 class="focus-title">专注</h1>
      <p class="focus-sub">番茄工作法 · 关联今日任务 · 让每段专注都有归属</p>
    </div>

    <!-- 主卡片：圆环 + 控制 -->
    <Card class="focus-main" surface="accent">
      <div class="focus-timer-wrap">
        <!-- 圆环进度（SVG，逆时针减少） -->
        <svg class="focus-ring" viewBox="0 0 260 260" role="img" aria-label="专注计时">
          <circle class="ring-track" cx="130" cy="130" r="116" />
          <circle
            class="ring-progress"
            cx="130"
            cy="130"
            r="116"
            :stroke-dasharray="2 * Math.PI * 116"
            :stroke-dashoffset="(2 * Math.PI * 116) * (1 - focus.progress)"
            transform="rotate(-90 130 130)"
          />
        </svg>
        <div class="ring-center">
          <Badge :variant="focus.phase === 'focus' ? 'info' : 'success'" class="phase-badge">
            <component :is="focus.phase === 'focus' ? BookOpen : Coffee" :size="13" />
            {{ focus.phaseLabel }}
          </Badge>
          <span class="ring-time">{{ focus.displayText }}</span>
          <span class="ring-status">
            {{ focus.sub === 'idle' ? "准备开始" : focus.isPaused ? "已暂停" : focus.isRunning ? "进行中" : "" }}
          </span>
        </div>
      </div>

      <!-- 控制区 -->
      <div class="controls">
        <!-- 未开始（含模式切换） -->
        <template v-if="focus.sub === 'idle'">
          <Button
            v-if="focus.mode === 'countdown'"
            variant="primary"
            size="lg"
            @click="focus.startFocus"
          >
            <Play :size="18" /> 开始专注
          </Button>
          <Button
            v-else
            variant="primary"
            size="lg"
            @click="focus.startStopwatch"
          >
            <Play :size="18" /> 开始计时
          </Button>
          <div class="mode-switch">
            <Button :variant="focus.mode === 'countdown' ? 'primary' : 'ghost'" size="sm" @click="focus.mode = 'countdown'">
              <Timer :size="15" /> 倒计时
            </Button>
            <Button :variant="focus.mode === 'stopwatch' ? 'primary' : 'ghost'" size="sm" @click="focus.mode = 'stopwatch'">
              <Hourglass :size="15" /> 正计时
            </Button>
          </div>
        </template>

        <!-- 运行/暂停中 -->
        <template v-if="focus.isRunning || focus.isPaused">
          <Button :variant="focus.isRunning ? 'secondary' : 'primary'" size="lg" @click="focus.togglePause">
            <Pause v-if="focus.isRunning" :size="18" /> <Play v-else :size="18" />
            {{ focus.isRunning ? "暂停" : "继续" }}
          </Button>
          <Button variant="ghost" size="lg" @click="focus.resetAll">
            <RotateCcw :size="18" /> 重置
          </Button>
          <!-- 正计时：随时可结束并计入关联任务时间 -->
          <Button
            v-if="focus.mode === 'stopwatch'"
            variant="primary"
            size="lg"
            @click="focus.finishStopwatch"
          >
            <Check :size="18" /> 结束
          </Button>
          <!-- 手动模式：学习结束转正计时后（非倒计时运行中）可点击「开始休息」（M4） -->
          <Button
            v-if="focus.phase === 'focus' && !focus.config.autoBreak && focus.mode === 'stopwatch' && focus.sub === 'running'"
            variant="secondary"
            size="lg"
            @click="focus.skipToBreak"
          >
            <SkipForward :size="18" /> 开始休息
          </Button>
        </template>

        <!-- 休息结束：继续/结束 -->
        <template v-if="focus.sub === 'breakEnded'">
          <Button variant="primary" size="lg" @click="focus.continueRound">
            <Play :size="18" /> 继续新的一轮
          </Button>
          <Button variant="ghost" size="lg" @click="focus.endRound">
            <Check :size="18" /> 结束番茄钟
          </Button>
        </template>
      </div>

      <!-- 今日统计 + 关联任务 -->
      <div class="focus-footer">
        <div class="stats">
          <span class="stat-item">
            <span class="stat-num">{{ focus.todayStats.pomodoros }}</span>
            <span class="stat-label">今日番茄</span>
          </span>
          <span class="stat-item">
            <span class="stat-num">{{ focus.todayStats.focus_minutes }}</span>
            <span class="stat-label">今日专注(分)</span>
          </span>
        </div>

        <div class="task-link">
          <div class="task-link-head">
            <ListChecks :size="15" />
            <span>关联今日任务</span>
          </div>
          <Select
            v-model="focus.linkedTaskId"
            :disabled="todayTasks.length === 0"
            :max-width="'240px'"
          >
            <option :value="null">不关联任务</option>
            <option v-for="t in todayTasks" :key="t.id" :value="t.id">
              {{ t.title }}
            </option>
          </Select>
          <Button
            v-if="linkedTask"
            variant="secondary"
            size="sm"
            @click="completeLinkedTask"
          >
            <Check :size="15" /> 完成该任务
          </Button>
        </div>
      </div>
    </Card>

    <!-- 配置卡 -->
    <Card class="focus-config">
      <div class="config-grid">
        <label class="config-field">
          <span class="config-label">学习时长（分钟）</span>
          <input
            v-model.number="focus.config.focusMinutes"
            type="number"
            min="1"
            max="180"
            class="config-input"
            :disabled="focus.isRunning || focus.isPaused"
          />
        </label>
        <label class="config-field">
          <span class="config-label">休息时长（分钟）</span>
          <input
            v-model.number="focus.config.breakMinutes"
            type="number"
            min="1"
            max="60"
            class="config-input"
            :disabled="focus.isRunning || focus.isPaused"
          />
        </label>
        <div class="config-field config-toggle">
          <span class="config-label">学习结束后自动进入休息</span>
          <button
            class="toggle-switch"
            :class="{ on: focus.config.autoBreak }"
            role="switch"
            :aria-checked="focus.config.autoBreak"
            :disabled="focus.isRunning || focus.isPaused"
            @click="focus.config.autoBreak = !focus.config.autoBreak"
          >
            <span class="toggle-thumb" />
          </button>
        </div>
        <div class="config-field config-toggle">
          <span class="config-label">长休息（每 N 个番茄后）</span>
          <button
            class="toggle-switch"
            :class="{ on: focus.config.longBreakEnabled }"
            role="switch"
            :aria-checked="focus.config.longBreakEnabled"
            :disabled="focus.isRunning || focus.isPaused"
            @click="focus.config.longBreakEnabled = !focus.config.longBreakEnabled"
          >
            <span class="toggle-thumb" />
          </button>
        </div>
        <label class="config-field">
          <span class="config-label">长休时长（分钟）</span>
          <input
            v-model.number="focus.config.longBreakMinutes"
            type="number"
            min="1"
            max="60"
            class="config-input"
            :disabled="!focus.config.longBreakEnabled || focus.isRunning || focus.isPaused"
          />
        </label>
        <label class="config-field">
          <span class="config-label">长休间隔（每 N 个番茄）</span>
          <input
            v-model.number="focus.config.longBreakInterval"
            type="number"
            min="2"
            max="10"
            class="config-input"
            :disabled="!focus.config.longBreakEnabled || focus.isRunning || focus.isPaused"
          />
        </label>
      </div>
      <p class="config-hint">
        关闭自动休息时，学习倒计时结束后将进入正计时，直到你点击「开始休息」。开启长休息后，每完成 N 个番茄会自动进入长休息。
      </p>
    </Card>

    <!-- 专注记录 -->
    <Card class="focus-history">
      <div class="history-head">
        <History :size="16" />
        <span>专注记录</span>
      </div>
      <div class="week-stats">
        <span class="week-stat">
          <b>{{ focus.weekStats.pomodoros }}</b> 近 7 天番茄
        </span>
        <span class="week-stat">
          <b>{{ focus.weekStats.focus_minutes }}</b> 近 7 天专注分钟
        </span>
      </div>
      <div v-if="todayRecordList.length === 0" class="history-empty">
        今天还没有专注记录，完成第一个番茄后这里会展示。
      </div>
      <ul v-else class="session-list">
        <li v-for="s in todayRecordList" :key="s.id" class="session-item">
          <span class="session-type" :class="(s.type === 'focus' || s.type === 'stopwatch') ? 'is-focus' : 'is-break'">
            {{ sessionLabel(s.type) }}
          </span>
          <span class="session-time">{{ formatTime(s.started_at) }} - {{ formatTime(s.ended_at) }}</span>
          <span class="session-duration">{{ s.duration_minutes }} 分钟</span>
          <span class="session-status" :class="{ interrupted: s.status !== 'completed' }">
            {{ sessionStatusLabel(s.status) }}
          </span>
        </li>
      </ul>
    </Card>
  </div>
</template>

<style scoped>
.focus-page {
  max-width: 720px;
  margin: 0 auto;
  padding: var(--page-padding);
  display: flex;
  flex-direction: column;
  gap: var(--space-5);
}

.focus-head {
  text-align: center;
}
.focus-title {
  font-size: var(--text-2xl);
  font-weight: var(--font-display);
  color: var(--text-primary);
  margin: 0;
  letter-spacing: -0.02em;
}
.focus-sub {
  color: var(--text-tertiary);
  font-size: var(--text-sm);
  margin: var(--space-2) 0 0;
}

.focus-main {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: var(--space-6);
}

.focus-timer-wrap {
  position: relative;
  width: 260px;
  height: 260px;
}
.focus-ring {
  width: 100%;
  height: 100%;
  /* 圆环逆时针减少：起点在顶部（rotate(-90)），dashoffset 从满到 0，剩余弧逆时针收缩 */
}
.ring-track {
  fill: none;
  stroke: var(--accent-subtle);
  stroke-width: 14;
}
.ring-progress {
  fill: none;
  stroke: var(--accent);
  stroke-width: 14;
  stroke-linecap: round;
  transition: stroke-dashoffset 0.4s linear;
}
.ring-center {
  position: absolute;
  inset: 0;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: var(--space-2);
}
.phase-badge {
  display: inline-flex;
  align-items: center;
  gap: 4px;
}
.ring-time {
  font-size: 52px;
  font-weight: var(--font-bold);
  font-variant-numeric: tabular-nums;
  color: var(--text-primary);
  letter-spacing: -0.03em;
  line-height: 1;
}
.ring-status {
  font-size: var(--text-sm);
  color: var(--text-tertiary);
}

.controls {
  display: flex;
  align-items: center;
  justify-content: center;
  gap: var(--space-3);
  flex-wrap: wrap;
}
.mode-switch {
  display: flex;
  gap: var(--space-2);
}

.focus-footer {
  width: 100%;
  border-top: 1px solid var(--divider-color);
  padding-top: var(--space-4);
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--space-4);
  flex-wrap: wrap;
}
.stats {
  display: flex;
  gap: var(--space-6);
}
.stat-item {
  display: flex;
  flex-direction: column;
  align-items: center;
}
.stat-num {
  font-size: var(--text-xl);
  font-weight: var(--font-bold);
  color: var(--accent);
}
.stat-label {
  font-size: var(--text-xs);
  color: var(--text-tertiary);
}

.task-link {
  display: flex;
  align-items: center;
  gap: var(--space-3);
}
.task-link-head {
  display: flex;
  align-items: center;
  gap: 4px;
  font-size: var(--text-sm);
  color: var(--text-secondary);
  font-weight: var(--font-label);
  /* 防止 flex 压缩导致标题文字逐个折行 */
  flex-shrink: 0;
  white-space: nowrap;
}
.focus-config {
  padding: var(--space-5);
}
.config-grid {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(180px, 1fr));
  gap: var(--space-4);
}
.config-field {
  display: flex;
  flex-direction: column;
  gap: var(--space-2);
}
.config-label {
  font-size: var(--text-sm);
  color: var(--text-secondary);
  font-weight: var(--font-label);
}
.config-input {
  font-family: inherit;
  font-size: var(--text-base);
  padding: var(--space-2) var(--space-3);
  border: 1px solid var(--border-color-strong);
  border-radius: var(--radius-sm);
  background: var(--bg-secondary);
  color: var(--text-primary);
  width: 100%;
}
.config-toggle {
  flex-direction: row;
  align-items: center;
  gap: var(--space-3);
}
/* 滑动开关 */
.toggle-switch {
  position: relative;
  width: 40px;
  height: 22px;
  border-radius: 11px;
  background: var(--bg-tertiary);
  border: 1px solid var(--border-color-strong);
  cursor: pointer;
  padding: 0;
  flex-shrink: 0;
  transition: background var(--transition-fast), border-color var(--transition-fast);
}
.toggle-switch.on {
  background: var(--accent);
  border-color: var(--accent);
}
.toggle-switch:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}
.toggle-thumb {
  position: absolute;
  top: 50%;
  left: 2px;
  width: 16px;
  height: 16px;
  border-radius: 50%;
  background: #fff;
  transform: translateY(-50%);
  transition: transform var(--transition-fast);
  box-shadow: 0 1px 3px rgba(0, 0, 0, 0.2);
}
.toggle-switch.on .toggle-thumb {
  transform: translateX(18px) translateY(-50%);
}
.config-hint {
  font-size: var(--text-xs);
  color: var(--text-tertiary);
  margin: var(--space-3) 0 0;
}

/* ── 专注记录 ── */
.focus-history {
  padding: var(--space-5);
}
.history-head {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  font-size: var(--text-sm);
  font-weight: var(--font-label);
  color: var(--text-secondary);
  margin-bottom: var(--space-3);
}
.week-stats {
  display: flex;
  gap: var(--space-6);
  margin-bottom: var(--space-3);
}
.week-stat {
  font-size: var(--text-sm);
  color: var(--text-tertiary);
}
.week-stat b {
  color: var(--accent);
  font-size: var(--text-lg);
  margin-right: 2px;
}
.history-empty {
  padding: var(--space-4);
  text-align: center;
  color: var(--text-tertiary);
  font-size: var(--text-sm);
  background: var(--bg-tertiary);
  border-radius: var(--radius-md);
}
.session-list {
  list-style: none;
  margin: 0;
  padding: 0;
  display: flex;
  flex-direction: column;
  gap: var(--space-2);
  max-height: 260px;
  overflow-y: auto;
}
.session-item {
  display: flex;
  align-items: center;
  gap: var(--space-3);
  padding: var(--space-2) var(--space-3);
  background: var(--bg-tertiary);
  border-radius: var(--radius-md);
  font-size: var(--text-sm);
}
.session-type {
  flex-shrink: 0;
  font-size: var(--text-xs);
  padding: 2px 8px;
  border-radius: var(--radius-full);
  font-weight: var(--font-medium);
}
.session-type.is-focus {
  color: var(--accent);
  background: var(--accent-subtle);
}
.session-type.is-break {
  color: var(--color-success);
  background: var(--color-success-subtle, var(--bg-overlay));
}
.session-time {
  flex: 1;
  color: var(--text-secondary);
  font-variant-numeric: tabular-nums;
}
.session-duration {
  color: var(--text-secondary);
}
.session-status {
  flex-shrink: 0;
  font-size: var(--text-xs);
  color: var(--color-success);
}
.session-status.interrupted {
  color: var(--color-warning);
}
</style>
