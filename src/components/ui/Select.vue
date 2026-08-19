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

const slots = useSlots();
const open = ref(false);
const rootRef = ref<HTMLElement | null>(null);

/** 提取 option 子节点的纯文本 */
function extractText(children: unknown): string {
  if (children == null) return "";
  if (typeof children === "string") return children;
  if (Array.isArray(children)) return children.map(extractText).join("");
  return "";
}

/** 从默认插槽的 <option> vnodes 提取选项列表，保持与原生 select 用法完全一致 */
const options = computed<SelectOption[]>(() => {
  const nodes = slots.default?.() ?? [];
  const result: SelectOption[] = [];
  const walk = (list: VNode[]) => {
    for (const n of list) {
      const isOption = n.type === "option" || (n.type as any)?.name === "option";
      if (isOption) {
        result.push({
          value: String((n.props as Record<string, unknown>)?.value ?? ""),
          label: extractText(n.children),
          disabled: !!(n.props as Record<string, unknown>)?.disabled,
        });
      } else if (n.children && typeof n.children === "object") {
        const childArr = Array.isArray(n.children) ? (n.children as VNode[]) : [(n.children as any) as VNode];
        walk(childArr.filter((c) => c && typeof c === "object"));
      }
    }
  };
  walk(nodes.filter((n) => n && typeof n === "object"));
  return result;
});

const currentKey = computed(() => String(props.modelValue ?? ""));

const displayLabel = computed(() => {
  const opt = options.value.find((o) => o.value === currentKey.value);
  if (opt) return opt.label;
  return props.modelValue != null && props.modelValue !== "" ? String(props.modelValue) : "";
});

function toggle() {
  if (props.disabled) return;
  open.value = !open.value;
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
      :aria-expanded="open"
      @click="toggle"
    >
      <span class="select-value" :class="{ placeholder: !displayLabel }">
        {{ displayLabel || placeholder }}
      </span>
      <ChevronDown :size="14" class="select-arrow" :class="{ open }" />
    </button>

    <transition name="select-pop">
      <div v-if="open" class="select-panel" role="listbox">
        <button
          v-for="opt in options"
          :key="opt.value"
          type="button"
          class="select-option"
          :class="{ selected: opt.value === currentKey }"
          :disabled="opt.disabled"
          role="option"
          :aria-selected="opt.value === currentKey"
          @click="choose(opt)"
        >
          {{ opt.label }}
        </button>
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

.select-option:disabled {
  opacity: 0.4;
  cursor: not-allowed;
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
