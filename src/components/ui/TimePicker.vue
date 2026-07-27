<script setup lang="ts">
import { ref, computed, watch, onMounted, onBeforeUnmount } from "vue";
import { ChevronDown } from "lucide-vue-next";

const props = withDefaults(
  defineProps<{
    /** 选中时间（HH:mm） */
    modelValue: string;
    /** 占位符 */
    placeholder?: string;
    /** 分钟步进：默认 1（00-59）；可设为 5/15/30 等 */
    minuteStep?: number;
    /** 禁用 */
    disabled?: boolean;
  }>(),
  {
    placeholder: "选择时间",
    minuteStep: 1,
    disabled: false,
  }
);

const emit = defineEmits<{
  (e: "update:modelValue", value: string): void;
}>();

const open = ref(false);
const rootRef = ref<HTMLElement | null>(null);
const hourListRef = ref<HTMLElement | null>(null);
const minuteListRef = ref<HTMLElement | null>(null);

// 解析当前小时/分钟
const currentHour = ref(0);
const currentMinute = ref(0);

function parseModelValue(v: string) {
  const m = /^\s*(\d{1,2}):(\d{2})\s*$/.exec(v ?? "");
  if (!m) {
    currentHour.value = 0;
    currentMinute.value = 0;
    return;
  }
  const h = parseInt(m[1], 10);
  const min = parseInt(m[2], 10);
  currentHour.value = Math.max(0, Math.min(23, isNaN(h) ? 0 : h));
  currentMinute.value = Math.max(0, Math.min(59, isNaN(min) ? 0 : min));
}

onMounted(() => {
  parseModelValue(props.modelValue);
});
watch(
  () => props.modelValue,
  (v) => parseModelValue(v)
);

const displayValue = computed(() => {
  if (!props.modelValue) return "";
  const h = String(currentHour.value).padStart(2, "0");
  const min = String(currentMinute.value).padStart(2, "0");
  return `${h}:${min}`;
});

// 小时列表 00-23
const hours = computed(() =>
  Array.from({ length: 24 }, (_, i) => String(i).padStart(2, "0"))
);

// 分钟列表（按 minuteStep 生成）
const minutes = computed(() => {
  const step = Math.max(1, Math.min(60, Math.floor(props.minuteStep)));
  const arr: string[] = [];
  for (let i = 0; i < 60; i += step) {
    arr.push(String(i).padStart(2, "0"));
  }
  return arr;
});

function emitChange() {
  const h = String(currentHour.value).padStart(2, "0");
  const min = String(currentMinute.value).padStart(2, "0");
  emit("update:modelValue", `${h}:${min}`);
}

function selectHour(h: string) {
  currentHour.value = parseInt(h, 10);
  emitChange();
}

function selectMinute(m: string) {
  currentMinute.value = parseInt(m, 10);
  emitChange();
}

function toggleOpen() {
  if (props.disabled) return;
  open.value = !open.value;
  if (open.value) {
    // 打开后滚动到选中项
    setTimeout(scrollToSelected, 0);
  }
}

function close() {
  open.value = false;
}

function onDocClick(e: MouseEvent) {
  if (!rootRef.value) return;
  if (!rootRef.value.contains(e.target as Node)) {
    close();
  }
}

onMounted(() => {
  document.addEventListener("mousedown", onDocClick);
});
onBeforeUnmount(() => {
  document.removeEventListener("mousedown", onDocClick);
});

// 滚动到选中项
function scrollToSelected() {
  const scrollTo = (container: HTMLElement | null, selector: string) => {
    if (!container) return;
    const el = container.querySelector(selector) as HTMLElement | null;
    if (el) {
      container.scrollTo({ top: el.offsetTop - container.clientHeight / 2 + el.clientHeight / 2, behavior: "auto" });
    }
  };
  scrollTo(hourListRef.value, ".time-col-item.hour-active");
  scrollTo(minuteListRef.value, ".time-col-item.minute-active");
}
</script>

