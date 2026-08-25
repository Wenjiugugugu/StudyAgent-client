<script setup lang="ts">
import { computed, onMounted, onBeforeUnmount, ref } from "vue";
import { useRoute, useRouter } from "vue-router";
import { useSettingsStore } from "@/stores/settings";
import { useAssistantStore } from "@/stores/assistant";
import { useUpdateStore } from "@/stores/update";
import { useTheme } from "@/composables/useTheme";
import * as api from "@/api";
import { isTauri } from "@/api/tauri";
import { currentMinutesShanghai, timeStringToMinutes, todayString, weekdayName, getWeekStart } from "@/utils/date";
import AppLayout from "@/layouts/AppLayout.vue";
import Modal from "@/components/ui/Modal.vue";
import Button from "@/components/ui/Button.vue";
import MarkdownText from "@/components/MarkdownText.vue";
import {
  Sparkles,
  Power,
  Minimize2,
} from "lucide-vue-next";

const route = useRoute();
const router = useRouter();
const settingsStore = useSettingsStore();
const assistantStore = useAssistantStore();
const updateStore = useUpdateStore();
useTheme();

// 独立路由（如引导页）不套用 AppLayout，全屏渲染
const isStandalone = computed(() => route.meta.standalone === true);

// ── 关闭窗口对话框（close_action = "ask" 时由后端 close-requested 事件触发） ──
const closeDialogVisible = ref(false);
const closeRemember = ref(false);
let closeUnlisten: (() => void) | null = null;
let trayMinimizeUnlisten: (() => void) | null = null;

async function performCloseAction(action: "tray" | "quit") {
  try {
    if (closeRemember.value) {
      // 持久化用户选择，下次不再询问
      await api.setCloseAction(action);
    }
  } catch (e) {
    console.warn("[CloseAction] 保存关闭动作失败:", e);
  } finally {
    closeDialogVisible.value = false;
    closeRemember.value = false;
  }

  try {
    if (action === "tray") {
      const { getCurrentWindow } = await import("@tauri-apps/api/window");
      await getCurrentWindow().hide();
    } else {
      // 注意：不能调用 window.close()，否则会再次触发 CloseRequested 事件，
      // 后端 close_action 仍为 "ask" 时会 prevent_close 并再次弹窗，形成死循环。
      // 也不能用 window.destroy()：存在 tray icon 时，销毁窗口后进程仍会驻留。
      // 改用后端 quit_app 命令，调用 app.exit(0) 真正退出整个应用进程。
      await api.quitApp();
    }
  } catch (e) {
    console.error("[CloseAction] 执行窗口动作失败:", e);
  }
}

function cancelClose() {
  closeDialogVisible.value = false;
  closeRemember.value = false;
}

async function listenCloseEvents() {
  if (!isTauri()) return;
  try {
    const { listen } = await import("@tauri-apps/api/event");
    closeUnlisten = await listen("close-requested", () => {
      closeDialogVisible.value = true;
    });
    trayMinimizeUnlisten = await listen("window-minimized-to-tray", () => {
      console.info("[Tray] 窗口已最小化到系统托盘");
    });
  } catch (e) {
    console.warn("[CloseAction] 监听关闭事件失败:", e);
  }
}

