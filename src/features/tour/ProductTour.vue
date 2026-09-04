<script setup lang="ts">
/**
 * 新手产品导览（Product Tour）
 *
 * 首次完成引导配置、进入工作台后，用 coach-mark 高亮引导用户认识核心界面。
 * 触发条件：onboarding 完成时写入 tour.pending=1，且本机 tour.done 未置位；
 * 仅在 /dashboard 路由下展示，完成后（或点「跳过」）写入 tour.done=1。
 *
 * 实现：全屏半透明遮罩 + 聚光灯开孔（box-shadow 挖洞）+ 定位气泡卡片。
 * 遮罩 pointer-events:none，不拦截任何点击；气泡卡片可交互。
 * 目标元素通过 CSS 选择器定位，等待其出现（工作台数据异步加载）后显示。
 */
import { ref, computed, watch, onMounted, onBeforeUnmount, nextTick } from "vue";
import { useRoute, useRouter } from "vue-router";
import { getUiFlag, setUiFlag } from "@/api";
import { ChevronLeft, ChevronRight, X } from "lucide-vue-next";

const TOUR_PENDING_KEY = "studyagent.tour.pending";
const TOUR_DONE_KEY = "studyagent.tour.done";

interface TourStep {
  /** 该步要进入的页面（导览会在上一步结束后自动跳转） */
  route: string;
  /** 目标元素 CSS 选择器（须在目标页面 DOM 中唯一） */
  target: string;
  title: string;
  description: string;
  placement: "top" | "bottom" | "left" | "right";
}

const steps: TourStep[] = [
  {
    route: "/dashboard",
    target: ".dashboard-view header.hero",
    title: "工作台",
    description:
      "每天打开应用后的第一站：考研倒计时、AI 每日简报和昨日复盘都在这里。",
    placement: "bottom",
  },
  {
    route: "/today",
    target: ".today-view",
    title: "计划页",
    description:
      "今日任务与周计划一览无余。AI 根据你的进度安排每天的学习任务，完成后勾选即可。",
    placement: "bottom",
  },
  {
    route: "/review",
    target: ".review-view",
    title: "复盘页",
    description:
      "每天学完在这里做复盘：记录完成情况、专注时长与收获，让每一天都形成闭环。",
    placement: "bottom",
  },
  {
    route: "/analytics",
    target: ".analytics-view",
    title: "分析页",
    description: "学习时长、科目分布、连续学习天数……用数据看清自己的进步。",
    placement: "bottom",
  },
  {
    route: "/dashboard",
    target: ".sidebar",
    title: "其余模块，自由探索",
    description:
      "专注计时、教材、解惑、时间线等更多功能都在左侧侧边栏里。现在，开始你的学习之旅吧！",
    placement: "right",
  },
];

const route = useRoute();
const router = useRouter();

const visible = ref(false);
const stepIndex = ref(0);
/** pending=1 且 done!=1 时待展示 */
const pending = ref(false);
const flagsLoaded = ref(false);
/** 正在由导览驱动的路由跳转（跳转期间不因目标元素卸载而隐藏） */
const navigating = ref(false);

const step = computed(() => steps[stepIndex.value]);
const isLastStep = computed(() => stepIndex.value === steps.length - 1);

// ── 聚光灯 / 气泡定位 ──
const targetEl = ref<HTMLElement | null>(null);
const tooltipRef = ref<HTMLElement | null>(null);
const spotlightStyle = ref<Record<string, string>>({});
const tooltipStyle = ref<Record<string, string>>({ opacity: "0" });
let cachedRect: DOMRect | null = null;
let rafId = 0;

/** 等待目标元素出现（工作台数据异步加载后才会渲染） */
function waitForElement(selector: string, timeout = 8000): Promise<HTMLElement | null> {
  return new Promise((resolve) => {
    const start = Date.now();
    const probe = () => {
      const el = document.querySelector<HTMLElement>(selector);
      if (el) return resolve(el);
      if (Date.now() - start > timeout) return resolve(null);
      requestAnimationFrame(probe);
    };
    probe();
  });
}

