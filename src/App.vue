<script setup lang="ts">
import { computed, onMounted, onBeforeUnmount, ref } from "vue";
import { useRoute, useRouter } from "vue-router";
import { useSettingsStore } from "@/stores/settings";
import { useAssistantStore } from "@/stores/assistant";
import { useTheme } from "@/composables/useTheme";
import * as api from "@/api";
import { isTauri } from "@/api/tauri";
import type { UpdateCheckResult } from "@/types";
import AppLayout from "@/layouts/AppLayout.vue";
import Modal from "@/components/ui/Modal.vue";
import Button from "@/components/ui/Button.vue";
import MarkdownText from "@/components/MarkdownText.vue";
import {
  Bell,
  Download,
  Sparkles,
  Power,
  Minimize2,
} from "lucide-vue-next";

const route = useRoute();
const router = useRouter();
const settingsStore = useSettingsStore();
const assistantStore = useAssistantStore();
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
    const { getCurrentWindow } = await import("@tauri-apps/api/window");
    if (action === "tray") {
      await getCurrentWindow().hide();
    } else {
      await getCurrentWindow().close();
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

// ── 启动更新检查（dashboard popup） ──
const updatePopupVisible = ref(false);
const updateInfo = ref<UpdateCheckResult | null>(null);
const updateLoading = ref(false);
let startupUpdateChecked = false;

async function checkStartupUpdate() {
  if (startupUpdateChecked || !isTauri()) return;
  startupUpdateChecked = true;
  updateLoading.value = true;
  try {
    const result = await api.checkForUpdates();
    if (result.has_update) {
      updateInfo.value = result;
      updatePopupVisible.value = true;
    }
  } catch (e) {
    console.warn("[Update] 启动检查更新失败:", e);
  } finally {
    updateLoading.value = false;
  }
}

function closeUpdatePopup() {
  updatePopupVisible.value = false;
}

function goToSettingsUpdate() {
  updatePopupVisible.value = false;
  router.push("/settings#settings-update");
}

// ── 更新日志弹窗（首次启动新版本时显示） ──
const changelogVisible = ref(false);
const changelogVersion = ref("");
const changelogContent = ref("");

/** 内置的版本更新日志（按版本号映射） */
const VERSION_CHANGELOGS: Record<string, string> = {
  "0.2.1": [
    "## 0.2.1 更新内容",
    "",
    "### 新增",
    "- 统一的时间选择器组件，替代原生 input[type=time]",
    "- 系统托盘：关闭窗口时可选择最小化到托盘或退出，支持记住选择",
    "- 开机自启动：可在「设置 → 通用」中开关",
    "- 更新日志弹窗：升级新版本后首次启动自动展示更新内容",
    "- 每日学习提醒：开始/结束/复盘时间到点自动通知",
    "- 启动时自动检查更新，发现新版本在工作台弹窗提示",
    "- MCP 配置小贴士：在设置页提供常用 MCP Server 配置示例",
    "",
    "### 改进",
    "- 不再自动将未完成任务标记为「已放弃」，状态完全由复盘时的勾选决定",
    "- 优化窗口关闭逻辑，避免误触退出",
  ].join("\n"),
};

async function checkChangelog() {
  if (!isTauri()) return;
  try {
    const currentVersion = await api.getAppVersion();
    const lastSeenKey = "studyagent.last_changelog_version";
    const lastSeen = localStorage.getItem(lastSeenKey);

    // 首次启动或版本升级时展示对应版本的更新日志
    if (lastSeen !== currentVersion && VERSION_CHANGELOGS[currentVersion]) {
      changelogVersion.value = currentVersion;
      changelogContent.value = VERSION_CHANGELOGS[currentVersion];
      changelogVisible.value = true;
      localStorage.setItem(lastSeenKey, currentVersion);
    } else if (lastSeen !== currentVersion) {
      // 没有内置 changelog 的版本，仅记录已展示过
      localStorage.setItem(lastSeenKey, currentVersion);
    }
  } catch (e) {
    console.warn("[Changelog] 检查更新日志失败:", e);
  }
}

function closeChangelog() {
  changelogVisible.value = false;
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
  // 引导完成后再做启动更新检查，避免引导页被打断
  if (settingsStore.onboardingCompleted) {
    void checkStartupUpdate();
  }
});

onBeforeUnmount(() => {
  closeUnlisten?.();
  trayMinimizeUnlisten?.();
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

  <!-- 启动时发现新版本弹窗 -->
  <Modal
    :open="updatePopupVisible"
    :title="'发现新版本'"
    :close-on-overlay="true"
    :close-on-esc="true"
    :show-close="true"
    :width="460"
    @close="closeUpdatePopup"
  >
    <div v-if="updateInfo" class="update-popup-body">
      <div class="update-banner">
        <Bell :size="20" />
      </div>
      <p class="update-headline">
        <span class="update-version">v{{ updateInfo.latest_version }}</span>
        已发布，你正在使用
        <span class="update-current">v{{ updateInfo.current_version }}</span>
      </p>
      <p v-if="updateInfo.release_name" class="update-sub">{{ updateInfo.release_name }}</p>
      <p class="update-tip">可在设置页面查看完整更新说明并下载安装包。</p>
    </div>
    <template #footer>
      <Button variant="ghost" size="sm" @click="closeUpdatePopup">稍后再说</Button>
      <Button variant="primary" size="sm" @click="goToSettingsUpdate">
        <Download :size="14" />
        <span>前往更新</span>
      </Button>
    </template>
  </Modal>
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

/* ── 启动更新检查弹窗 ── */
.update-popup-body {
  display: flex;
  flex-direction: column;
  gap: var(--space-2);
  align-items: center;
  text-align: center;
}

.update-banner {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 48px;
  height: 48px;
  border-radius: 50%;
  background: var(--accent-subtle);
  color: var(--accent);
  margin-bottom: var(--space-1);
}

.update-headline {
  margin: 0;
  font-size: var(--text-base);
  color: var(--text-primary);
  line-height: var(--leading-relaxed);
}

.update-version {
  font-family: var(--font-mono);
  font-weight: var(--font-semibold);
  color: var(--accent);
}

.update-current {
  font-family: var(--font-mono);
  color: var(--text-tertiary);
}

.update-sub {
  margin: 0;
  font-size: var(--text-sm);
  color: var(--text-secondary);
}

.update-tip {
  margin: var(--space-1) 0 0;
  font-size: var(--text-xs);
  color: var(--text-tertiary);
}
</style>