<template>
  <div ref="rootRef" class="time-picker" :class="{ disabled }">
    <button
      type="button"
      class="tp-display"
      :disabled="disabled"
      @click="toggleOpen"
    >
      <span v-if="displayValue" class="tp-value">{{ displayValue }}</span>
      <span v-else class="tp-placeholder">{{ placeholder }}</span>
      <ChevronDown :size="14" class="tp-arrow" :class="{ open }" />
    </button>

    <transition name="tp-pop">
      <div v-if="open" class="tp-panel">
        <div class="tp-col-wrap">
          <div class="tp-col-head">时</div>
          <div ref="hourListRef" class="tp-col">
            <button
              v-for="h in hours"
              :key="h"
              type="button"
              class="time-col-item"
              :class="{ 'hour-active': parseInt(h, 10) === currentHour }"
              @click="selectHour(h)"
            >
              {{ h }}
            </button>
          </div>
        </div>
        <div class="tp-col-sep">:</div>
        <div class="tp-col-wrap">
          <div class="tp-col-head">分</div>
          <div ref="minuteListRef" class="tp-col">
            <button
              v-for="m in minutes"
              :key="m"
              type="button"
              class="time-col-item"
              :class="{ 'minute-active': parseInt(m, 10) === currentMinute }"
              @click="selectMinute(m)"
            >
              {{ m }}
            </button>
          </div>
        </div>
      </div>
    </transition>
  </div>
</template>

<style scoped>
.time-picker {
  position: relative;
  display: inline-block;
  width: 100%;
}

.time-picker.disabled {
  opacity: 0.6;
  pointer-events: none;
}

.tp-display {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--space-2);
  background: var(--bg-elevated);
  border: 1px solid var(--border-color);
  border-radius: var(--radius-sm);
  padding: var(--space-2) var(--space-3);
  font-size: var(--text-sm);
  font-family: inherit;
  color: var(--text-primary);
  cursor: pointer;
  width: 100%;
  outline: none;
  transition: border-color var(--transition-fast);
}

.tp-display:hover {
  border-color: var(--accent);
}

.tp-display:focus-visible {
  border-color: var(--accent);
  outline: 2px solid var(--accent);
  outline-offset: -2px;
}

.tp-value {
  font-family: var(--font-mono);
  font-weight: var(--font-medium);
}

.tp-placeholder {
  color: var(--text-tertiary);
}

.tp-arrow {
  color: var(--text-tertiary);
  transition: transform var(--transition-fast);
  flex-shrink: 0;
}

.tp-arrow.open {
  transform: rotate(180deg);
}

.tp-panel {
  position: absolute;
  top: calc(100% + 4px);
  left: 0;
  z-index: 50;
  display: flex;
  align-items: stretch;
  gap: 0;
  background: var(--bg-elevated);
  border: 1px solid var(--border-color);
  border-radius: var(--radius-md);
  box-shadow: var(--shadow-lg);
  padding: var(--space-2);
  min-width: 180px;
}

.tp-col-wrap {
  display: flex;
  flex-direction: column;
  flex: 1;
  min-width: 0;
}

.tp-col-head {
  font-size: var(--text-xs);
  color: var(--text-tertiary);
  text-align: center;
  padding: 2px 0 var(--space-1);
  font-weight: var(--font-semibold);
  letter-spacing: 0.05em;
}

.tp-col {
  display: flex;
  flex-direction: column;
  align-items: stretch;
  max-height: 200px;
  overflow-y: auto;
  scrollbar-width: thin;
  padding: 2px;
}

.tp-col::-webkit-scrollbar {
  width: 4px;
}

.tp-col::-webkit-scrollbar-thumb {
  background: var(--border-color);
  border-radius: var(--radius-xs);
}

.time-col-item {
  border: none;
  background: transparent;
  color: var(--text-secondary);
  font-family: var(--font-mono);
  font-size: var(--text-sm);
  padding: 4px var(--space-2);
  border-radius: var(--radius-sm);
  cursor: pointer;
  transition: all var(--transition-fast);
  text-align: center;
  font-weight: var(--font-medium);
}

.time-col-item:hover {
  background: var(--bg-tertiary);
  color: var(--text-primary);
}

.time-col-item.hour-active,
.time-col-item.minute-active {
  background: var(--accent-subtle);
  color: var(--accent);
  font-weight: var(--font-semibold);
}

.tp-col-sep {
  display: flex;
  align-items: center;
  justify-content: center;
  padding: 0 2px;
  color: var(--text-tertiary);
  font-weight: var(--font-semibold);
  margin-top: 18px;
}

/* 弹出动画 */
.tp-pop-enter-active,
.tp-pop-leave-active {
  transition: opacity var(--transition-fast), transform var(--transition-fast);
}

.tp-pop-enter-from,
.tp-pop-leave-to {
  opacity: 0;
  transform: translateY(-4px);
}
</style>