function positionSpotlight() {
  if (!targetEl.value) return;
  const r = targetEl.value.getBoundingClientRect();
  if (r.width === 0 && r.height === 0) return;
  const radius = getComputedStyle(targetEl.value).borderRadius || "14px";
  spotlightStyle.value = {
    left: `${r.left}px`,
    top: `${r.top}px`,
    width: `${r.width}px`,
    height: `${r.height}px`,
    borderRadius: radius,
  };
}

function positionTooltip() {
  if (!targetEl.value || !tooltipRef.value) return;
  const r = targetEl.value.getBoundingClientRect();
  if (r.width === 0 && r.height === 0) return;
  const tw = tooltipRef.value.offsetWidth;
  const th = tooltipRef.value.offsetHeight;
  const gap = 14;
  let left = 0;
  let top = 0;
  switch (step.value.placement) {
    case "right":
      left = r.right + gap;
      top = r.top + r.height / 2 - th / 2;
      break;
    case "left":
      left = r.left - gap - tw;
      top = r.top + r.height / 2 - th / 2;
      break;
    case "top":
      left = r.left + r.width / 2 - tw / 2;
      top = r.top - gap - th;
      break;
    case "bottom":
      left = r.left + r.width / 2 - tw / 2;
      top = r.bottom + gap;
      break;
  }
  // 视口内夹取，避免溢出
  const pad = 10;
  left = Math.max(pad, Math.min(left, window.innerWidth - tw - pad));
  top = Math.max(pad, Math.min(top, window.innerHeight - th - pad));
  tooltipStyle.value = { left: `${left}px`, top: `${top}px`, opacity: "1" };
}

async function gotoStep(index: number) {
  navigating.value = true;
  try {
    stepIndex.value = index;
    tooltipStyle.value = { opacity: "0" };
    const stepDef = steps[index];
    // 需要进入其它页面时先跳转，再等待目标元素渲染
    if (stepDef.route && route.path !== stepDef.route) {
      await router.push(stepDef.route);
    }
    const el = await waitForElement(stepDef.target);
    if (!el) {
      // 目标始终未出现（异常），直接结束导览
      markDone();
      return;
    }
    targetEl.value = el;
    cachedRect = null;
    await nextTick();
    positionSpotlight();
    await nextTick();
    positionTooltip();
  } finally {
    navigating.value = false;
  }
}

function next() {
  if (isLastStep.value) {
    markDone();
    return;
  }
  gotoStep(stepIndex.value + 1);
}

function prev() {
  if (stepIndex.value === 0) return;
  gotoStep(stepIndex.value - 1);
}

/** 完成 / 跳过：写 done 标记并清除 pending，之后不再展示 */
async function markDone() {
  visible.value = false;
  stopLoop();
  // 同步清空内存中的 pending，避免 watch 立即重启导览
  pending.value = false;
  try {
    await Promise.all([
      setUiFlag(TOUR_DONE_KEY, "1"),
      setUiFlag(TOUR_PENDING_KEY, "0"),
    ]);
  } catch {
    /* 标记写入失败不影响界面 */
  }
}

// ── 持续跟随目标（侧边栏收起 / 入场动画 / 滚动导致的位置变化） ──
function startLoop() {
  stopLoop();
  const loop = () => {
    if (!visible.value || !targetEl.value) {
      rafId = 0;
      return;
    }
    // 用户手动离开当前页面（目标元素被卸载）：隐藏但不标记完成，
    // 待回到工作台后再继续；导览自身跳转期间（navigating）不触发
    if (!targetEl.value.isConnected) {
      if (!navigating.value) {
        visible.value = false;
        stopLoop();
        rafId = 0;
        return;
      }
    }
    const r = targetEl.value.getBoundingClientRect();
    const prev = cachedRect;
    if (
      !prev ||
      Math.abs(r.left - prev.left) > 0.5 ||
      Math.abs(r.top - prev.top) > 0.5 ||
      Math.abs(r.width - prev.width) > 0.5 ||
      Math.abs(r.height - prev.height) > 0.5
    ) {
      cachedRect = r;
      positionSpotlight();
      positionTooltip();
    }
    rafId = requestAnimationFrame(loop);
  };
  rafId = requestAnimationFrame(loop);
}

