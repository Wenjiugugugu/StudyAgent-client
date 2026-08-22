import { defineStore } from "pinia";
import { ref, computed, watch } from "vue";
import * as api from "@/api";
import { todayString, prevDateString, formatLocalIso } from "@/utils/date";
import { getCurrentWindow } from "@tauri-apps/api/window";

export type Phase = "focus" | "break" | "longBreak";
export type TimerMode = "countdown" | "stopwatch";
// idle(未开始) / running(运行中) / paused(已暂停) /
// focusEnded(学习结束，等待进入休息：自动则直接转 break；手动则转 stopwatch) /
// breakEnded(休息结束，等待选择继续或结束)
export type CountdownSub = "idle" | "running" | "paused" | "focusEnded" | "breakEnded";

export interface FocusConfig {
  focusMinutes: number;
  breakMinutes: number;
  autoBreak: boolean;
  longBreakEnabled: boolean;
  longBreakMinutes: number;
  longBreakInterval: number;
}

const CONFIG_KEY = "focus.config.v1";
const LINKED_TASK_KEY = "focus.linkedTask.v1";
/** M6：计时运行状态持久化 key（应用重启后恢复进行中会话） */
const STATE_KEY = "focus.state.v1";
/** L7：ticker 间隔（毫秒） */
const TICK_INTERVAL_MS = 250;
/** L7：结束提示音参数（音符/包络/间隔） */
const CHIME_NOTES = [880, 1174.66]; // A5 → D6，先低后高
const CHIME_NOTE_GAP_S = 0.18;
const CHIME_GAIN_START = 0.0001;
const CHIME_GAIN_PEAK = 0.25;
const CHIME_ATTACK_S = 0.02;
const CHIME_DECAY_S = 0.5;
const CHIME_DURATION_S = 0.55;
const defaultConfig: FocusConfig = {
  focusMinutes: 25,
  breakMinutes: 5,
  autoBreak: false,
  longBreakEnabled: false,
  longBreakMinutes: 15,
  longBreakInterval: 4,
};

/** 校验并夹取配置值，防止清空输入导致的 ""/NaN 进入计时逻辑（M7） */
function clampConfig(c: FocusConfig): FocusConfig {
  const num = (v: unknown, def: number, min: number, max: number) =>
    typeof v === "number" && Number.isFinite(v) && v >= min && v <= max ? v : def;
  return {
    focusMinutes: num(c.focusMinutes, 25, 1, 180),
    breakMinutes: num(c.breakMinutes, 5, 1, 60),
    autoBreak: !!c.autoBreak,
    longBreakEnabled: !!c.longBreakEnabled,
    longBreakMinutes: num(c.longBreakMinutes, 15, 1, 120),
    longBreakInterval: num(c.longBreakInterval, 4, 1, 12),
  };
}

function loadConfig(): FocusConfig {
  try {
    const raw = localStorage.getItem(CONFIG_KEY);
    if (raw) return clampConfig({ ...defaultConfig, ...JSON.parse(raw) });
  } catch {
    /* 忽略 */
  }
  return { ...defaultConfig };
}

/**
 * 番茄钟全局状态
 *
 * 计时状态上收到 Pinia store（应用生命周期内单例），离开专注页 / 切换路由不中断计时；
 * 剩余时间基于「到期时间戳」计算，窗口最小化/托盘导致 WebView 定时器被节流时也不会漂移。
 */
