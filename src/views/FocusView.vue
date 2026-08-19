<script setup lang="ts">
/**
 * 专注（番茄钟）页
 *
 * - 支持倒计时 / 正计时两种计时方式
 * - 倒计时：学习 + 休息两段，默认 25 分钟学习 / 5 分钟休息
 * - 学习结束后可手动或自动进入休息；手动模式下先转正计时，等待用户点击开始休息
 * - 休息结束后可选择继续新的一轮或结束
 * - 可关联今日具体任务：学习番茄完成时把专注分钟累加到任务，勾选完成后同步任务状态
 * - 圆环进度条：中间显示剩余时间，圆环随时间逆时针减少
 */
import { computed, onMounted, onUnmounted, ref, watch } from "vue";
import { useTodayStore } from "@/stores/today";
import * as api from "@/api";
import { todayString } from "@/utils/date";
import { getCurrentWindow } from "@tauri-apps/api/window";
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
} from "lucide-vue-next";
import type { PlanTask } from "@/types";

const todayStore = useTodayStore();

// ── 配置（localStorage 持久化）──
interface FocusConfig {
  focusMinutes: number; // 学习时长（分钟）
  breakMinutes: number; // 休息时长（分钟）
  autoBreak: boolean; // 学习结束是否自动进入休息
}
const CONFIG_KEY = "focus.config.v1";
const defaultConfig: FocusConfig = { focusMinutes: 25, breakMinutes: 5, autoBreak: false };
function loadConfig(): FocusConfig {
  try {
    const raw = localStorage.getItem(CONFIG_KEY);
    if (raw) return { ...defaultConfig, ...JSON.parse(raw) };
  } catch { /* 忽略 */ }
  return { ...defaultConfig };
}
const config = ref<FocusConfig>(loadConfig());
watch(config, (v) => {
  try { localStorage.setItem(CONFIG_KEY, JSON.stringify(v)); } catch { /* 忽略 */ }
}, { deep: true });

// ── 状态 ──
type Phase = "focus" | "break"; // 学习 / 休息
type TimerMode = "countdown" | "stopwatch"; // 倒计时 / 正计时
// 倒计时的子状态：
// idle(未开始) / running(运行中) / paused(已暂停) /
// focusEnded(学习结束，等待进入休息：自动则直接转 break；手动则转 stopwatch) /
// breakEnded(休息结束，等待选择继续或结束)
type CountdownSub = "idle" | "running" | "paused" | "focusEnded" | "breakEnded";

const mode = ref<TimerMode>("countdown");
const phase = ref<Phase>("focus");
const sub = ref<CountdownSub>("idle");

/** 倒计时剩余秒数（学习/休息共用，phase 决定目标时长） */
const remainingSec = ref(config.value.focusMinutes * 60);
/** 本轮倒计时总秒数（用于圆环比例） */
const totalSec = ref(config.value.focusMinutes * 60);
/** 正计时累计秒数 */
const stopwatchSec = ref(0);

let interval: ReturnType<typeof setInterval> | undefined;

/** 每秒推进一次：倒计时剩余减 1，正计时累计加 1（setInterval 1000ms） */
function tick() {
  if (mode.value === "countdown") {
    if (sub.value === "running") {
      if (remainingSec.value > 0) {
        remainingSec.value -= 1;
      }
      if (remainingSec.value <= 0) {
        remainingSec.value = 0;
        onCountdownEnd();
      }
    }
  } else {
    if (sub.value === "running") {
      stopwatchSec.value += 1;
    }
  }
}

/** 倒计时结束：学习→休息 / 休息→结束 */
function onCountdownEnd() {
  if (phase.value === "focus") {
    // 学习结束：记录一个完整番茄（关联任务累加专注分钟）
    void recordFocusCompletion(Math.round(totalSec.value / 60));
    // 结束提醒：提示音 + 窗口不在前台时弹系统通知
    void notifyPhaseEnd("学习结束", "专注已完成，可以开始休息了。");
    // 自动休息则直接进入休息倒计时；手动则停在 focusEnded（下方转正计时）
    if (config.value.autoBreak) {
      startBreak();
    } else {
      sub.value = "focusEnded";
      // 手动模式：默认进入正计时，直到用户点击「开始休息」
      mode.value = "stopwatch";
      stopwatchSec.value = 0;
      sub.value = "running";
    }
  } else {
    // 休息结束
    void notifyPhaseEnd("休息结束", "休息时间到，可以开始新一轮学习了。");
    sub.value = "breakEnded";
    pauseInterval();
  }
}

