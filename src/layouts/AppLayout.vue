<script setup lang="ts">
import { computed, ref, onMounted, watch, nextTick } from "vue";
import { useRoute } from "vue-router";
import { useAssistantStore } from "@/stores/assistant";
import SideBar from "./SideBar.vue";
import AssistantPanel from "@/components/assistant/AssistantPanel.vue";
import TitleBar from "@/components/TitleBar.vue";

const route = useRoute();
const assistantStore = useAssistantStore();
const titleBarRef = ref<InstanceType<typeof TitleBar> | null>(null);
const isMaximized = ref(false);
const contentBodyRef = ref<HTMLElement | null>(null);

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
  },
);

/**
 * 生成液态玻璃边缘折射位移图（SDF）
 * 原理：中间区域无位移 (128,128)，仅边缘带状区域沿法线方向位移
 * 这样 feDisplacementMap 只折射边缘，中间保持清晰 — 符合真实液态玻璃
 *
 * 液态「膨胀」效果：边缘处位移沿法线向内，造成透镜放大感
 * 边缘带越宽、位移越强，膨胀越明显
 */
function generateLiquidGlassDisplacementMap() {
  const W = 256;
  const H = 256;
  const canvas = document.createElement("canvas");
  canvas.width = W;
  canvas.height = H;
  const ctx = canvas.getContext("2d");
  if (!ctx) return;
  const imgData = ctx.createImageData(W, H);
  const data = imgData.data;
  // 圆角矩形参数（归一化坐标 0~1）
  const cx = 0.5, cy = 0.5;
  const halfW = 0.44, halfH = 0.44;
  const cornerR = 0.14;
  const edgeBand = 0.18; // 更宽的折射带，增强液态膨胀感
  const dispStrength = 55; // 更强的边缘位移，接近 rdev 的折射强度
  for (let y = 0; y < H; y++) {
    for (let x = 0; x < W; x++) {
      const u = x / W;
      const v = y / H;
      const ix = u - cx;
      const iy = v - cy;
      // 圆角矩形 SDF
      const qx = Math.abs(ix) - halfW + cornerR;
      const qy = Math.abs(iy) - halfH + cornerR;
      const sx = Math.max(qx, 0);
      const sy = Math.max(qy, 0);
      const outside = Math.sqrt(sx * sx + sy * sy);
      const inside = Math.min(Math.max(qx, qy), 0);
      const sdf = outside + inside - cornerR;
      // 边缘带：|sdf| < edgeBand 时有位移，中间 (sdf < -edgeBand) 无位移
      let disp = 0;
      if (sdf > -edgeBand) {
        // 平滑过渡：边缘最强，向内/外衰减
        // 偏移中心使峰值靠近边缘内侧 → 膨胀感更集中在轮廓
        const peak = sdf + edgeBand * 0.35;
        const t = Math.max(0, 1 - Math.abs(peak) / edgeBand);
        disp = t * t * (3 - 2 * t); // smoothstep
      }
      // 法线方向（指向边缘外）：SDF 梯度近似
      const eps = 0.01;
      const gx = (sdfAt(ix + eps, iy, halfW, halfH, cornerR) - sdfAt(ix - eps, iy, halfW, halfH, cornerR)) / (2 * eps);
      const gy = (sdfAt(ix, iy + eps, halfW, halfH, cornerR) - sdfAt(ix, iy - eps, halfW, halfH, cornerR)) / (2 * eps);
      const len = Math.sqrt(gx * gx + gy * gy) || 1;
      // 位移沿法线方向（向内收缩 = 边缘透镜放大 = 膨胀感）
      const dx = (-gx / len) * disp * dispStrength;
      const dy = (-gy / len) * disp * dispStrength;
      const idx = (y * W + x) * 4;
      data[idx] = Math.round(128 + dx);     // R → X 位移
      data[idx + 1] = Math.round(128 + dy); // G → Y 位移
      data[idx + 2] = 0;
      data[idx + 3] = 255;
    }
  }
  ctx.putImageData(imgData, 0, 0);
  const dataUrl = canvas.toDataURL();
  const feImage = document.getElementById("lg-refract-map");
  if (feImage) {
    feImage.setAttributeNS("http://www.w3.org/1999/xlink", "href", dataUrl);
    feImage.setAttribute("href", dataUrl);
  }
}

function sdfAt(ix: number, iy: number, halfW: number, halfH: number, r: number): number {
  const qx = Math.abs(ix) - halfW + r;
  const qy = Math.abs(iy) - halfH + r;
  const sx = Math.max(qx, 0);
  const sy = Math.max(qy, 0);
  return Math.sqrt(sx * sx + sy * sy) + Math.min(Math.max(qx, qy), 0) - r;
}

onMounted(() => {
  generateLiquidGlassDisplacementMap();
});
</script>

