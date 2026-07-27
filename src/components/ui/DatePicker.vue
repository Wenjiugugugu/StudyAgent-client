<script setup lang="ts">
import { ref, computed, watch, onMounted, onUnmounted } from "vue";
import { ChevronLeft, ChevronRight, Calendar } from "lucide-vue-next";

const props = withDefaults(
  defineProps<{
    /** 选中日期 (YYYY-MM-DD)，空字符串表示未选 */
    modelValue: string;
    /** 占位符 */
    placeholder?: string;
    /** 最小可选日期 (YYYY-MM-DD) */
    min?: string;
    /** 最大可选日期 (YYYY-MM-DD) */
    max?: string;
    /** 是否允许清空 */
    clearable?: boolean;
    /** 禁用 */
    disabled?: boolean;
  }>(),
  {
    placeholder: "选择日期",
    clearable: true,
    disabled: false,
  }
);

const emit = defineEmits<{
  (e: "update:modelValue", value: string): void;
}>();

const open = ref(false);
const rootRef = ref<HTMLElement | null>(null);

// 视图月份（默认为 modelValue 对应月份，否则当前月）
const viewYear = ref(2026);
const viewMonth = ref(0); // 0-11

const todayStr = (() => {
  const d = new Date();
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`;
})();

function parseDate(s: string): Date | null {
  if (!s) return null;
  const m = s.match(/^(\d{4})-(\d{2})-(\d{2})$/);
  if (!m) return null;
  return new Date(parseInt(m[1]), parseInt(m[2]) - 1, parseInt(m[3]));
}

function formatDate(s: string): string {
  if (!s) return "";
  return s; // 直接返回 YYYY-MM-DD
}

const displayValue = computed(() => (props.modelValue ? formatDate(props.modelValue) : ""));

// 初始化视图月份
function initViewModel() {
  const d = parseDate(props.modelValue) ?? new Date();
  viewYear.value = d.getFullYear();
  viewMonth.value = d.getMonth();
}
onMounted(initViewModel);
watch(() => props.modelValue, initViewModel);

// 日历网格（6 行 x 7 列）
const weekHeaders = ["一", "二", "三", "四", "五", "六", "日"];

const calendarCells = computed(() => {
  const first = new Date(viewYear.value, viewMonth.value, 1);
  const last = new Date(viewYear.value, viewMonth.value + 1, 0);
  // 周一为第一天：getDay()=0(周日)→6, 1(周一)→0
  const firstWeekday = (first.getDay() + 6) % 7;
  const daysInMonth = last.getDate();
  const cells: { date: string; day: number; inMonth: boolean; isToday: boolean; disabled: boolean }[] = [];
  // 上月填充
  const prevLast = new Date(viewYear.value, viewMonth.value, 0);
  for (let i = firstWeekday - 1; i >= 0; i--) {
    const d = prevLast.getDate() - i;
    const date = `${prevLast.getFullYear()}-${String(prevLast.getMonth() + 1).padStart(2, "0")}-${String(d).padStart(2, "0")}`;
    cells.push({ date, day: d, inMonth: false, isToday: date === todayStr, disabled: isDisabled(date) });
  }
  // 本月
  for (let d = 1; d <= daysInMonth; d++) {
    const date = `${viewYear.value}-${String(viewMonth.value + 1).padStart(2, "0")}-${String(d).padStart(2, "0")}`;
    cells.push({ date, day: d, inMonth: true, isToday: date === todayStr, disabled: isDisabled(date) });
  }
  // 下月填充至 42 格
  const next = new Date(viewYear.value, viewMonth.value + 1, 1);
  while (cells.length < 42) {
    const d = cells.length - (firstWeekday + daysInMonth) + 1;
    const date = `${next.getFullYear()}-${String(next.getMonth() + 1).padStart(2, "0")}-${String(d).padStart(2, "0")}`;
    cells.push({ date, day: d, inMonth: false, isToday: date === todayStr, disabled: isDisabled(date) });
  }
  return cells;
});

function isDisabled(date: string): boolean {
  if (props.min && date < props.min) return true;
  if (props.max && date > props.max) return true;
  return false;
}

function prevMonth() {
  if (viewMonth.value === 0) {
    viewMonth.value = 11;
    viewYear.value -= 1;
  } else {
    viewMonth.value -= 1;
  }
}

function nextMonth() {
  if (viewMonth.value === 11) {
    viewMonth.value = 0;
    viewYear.value += 1;
  } else {
    viewMonth.value += 1;
  }
}

const viewMonthLabel = computed(() => `${viewYear.value} 年 ${viewMonth.value + 1} 月`);

function selectDate(date: string) {
  if (isDisabled(date)) return;
  emit("update:modelValue", date);
  open.value = false;
}

function clearValue() {
  emit("update:modelValue", "");
  open.value = false;
}

function toggleOpen() {
  if (props.disabled) return;
  open.value = !open.value;
}

function handleClickOutside(e: MouseEvent) {
  if (rootRef.value && !rootRef.value.contains(e.target as Node)) {
    open.value = false;
  }
}

onMounted(() => document.addEventListener("mousedown", handleClickOutside));
onUnmounted(() => document.removeEventListener("mousedown", handleClickOutside));
</script>

<template>
  <div ref="rootRef" class="date-picker" :class="{ disabled }">
    <button type="button" class="dp-input" :disabled="disabled" @click="toggleOpen">
      <Calendar :size="14" class="dp-icon" />
      <span class="dp-value" :class="{ placeholder: !displayValue }">
        {{ displayValue || placeholder }}
      </span>
      <button
        v-if="clearable && displayValue && !disabled"
        type="button"
        class="dp-clear"
        @click.stop="clearValue"
      >
        ×
      </button>
    </button>

    <transition name="dp-fade">
      <div v-if="open" class="dp-panel">
        <div class="dp-header">
          <button type="button" class="dp-nav" @click="prevMonth"><ChevronLeft :size="16" /></button>
          <span class="dp-month-label">{{ viewMonthLabel }}</span>
          <button type="button" class="dp-nav" @click="nextMonth"><ChevronRight :size="16" /></button>
        </div>
        <div class="dp-weekdays">
          <span v-for="w in weekHeaders" :key="w" class="dp-weekday">{{ w }}</span>
        </div>
        <div class="dp-grid">
          <button
            v-for="cell in calendarCells"
            :key="cell.date"
            type="button"
            class="dp-cell"
            :class="{
              'out-month': !cell.inMonth,
              today: cell.isToday,
              selected: cell.date === modelValue,
              disabled: cell.disabled,
            }"
            :disabled="cell.disabled"
            @click="selectDate(cell.date)"
          >
            {{ cell.day }}
          </button>
        </div>
      </div>
    </transition>
  </div>
</template>

<style scoped>
.date-picker {
  position: relative;
  display: inline-block;
  width: 100%;
}

.date-picker.disabled {
  opacity: 0.6;
  pointer-events: none;
}

.dp-input {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  width: 100%;
  padding: var(--space-2) var(--space-3);
  background: var(--bg-secondary);
  border: 1px solid var(--border-color);
  border-radius: var(--radius-md);
  color: var(--text-primary);
  font-size: var(--text-sm);
  font-family: inherit;
  cursor: pointer;
  transition: border-color var(--transition-fast);
}

.dp-input:hover {
  border-color: var(--accent);
}

.dp-icon {
  color: var(--text-tertiary);
  flex-shrink: 0;
}

.dp-value {
  flex: 1;
  text-align: left;
  min-width: 0;
}

.dp-value.placeholder {
  color: var(--text-tertiary);
}

.dp-clear {
  background: none;
  border: none;
  color: var(--text-tertiary);
  font-size: 16px;
  line-height: 1;
  cursor: pointer;
  padding: 0 4px;
}

.dp-clear:hover {
  color: var(--text-primary);
}

.dp-panel {
  position: absolute;
  top: calc(100% + 4px);
  left: 0;
  z-index: 100;
  width: 280px;
  padding: var(--space-3);
  background: var(--bg-elevated, var(--bg-primary));
  border: 1px solid var(--border-color);
  border-radius: var(--radius-md);
  box-shadow: var(--shadow-md, 0 4px 12px rgba(0, 0, 0, 0.12));
}

.dp-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  margin-bottom: var(--space-2);
}

.dp-nav {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 28px;
  height: 28px;
  background: transparent;
  border: none;
  border-radius: var(--radius-sm);
  color: var(--text-secondary);
  cursor: pointer;
}

.dp-nav:hover {
  background: var(--bg-secondary);
  color: var(--text-primary);
}

.dp-month-label {
  font-size: var(--text-sm);
  font-weight: var(--font-semibold);
  color: var(--text-primary);
}

.dp-weekdays {
  display: grid;
  grid-template-columns: repeat(7, 1fr);
  gap: 2px;
  margin-bottom: 4px;
}

.dp-weekday {
  text-align: center;
  font-size: 11px;
  color: var(--text-tertiary);
  padding: 4px 0;
}

.dp-grid {
  display: grid;
  grid-template-columns: repeat(7, 1fr);
  gap: 2px;
}

.dp-cell {
  display: flex;
  align-items: center;
  justify-content: center;
  height: 32px;
  background: transparent;
  border: 1px solid transparent;
  border-radius: var(--radius-sm);
  color: var(--text-primary);
  font-size: var(--text-sm);
  cursor: pointer;
  transition: background var(--transition-fast);
}

.dp-cell:hover:not(.disabled) {
  background: var(--bg-secondary);
}

.dp-cell.out-month {
  color: var(--text-tertiary);
  opacity: 0.5;
}

.dp-cell.today {
  border-color: var(--accent);
  color: var(--accent);
  font-weight: var(--font-semibold);
}

.dp-cell.selected {
  background: var(--accent);
  color: white;
  font-weight: var(--font-semibold);
}

.dp-cell.selected.today {
  border-color: var(--accent);
}

.dp-cell.disabled {
  opacity: 0.35;
  cursor: not-allowed;
}

.dp-fade-enter-active,
.dp-fade-leave-active {
  transition: opacity 0.15s ease, transform 0.15s ease;
}

.dp-fade-enter-from,
.dp-fade-leave-to {
  opacity: 0;
  transform: translateY(-4px);
}
</style>
