<script setup lang="ts">
import { ref, onUnmounted } from "vue";
import Button from "@/components/ui/Button.vue";
import { Check, Copy } from "lucide-vue-next";

const props = defineProps<{
  code: string;
  label?: string;
}>();

const copied = ref(false);
let copyTimer: ReturnType<typeof setTimeout> | null = null;

async function copy() {
  try {
    if (navigator.clipboard && navigator.clipboard.writeText) {
      await navigator.clipboard.writeText(props.code);
    } else {
      // Fallback for environments without clipboard API
      const textarea = document.createElement("textarea");
      textarea.value = props.code;
      textarea.style.position = "fixed";
      textarea.style.opacity = "0";
      document.body.appendChild(textarea);
      textarea.select();
      document.execCommand("copy");
      document.body.removeChild(textarea);
    }
    copied.value = true;
    if (copyTimer) clearTimeout(copyTimer);
    copyTimer = setTimeout(() => {
      copied.value = false;
      copyTimer = null;
    }, 1500);
  } catch (e) {
    console.error("复制失败:", e);
  }
}

// 组件卸载时清除定时器，避免在已卸载组件上修改 ref
onUnmounted(() => {
  if (copyTimer) clearTimeout(copyTimer);
});
</script>

<template>
  <div class="code-block-wrapper">
    <div class="code-block-header">
      <span v-if="label" class="code-block-label">{{ label }}</span>
      <Button variant="ghost" size="sm" @click="copy">
        <Check v-if="copied" :size="14" />
        <Copy v-else :size="14" />
        <span>{{ copied ? "已复制" : "复制" }}</span>
      </Button>
    </div>
    <pre class="code-block">{{ code }}</pre>
  </div>
</template>

<style scoped>
.code-block-wrapper {
  display: flex;
  flex-direction: column;
  gap: var(--space-2);
}

.code-block-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--space-2);
}

.code-block-label {
  font-size: var(--text-xs);
  color: var(--text-tertiary);
  font-family: var(--font-mono);
}

.code-block {
  margin: 0;
  padding: var(--space-4);
  background: var(--bg-tertiary);
  border-radius: var(--radius-md);
  font-family: var(--font-mono);
  font-size: var(--text-xs);
  line-height: var(--leading-relaxed);
  color: var(--text-primary);
  overflow-x: auto;
  white-space: pre;
  max-height: 360px;
  overflow-y: auto;
}
</style>