<template>
  <div class="app-layout" :class="{ 'is-maximized': isMaximized }">
    <!-- 自定义背景图层（由设置中的 background_image 驱动） -->
    <div class="app-background-layer" aria-hidden="true" />
    <!-- 液态玻璃 SVG 边缘折射 filter — 仅折射边缘，中间无变形 -->
    <svg class="liquid-glass-svg" aria-hidden="true" xmlns="http://www.w3.org/2000/svg">
      <defs>
        <filter id="lg-refract" x="-25%" y="-25%" width="150%" height="150%" filterUnits="objectBoundingBox" primitiveUnits="userSpaceOnUse" color-interpolation-filters="sRGB">
      <feImage id="lg-refract-map" x="0" y="0" width="100%" height="100%" preserveAspectRatio="none" result="MAP" />

      <!-- 边缘 mask：从位移图提取，只让边缘区域产生折射/色差 -->
      <feColorMatrix in="MAP" type="matrix" values="0.3 0.3 0.3 0 0  0.3 0.3 0.3 0 0  0.3 0.3 0.3 0 0  0 0 0 1 0" result="EDGE_INTENSITY" />
      <feComponentTransfer in="EDGE_INTENSITY" result="EDGE_MASK">
        <feFuncA type="discrete" tableValues="0 0.35 1" />
      </feComponentTransfer>

      <!-- Red 通道：位移最强 -->
      <feDisplacementMap in="SourceGraphic" in2="MAP" scale="55" xChannelSelector="R" yChannelSelector="G" result="RED_DISP" />
      <feColorMatrix in="RED_DISP" type="matrix" values="1 0 0 0 0  0 0 0 0 0  0 0 0 0 0  0 0 0 1 0" result="RED_CH" />

      <!-- Green 通道：位移中等 -->
      <feDisplacementMap in="SourceGraphic" in2="MAP" scale="48" xChannelSelector="R" yChannelSelector="G" result="GREEN_DISP" />
      <feColorMatrix in="GREEN_DISP" type="matrix" values="0 0 0 0 0  0 1 0 0 0  0 0 0 0 0  0 0 0 1 0" result="GREEN_CH" />

      <!-- Blue 通道：位移最弱 -->
      <feDisplacementMap in="SourceGraphic" in2="MAP" scale="42" xChannelSelector="R" yChannelSelector="G" result="BLUE_DISP" />
      <feColorMatrix in="BLUE_DISP" type="matrix" values="0 0 0 0 0  0 0 0 0 0  0 0 1 0 0  0 0 0 1 0" result="BLUE_CH" />

      <!-- RGB 通道以 screen 模式合并，产生边缘色差 -->
      <feBlend in="GREEN_CH" in2="BLUE_CH" mode="screen" result="GB" />
      <feBlend in="RED_CH" in2="GB" mode="screen" result="RGB_COMBINED" />

      <!-- 轻微柔化色差，避免生硬 -->
      <feGaussianBlur in="RGB_COMBINED" stdDeviation="0.4" result="RGB_SOFT" />

      <!-- 只保留边缘区域的色差 -->
      <feComposite in="RGB_SOFT" in2="EDGE_MASK" operator="in" result="EDGE_COLOR" />

      <!-- 中间区域保持原始清晰 -->
      <feComponentTransfer in="EDGE_MASK" result="INVERTED_MASK">
        <feFuncA type="table" tableValues="1 0" />
      </feComponentTransfer>
      <feComposite in="SourceGraphic" in2="INVERTED_MASK" operator="in" result="CENTER_CLEAN" />

      <!-- 边缘色差 + 清晰中心 -->
      <feComposite in="EDGE_COLOR" in2="CENTER_CLEAN" operator="over" />
    </filter>
      </defs>
    </svg>
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
            <button
              class="assistant-toggle"
              :class="{ active: assistantStore.panelOpen }"
              @click="assistantStore.togglePanel()"
              title="AI 助手"
              data-tauri-drag-region="ignore"
            >
              <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round">
                <path d="M12 8V4H8" />
                <rect width="16" height="12" x="4" y="8" rx="2" />
                <path d="M2 14h2M20 14h2M15 13v2M9 13v2" />
              </svg>
              <span>助手</span>
            </button>

            <TitleBar
              ref="titleBarRef"
              @update:is-maximized="isMaximized = $event"
            />
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

      <!-- Right Assistant Panel -->
      <transition name="slide">
        <AssistantPanel v-if="assistantStore.panelOpen" />
      </transition>
    </div>
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
  transition: opacity 0.3s ease, filter 0.3s ease;
}

/* 所有实际内容必须堆叠在背景层之上 */
.app-layout > :not(.app-background-layer) {
  position: relative;
  z-index: 1;
}

/* SVG filter 始终固定为 0×0，避免标准模式下默认占位产生顶部空白 */
.liquid-glass-svg {
  position: fixed;
  width: 0;
  height: 0;
  pointer-events: none;
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

/* Apple-style: capsule toggle, restrained */
.assistant-toggle {
  display: flex;
  align-items: center;
  align-self: center;
  gap: var(--space-2);
  padding: var(--space-1) var(--space-3);
  border: none;
  background: transparent;
  color: var(--text-secondary);
  font-size: var(--text-sm);
  font-weight: var(--font-medium);
  border-radius: var(--radius-full);
  cursor: pointer;
  transition: background-color var(--transition-fast), color var(--transition-fast);
  letter-spacing: -0.01em;
}

.assistant-toggle:hover {
  background: var(--sidebar-item-hover);
  color: var(--text-primary);
}

.assistant-toggle.active {
  background: var(--accent-subtle);
  color: var(--accent);
}

.content-body {
  flex: 1;
  overflow-y: auto;
  overflow-x: hidden;
}

/* 页面切换过渡 — Apple motion curve */
.view-fade-enter-active,
.view-fade-leave-active {
  transition: opacity 0.25s cubic-bezier(0.32, 0.72, 0, 1),
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
