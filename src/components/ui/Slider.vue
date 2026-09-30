<script setup lang="ts">
import { computed, ref } from "vue";
import { vLiquidGlass, holdLiquidGlass, pulseLiquidGlass } from "@/directives/liquidGlass";
const props = withDefaults(
  defineProps<{
    modelValue: number;
    min?: number;
    max?: number;
    step?: number;
    label: string;
    color?: string;
    disabled?: boolean;
  }>(),
  { min: 0, max: 100, step: 1, color: "var(--accent)" }
);
const emit = defineEmits<{ "update:modelValue": [value: number]; change: [value: number] }>();
const pressed = ref(false);
const lens = ref<HTMLElement | null>(null);
const progress = computed(() =>
  Math.max(0, Math.min(100, ((props.modelValue - props.min) / (props.max - props.min || 1)) * 100))
);
function start(event: PointerEvent) {
  if (props.disabled || event.button !== 0) return;
  (event.currentTarget as HTMLElement).setPointerCapture(event.pointerId);
  pressed.value = true;
  holdLiquidGlass(lens.value, 0.18);
}
function end() {
  pressed.value = false;
  pulseLiquidGlass(lens.value, 300);
}
</script>
<template>
  <div class="ui-slider" :class="{ pressed, disabled }" :style="{ '--slider-color': color }">
    <div class="slider-rail" aria-hidden="true">
      <span class="slider-fill" :style="{ width: `${progress}%` }" />
      <span class="slider-position" :style="{ left: `${progress}%` }"
        ><span ref="lens" v-liquid-glass="{ strength: 0 }" class="slider-lens"
      /></span>
    </div>
    <input
      type="range"
      :min="min"
      :max="max"
      :step="step"
      :value="modelValue"
      :disabled="disabled"
      :aria-label="label"
      @input="emit('update:modelValue', Number(($event.target as HTMLInputElement).value))"
      @change="emit('change', Number(($event.target as HTMLInputElement).value))"
      @pointerdown="start"
      @pointerup="end"
      @pointercancel="end"
      @lostpointercapture="end"
      @blur="end"
    />
  </div>
</template>
<style scoped>
.ui-slider {
  position: relative;
  min-width: 80px;
  height: 40px;
  flex: 1;
}
.slider-rail {
  position: absolute;
  top: 18px;
  left: 12px;
  right: 12px;
  height: 4px;
  border-radius: 99px;
  background: var(--border-color);
  pointer-events: none;
}
.slider-fill {
  display: block;
  height: 100%;
  background: var(--slider-color);
  border-radius: inherit;
}
.slider-position {
  position: absolute;
  width: 24px;
  height: 18px;
  top: -7px;
  transform: translateX(-50%);
}
.slider-lens {
  display: block;
  width: 100%;
  height: 100%;
  border-radius: 99px;
  background: var(--bg-primary);
  border: 1px solid var(--border-color);
  box-shadow: 0 2px 5px rgba(0, 0, 0, 0.15);
  transition:
    transform 200ms,
    background 200ms;
}
.ui-slider input {
  position: absolute;
  inset: 0;
  margin: 0;
  width: 100%;
  height: 40px;
  opacity: 0;
  cursor: pointer;
  touch-action: pan-y;
  appearance: none;
}
.ui-slider input::-webkit-slider-thumb {
  appearance: none;
  width: 24px;
  height: 18px;
}
.ui-slider input::-moz-range-thumb {
  width: 24px;
  height: 18px;
  border: none;
}
.ui-slider:focus-within .slider-lens {
  outline: 2px solid var(--slider-color);
  outline-offset: 3px;
}
.ui-slider.disabled {
  opacity: 0.5;
}
:root[data-visual-mode="liquid-glass"] .slider-lens {
  border-color: var(--lg-edge-low);
  backdrop-filter: var(--lg-refraction, blur(0px)) saturate(120%);
  box-shadow:
    inset 0 1px 0 var(--lg-edge),
    0 3px 7px var(--lg-shadow);
}
:root[data-visual-mode="liquid-glass"] .pressed .slider-lens {
  transform: scale(1.25, 1.2);
  background: var(--lg-clear);
}
@media (prefers-reduced-motion: reduce) {
  .slider-lens {
    transition: none;
  }
  :root[data-visual-mode="liquid-glass"] .pressed .slider-lens {
    transform: none;
  }
}
@media (prefers-reduced-transparency: reduce), (forced-colors: active) {
  :root[data-visual-mode="liquid-glass"] .slider-lens {
    background: var(--bg-primary);
    backdrop-filter: none;
  }
}
</style>