// ── 启动时检测是否到达复盘提醒时间，自动跳转复盘页 ──
// 避免缺失复盘：若当前时间已过 review_reminder_time 且今日尚未复盘（且非休息/排除日），自动跳转 /review
async function checkReviewOnStartup() {
  if (!isTauri()) return;
  try {
    const settings = settingsStore.settings;
    if (!settings?.study_schedule) return;

    const reviewTime = settings.study_schedule.review_reminder_time;
    if (!reviewTime) return;

    const now = currentMinutesShanghai();
    const target = timeStringToMinutes(reviewTime);
    if (target < 0 || now < target) return; // 未到复盘时间

    const today = todayString();

    // 每天仅自动跳转一次（sessionStorage 标记）
    const navKey = "studyagent.auto_review_nav";
    const navRaw = sessionStorage.getItem(navKey);
    if (navRaw === today) return; // 今日已自动跳转过

    // 休息日和排除日跳过
    const restDays = settings.study_schedule.rest_days ?? ["周日"];
    if (restDays.includes(weekdayName(today))) return;

    try {
      const wp = await api.getWeekPlan(getWeekStart(today));
      if (wp.data?.excluded_days?.some((d) => d.date === today)) return;
    } catch {
      // 无周计划，按正常流程检查复盘
    }

    // 检查今日复盘是否已存在
    try {
      await api.getReview(today);
      // 复盘已存在，无需跳转
      sessionStorage.setItem(navKey, today);
      return;
    } catch {
      // 今日复盘不存在，自动跳转
    }

    // 避免在引导页或已离开首页时打断用户
    if (route.path === "/onboarding") return;

    sessionStorage.setItem(navKey, today);
    router.push("/review");
  } catch (e) {
    console.warn("[AutoReview] 启动复盘检查失败:", e);
  }
}

// ── 启动更新检查（结果存入 updateStore，由首页弹窗展示） ──
async function checkStartupUpdate() {
  if (!isTauri()) return;
  // H28：函数内部兜底 catch，避免调用方 void 导致未处理 Promise 拒绝
  try {
    await updateStore.checkOnStartup();
  } catch (e) {
    console.error("[Update] 启动更新检查失败:", e);
  }
}

// ── 更新日志弹窗（首次启动新版本时显示） ──
const changelogVisible = ref(false);
const changelogVersion = ref("");
import { VERSION_CHANGELOGS } from "@/changelogs";

const changelogContent = ref("");

async function checkChangelog() {
  if (!isTauri()) return;
  try {
    const currentVersion = await api.getAppVersion();
    const lastSeenKey = "last_changelog_version";
    // 优先读取后端持久化标记（重启保留）；未初始化时迁移仅存的 localStorage 旧值
    let lastSeen = await api.getUiFlag(lastSeenKey);
    if (!lastSeen) {
      lastSeen = localStorage.getItem(`studyagent.${lastSeenKey}`) ?? "";
    }

    // 首次启动或版本升级时展示对应版本的更新日志
    if (lastSeen !== currentVersion && VERSION_CHANGELOGS[currentVersion]) {
      changelogVersion.value = currentVersion;
      const raw = VERSION_CHANGELOGS[currentVersion];
      changelogContent.value = Array.isArray(raw) ? raw.join("\n") : raw;
      changelogVisible.value = true;
      await api.setUiFlag(lastSeenKey, currentVersion);
    } else if (lastSeen !== currentVersion) {
      // 没有内置 changelog 的版本，仅记录已展示过
      await api.setUiFlag(lastSeenKey, currentVersion);
    }
  } catch (e) {
    console.warn("[Changelog] 检查更新日志失败:", e);
  }
}

function closeChangelog() {
  changelogVisible.value = false;
}

// ── 每日学习提醒（开始/结束/复盘时间到点通知） ──
let reminderInterval: number | undefined;

