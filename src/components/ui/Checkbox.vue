<script setup lang="ts">
/**
 * 统一复选框：圆角方形 + 主题色填充 + 勾选动画，与 Select/DatePicker 等控件风格一致。
 *
 * 用法与原生 checkbox 基本一致：v-model 双向绑定，或 :checked + @change(boolean) 单向写法；
 * disabled 禁用。组件根为 <span>（内部含屏幕阅读器可见的原生 input），
 * 可放心嵌套在 <label> 中使用：点击 label 文本或视觉盒子都会切换状态，Tab 聚焦后空格切换。
 */
import { computed } from "vue";
import { Check } from "lucide-vue-next";

const props = withDefaults(
  defineProps<{
    /** v-model 绑定值 */
    modelValue?: boolean;
    /** 兼容单向 :checked 用法 */
    checked?: boolean;
    /** 是否禁用 */
    disabled?: boolean;
    /** 选中态颜色，默认主题色 var(--accent) */
    color?: string;
  }>(),
  {
    modelValue: undefined,
    checked: false,
    disabled: false,
    color: undefined,
  }
);

const emit = defineEmits<{
  (e: "update:modelValue", value: boolean): void;
  (e: "change", value: boolean): void;
}>();

const isChecked = computed(() => props.modelValue ?? props.checked);

const boxColor = computed(() =>
  props.color
    ? {
        background: isChecked.value ? props.color : undefined,
        borderColor: props.color,
      }
    : undefined
);

function onInput(e: Event) {
  const next = (e.target as HTMLInputElement).checked;
  emit("update:modelValue", next);
  emit("change", next);
}
</script>

<template>
  <span class="checkbox" :class="{ checked: isChecked, disabled }">
    <input
      type="checkbox"
      class="checkbox-input"
      :checked="isChecked"
      :disabled="disabled"
      @change="onInput"
    />
    <span class="checkbox-box" :style="boxColor" aria-hidden="true">
      <Check v-if="isChecked" :size="12" :stroke-width="3" class="checkbox-check" />
    </span>
  </span>
</template>

<style scoped>
.checkbox {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  /* 隐藏 input 的绝对定位以本盒子为包含块（落在点击处），
     聚焦时浏览器无需滚动显示焦点元素，避免 WebView2 伪滚动导致整页上跳 */
  position: relative;
  flex-shrink: 0;
  width: 18px;
  height: 18px;
  cursor: pointer;
  vertical-align: middle;
}

.checkbox.disabled {
  opacity: 0.5;
  cursor: not-allowed;
}

/* 原生 input 保留语义与键盘可达性，仅视觉隐藏 */
.checkbox-input {
  position: absolute;
  width: 1px;
  height: 1px;
  margin: 0;
  padding: 0;
  border: 0;
  clip-path: inset(50%);
  overflow: hidden;
  white-space: nowrap;
}

.checkbox-box {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 18px;
  height: 18px;
  border: 1px solid var(--border-color-strong);
  border-radius: var(--radius-xs);
  background: var(--bg-elevated);
  color: var(--text-on-accent);
  transition: border-color var(--transition-fast), background var(--transition-fast),
    box-shadow var(--transition-fast);
}

.checkbox:hover:not(.disabled) .checkbox-box {
  border-color: var(--accent);
}

.checkbox.checked .checkbox-box {
  background: var(--accent);
  border-color: var(--accent);
}

.checkbox:focus-within .checkbox-box {
  box-shadow: 0 0 0 3px var(--accent-subtle);
}

.checkbox-check {
  transform: scale(0);
  transition: transform var(--transition-bounce);
}

.checkbox.checked .checkbox-check {
  transform: scale(1);
}
</style>