function stopLoop() {
  if (rafId) cancelAnimationFrame(rafId);
  rafId = 0;
}

// ── 触发与生命周期 ──
async function checkFlags() {
  try {
    const [pendingVal, doneVal] = await Promise.all([
      getUiFlag(TOUR_PENDING_KEY),
      getUiFlag(TOUR_DONE_KEY),
    ]);
    pending.value = pendingVal === "1" && doneVal !== "1";
  } catch {
    pending.value = false;
  } finally {
    flagsLoaded.value = true;
  }
}

watch(
  () => [pending.value, flagsLoaded.value, route.path] as const,
  ([isPending, loaded]) => {
    if (!loaded) return;
    // 首次进入工作台且标记有效时启动导览；导览进行中（visible=true）
    // 或已进入其它页面时都不重复触发
    if (isPending && !visible.value && route.path === "/dashboard") {
      visible.value = true;
      gotoStep(0);
      startLoop();
    }
  },
  { immediate: true },
);

onMounted(checkFlags);
onBeforeUnmount(() => {
  stopLoop();
});
</script>

<template>
  <div
    v-if="visible"
    class="tour-overlay"
    role="dialog"
    aria-modal="true"
    aria-label="产品导览"
  >
    <!-- 聚光灯开孔：通过超大 box-shadow 形成半透明遮罩 + 高亮孔洞 -->
    <div class="tour-spotlight" :style="spotlightStyle" aria-hidden="true"></div>

    <!-- 气泡卡片 -->
    <div
      ref="tooltipRef"
      class="tour-tooltip"
      :class="`placement-${step.placement}`"
      :style="tooltipStyle"
    >
      <span class="tour-arrow" aria-hidden="true"></span>
      <div class="tour-head">
        <span class="tour-step-label">第 {{ stepIndex + 1 }} / {{ steps.length }} 步</span>
        <button type="button" class="tour-close" @click="markDone">
          <X :size="14" />
          跳过
        </button>
      </div>
      <h3 class="tour-title">{{ step.title }}</h3>
      <p class="tour-desc">{{ step.description }}</p>
      <div class="tour-footer">
        <div class="tour-dots" aria-hidden="true">
          <span
            v-for="(_, i) in steps"
            :key="i"
            class="tour-dot"
            :class="{ active: i === stepIndex }"
          ></span>
        </div>
        <div class="tour-actions">
          <button
            v-if="stepIndex > 0"
            type="button"
            class="tour-btn ghost"
            @click="prev"
          >
            <ChevronLeft :size="15" />
            上一步
          </button>
          <button type="button" class="tour-btn primary" @click="next">
            <template v-if="isLastStep">开始使用</template>
            <template v-else>下一步 <ChevronRight :size="15" /></template>
          </button>
        </div>
      </div>
    </div>
  </div>
</template>

<style scoped>
/* Apple design library: coach-mark spotlight + floating tooltip card */
.tour-overlay {
  position: fixed;
  inset: 0;
  z-index: 500;
  pointer-events: none;
}

.tour-spotlight {
  position: fixed;
  transition: left 0.25s cubic-bezier(0.32, 0.72, 0, 1),
    top 0.25s cubic-bezier(0.32, 0.72, 0, 1),
    width 0.25s cubic-bezier(0.32, 0.72, 0, 1),
    height 0.25s cubic-bezier(0.32, 0.72, 0, 1);
  box-shadow: 0 0 0 9999px rgba(0, 0, 0, 0.42);
  border: 1.5px solid var(--accent);
  box-sizing: border-box;
}

