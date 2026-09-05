<script setup lang="ts">
/**
 * 统一下拉选择器（胶囊风格，与整体 UI 一致，带弹出动画）
 *
 * 用法与原生 select 保持一致：通过默认插槽传入 <option> 子节点，
 * v-model / change / disabled 语义不变。内部渲染为自定义按钮 + 弹出面板，
 * 面板带圆角与淡入动画（对齐 DatePicker / TimePicker 的风格）。
 *
 * 用法：
 * <Select v-model="value" :disabled="false">
 *   <option :value="null">请选择</option>
 *   <option v-for="o in options" :key="o" :value="o">{{ o }}</option>
 * </Select>
 */
import { ref, computed, useSlots, onMounted, onUnmounted, type VNode } from "vue";
import { ChevronDown } from "lucide-vue-next";

interface SelectOption {
  value: string;
  label: string;
  group?: string;
  disabled?: boolean;
}

const props = withDefaults(
  defineProps<{
    /** 绑定值 */
    modelValue: string | number | null;
    /** 是否禁用 */
    disabled?: boolean;
    /** 原生 select 的 name 属性 */
    name?: string;
    /** 原生 select 的 id 属性（供 label for 关联） */
    id?: string;
    /** 最大宽度 */
    maxWidth?: string;
    /** 未选中时的占位文案 */
    placeholder?: string;
  }>(),
  {
    disabled: false,
    name: undefined,
    id: undefined,
    maxWidth: undefined,
    placeholder: "请选择",
  }
);

const emit = defineEmits<{
  (e: "update:modelValue", value: string | number | null): void;
  (e: "change", value: string | number | null): void;
}>();

const slots = useSlots() as { default?: () => VNode[] };
const open = ref(false);
const rootRef = ref<HTMLElement | null>(null);
/** 键盘导航高亮索引（M17） */
const highlightIndex = ref(-1);
/** 实例唯一 ID（用于 aria-controls / aria-activedescendant 关联） */
let uidCounter = 0;
const uid = `select-${++uidCounter}`;

/** 提取 option 子节点的纯文本 */
function extractText(children: unknown): string {
  if (children == null) return "";
  if (typeof children === "string") return children;
  if (Array.isArray(children)) return children.map(extractText).join("");
  return "";
}

/** 从默认插槽的 <option>/<optgroup> vnodes 提取选项列表，保持与原生 select 用法完全一致 */
const options = computed<SelectOption[]>(() => {
  const nodes = slots.default?.() ?? [];
  const result: SelectOption[] = [];
  const walk = (list: VNode[], group?: string) => {
    for (const n of list) {
      const isOption = n.type === "option" || (n.type as any)?.name === "option";
      if (isOption) {
        result.push({
          value: String((n.props as Record<string, unknown>)?.value ?? ""),
          label: extractText(n.children),
          group,
          disabled: !!(n.props as Record<string, unknown>)?.disabled,
        });
      } else if (n.children && typeof n.children === "object") {
        const childArr = Array.isArray(n.children) ? (n.children as VNode[]) : [(n.children as any) as VNode];
        // optgroup：取 label 作为分组名，组内 option 继续提取
        const isGroup = n.type === "optgroup" || (n.type as any)?.name === "optgroup";
        const g = isGroup ? String((n.props as Record<string, unknown>)?.label ?? "") : group;
        walk(childArr.filter((c) => c && typeof c === "object"), g);
      }
    }
  };
  walk(nodes.filter((n) => n && typeof n === "object"));
  return result;
});

/** 按 optgroup 分组后的选项（保持原始顺序，连续同组合并）；条目带扁平下标供面板渲染定位 */
const groupedOptions = computed<{ group: string; items: (SelectOption & { _idx: number })[] }[]>(() => {
  const res: { group: string; items: (SelectOption & { _idx: number })[] }[] = [];
  let idx = 0;
  for (const o of options.value) {
    const item = { ...o, _idx: idx++ };
    const key = o.group ?? "";
    const last = res[res.length - 1];
    if (last && last.group === key) last.items.push(item);
    else res.push({ group: key, items: [item] });
  }
  return res;
});

const currentKey = computed(() => String(props.modelValue ?? ""));