export const useFocusStore = defineStore("focus", () => {
  const config = ref<FocusConfig>(loadConfig());
  const linkedTaskId = ref<string | null>(localStorage.getItem(LINKED_TASK_KEY) || null);

  // ── 计时状态 ──
  const mode = ref<TimerMode>("countdown");
  const phase = ref<Phase>("focus");
  const sub = ref<CountdownSub>("idle");
  /** 本轮倒计时总秒数（用于圆环比例） */
  const totalSec = ref(config.value.focusMinutes * 60);
  /** 倒计时运行中的到期时间戳（ms）；非运行时为 null */
  const endsAt = ref<number | null>(null);
  /** 倒计时暂停时保留的剩余秒数 */
  const pausedRemainingSec = ref(0);
  /** 正计时：运行前的累计秒 */
  const stopwatchBaseSec = ref(0);
  /** 正计时：当前运行段开始时间戳 */
  const stopwatchRunStartedAt = ref<number | null>(null);
  /** 当前会话（学习/休息）开始时间戳，用于记录会话起止 */
  const sessionStartedAt = ref<number | null>(null);
  /** 当前循环已完成的番茄数（用于长休判定） */
  const roundCount = ref(0);

  const now = ref(Date.now());
  let ticker: ReturnType<typeof setInterval> | undefined;

  // ── 统计与记录（后端持久化）──
  const todayStats = ref<api.FocusDayStats>({ date: "", pomodoros: 0, focus_minutes: 0, breaks: 0 });
  const todaySessions = ref<api.FocusSession[]>([]);
  const weekSessions = ref<api.FocusSession[]>([]);

  // ── 派生状态 ──
  /** 倒计时剩余秒数（基于到期时间戳与当前真实时刻计算，避免显示多 1 秒） */
  const remainingSec = computed(() => {
    if (mode.value !== "countdown") return 0;
    if (sub.value === "running" && endsAt.value != null) {
      // now 由 250ms 定时器驱动，仅用于触发响应式重算；数值取当前真实时刻
      void now.value;
      return Math.max(0, Math.ceil((endsAt.value - Date.now()) / 1000));
    }
    if (sub.value === "paused") return pausedRemainingSec.value;
    // L4：休息结束显示 0 而非满时长
    if (sub.value === "breakEnded") return 0;
    return totalSec.value;
  });

  /** 正计时累计秒数 */
  const stopwatchSec = computed(() => {
    if (mode.value !== "stopwatch") return 0;
    if (sub.value === "running" && stopwatchRunStartedAt.value != null) {
      void now.value;
      return stopwatchBaseSec.value + Math.floor((Date.now() - stopwatchRunStartedAt.value) / 1000);
    }
    return stopwatchBaseSec.value;
  });

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
    const cap = config.value.focusMinutes * 60;
    return cap > 0 ? (stopwatchSec.value % cap) / cap : 0;
  });

  const phaseLabel = computed(() => {
    if (mode.value === "stopwatch" && phase.value === "focus") return "正计时（专注）";
    if (phase.value === "focus") return "学习";
    return phase.value === "longBreak" ? "长休息" : "短休息";
  });

  const isRunning = computed(() => sub.value === "running");
  const isPaused = computed(() => sub.value === "paused");

  /** 近 7 天统计（由 weekSessions 聚合） */
  const weekStats = computed(() => {
    let pomodoros = 0;
    let focus_minutes = 0;
    for (const s of weekSessions.value) {
      if (s.type !== "focus" || s.status !== "completed") continue;
      pomodoros += 1;
      focus_minutes += s.duration_minutes;
    }
    return { pomodoros, focus_minutes };
  });

  // ── 持久化配置 / 关联任务 ──
  watch(config, (v) => {
    try {
      localStorage.setItem(CONFIG_KEY, JSON.stringify(clampConfig(v)));
    } catch {
      /* 忽略 */
    }
  }, { deep: true });
  watch(linkedTaskId, (v) => {
    try {
      if (v) localStorage.setItem(LINKED_TASK_KEY, v);
      else localStorage.removeItem(LINKED_TASK_KEY);
    } catch {
      /* 忽略 */
    }
  });

  // ── 计时引擎：基于时间戳，跨页面持续 ──
  function ensureTicker() {
    if (!ticker) {
      ticker = setInterval(() => {
        now.value = Date.now();
        if (mode.value === "countdown" && sub.value === "running" && endsAt.value != null) {
          if (now.value >= endsAt.value) {
            finishCountdown();
          }
        }
      }, TICK_INTERVAL_MS);
    }
  }
  function stopTicker() {
    if (ticker) {
      clearInterval(ticker);
      ticker = undefined;
    }
  }

  function beginCountdown(nextPhase: Phase, minutes: number) {
    // M7：防御非法时长（清空输入 / NaN），避免产生瞬间完成的虚假番茄
    if (!Number.isFinite(minutes) || minutes <= 0) {
      console.warn("[Focus] 无效的时长配置，忽略开始", minutes);
      return;
    }
    mode.value = "countdown";
    phase.value = nextPhase;
    totalSec.value = minutes * 60;
    pausedRemainingSec.value = 0;
    sessionStartedAt.value = Date.now();
    sub.value = "running";
    endsAt.value = Date.now() + minutes * 60_000;
    ensureTicker();
  }

  function startFocus() {
    beginCountdown("focus", config.value.focusMinutes);
  }
  function startBreak() {
    beginCountdown("break", config.value.breakMinutes);
  }
  function startLongBreak() {
    beginCountdown("longBreak", config.value.longBreakMinutes);
  }
  function startStopwatch() {
    mode.value = "stopwatch";
    phase.value = "focus";
    stopwatchBaseSec.value = 0;
    sessionStartedAt.value = Date.now();
    stopwatchRunStartedAt.value = Date.now();
    sub.value = "running";
    ensureTicker();
  }

  function togglePause() {
    if (sub.value === "running") {
      if (mode.value === "countdown") {
        pausedRemainingSec.value = remainingSec.value;
        endsAt.value = null;
      } else {
        stopwatchBaseSec.value = stopwatchSec.value;
        stopwatchRunStartedAt.value = null;
      }
      sub.value = "paused";
      // M5：暂停后停止 ticker，避免整段暂停期间 250ms 空转
      stopTicker();
    } else if (sub.value === "paused") {
      if (mode.value === "countdown") {
        endsAt.value = Date.now() + pausedRemainingSec.value * 1000;
      } else {
        stopwatchRunStartedAt.value = Date.now();
      }
      sub.value = "running";
      ensureTicker();
    }
  }

  /** 手动模式学习结束后（已转正计时），点击「开始休息」 */
  function skipToBreak() {
    // M4：仅允许在「学习结束转正计时」时触发；倒计时进行中点按会静默丢弃当前会话
    if (phase.value === "focus" && mode.value === "stopwatch" && sub.value === "running") {
      // 把「学习倒计时结束 → 点开始休息」之间的正计时记录为一次完成的专注会话，
      // 并（若关联任务）累加专注分钟：这段仍处于专注等待休息，不应静默丢失、
      // 不计入今日学习时长。不足 1 分钟按 0 处理，避免产生无效记录。
      const elapsedMin = Math.max(0, Math.round(stopwatchSec.value / 60));
      const startedAt = sessionStartedAt.value;
      if (elapsedMin > 0) {
        void recordSession("stopwatch", elapsedMin, "completed", startedAt);
      }
      startBreak();
    }
  }

  /** 休息结束：继续新的一轮学习 */
  function continueRound() {
    startFocus();
  }

  /** 结束番茄钟 / 重置到空闲 */
  function resetToIdle(recordInterrupt: boolean) {
    // 进行中的学习会话记为「打断」（L3：不足 1 分钟记为 0，避免统计虚高）
    if (
      recordInterrupt &&
      mode.value === "countdown" &&
      phase.value === "focus" &&
      (sub.value === "running" || sub.value === "paused")
    ) {
      const elapsedMin = Math.max(0, Math.round((totalSec.value - remainingSec.value) / 60));
      void recordSession("focus", elapsedMin, "interrupted");
    }
    // L5：正计时结束/重置时记录会话（此前 stopwatch 类型从无落盘）
    if (recordInterrupt && mode.value === "stopwatch" && (sub.value === "running" || sub.value === "paused")) {
      const elapsedMin = Math.max(0, Math.round(stopwatchSec.value / 60));
      const startedAt = sessionStartedAt.value;
      void recordSession("stopwatch", elapsedMin, "interrupted", startedAt);
    }
    mode.value = "countdown";
    phase.value = "focus";
    sub.value = "idle";
    totalSec.value = config.value.focusMinutes * 60;
    endsAt.value = null;
    pausedRemainingSec.value = 0;
    stopwatchBaseSec.value = 0;
    stopwatchRunStartedAt.value = null;
    sessionStartedAt.value = null;
    // L9：重置后长休轮次计数归零，回到全新状态
    roundCount.value = 0;
    stopTicker();
  }
  function resetAll() {
    resetToIdle(true);
  }
  function endRound() {
    resetToIdle(false);
  }

  /** 正计时随时结束：正常完成并计入关联任务时间，结束保持在正计时模式（不切回倒计时） */
  function finishStopwatch() {
    if (mode.value !== "stopwatch" || (sub.value !== "running" && sub.value !== "paused")) return;
    const elapsedMin = Math.max(0, Math.round(stopwatchSec.value / 60));
    const startedAt = sessionStartedAt.value;
    // 清理正计时状态，但保持 mode="stopwatch"，让「开始计时」成为下一个动作
    phase.value = "focus";
    sub.value = "idle";
    totalSec.value = config.value.focusMinutes * 60;
    endsAt.value = null;
    pausedRemainingSec.value = 0;
    stopwatchBaseSec.value = 0;
    stopwatchRunStartedAt.value = null;
    sessionStartedAt.value = null;
    roundCount.value = 0;
    stopTicker();
    // 正计时结束按「完成」落盘，并累加到关联任务（recordSession 已支持 stopwatch+completed）
    void recordSession("stopwatch", elapsedMin, "completed", startedAt);
  }

  // ── 倒计时到点：学习→休息 / 休息→结束 ──
  function finishCountdown() {
    if (mode.value !== "countdown" || sub.value !== "running") return;
    endsAt.value = null;

    if (phase.value === "focus") {
      // 学习结束：记录一个完整番茄（关联任务累加专注分钟 + 落盘会话）
      const minutes = Math.max(1, Math.round(totalSec.value / 60));
      // M3：先取学习开始时间，避免 await 期间被 beginCountdown 覆盖为休息开始时间
      const startedAt = sessionStartedAt.value;
      roundCount.value += 1;
      void recordSession("focus", minutes, "completed", startedAt);
      void notifyPhaseEnd("学习结束", "专注已完成，可以开始休息了。");

      // 自动休息则直接进入休息（到长休轮次进长休）；手动则转正计时等待点击
      if (config.value.autoBreak) {
        if (config.value.longBreakEnabled && roundCount.value % config.value.longBreakInterval === 0) {
          startLongBreak();
        } else {
          startBreak();
        }
      } else {
        mode.value = "stopwatch";
        stopwatchBaseSec.value = 0;
        stopwatchRunStartedAt.value = Date.now();
        sub.value = "running";
        ensureTicker();
      }
    } else {
      // 休息结束：长休结束后重置轮次
      const isLong = phase.value === "longBreak";
      if (isLong) roundCount.value = 0;
      void recordSession(isLong ? "long_break" : "short_break", Math.max(1, Math.round(totalSec.value / 60)), "completed");
      void notifyPhaseEnd("休息结束", "休息时间到，可以开始新一轮学习了。");
      sub.value = "breakEnded";
      stopTicker();
    }
  }

  // ── 会话记录 / 统计 ──
  async function recordSession(
    type: api.FocusSessionType,
    minutes: number,
    status: api.FocusSessionStatus,
    startedAt: number | null = null,
  ) {
    if (
      (type === "focus" || type === "stopwatch") &&
      status === "completed" &&
      linkedTaskId.value
    ) {
      try {
        await api.focusAddMinutes(linkedTaskId.value, minutes);
      } catch (e) {
        console.warn("[Focus] 关联任务累加专注分钟失败:", e);
      }
    }
    const nowMs = Date.now();
    const started = startedAt ?? sessionStartedAt.value ?? nowMs - minutes * 60_000;
    try {
      await api.recordFocusSession({
        id: `${type}_${nowMs}`,
        type,
        // 用上海本地时间落盘（后端按字符串前 10 位取日期），避免凌晨被 UTC 挪到前一天
        started_at: formatLocalIso(new Date(started)),
        ended_at: formatLocalIso(new Date(nowMs)),
        duration_minutes: minutes,
        // M16：空串视为未关联任务，写入 null；正计时完成同样关联任务
        task_id: type === "focus" || type === "stopwatch" ? linkedTaskId.value || null : null,
        status,
      });
      await refreshTodayStats();
      // M8：完成番茄后同步刷新近 7 天统计，避免页面停留期间数据陈旧
      await refreshWeekSessions();
    } catch (e) {
      console.warn("[Focus] 记录专注会话失败:", e);
    }
  }

  async function refreshTodayStats() {
    try {
      todayStats.value = await api.getFocusTodayStats();
      todaySessions.value = await api.getFocusSessions(todayString());
    } catch (e) {
      console.warn("[Focus] 刷新今日专注统计失败:", e);
    }
  }

  async function refreshWeekSessions() {
    try {
      let start = todayString();
      for (let i = 0; i < 6; i++) start = prevDateString(start);
      weekSessions.value = await api.getFocusSessionsRange(start, todayString());
    } catch (e) {
      console.warn("[Focus] 刷新近 7 天专注记录失败:", e);
    }
  }

  async function loadStats() {
    await Promise.allSettled([refreshTodayStats(), refreshWeekSessions()]);
  }

  // ── 运行状态持久化（M6）：应用重启后恢复进行中的会话，避免静默丢失 ──
  interface FocusTimerState {
    mode: TimerMode;
    phase: Phase;
    sub: CountdownSub;
    endsAt: number | null;
    pausedRemainingSec: number;
    stopwatchBaseSec: number;
    stopwatchRunStartedAt: number | null;
    sessionStartedAt: number | null;
    roundCount: number;
    totalSec: number;
  }

  function persistState() {
    try {
      const s: FocusTimerState = {
        mode: mode.value,
        phase: phase.value,
        sub: sub.value,
        endsAt: endsAt.value,
        pausedRemainingSec: pausedRemainingSec.value,
        stopwatchBaseSec: stopwatchBaseSec.value,
        stopwatchRunStartedAt: stopwatchRunStartedAt.value,
        sessionStartedAt: sessionStartedAt.value,
        roundCount: roundCount.value,
        totalSec: totalSec.value,
      };
      localStorage.setItem(STATE_KEY, JSON.stringify(s));
    } catch {
      /* 忽略 */
    }
  }

  watch(
    [mode, phase, sub, endsAt, pausedRemainingSec, stopwatchBaseSec, stopwatchRunStartedAt, sessionStartedAt, roundCount, totalSec],
    persistState,
  );

  function restoreState() {
    try {
      const raw = localStorage.getItem(STATE_KEY);
      if (!raw) return;
      const s = JSON.parse(raw) as Partial<FocusTimerState>;
      if (!s.sub || s.sub === "idle") return;
      mode.value = s.mode ?? "countdown";
      phase.value = s.phase ?? "focus";
      sub.value = s.sub;
      totalSec.value = s.totalSec ?? config.value.focusMinutes * 60;
      pausedRemainingSec.value = s.pausedRemainingSec ?? 0;
      stopwatchBaseSec.value = s.stopwatchBaseSec ?? 0;
      sessionStartedAt.value = s.sessionStartedAt ?? null;
      roundCount.value = s.roundCount ?? 0;
      if (sub.value === "running" && mode.value === "countdown") {
        const ends = s.endsAt ?? 0;
        if (ends <= Date.now()) {
          // 关闭期间已到点：不补记，回到空闲
          resetToIdle(false);
        } else {
          endsAt.value = ends;
          stopwatchRunStartedAt.value = null;
          ensureTicker();
        }
      } else if (sub.value === "running" && mode.value === "stopwatch") {
        stopwatchRunStartedAt.value = s.stopwatchRunStartedAt ?? Date.now();
        ensureTicker();
      } else if (sub.value === "paused") {
        if (mode.value === "countdown") {
          endsAt.value = null;
          pausedRemainingSec.value = s.pausedRemainingSec ?? totalSec.value;
        } else {
          stopwatchRunStartedAt.value = null;
        }
      } else {
        // focusEnded / breakEnded：结束态不恢复运行
        sub.value = "idle";
        stopTicker();
      }
    } catch {
      /* 忽略 */
    }
  }

  restoreState();

  // ── 结束提醒：提示音 + 窗口不在前台时弹系统通知 ──
  let audioCtx: AudioContext | null = null;
  function playChime() {
    try {
      const ctx = audioCtx ?? new AudioContext();
      audioCtx = ctx;
      if (ctx.state === "suspended") {
        // L6：捕获自动播放策略导致的 rejection
        void ctx.resume().catch(() => {});
      }
      const nowMs = ctx.currentTime;
      for (let i = 0; i < CHIME_NOTES.length; i++) {
        const osc = ctx.createOscillator();
        const gain = ctx.createGain();
        osc.type = "sine";
        osc.frequency.value = CHIME_NOTES[i];
        const start = nowMs + i * CHIME_NOTE_GAP_S;
        gain.gain.setValueAtTime(CHIME_GAIN_START, start);
        gain.gain.exponentialRampToValueAtTime(CHIME_GAIN_PEAK, start + CHIME_ATTACK_S);
        gain.gain.exponentialRampToValueAtTime(CHIME_GAIN_START, start + CHIME_DECAY_S);
        osc.connect(gain).connect(ctx.destination);
        osc.start(start);
        osc.stop(start + CHIME_DURATION_S);
      }
    } catch (e) {
      console.warn("[Focus] 播放提示音失败:", e);
    }
  }

  async function notifyPhaseEnd(title: string, body: string) {
    playChime();
    if (!api.isTauri()) return;
    try {
      const win = getCurrentWindow();
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

  return {
    config,
    linkedTaskId,
    mode,
    phase,
    sub,
    totalSec,
    remainingSec,
    stopwatchSec,
    displaySec,
    displayText,
    progress,
    phaseLabel,
    isRunning,
    isPaused,
    todayStats,
    todaySessions,
    weekSessions,
    weekStats,
    startFocus,
    startBreak,
    startLongBreak,
    startStopwatch,
    togglePause,
    skipToBreak,
    continueRound,
    resetAll,
    endRound,
    finishStopwatch,
    loadStats,
  };
});
