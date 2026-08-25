/**
 * 工作台 — 简报提示遮罩与首屏入场动画
 *
 * 管理全屏简报提示遮罩（背景变暗 + 文字快速闪烁三次后淡出）以及
 * 简报卡片 / 侧栏的入场动画状态。H30：组件卸载后不再更新 state / 操作 DOM。
 */
import { ref } from "vue";
import { getUiFlag, setUiFlag } from "@/api";

export function useDashboardAnimation() {
  // 默认显示内容，避免首次加载时卡片 opacity 为 0 导致空白
  const briefingAnimated = ref(true);
  const sidebarAnimated = ref(true);

  const showBriefingOverlay = ref(false);
  const overlayLeaving = ref(false);

  let hintTimer: number | undefined;
  let overlayFadeTimer: number | undefined;
  // H30：组件卸载后不再更新 state / 操作 DOM
  let unmounted = false;

  function markUnmounted() {
    unmounted = true;
  }

  async function triggerBriefingHint(todayDateStr: string) {
    const hintKey = "briefing_hint_viewed";
    // 用「后端文件持久化」按日期记录「今日是否已提示过」：每日首次打开时提示一次，
    // 之后同一天内关闭再打开不再重复（localStorage 在部分环境下随重启丢失，故改用后端落盘）。
    let stored = "";
    try {
      stored = await getUiFlag(hintKey);
    } catch {
      stored = localStorage.getItem(`studyagent.${hintKey}`) ?? "";
    }
    if (stored === todayDateStr) return;
    try {
      await setUiFlag(hintKey, todayDateStr);
    } catch {
      localStorage.setItem(`studyagent.${hintKey}`, todayDateStr);
    }
    // 全屏遮罩：背景变暗 + 文字闪烁
    showBriefingOverlay.value = true;
    overlayLeaving.value = false;
    // 快速闪烁三次（约 0.75 秒）后停留 1 秒，再淡出恢复背景
    hintTimer = window.setTimeout(() => {
      overlayLeaving.value = true;
      overlayFadeTimer = window.setTimeout(() => {
        if (unmounted) return;
        showBriefingOverlay.value = false;
        overlayLeaving.value = false;
      }, 350);
    }, 1750);
  }

  function playEntranceAnimation() {
    briefingAnimated.value = false;
    sidebarAnimated.value = false;
    // 触发重排后添加动画 class
    requestAnimationFrame(() => {
      briefingAnimated.value = true;
      setTimeout(() => {
        sidebarAnimated.value = true;
      }, 150);
    });
  }

  function clearTimers() {
    if (hintTimer !== undefined) {
      clearTimeout(hintTimer);
      hintTimer = undefined;
    }
    if (overlayFadeTimer !== undefined) {
      clearTimeout(overlayFadeTimer);
      overlayFadeTimer = undefined;
    }
  }

  return {
    briefingAnimated,
    sidebarAnimated,
    showBriefingOverlay,
    overlayLeaving,
    triggerBriefingHint,
    playEntranceAnimation,
    markUnmounted,
    clearTimers,
  };
}
