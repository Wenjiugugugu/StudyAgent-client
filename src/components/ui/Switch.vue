<script setup lang="ts">
import { computed, ref, watch, onBeforeUnmount } from "vue";
import { vLiquidGlass } from "@/directives/liquidGlass";
import { swellLiquidGlass } from "@/utils/liquidGlassMotion";

const props = withDefaults(
  defineProps<{
    modelValue: boolean;
    label: string;
    disabled?: boolean;
    loading?: boolean;
  }>(),
  { disabled: false, loading: false }
);
const emit = defineEmits<{
  (event: "update:modelValue", value: boolean): void;
  (event: "change", value: boolean): void;
}>();

const lens = ref<HTMLElement | null>(null);
const pressed = ref(false);
const dragging = ref(false);
const settling = ref(false);
const dragProgress = ref(0);
const progress = computed(() => (dragging.value ? dragProgress.value : Number(props.modelValue)));
const unavailable = computed(() => props.disabled || props.loading);
let pointerId: number | undefined;
let startX = 0;
let startProgress = 0;
let suppressClick = false;
let settleTimer: ReturnType<typeof setTimeout> | undefined;
let animation: Animation | undefined;

function commit(value: boolean) {
  if (unavailable.value || value === props.modelValue) return;
  emit("update:modelValue", value);
  emit("change", value);
}
function click() {
  if (suppressClick) {
    suppressClick = false;
    return;
  }
  commit(!props.modelValue);
}
function pointerDown(event: PointerEvent) {
  if (unavailable.value || !event.isPrimary || event.button !== 0) return;
  suppressClick = false;
  pointerId = event.pointerId;
  startX = event.clientX;
  startProgress = Number(props.modelValue);
  dragProgress.value = startProgress;
  pressed.value = true;
  (event.currentTarget as HTMLElement).setPointerCapture(event.pointerId);
}
function pointerMove(event: PointerEvent) {
  if (event.pointerId !== pointerId) return;
  const delta = event.clientX - startX;
  if (Math.abs(delta) > 3) dragging.value = true;
  if (dragging.value) dragProgress.value = Math.max(0, Math.min(1, startProgress + delta / 20));
}
function pointerUp(event: PointerEvent) {
  if (event.pointerId !== pointerId) return;
  pointerId = undefined;
  if (dragging.value) {
    suppressClick = true;
    commit(dragProgress.value >= 0.5);
  }
  dragging.value = false;
  pressed.value = false;
}
function cancel() {
  pointerId = undefined;
  pressed.value = false;
  dragging.value = false;
}
watch(
  () => props.modelValue,
  () => {
    clearTimeout(settleTimer);
    settling.value = true;
    animation = swellLiquidGlass(lens.value, animation, { x: 1.26, y: 1.16, duration: 560 });
    settleTimer = setTimeout(() => {
      settling.value = false;
    }, 560);
  }
);
watch(unavailable, (value) => {
  if (value) cancel();
});
onBeforeUnmount(() => {
  clearTimeout(settleTimer);
  animation?.cancel();
});
</script>

<template>
  <button
    type="button"
    role="switch"
    class="ui-switch"
    :class="{ on: modelValue, pressed, dragging, settling }"
    :style="{ '--switch-progress': progress }"
    :aria-label="label"
    :aria-checked="modelValue"
    :aria-busy="loading"
    :disabled="unavailable"
    @click="click"
    @pointerdown="pointerDown"
    @pointermove="pointerMove"
    @pointerup="pointerUp"
    @pointercancel="cancel"
    @lostpointercapture="cancel"
  >
    <span class="ui-switch-fill" aria-hidden="true" />
    <span class="ui-switch-carriage" aria-hidden="true">
      <span ref="lens" v-liquid-glass="{ strength: 0.16 }" class="ui-switch-lens" />
    </span>
  </button>
</template>

