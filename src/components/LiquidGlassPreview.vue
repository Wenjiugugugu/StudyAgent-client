<script setup lang="ts">
import { ref, onBeforeUnmount } from "vue";
import { BookOpen, ChartNoAxesCombined, Timer } from "lucide-vue-next";
import { vLiquidGlass } from "@/directives/liquidGlass";
import { swellLiquidGlass } from "@/utils/liquidGlassMotion";

const options = [
  { label: "学习", icon: BookOpen },
  { label: "复盘", icon: ChartNoAxesCombined },
  { label: "专注", icon: Timer },
];
const selected = ref(1);
const lens = ref<HTMLElement | null>(null);
let swell: Animation | undefined;
function select(index: number) {
  if (selected.value === index) return;
  selected.value = index;
  swell = swellLiquidGlass(lens.value, swell);
}
function onKey(event: KeyboardEvent) {
  let index = selected.value;
  if (event.key === "ArrowRight") index = (index + 1) % options.length;
  else if (event.key === "ArrowLeft") index = (index + options.length - 1) % options.length;
  else if (event.key === "Home") index = 0;
  else if (event.key === "End") index = options.length - 1;
  else return;
  event.preventDefault();
  select(index);
  (event.currentTarget as HTMLElement).querySelectorAll("button")[index]?.focus();
}
onBeforeUnmount(() => swell?.cancel());
</script>

<template>
  <div class="lg-preview">
    <div class="lg-preview-art" aria-hidden="true"><span>专注当下</span></div>
    <div class="lg-preview-options" role="group" aria-label="液态玻璃交互预览" @keydown="onKey">
      <div
        class="lg-preview-carriage"
        :style="{ transform: `translateX(${selected * 100}%)` }"
        aria-hidden="true"
      >
        <span ref="lens" v-liquid-glass class="lg-preview-lens" />
      </div>
      <button
        v-for="(option, index) in options"
        :key="option.label"
        type="button"
        :aria-pressed="selected === index"
        @click="select(index)"
      >
        <component :is="option.icon" :size="21" :stroke-width="1.6" />
        <span>{{ option.label }}</span>
      </button>
    </div>
    <p class="lg-preview-caption">点击切换，感受玻璃的流动与折射</p>
  </div>
</template>