function startInterval() {
  if (!interval) {
    interval = setInterval(tick, 1000);
  }
}
function pauseInterval() {
  if (interval) { clearInterval(interval); interval = undefined; }
}

/** 开始正计时 */
function startStopwatch() {
  mode.value = "stopwatch";
  phase.value = "focus";
  stopwatchSec.value = 0;
  sub.value = "running";
  startInterval();
}

/** 开始倒计时学习 */
function startFocus() {
  mode.value = "countdown";
  phase.value = "focus";
  totalSec.value = config.value.focusMinutes * 60;
  remainingSec.value = totalSec.value;
  sub.value = "running";
  startInterval();
}

/** 开始休息倒计时 */
function startBreak() {
  mode.value = "countdown";
  phase.value = "break";
  totalSec.value = config.value.breakMinutes * 60;
  remainingSec.value = totalSec.value;
  sub.value = "running";
  startInterval();
}

function togglePause() {
  if (mode.value === "countdown" && sub.value === "running") {
    sub.value = "paused";
    pauseInterval();
  } else if (mode.value === "countdown" && sub.value === "paused") {
    sub.value = "running";
    startInterval();
  } else if (mode.value === "stopwatch" && sub.value === "running") {
    sub.value = "paused";
    pauseInterval();
  } else if (mode.value === "stopwatch" && sub.value === "paused") {
    sub.value = "running";
    startInterval();
  }
}

function skipToBreak() {
  // 手动模式学习结束后（或学习中），点击「开始休息」
  if (phase.value === "focus") {
    pauseInterval();
    startBreak();
  }
}

/** 休息结束：继续新的一轮学习 */
function continueRound() {
  startFocus();
}

/** 休息结束：结束番茄钟，回到空闲 */
function endRound() {
  mode.value = "countdown";
  phase.value = "focus";
  sub.value = "idle";
  remainingSec.value = config.value.focusMinutes * 60;
  totalSec.value = config.value.focusMinutes * 60;
  stopwatchSec.value = 0;
  pauseInterval();
}

function resetAll() {
  pauseInterval();
  mode.value = "countdown";
  phase.value = "focus";
  sub.value = "idle";
  remainingSec.value = config.value.focusMinutes * 60;
  totalSec.value = config.value.focusMinutes * 60;
  stopwatchSec.value = 0;
}

// ── 结束提醒：提示音 + 窗口不在前台时弹系统通知 ──
let audioCtx: AudioContext | null = null;

/** 播放两声音阶提示音（Web Audio 合成，无需音频资源） */
function playChime() {
  try {
    const ctx = audioCtx ?? new AudioContext();
    audioCtx = ctx;
    // 浏览器自动播放策略下，若上下文被挂起则尝试恢复
    if (ctx.state === "suspended") {
      void ctx.resume();
    }
    const now = ctx.currentTime;
    const notes = [880, 1174.66]; // A5 → D6，先低后高
    for (let i = 0; i < notes.length; i++) {
      const osc = ctx.createOscillator();
      const gain = ctx.createGain();
      osc.type = "sine";
      osc.frequency.value = notes[i];
      const start = now + i * 0.18;
      gain.gain.setValueAtTime(0.0001, start);
      gain.gain.exponentialRampToValueAtTime(0.25, start + 0.02);
      gain.gain.exponentialRampToValueAtTime(0.0001, start + 0.5);
      osc.connect(gain).connect(ctx.destination);
      osc.start(start);
      osc.stop(start + 0.55);
    }
  } catch (e) {
    console.warn("[Focus] 播放提示音失败:", e);
  }
}