function startReminderChecker() {
  if (!isTauri()) return;
  // 每 60 秒检查一次是否到达提醒时间点
  reminderInterval = window.setInterval(async () => {
    try {
      const settings = settingsStore.settings;
      if (!settings?.study_schedule) return;

      const now = currentMinutesShanghai();
      const today = todayString();
      const firedKey = "studyagent.reminders_fired";
      const firedRaw = localStorage.getItem(firedKey) || "{}";
      let firedToday: Record<string, string> = {};
      try {
        const parsed = JSON.parse(firedRaw);
        if (parsed && parsed.date === today) firedToday = parsed.times || {};
      } catch {
        // 解析失败忽略
      }

      const reminders = [
        {
          key: "start",
          time: settings.study_schedule.start_time,
          title: "学习时间开始",
          body: "查看今日计划并开始今天的学习吧",
        },
        {
          key: "end",
          time: settings.study_schedule.end_time,
          title: "学习时间结束",
          body: "今天辛苦了，记得完成今日复盘",
        },
        {
          key: "review",
          time: settings.study_schedule.review_reminder_time,
          title: "复盘提醒",
          body: "该做今日复盘了，回顾一下今天的学习",
        },
      ];

      let changed = false;
      for (const r of reminders) {
        if (!r.time || firedToday[r.key]) continue;
        const target = timeStringToMinutes(r.time);
        if (target < 0) continue;
        // 在目标时间前后 2 分钟内触发
        if (Math.abs(now - target) <= 2) {
          await showNotification(r.title, r.body);
          firedToday[r.key] = r.time;
          changed = true;
        }
      }

      if (changed) {
        localStorage.setItem(
          firedKey,
          JSON.stringify({ date: today, times: firedToday }),
        );
      }

      // 跨天重置已触发记录
      // 上面读取时已校验 date === today，若 date 不匹配则 firedToday 为空，
      // 写入时会自动覆盖为新日期的记录
    } catch (e) {
      console.warn("[Reminder] 检查提醒失败:", e);
    }
  }, 60_000);
}

async function showNotification(title: string, body: string) {
  try {
    const { sendNotification, isPermissionGranted, requestPermission } = await import("@tauri-apps/plugin-notification");
    let granted = await isPermissionGranted();
    if (!granted) {
      const permission = await requestPermission();
      granted = permission === "granted";
    }
    if (granted) {
      sendNotification({ title, body });
    }
  } catch (e) {
    console.warn("[Reminder] 发送通知失败:", e);
  }
}

onMounted(async () => {
  await settingsStore.load();

  // 引导状态检查：未完成则跳转引导页
  if (!settingsStore.onboardingCompleted && route.path !== "/onboarding") {
    router.replace("/onboarding");
    return;
  }

  assistantStore.setContext({ current_view: "dashboard" });

  // 监听关闭事件、检查更新日志、检查更新（仅在 Tauri 环境下）
  await listenCloseEvents();
  await checkChangelog();
  // 启动每日提醒检查器
  startReminderChecker();
  // 引导完成后再做启动更新检查，避免引导页被打断
  if (settingsStore.onboardingCompleted) {
    void checkStartupUpdate();
    // 到达复盘提醒时间则自动跳转复盘页（避免缺失复盘）
    void checkReviewOnStartup();
  }
});

onBeforeUnmount(() => {
  closeUnlisten?.();
  trayMinimizeUnlisten?.();
  if (reminderInterval !== undefined) {
    clearInterval(reminderInterval);
    reminderInterval = undefined;
  }
});
</script>