const displayLabel = computed(() => {
  const opt = options.value.find((o) => o.value === currentKey.value);
  if (opt) return opt.label;
  return props.modelValue != null && props.modelValue !== "" ? String(props.modelValue) : "";
});

function openPanel() {
  open.value = true;
  // 打开时高亮当前选中项，无选中则高亮第一个可用项
  const current = currentKey.value;
  const idx = options.value.findIndex((o) => o.value === current && !o.disabled);
  highlightIndex.value =
    idx >= 0 ? idx : options.value.findIndex((o) => !o.disabled);
}

function toggle() {
  if (props.disabled) return;
  if (open.value) {
    open.value = false;
  } else {
    openPanel();
  }
}

function moveHighlight(dir: number) {
  const opts = options.value;
  if (opts.length === 0) return;
  let i = highlightIndex.value < 0 ? (dir > 0 ? -1 : 0) : highlightIndex.value;
  for (let step = 0; step < opts.length; step++) {
    i = (i + dir + opts.length) % opts.length;
    if (!opts[i].disabled) {
      highlightIndex.value = i;
      return;
    }
  }
}

function moveHighlightTo(index: number) {
  const opts = options.value;
  if (opts.length === 0) return;
  const clamped = Math.max(0, Math.min(index, opts.length - 1));
  for (let step = 0; step < opts.length; step++) {
    const i = (clamped + step) % opts.length;
    if (!opts[i].disabled) {
      highlightIndex.value = i;
      return;
    }
  }
}

function chooseHighlighted() {
  const opt = options.value[highlightIndex.value];
  if (opt && !opt.disabled) choose(opt);
}

/** 触发按钮键盘操作（M17）：方向键导航、Enter/Space 选择、Escape 关闭 */
function onTriggerKeydown(e: KeyboardEvent) {
  if (props.disabled) return;
  switch (e.key) {
    case "ArrowDown":
    case "ArrowUp":
      e.preventDefault();
      if (!open.value) openPanel();
      moveHighlight(e.key === "ArrowDown" ? 1 : -1);
      break;
    case "Enter":
    case " ":
      e.preventDefault();
      if (!open.value) openPanel();
      else chooseHighlighted();
      break;
    case "Escape":
      open.value = false;
      break;
  }
}

/** 面板键盘操作：方向键/Home/End 导航、Enter/Space 选择、Escape/Tab 关闭 */
function onPanelKeydown(e: KeyboardEvent) {
  switch (e.key) {
    case "ArrowDown":
      e.preventDefault();
      moveHighlight(1);
      break;
    case "ArrowUp":
      e.preventDefault();
      moveHighlight(-1);
      break;
    case "Home":
      e.preventDefault();
      moveHighlightTo(0);
      break;
    case "End":
      e.preventDefault();
      moveHighlightTo(options.value.length - 1);
      break;
    case "Enter":
    case " ":
      e.preventDefault();
      chooseHighlighted();
      break;
    case "Escape":
    case "Tab":
      open.value = false;
      break;
  }
}

function choose(opt: SelectOption) {
  if (opt.disabled) return;
  emit("update:modelValue", opt.value);
  emit("change", opt.value);
  open.value = false;
}

function onDocClick(e: MouseEvent) {
  if (rootRef.value && !rootRef.value.contains(e.target as Node)) {
    open.value = false;
  }
}

onMounted(() => document.addEventListener("mousedown", onDocClick));
onUnmounted(() => document.removeEventListener("mousedown", onDocClick));
</script>