<style scoped>
.ui-switch {
  --switch-progress: 0;
  position: relative;
  display: inline-block;
  flex-shrink: 0;
  width: 58px;
  height: 28px;
  padding: 0;
  border: 0;
  border-radius: var(--radius-full);
  background: color-mix(in srgb, var(--text-tertiary) 24%, transparent);
  box-shadow: inset 0 1px 2px rgba(0, 0, 0, 0.07);
  cursor: pointer;
  touch-action: pan-y;
  -webkit-tap-highlight-color: transparent;
}
/* Keep an easy hit target while the visible track stays slim. */
.ui-switch::before {
  content: "";
  position: absolute;
  inset: -6px 0;
}
.ui-switch-fill {
  position: absolute;
  inset: 0;
  border-radius: inherit;
  background: var(--accent);
  opacity: var(--switch-progress);
  transition: opacity 320ms ease;
}
.ui-switch-carriage {
  position: absolute;
  top: 3px;
  left: 3px;
  width: 32px;
  height: 22px;
  border-radius: var(--radius-full);
  transform: translateX(calc(var(--switch-progress) * 20px));
  transition: transform 420ms cubic-bezier(0.22, 0.85, 0.26, 1);
  pointer-events: none;
}
.ui-switch-lens {
  position: absolute;
  inset: 0;
  border-radius: inherit;
  background: white;
  box-shadow:
    0 2px 4px rgba(0, 0, 0, 0.17),
    0 0.5px 1px rgba(0, 0, 0, 0.12);
  transition:
    transform 200ms ease,
    background 180ms ease,
    box-shadow 180ms ease;
}
.ui-switch:focus-visible {
  outline: 2px solid var(--accent);
  outline-offset: 4px;
}
.ui-switch:disabled {
  opacity: 0.4;
  cursor: not-allowed;
}
.ui-switch.dragging :is(.ui-switch-carriage, .ui-switch-fill) {
  transition: none;
}
[data-visual-mode="liquid-glass"] .ui-switch-lens {
  background: rgba(255, 255, 255, 0.06);
  -webkit-backdrop-filter: var(--lg-refraction, blur(0.5px)) saturate(115%);
  backdrop-filter: var(--lg-refraction, blur(0.5px)) saturate(115%);
}
/* An opaque resting pill becomes a clear, oversized lens during interaction. */
[data-visual-mode="liquid-glass"] .ui-switch-lens::after {
  content: "";
  position: absolute;
  inset: 0;
  border-radius: inherit;
  background: #fff;
  transition: opacity 160ms ease;
}
[data-visual-mode="liquid-glass"] .ui-switch:is(.pressed, .settling) .ui-switch-lens::after {
  opacity: 0.12;
}
[data-visual-mode="liquid-glass"] .ui-switch:is(.pressed, .settling) .ui-switch-lens {
  box-shadow:
    0 4px 9px rgba(16, 38, 29, 0.2),
    inset 0 1px 1px rgba(255, 255, 255, 0.95),
    inset 0 -1px 1.5px rgba(18, 52, 41, 0.2);
}
[data-visual-mode="liquid-glass"] .ui-switch.pressed .ui-switch-lens {
  transform: scale(1.26, 1.16);
}
@media (prefers-reduced-motion: reduce) {
  .ui-switch,
  .ui-switch-fill,
  .ui-switch-carriage,
  .ui-switch-lens,
  .ui-switch-lens::after {
    transition: none !important;
  }
  [data-visual-mode="liquid-glass"] .ui-switch.pressed .ui-switch-lens {
    transform: none;
  }
}
@media (prefers-reduced-transparency: reduce) {
  [data-visual-mode="liquid-glass"] .ui-switch-lens {
    -webkit-backdrop-filter: none;
    backdrop-filter: none;
    background: white;
  }
}
@media (forced-colors: active) {
  .ui-switch {
    background: Canvas;
    border: 1px solid ButtonText;
  }
  .ui-switch-fill {
    background: Highlight;
  }
  .ui-switch-lens {
    background: ButtonText;
    box-shadow: none;
  }
  [data-visual-mode="liquid-glass"] .ui-switch-lens {
    -webkit-backdrop-filter: none;
    backdrop-filter: none;
    background: ButtonText;
  }
  [data-visual-mode="liquid-glass"] .ui-switch-lens::after {
    display: none;
  }
}
</style>
