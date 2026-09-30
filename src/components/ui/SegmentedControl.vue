<script setup lang="ts">
import { computed, ref, watch, nextTick, onBeforeUnmount, type Component } from "vue";
import { vLiquidGlass, pulseLiquidGlass } from "@/directives/liquidGlass";
import { swellLiquidGlass } from "@/utils/liquidGlassMotion";
const props = defineProps<{
  modelValue: string;
  options: { value: string; label: string; icon?: Component }[];
  label: string;
  disabled?: boolean;
}>();
const emit = defineEmits<{ "update:modelValue": [value: string] }>();
const root = ref<HTMLElement | null>(null);
const lens = ref<HTMLElement | null>(null);
const index = computed(() =>
  props.options.findIndex((option) => option.value === props.modelValue)
);
let animation: Animation | undefined;
watch(
  () => props.modelValue,
  async () => {
    await nextTick();
    pulseLiquidGlass(lens.value);
    animation = swellLiquidGlass(lens.value, animation, { x: 1.06, y: 1.2, duration: 480 });
  }
);
onBeforeUnmount(() => animation?.cancel());
function choose(value: string) {
  if (!props.disabled && value !== props.modelValue) emit("update:modelValue", value);
}
function keydown(event: KeyboardEvent) {
  if (props.disabled || !props.options.length) return;
  let next = index.value;
  if (event.key === "ArrowRight" || event.key === "ArrowDown")
    next = (next + 1) % props.options.length;
  else if (event.key === "ArrowLeft" || event.key === "ArrowUp")
    next = (next - 1 + props.options.length) % props.options.length;
  else if (event.key === "Home") next = 0;
  else if (event.key === "End") next = props.options.length - 1;
  else return;
  event.preventDefault();
  choose(props.options[next].value);
  root.value?.querySelectorAll<HTMLButtonElement>("button")[next]?.focus();
}
</script>
<template>
  <div
    ref="root"
    class="ui-segmented"
    role="group"
    :aria-label="label"
    :style="{ '--segments': options.length }"
    @keydown="keydown"
  >
    <div
      v-if="index >= 0"
      class="segment-indicator"
      :style="{ transform: `translateX(${index * 100}%)` }"
      aria-hidden="true"
    >
      <span ref="lens" v-liquid-glass="{ strength: 0 }" class="segment-lens" />
    </div>
    <button
      v-for="option in options"
      :key="option.value"
      type="button"
      :disabled="disabled"
      :aria-pressed="option.value === modelValue"
      :tabindex="index < 0 || option.value === modelValue ? 0 : -1"
      @click="choose(option.value)"
    >
      <component :is="option.icon" v-if="option.icon" :size="15" />{{ option.label }}
    </button>
  </div>
</template>
<style scoped>
.ui-segmented {
  position: relative;
  display: inline-grid;
  grid-template-columns: repeat(var(--segments), minmax(0, 1fr));
  padding: 3px;
  border-radius: 999px;
  background: var(--bg-secondary);
  isolation: isolate;
}
.ui-segmented button {
  position: relative;
  z-index: 1;
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 6px;
  padding: 7px 13px;
  border: 0;
  border-radius: 999px;
  background: transparent;
  color: var(--text-secondary);
  font: inherit;
  font-size: 13px;
  white-space: nowrap;
  cursor: pointer;
}
.ui-segmented button[aria-pressed="true"] {
  color: var(--text-primary);
  font-weight: 600;
}
.ui-segmented button:focus-visible {
  outline: 2px solid var(--accent);
  outline-offset: -2px;
}
.ui-segmented button:disabled {
  opacity: 0.5;
  cursor: default;
}
.segment-indicator {
  position: absolute;
  top: 3px;
  bottom: 3px;
  left: 3px;
  width: calc((100% - 6px) / var(--segments));
  pointer-events: none;
  transition: transform 380ms cubic-bezier(0.32, 0.72, 0, 1);
}
.segment-lens {
  display: block;
  width: 100%;
  height: 100%;
  border-radius: 999px;
  background: var(--bg-primary);
  box-shadow: 0 2px 6px rgba(0, 0, 0, 0.08);
}
:root[data-visual-mode="liquid-glass"] .ui-segmented {
  background: var(--lg-clear);
  border: 1px solid var(--lg-edge-low);
}
:root[data-visual-mode="liquid-glass"] .segment-indicator {
  z-index: 2;
}
:root[data-visual-mode="liquid-glass"] .segment-lens {
  background: var(--lg-fill);
  backdrop-filter: var(--lg-refraction, blur(0px)) saturate(120%);
  box-shadow:
    inset 0 1px 0 var(--lg-edge),
    inset 0 -1px 0 var(--lg-edge-low),
    0 3px 9px var(--lg-shadow);
}
@media (prefers-reduced-motion: reduce) {
  .segment-indicator {
    transition: none;
  }
}
@media (prefers-reduced-transparency: reduce), (forced-colors: active) {
  :root[data-visual-mode="liquid-glass"] .segment-lens {
    background: var(--bg-primary);
    backdrop-filter: none;
  }
}
</style>
