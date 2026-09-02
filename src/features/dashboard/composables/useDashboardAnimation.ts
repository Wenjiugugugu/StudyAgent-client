/**
 * 工作台 — 首屏入场动画
 *
 * 管理简报卡片 / 侧栏的入场动画状态。H30：组件卸载后不再更新 state / 操作 DOM。
 */
import { ref } from "vue";

export function useDashboardAnimation() {
  // 默认显示内容，避免首次加载时卡片 opacity 为 0 导致空白
  const briefingAnimated = ref(true);
  const sidebarAnimated = ref(true);

  // H30：组件卸载后不再更新 state / 操作 DOM
  let unmounted = false;

  function markUnmounted() {
    unmounted = true;
  }

  function playEntranceAnimation() {
    briefingAnimated.value = false;
    sidebarAnimated.value = false;
    // 触发重排后添加动画 class
    requestAnimationFrame(() => {
      if (unmounted) return;
      briefingAnimated.value = true;
      setTimeout(() => {
        if (unmounted) return;
        sidebarAnimated.value = true;
      }, 150);
    });
  }

  return {
    briefingAnimated,
    sidebarAnimated,
    playEntranceAnimation,
    markUnmounted,
  };
}