<template>
  <div
    ref="rootRef"
    class="select-root"
    :class="{ disabled }"
    :style="maxWidth ? { maxWidth } : undefined"
  >
    <button
      type="button"
      :id="id"
      :name="name"
      class="select-trigger"
      :disabled="disabled"
      role="combobox"
      aria-haspopup="listbox"
      :aria-expanded="open"
      :aria-controls="`${uid}-panel`"
      :aria-activedescendant="open && highlightIndex >= 0 ? `${uid}-opt-${highlightIndex}` : undefined"
      @click="toggle"
      @keydown="onTriggerKeydown"
    >
      <span class="select-value" :class="{ placeholder: !displayLabel }">
        {{ displayLabel || placeholder }}
      </span>
      <ChevronDown :size="14" class="select-arrow" :class="{ open }" />
    </button>

    <transition name="select-pop">
      <div
        v-if="open"
        :id="`${uid}-panel`"
        class="select-panel"
        role="listbox"
        @keydown="onPanelKeydown"
      >
        <template v-for="(grp, gi) in groupedOptions" :key="grp.group || `g${gi}`">
          <div v-if="grp.group" class="select-group-label">{{ grp.group }}</div>
          <button
            v-for="opt in grp.items"
            :id="`${uid}-opt-${opt._idx}`"
            :key="`${opt.value}-${opt._idx}`"
            type="button"
            class="select-option"
            :class="{ selected: opt.value === currentKey, highlighted: highlightIndex === opt._idx }"
            :disabled="opt.disabled"
            role="option"
            :aria-selected="opt.value === currentKey"
            @click="choose(opt)"
          >
            {{ opt.label }}
          </button>
        </template>
        <div v-if="options.length === 0" class="select-empty">暂无选项</div>
      </div>
    </transition>
  </div>
</template>

<style scoped>
.select-root {
  position: relative;
  display: inline-flex;
  align-items: center;
  width: 100%;
}

.select-root.disabled {
  opacity: 0.5;
  pointer-events: none;
}

.select-trigger {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--space-2);
  width: 100%;
  min-height: 36px;
  padding: var(--space-2) var(--space-3);
  font-family: inherit;
  font-size: var(--text-sm);
  border: 1px solid var(--border-color);
  border-radius: var(--radius-full);
  background: var(--bg-elevated);
  color: var(--text-primary);
  cursor: pointer;
  transition: border-color var(--transition-fast), background var(--transition-fast);
}

.select-trigger:hover:not(:disabled) {
  border-color: var(--border-color-strong);
  background: var(--bg-tertiary);
}

.select-value {
  flex: 1;
  min-width: 0;
  text-align: left;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.select-value.placeholder {
  color: var(--text-tertiary);
}

.select-arrow {
  color: var(--text-tertiary);
  flex-shrink: 0;
  transition: transform var(--transition-fast);
}

.select-arrow.open {
  transform: rotate(180deg);
}

.select-panel {
  position: absolute;
  top: calc(100% + 4px);
  left: 0;
  right: 0;
  z-index: 100;
  max-height: 240px;
  overflow-y: auto;
  padding: var(--space-1);
  background: var(--bg-elevated, var(--bg-primary));
  border: 1px solid var(--border-color);
  border-radius: var(--radius-md);
  box-shadow: var(--shadow-md, 0 4px 12px rgba(0, 0, 0, 0.12));
}

.select-option {
  display: block;
  width: 100%;
  padding: var(--space-2) var(--space-3);
  border: none;
  background: transparent;
  color: var(--text-secondary);
  font-family: inherit;
  font-size: var(--text-sm);
  text-align: left;
  border-radius: var(--radius-sm);
  cursor: pointer;
  transition: background var(--transition-fast), color var(--transition-fast);
}

.select-option:hover:not(:disabled) {
  background: var(--bg-overlay);
  color: var(--text-primary);
}

.select-option.selected {
  background: var(--accent-subtle);
  color: var(--accent);
  font-weight: var(--font-semibold);
}

/* M17：键盘导航高亮（区别于悬停/选中） */
.select-option.highlighted:not(:disabled) {
  background: var(--bg-overlay);
  color: var(--text-primary);
}

.select-option:disabled {
  opacity: 0.4;
  cursor: not-allowed;
}

.select-group-label {
  margin-top: var(--space-1);
  padding: var(--space-1) var(--space-3);
  font-size: var(--text-xs);
  font-weight: var(--font-semibold);
  color: var(--text-tertiary);
  user-select: none;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.select-group-label + .select-group-label {
  margin-top: var(--space-1);
  border-top: 1px solid var(--border-color);
  padding-top: var(--space-2);
}

.select-empty {
  padding: var(--space-3);
  text-align: center;
  font-size: var(--text-sm);
  color: var(--text-tertiary);
}

/* 弹出动画（对齐 DatePicker / TimePicker） */
.select-pop-enter-active,
.select-pop-leave-active {
  transition: opacity var(--transition-fast), transform var(--transition-fast);
}

.select-pop-enter-from,
.select-pop-leave-to {
  opacity: 0;
  transform: translateY(-4px);
}
</style>
