<script setup lang="ts">
/**
 * 通用模态对话框组件
 *
 * 用法：
 * <Modal :open="visible" title="标题" @close="visible = false">
 *   <p>内容</p>
 *   <template #footer>
 *     <Button @click="...">确定</Button>
 *   </template>
 * </Modal>
 */
import { onMounted, onBeforeUnmount, watch } from "vue";
import { X } from "lucide-vue-next";

const props = withDefaults(
  defineProps<{
    /** 是否显示 */
    open: boolean;
    /** 标题 */
    title?: string;
    /** 是否允许点击遮罩关闭 */
    closeOnOverlay?: boolean;
    /** 是否允许 ESC 关闭 */
    closeOnEsc?: boolean;
    /** 是否显示关闭按钮 */
    showClose?: boolean;
    /** 宽度（px） */
    width?: number;
  }>(),
  {
    title: "",
    closeOnOverlay: true,
    closeOnEsc: true,
    showClose: true,
    width: 440,
  }
);

const emit = defineEmits<{
  (e: "close"): void;
}>();

function close() {
  emit("close");
}

function onOverlayClick() {
  if (props.closeOnOverlay) close();
}

function onKeydown(e: KeyboardEvent) {
  if (!props.open) return;
  if (e.key === "Escape" && props.closeOnEsc) {
    e.preventDefault();
    close();
  }
}

onMounted(() => document.addEventListener("keydown", onKeydown));
onBeforeUnmount(() => document.removeEventListener("keydown", onKeydown));

// 打开时锁定 body 滚动
watch(
  () => props.open,
  (v) => {
    if (typeof document === "undefined") return;
    document.body.style.overflow = v ? "hidden" : "";
  }
);
</script>

<template>
  <transition name="modal-fade">
    <div v-if="open" class="modal-overlay" @click.self="onOverlayClick">
      <div
        class="modal-dialog"
        :style="{ width: `${width}px`, maxWidth: 'calc(100vw - 32px)' }"
        role="dialog"
        aria-modal="true"
      >
        <header v-if="title || showClose" class="modal-header">
          <h3 class="modal-title">{{ title }}</h3>
          <button v-if="showClose" class="modal-close" type="button" @click="close" aria-label="关闭">
            <X :size="16" />
          </button>
        </header>

        <div class="modal-body">
          <slot />
        </div>

        <footer v-if="$slots.footer" class="modal-footer">
          <slot name="footer" />
        </footer>
      </div>
    </div>
  </transition>
</template>

<style scoped>
.modal-overlay {
  position: fixed;
  inset: 0;
  background: rgba(0, 0, 0, 0.45);
  backdrop-filter: blur(6px);
  -webkit-backdrop-filter: blur(6px);
  z-index: 1000;
  display: flex;
  align-items: center;
  justify-content: center;
  padding: var(--space-4);
}

.modal-dialog {
  background: var(--bg-primary);
  border: 1px solid var(--border-color);
  border-radius: var(--radius-lg);
  box-shadow: var(--shadow-xl, 0 10px 40px rgba(0, 0, 0, 0.2));
  max-height: calc(100vh - 64px);
  display: flex;
  flex-direction: column;
  overflow: hidden;
}

.modal-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: var(--space-4) var(--space-5);
  border-bottom: 1px solid var(--divider-color);
}

.modal-title {
  margin: 0;
  font-size: var(--text-base);
  font-weight: var(--font-semibold);
  color: var(--text-primary);
  letter-spacing: -0.01em;
}

.modal-close {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 28px;
  height: 28px;
  border: none;
  background: transparent;
  color: var(--text-tertiary);
  cursor: pointer;
  border-radius: var(--radius-sm);
  transition: background var(--transition-fast), color var(--transition-fast);
}

.modal-close:hover {
  background: var(--bg-tertiary);
  color: var(--text-primary);
}

.modal-body {
  padding: var(--space-5);
  overflow-y: auto;
  flex: 1;
}

.modal-footer {
  display: flex;
  align-items: center;
  justify-content: flex-end;
  gap: var(--space-2);
  padding: var(--space-3) var(--space-5);
  border-top: 1px solid var(--divider-color);
  background: var(--bg-secondary);
}

.modal-fade-enter-active,
.modal-fade-leave-active {
  transition: opacity var(--transition-fast);
}

.modal-fade-enter-active .modal-dialog,
.modal-fade-leave-active .modal-dialog {
  transition: transform var(--transition-fast), opacity var(--transition-fast);
}

.modal-fade-enter-from,
.modal-fade-leave-to {
  opacity: 0;
}

.modal-fade-enter-from .modal-dialog,
.modal-fade-leave-to .modal-dialog {
  transform: scale(0.96) translateY(-8px);
  opacity: 0;
}
</style>
