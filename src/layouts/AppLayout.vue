<script setup lang="ts">
import { computed, ref, watch, nextTick } from "vue";
import { useRoute } from "vue-router";
import { useSettingsStore } from "@/stores/settings";
import SideBar from "./SideBar.vue";
import TitleBar from "@/components/TitleBar.vue";
import ProductTour from "@/features/tour/ProductTour.vue";

const route = useRoute();
const settingsStore = useSettingsStore();
const titleBarRef = ref<InstanceType<typeof TitleBar> | null>(null);
const isMaximized = ref(false);
const contentBodyRef = ref<HTMLElement | null>(null);

/** 是否启用悬浮岛式侧边栏（用于调整顶部标题的布局与材质） */
const isFloating = computed(() => settingsStore.sidebarStyle === "floating");

const pageTitle = computed(() => (route.meta.title as string) || "StudyAgent");
const isReserved = computed(() => route.meta.reserved === true);

// 切换路由时重置内容区滚动位置，避免调试/设置页共享滚动条位置
watch(
  () => route.path,
  () => {
    nextTick(() => {
      if (contentBodyRef.value) {
        contentBodyRef.value.scrollTop = 0;
      }
    });
  }
);
</script>

<template>
  <div class="app-layout" :class="{ 'is-maximized': isMaximized, 'sidebar-floating': isFloating }">
    <!-- 自定义背景图层（由设置中的 background_image 驱动） -->
    <div class="app-background-layer" aria-hidden="true"></div>
    <div class="app-body">
      <!-- Left Sidebar -->
      <SideBar />

      <!-- Main Content -->
      <main class="main-content">
        <header
          class="content-header"
          data-tauri-drag-region
          @dblclick="titleBarRef?.toggleMaximize()"
        >
          <div class="header-left">
            <h1 class="page-title">{{ pageTitle }}</h1>
            <span v-if="isReserved" class="reserved-badge">预留</span>
          </div>
          <div class="header-right">
            <TitleBar ref="titleBarRef" @update:is-maximized="isMaximized = $event" />
          </div>
        </header>

        <div ref="contentBodyRef" class="content-body">
          <router-view v-slot="{ Component }">
            <transition name="view-fade" mode="out-in">
              <component :is="Component" />
            </transition>
          </router-view>
        </div>
      </main>
    </div>

    <!-- 新手产品导览（首次完成引导后展示一次） -->
    <ProductTour />
  </div>
</template>

<style scoped>
/* Apple design library: restrained top bar, 1px structural border, glass-effect header */
.app-layout {
  display: flex;
  flex-direction: column;
  width: 100vw;
  height: 100vh;
  overflow: hidden;
  /* 有自定义背景图时透明，否则使用默认纯色背景 */
  background: transparent;
  position: relative;
}

/* 自定义背景图层：fixed 铺满视口，位于所有内容之下（z-index: 0） */
.app-background-layer {
  position: fixed;
  inset: 0;
  z-index: 0;
  pointer-events: none;
  background-color: var(--bg-solid);
  background-image: var(--app-background-image, none);
  background-size: cover;
  background-position: center;
  background-repeat: no-repeat;
  filter: blur(var(--app-background-blur, 0px));
  opacity: var(--app-background-opacity, 1);
  /* 模糊时向外扩展避免边缘出现透明 */
  transform: scale(1.05);
  transition:
    opacity 0.3s ease,
    filter 0.3s ease;
}

/* 所有实际内容必须堆叠在背景层之上 */
.app-layout > :not(.app-background-layer) {
  position: relative;
  z-index: 1;
}

.app-layout.is-maximized {
  box-sizing: border-box;
  border: 1px solid var(--divider-color);
  padding: 8px;
}

.app-layout.is-maximized .app-body {
  border-radius: var(--radius-lg);
  border: 1px solid var(--divider-color);
  overflow: hidden;
}

.app-body {
  flex: 1;
  display: flex;
  overflow: hidden;
  min-height: 0;
}

.main-content {
  flex: 1;
  display: flex;
  flex-direction: column;
  min-width: 0;
  overflow: hidden;
  background: var(--bg-primary);
}

/* Apple-style header: translucent material + backdrop blur, 1px bottom border */
.content-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  height: var(--header-height);
  min-height: var(--header-height);
  padding: 0 0 0 var(--space-4);
  background: transparent;
  border-bottom: 1px solid var(--divider-color);
  user-select: none;
  -webkit-user-select: none;
}

.header-left {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  min-width: 0;
}

.page-title {
  font-size: var(--text-base);
  font-weight: var(--font-semibold);
  color: var(--text-primary);
  letter-spacing: -0.015em;
  white-space: nowrap;
}

.reserved-badge {
  font-size: 10px;
  color: var(--text-tertiary);
  background: var(--bg-tertiary);
  padding: 2px 8px;
  border-radius: var(--radius-full);
  font-weight: var(--font-medium);
  flex-shrink: 0;
}

.header-right {
  display: flex;
  align-items: stretch;
  height: 100%;
  gap: var(--space-1);
}

.content-body {
  flex: 1;
  overflow-y: auto;
  overflow-x: hidden;
}

/* ── 悬浮岛式侧边栏：顶部标题下移，与悬浮岛屿顶部对齐 ── */
.app-layout.sidebar-floating .content-header {
  position: relative;
  margin-top: var(--space-4);
  height: calc(var(--header-height) - var(--space-4) + var(--space-1));
  min-height: calc(var(--header-height) - var(--space-4) + var(--space-1));
}
/* 悬浮岛模式下窗口控件位置与默认一致：将右侧控件绝对定位回顶部，避免跟随 header 下移 */
.app-layout.sidebar-floating .content-header .header-right {
  position: absolute;
  top: calc(-1 * var(--space-4));
  right: 0;
  height: var(--header-height);
}
/* 液态玻璃模式下，悬浮岛的顶部标题不再使用玻璃背景，保持干净 */
[data-visual-mode="liquid-glass"] .app-layout.sidebar-floating .content-header,
[data-visual-mode="liquid-glass"] .app-layout.sidebar-floating .content-header::before {
  background: transparent;
  border-bottom-color: transparent;
  box-shadow: none;
  -webkit-backdrop-filter: none;
  backdrop-filter: none;
}
/* 悬浮岛的窗口控件绝对定位上移到顶部时会溢出 content-header；液态玻璃的
   overflow:hidden 会裁掉其顶部区域，导致可操控范围变小。此处解除裁切。 */
[data-visual-mode="liquid-glass"] .app-layout.sidebar-floating .content-header {
  overflow: visible;
}

/* 页面切换过渡 — Apple motion curve */
.view-fade-enter-active,
.view-fade-leave-active {
  transition:
    opacity 0.25s cubic-bezier(0.32, 0.72, 0, 1),
    transform 0.25s cubic-bezier(0.32, 0.72, 0, 1);
}

.view-fade-enter-from {
  opacity: 0;
  /* 向上轻移：占满视口的页面若向下位移，底部会短暂超出触发滚动条闪现，
     向上位移只收缩底部、不会产生额外滚动条，避免进入页面时抖动 */
  transform: translateY(-8px);
}

.view-fade-leave-to {
  opacity: 0;
  transform: translateY(-4px);
}
</style>