.tour-tooltip {
  position: fixed;
  width: 300px;
  box-sizing: border-box;
  pointer-events: auto;
  background: var(--bg-elevated);
  border: 1px solid var(--border-color);
  border-radius: var(--radius-lg);
  box-shadow: var(--shadow-lg);
  padding: var(--space-4);
  transition: left 0.25s cubic-bezier(0.32, 0.72, 0, 1),
    top 0.25s cubic-bezier(0.32, 0.72, 0, 1),
    opacity 0.2s ease;
  color: var(--text-primary);
}

.tour-arrow {
  position: absolute;
  width: 10px;
  height: 10px;
  background: var(--bg-elevated);
  border-left: 1px solid var(--border-color);
  border-top: 1px solid var(--border-color);
  transform: rotate(45deg);
}
.placement-right .tour-arrow {
  left: -6px;
  top: calc(50% - 5px);
}
.placement-left .tour-arrow {
  right: -6px;
  top: calc(50% - 5px);
  transform: rotate(225deg);
}
.placement-top .tour-arrow {
  left: calc(50% - 5px);
  bottom: -6px;
  transform: rotate(225deg);
}
.placement-bottom .tour-arrow {
  left: calc(50% - 5px);
  top: -6px;
  transform: rotate(45deg);
}

.tour-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  margin-bottom: var(--space-2);
}

.tour-step-label {
  font-size: var(--text-xs);
  font-weight: var(--font-semibold);
  color: var(--accent);
  letter-spacing: 0.02em;
}

.tour-close {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  border: none;
  background: transparent;
  color: var(--text-tertiary);
  font-size: var(--text-xs);
  font-weight: var(--font-medium);
  font-family: inherit;
  cursor: pointer;
  border-radius: var(--radius-sm);
  padding: 4px 6px;
  transition: color var(--transition-fast), background var(--transition-fast);
}
.tour-close:hover {
  color: var(--text-secondary);
  background: var(--bg-overlay);
}

.tour-title {
  margin: 0 0 var(--space-1);
  font-size: var(--text-lg);
  font-weight: var(--font-semibold);
  letter-spacing: -0.01em;
  color: var(--text-primary);
}

.tour-desc {
  margin: 0;
  font-size: var(--text-sm);
  line-height: var(--leading-relaxed);
  color: var(--text-secondary);
}

.tour-footer {
  display: flex;
  align-items: center;
  justify-content: space-between;
  margin-top: var(--space-4);
}

.tour-dots {
  display: flex;
  align-items: center;
  gap: 6px;
}

.tour-dot {
  width: 6px;
  height: 6px;
  border-radius: var(--radius-full);
  background: var(--border-color-strong);
  transition: background var(--transition-fast), transform var(--transition-fast);
}
.tour-dot.active {
  background: var(--accent);
  transform: scale(1.25);
}

.tour-actions {
  display: flex;
  align-items: center;
  gap: var(--space-2);
}

.tour-btn {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  border: none;
  border-radius: var(--radius-sm);
  font-size: var(--text-sm);
  font-weight: var(--font-semibold);
  font-family: inherit;
  cursor: pointer;
  padding: 6px 12px;
  transition: background var(--transition-fast), color var(--transition-fast),
    transform var(--transition-fast);
}
.tour-btn:active {
  transform: scale(0.97);
}
.tour-btn.ghost {
  background: transparent;
  color: var(--text-secondary);
}
.tour-btn.ghost:hover {
  background: var(--bg-overlay);
  color: var(--text-primary);
}
.tour-btn.primary {
  background: var(--accent);
  color: var(--text-on-accent);
}
.tour-btn.primary:hover {
  background: var(--accent-hover);
}

@media (prefers-reduced-motion: reduce) {
  .tour-spotlight,
  .tour-tooltip {
    transition: none;
  }
}
</style>