/** 阶段结束提醒：始终播放提示音；窗口不在前台时才弹系统通知 */
async function notifyPhaseEnd(title: string, body: string) {
  playChime();
  if (!api.isTauri()) return;
  try {
    const win = getCurrentWindow();
    // 窗口在前台时用户已能看到界面状态变化，无需弹系统通知
    if (await win.isFocused()) return;
    const { sendNotification, isPermissionGranted, requestPermission } = await import(
      "@tauri-apps/plugin-notification"
    );
    let granted = await isPermissionGranted();
    if (!granted) {
      const permission = await requestPermission();
      granted = permission === "granted";
    }
    if (granted) {
      sendNotification({ title, body });
    }
  } catch (e) {
    console.warn("[Focus] 发送结束通知失败:", e);
  }
}

// ── 展示 ──
const displaySec = computed(() =>
  mode.value === "countdown" ? remainingSec.value : stopwatchSec.value
);
const displayText = computed(() => {
  const total = displaySec.value;
  const m = Math.floor(total / 60);
  const s = total % 60;
  return `${String(m).padStart(2, "0")}:${String(s).padStart(2, "0")}`;
});

/** 圆环进度：倒计时为剩余比例，正计时为已过比例 */
const progress = computed(() => {
  if (mode.value === "countdown") {
    return totalSec.value > 0 ? remainingSec.value / totalSec.value : 0;
  }
  // 正计时圆环：按 25 分钟为一段展示
  const cap = config.value.focusMinutes * 60;
  return cap > 0 ? (stopwatchSec.value % cap) / cap : 0;
});

const phaseLabel = computed(() =>
  mode.value === "stopwatch" && phase.value === "focus"
    ? "正计时（专注）"
    : phase.value === "focus" ? "学习" : "休息"
);

const isRunning = computed(() => sub.value === "running");
const isPaused = computed(() => sub.value === "paused");

// ── 今日任务关联 ──
const todayTasks = computed<PlanTask[]>(() =>
  todayStore.allTasks.filter((t) => t.status !== "done")
);
const linkedTaskId = ref<string | null>(null);
const linkedTask = computed(() =>
  todayTasks.value.find((t) => t.id === linkedTaskId.value) ?? null
);
/** 今日已完成的番茄数（学习会话计 1 个） */
const todayPomodoros = ref(0);

const STATS_KEY = "focus.stats.v1";
function loadTodayStats() {
  try {
    const raw = localStorage.getItem(STATS_KEY);
    if (raw) {
      const d = JSON.parse(raw);
      if (d.date === todayString()) {
        todayPomodoros.value = d.count ?? 0;
        return;
      }
    }
  } catch { /* 忽略 */ }
  todayPomodoros.value = 0;
}
function saveTodayStats() {
  try {
    localStorage.setItem(STATS_KEY, JSON.stringify({ date: todayString(), count: todayPomodoros.value }));
  } catch { /* 忽略 */ }
}

/** 记录一个完成的专注会话：累加番茄数 + 关联任务分钟数 */
async function recordFocusCompletion(minutes: number) {
  todayPomodoros.value += 1;
  saveTodayStats();
  if (linkedTaskId.value) {
    try {
      await api.focusAddMinutes(linkedTaskId.value, minutes);
    } catch (e) {
      console.warn("关联任务累加专注分钟失败", e);
    }
  }
}

/** 关联任务勾选完成：同步任务状态为 done */
async function completeLinkedTask() {
  if (!linkedTaskId.value) return;
  try {
    await todayStore.updateTaskStatus(linkedTaskId.value, "done");
  } catch (e) {
    console.warn("同步任务完成状态失败", e);
  }
}

// ── 生命周期 ──
onMounted(async () => {
  loadTodayStats();
  await todayStore.loadToday();
});

