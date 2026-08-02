<script setup lang="ts">
import { computed, onMounted, onBeforeUnmount, ref } from "vue";
import { useRoute, useRouter } from "vue-router";
import { useSettingsStore } from "@/stores/settings";
import { useAssistantStore } from "@/stores/assistant";
import { useUpdateStore } from "@/stores/update";
import { useTheme } from "@/composables/useTheme";
import * as api from "@/api";
import { isTauri } from "@/api/tauri";
import { currentMinutesShanghai, timeStringToMinutes, todayString } from "@/utils/date";
import AppLayout from "@/layouts/AppLayout.vue";
import Modal from "@/components/ui/Modal.vue";
import Button from "@/components/ui/Button.vue";
import MarkdownText from "@/components/MarkdownText.vue";
import {
  Bell,
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

// ── 启动更新检查（结果存入 updateStore，由首页弹窗展示） ──
async function checkStartupUpdate() {
  if (!isTauri()) return;
  await updateStore.checkOnStartup();
}

// ── 更新日志弹窗（首次启动新版本时显示） ──
const changelogVisible = ref(false);
const changelogVersion = ref("");
const changelogContent = ref("");

/** 内置的版本更新日志（按版本号映射） */
const VERSION_CHANGELOGS: Record<string, string> = {
  "0.3.0": [
    "## 0.3.0 更新内容",
    "",
    "本次更新带来一次大更新。核心是把「数据分析」和「个性化控制」做深：新增完整的分析页面，用图表帮你看见长期学习趋势；同时把外观交给用户自己决定，支持自定义主色调和隐藏 Logo。复盘后的 AI 重排也在本版本上线——提交复盘后可自动调整本周剩余计划，配合明确的加载提示和完整的调试日志。",
    "",
    "### 新增",
    "- 全新「分析」页面：从「完成率」「学习量」「复盘质量」「周期对比」四个维度呈现学习数据，帮助你在长期备考中看清真实进度。",
    "  - 学习量趋势：近7天/近30天/全部时间范围切换，折线图展示每日完成率，柱状图对比任务量，叠加计划 vs 实际学习时长。",
    "  - 复盘质量分析：任务掌握度分布饼图、阻碍因素 Top 5、学习感受曲线、困难类型分布，一眼定位当前最卡你的环节。",
    "  - 周期对比：本周 vs 上周、本月 vs 上月的完成率、学习时长、任务量横向对比，方便评估近期调整是否有效。",
    "  - 目标达成预测：基于近7天完成率推算当前进度状态（健康/风险/偏离），为下周计划提供数据参考。",
    "- 引入 ECharts 图表库：所有图表支持响应式自适应与暗色模式，随窗口大小和主题切换自动重绘。",
    "- 任务计时功能（可选）：在「设置 → 学习」中开启后，今日任务卡会出现开始/暂停按钮，自动记录每项任务的专注时长；复盘时可将累计时长一键带入。",
    "  - 引导流程与设置页均支持开启/关闭计时功能，默认关闭，不影响只关注任务完成度的使用习惯。",
    "- 自定义主色调：在「设置 → 外观」中可在 9 种预设色之间切换，或使用色盘自由取色；全站强调色（按钮、进度条、选中态、Logo）会实时同步。",
    "- Logo 显示开关：同样在「设置 → 外观」中，可关闭侧边栏左上角的 Logo 图标，仅保留应用名称文字，适合喜欢极简界面的用户。",
    "- 复盘后 AI 自动调整本周剩余计划：当复盘存在未完成任务、感受困难或额外进度时，自动调用 AI 重新排程本周剩余天数；请求超时设为 300 秒，界面显示「正在调整计划，请勿关闭应用」提示，并在 logs/ai-debug.log 中完整记录请求发起、prompt 长度、响应返回、解析结果和失败原因，便于排查异常。",
    "",
    "### 移除",
    "- 移除「风险提示」模块：该模块与「薄弱章节」「当前重点」信息高度重复，AI 生成质量也不稳定。后续由 AI 在周计划中结合 weak_chapters 与 current_focus 感知滞后科目。",
    "- 移除「今日提醒」模块：实际运行中 reminders 字段几乎全为空或仅输出无营养内容（如「重点关注：」截断），对决策帮助有限，直接移除以减少干扰。",
    "- AI 复盘 prompt 不再要求生成 risks_resolved 字段：与风险提示模块一并下线。",
    "",
    "### 修复",
    "- 修复开始学习时间前仍可查看明日及未来日期计划的问题：现在早于每日开始时间时，今天及未来日期的计划都会隐藏，避免提前焦虑。",
    "",
    "### 优化",
    "- 侧边导航栏顺序调整：「分析」页面入口移至「时间线」上方，更符合使用频率。",
    "- 旧版本数据兼容：plan/state/review 文件中的 risks、reminders、risks_resolved 字段仍保留反序列化能力，旧数据不会导致崩溃，但新数据不再写入这些字段。",
  ].join("\n"),
  "0.3.1": [
    "## 0.3.1 更新内容",
    "",
    "### 新增",
    "- 休息日提示：今日计划和首页工作台在休息日不再显示「今日无计划」和生成计划入口，改为展示「今日是休息日」提示。",
    "- AI 用量日志持久化：每次 AI 调用后自动记录 token 消耗（输入/输出 token、总 token）、耗时、模型名到日志文件，重启后不丢失，最多保留 500 条。",
    "- AI 调用记录持久化：调试页面的 AI 调用记录通过 localStorage 持久化已完成记录，最多保留 30 条，重启后不丢失。",
    "- AI 用量与费用估算：调试页面新增「AI 用量日志」区块，展示历史调用明细与汇总统计，并根据各厂商官方定价（DeepSeek、通义千问、智谱 GLM、Kimi、OpenAI、Claude、Gemini）估算人民币费用。",
    "- 自定义应用背景图：在「设置 → 外观」中可上传本机图片作为应用背景，支持调整模糊度（0-20px）与不透明度（10%-100%）；图片保存在应用数据目录，重启后自动加载。",
    "",
    "### 修复",
    "- 修复分析页、历史计划、周期对比中实际学习时长恒为 0 的问题：结构化复盘提交时未将实际用时写入 total_hours，现已从任务级 actual_minutes 聚合，并对历史复盘文件做兜底读取。",
    "- 修复上传背景图后仍显示白色背景的问题：内容区使用不透明背景色遮挡背景图层，现已引入 `--bg-solid` 兜底背景色与 `data-has-background` 属性，启用背景图时内容区自动切换为半透明 rgba。",
    "",
    "### 优化",
    "- 学习时长展示与「记录学习时长」设置联动：关闭该设置时，每日计划、周计划、历史计划和分析页均隐藏学习时长相关信息（估时、实际学时、时长趋势图、周期对比中的时长维度）；开启时每日计划展示 AI 估时，复盘展示估时与实际用时。",
    "- 首页工作台「今日焦点」：当今日计划全部完成后，展示「今日计划已全部完成」提示，不再显示已完成的任务卡。",
    "- 历史计划与周计划中，未到达的日期展示「未开始」而非「未复盘」，只有当天及已过去的日期才显示「未复盘」。",
  ].join("\n"),
  "0.2.5": [
    "## 0.2.5 更新内容",
    "",
    "### 新增",
    "- 复盘后 AI 自动调整后续计划：未完成、感受困难或额外进度触发本周剩余天数重排",
    "- 昨日未复盘提醒：今日计划页学习前提示补复盘",
    "- 保留原始周计划副本，支持一周结束后对比原计划与现计划",
    "",
    "### 修复",
    "- 修复今日计划一打开就出现任务已完成的问题",
    "- DailyScheduler 生成日计划时无条件重置 current_task，所有任务状态初始为 Pending",
    "- 防止旧版本遗留的错位 task_id / done 状态被带到新一天计划",
    "",
    "### 优化",
    "- 首页发现新版本提示从工作台 inline 卡片改为 Modal 弹窗",
  ].join("\n"),
  "0.2.4": [
    "## 0.2.4 更新内容",
    "",
    "### 修复",
    "- 修复今日计划一打开就出现任务已完成的问题",
    "- DailyScheduler 生成日计划时无条件重置 current_task，所有任务状态初始为 Pending",
    "- 防止旧版本遗留的错位 task_id / done 状态被带到新一天计划",
    "",
    "### 优化",
    "- 首页发现新版本提示从工作台 inline 卡片改为 Modal 弹窗",
  ].join("\n"),
  "0.2.2": [
    "## 0.2.2 更新内容",
    "",
    "### 修复",
    "- 修复关闭窗口选择「退出应用」时反复弹出询问弹窗的问题",
    "- 修复首页在学习开始时间前仍展示今日计划的问题",
    "- 修复历史计划完成率计算错误：只完成一项却显示 100% 绿勾",
    "- 修复历史日期计划无法读取复盘中的任务完成状态",
    "- 完成率现在优先从结构化复盘（task_reviews）聚合计算",
    "- 历史日期计划合并状态时优先读取新版 task_reviews",
  ].join("\n"),
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