<template>
  <router-view v-if="isStandalone" />
  <AppLayout v-else />

  <!-- 关闭窗口询问对话框 -->
  <Modal
    :open="closeDialogVisible"
    :title="'关闭窗口'"
    :close-on-overlay="false"
    :close-on-esc="true"
    :width="420"
    @close="cancelClose"
  >
    <div class="close-dialog-body">
      <p class="close-question">关闭窗口后想要做什么？</p>
      <p class="close-hint">可以最小化到系统托盘保持后台运行，或直接退出应用。</p>

      <div class="close-actions">
        <button class="close-action-card" type="button" @click="performCloseAction('tray')">
          <div class="close-action-icon">
            <Minimize2 :size="20" />
          </div>
          <div class="close-action-text">
            <span class="close-action-title">最小化到托盘</span>
            <span class="close-action-desc">保持后台运行，可从托盘恢复</span>
          </div>
        </button>
        <button class="close-action-card danger" type="button" @click="performCloseAction('quit')">
          <div class="close-action-icon">
            <Power :size="20" />
          </div>
          <div class="close-action-text">
            <span class="close-action-title">退出应用</span>
            <span class="close-action-desc">完全关闭 StudyAgent</span>
          </div>
        </button>
      </div>

      <label class="close-remember">
        <input v-model="closeRemember" type="checkbox" />
        <span>记住选择，下次不再询问（可在设置中修改）</span>
      </label>
    </div>

    <template #footer>
      <Button variant="ghost" size="sm" @click="cancelClose">取消</Button>
    </template>
  </Modal>

  <!-- 更新日志弹窗（升级后首次启动） -->
  <Modal
    :open="changelogVisible"
    :title="`StudyAgent 已更新到 v${changelogVersion}`"
    :close-on-overlay="true"
    :close-on-esc="true"
    :show-close="true"
    :width="520"
    @close="closeChangelog"
  >
    <div class="changelog-body">
      <div class="changelog-banner">
        <Sparkles :size="22" />
      </div>
      <MarkdownText :content="changelogContent" />
    </div>
    <template #footer>
      <Button variant="primary" size="sm" @click="closeChangelog">知道了</Button>
    </template>
  </Modal>

  <!-- 启动时发现新版本：已改为首页 inline 展示，不再弹窗 -->
</template>

<style scoped>
/* ── 关闭对话框 ── */
.close-dialog-body {
  display: flex;
  flex-direction: column;
  gap: var(--space-3);
}

.close-question {
  margin: 0;
  font-size: var(--text-base);
  font-weight: var(--font-semibold);
  color: var(--text-primary);
}

.close-hint {
  margin: 0;
  font-size: var(--text-sm);
  color: var(--text-tertiary);
  line-height: var(--leading-relaxed);
}

.close-actions {
  display: flex;
  flex-direction: column;
  gap: var(--space-2);
  margin-top: var(--space-2);
}

.close-action-card {
  display: flex;
  align-items: center;
  gap: var(--space-3);
  padding: var(--space-3);
  background: var(--bg-tertiary);
  border: 1.5px solid transparent;
  border-radius: var(--radius-md);
  cursor: pointer;
  font-family: inherit;
  text-align: left;
  transition: all var(--transition-fast);
  color: var(--text-primary);
}

.close-action-card:hover {
  border-color: var(--accent);
  background: var(--accent-subtle);
}

.close-action-card.danger:hover {
  border-color: var(--color-danger, #ff3b30);
  background: var(--color-danger-subtle, rgba(255, 59, 48, 0.08));
}

.close-action-card.danger .close-action-icon {
  color: var(--color-danger, #ff3b30);
  background: var(--color-danger-subtle, rgba(255, 59, 48, 0.12));
}

.close-action-icon {
  flex-shrink: 0;
  width: 36px;
  height: 36px;
  display: flex;
  align-items: center;
  justify-content: center;
  border-radius: var(--radius-md);
  background: var(--accent-subtle);
  color: var(--accent);
}

.close-action-text {
  display: flex;
  flex-direction: column;
  gap: 2px;
  min-width: 0;
}

.close-action-title {
  font-size: var(--text-sm);
  font-weight: var(--font-semibold);
  color: var(--text-primary);
}

.close-action-desc {
  font-size: var(--text-xs);
  color: var(--text-tertiary);
}

.close-remember {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  margin-top: var(--space-2);
  padding: var(--space-2) var(--space-1);
  font-size: var(--text-xs);
  color: var(--text-secondary);
  cursor: pointer;
}

.close-remember input {
  width: 14px;
  height: 14px;
  accent-color: var(--accent);
  cursor: pointer;
}

/* ── 更新日志弹窗 ── */
.changelog-body {
  display: flex;
  flex-direction: column;
  gap: var(--space-3);
}

.changelog-banner {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 48px;
  height: 48px;
  border-radius: var(--radius-md);
  background: var(--accent-subtle);
  color: var(--accent);
  margin: 0 auto var(--space-1);
}

</style>