onUnmounted(() => {
  pauseInterval();
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
            :stroke-dashoffset="(2 * Math.PI * 116) * (1 - progress)"
            transform="rotate(-90 130 130)"
          />
        </svg>
        <div class="ring-center">
          <Badge :variant="phase === 'focus' ? 'info' : 'success'" class="phase-badge">
            <component :is="phase === 'focus' ? BookOpen : Coffee" :size="13" />
            {{ phaseLabel }}
          </Badge>
          <span class="ring-time">{{ displayText }}</span>
          <span class="ring-status">
            {{ sub === 'idle' ? "准备开始" : isPaused ? "已暂停" : isRunning ? "进行中" : "" }}
          </span>
        </div>
      </div>

      <!-- 控制区 -->
      <div class="controls">
        <!-- 未开始（含模式切换） -->
        <template v-if="sub === 'idle'">
          <Button
            v-if="mode === 'countdown'"
            variant="primary"
            size="lg"
            @click="startFocus"
          >
            <Play :size="18" /> 开始专注
          </Button>
          <Button
            v-else
            variant="primary"
            size="lg"
            @click="startStopwatch"
          >
            <Play :size="18" /> 开始计时
          </Button>
          <div class="mode-switch">
            <Button :variant="mode === 'countdown' ? 'primary' : 'ghost'" size="sm" @click="mode = 'countdown'">
              <Timer :size="15" /> 倒计时
            </Button>
            <Button :variant="mode === 'stopwatch' ? 'primary' : 'ghost'" size="sm" @click="mode = 'stopwatch'">
              <Hourglass :size="15" /> 正计时
            </Button>
          </div>
        </template>

        <!-- 运行/暂停中 -->
        <template v-if="isRunning || isPaused">
          <Button :variant="isRunning ? 'secondary' : 'primary'" size="lg" @click="togglePause">
            <Pause v-if="isRunning" :size="18" /> <Play v-else :size="18" />
            {{ isRunning ? "暂停" : "继续" }}
          </Button>
          <Button variant="ghost" size="lg" @click="resetAll">
            <RotateCcw :size="18" /> 重置
          </Button>
          <!-- 手动模式：学习中（倒计时或学习结束转正计时）可点击「开始休息」 -->
          <Button
            v-if="phase === 'focus' && !config.autoBreak"
            variant="secondary"
            size="lg"
            @click="skipToBreak"
          >
            <SkipForward :size="18" /> 开始休息
          </Button>
        </template>

        <!-- 休息结束：继续/结束 -->
        <template v-if="sub === 'breakEnded'">
          <Button variant="primary" size="lg" @click="continueRound">
            <Play :size="18" /> 继续新的一轮
          </Button>
          <Button variant="ghost" size="lg" @click="endRound">
            <Check :size="18" /> 结束番茄钟
          </Button>
        </template>
      </div>

      <!-- 今日统计 + 关联任务 -->
      <div class="focus-footer">
        <div class="stats">
          <span class="stat-item">
            <span class="stat-num">{{ todayPomodoros }}</span>
            <span class="stat-label">今日番茄</span>
          </span>
        </div>

        <div class="task-link">
          <div class="task-link-head">
            <ListChecks :size="15" />
            <span>关联今日任务</span>
          </div>
          <Select
            v-model="linkedTaskId"
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
            v-model.number="config.focusMinutes"
            type="number"
            min="1"
            max="180"
            class="config-input"
            :disabled="isRunning || isPaused"
          />
        </label>
        <label class="config-field">
          <span class="config-label">休息时长（分钟）</span>
          <input
            v-model.number="config.breakMinutes"
            type="number"
            min="1"
            max="60"
            class="config-input"
            :disabled="isRunning || isPaused"
          />
        </label>
        <div class="config-field config-toggle">
          <span class="config-label">学习结束后自动进入休息</span>
          <button
            class="toggle-switch"
            :class="{ on: config.autoBreak }"
            role="switch"
            :aria-checked="config.autoBreak"
            :disabled="isRunning || isPaused"
            @click="config.autoBreak = !config.autoBreak"
          >
            <span class="toggle-thumb" />
          </button>
        </div>
      </div>
      <p class="config-hint">
        关闭自动休息时，学习倒计时结束后将进入正计时，直到你点击「开始休息」。
      </p>
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
/* 滑动开关（与设置页一致） */
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
</style>
