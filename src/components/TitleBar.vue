<script setup lang="ts">
import { ref, onMounted, onUnmounted } from "vue";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { isTauri } from "@/api/tauri";

const emit = defineEmits<{ "update:isMaximized": [value: boolean] }>();

const isTauriEnv = isTauri();
const isMaximized = ref(false);

let unlistenResize: (() => void) | null = null;

async function updateMaximizedState() {
  if (!isTauriEnv) return;
  try {
    const appWindow = getCurrentWindow();
    isMaximized.value = await appWindow.isMaximized();
    emit("update:isMaximized", isMaximized.value);
  } catch (error) {
    console.error("[WindowControls] Failed to get maximized state:", error);
  }
}

async function minimize() {
  if (!isTauriEnv) return;
  try {
    await getCurrentWindow().minimize();
  } catch (error) {
    console.error("[WindowControls] Minimize failed:", error);
  }
}

async function toggleMaximize() {
  if (!isTauriEnv) return;
  try {
    const appWindow = getCurrentWindow();
    if (await appWindow.isMaximized()) {
      await appWindow.unmaximize();
    } else {
      await appWindow.maximize();
    }
    await updateMaximizedState();
  } catch (error) {
    console.error("[WindowControls] Toggle maximize failed:", error);
  }
}

async function close() {
  if (!isTauriEnv) return;
  try {
    await getCurrentWindow().close();
  } catch (error) {
    console.error("[WindowControls] Close failed:", error);
  }
}

onMounted(async () => {
  if (!isTauriEnv) return;
  await updateMaximizedState();
  try {
    const appWindow = getCurrentWindow();
    unlistenResize = await appWindow.onResized(() => {
      updateMaximizedState();
    });
  } catch (error) {
    console.error("[WindowControls] Failed to listen resize:", error);
  }
});

onUnmounted(() => {
  unlistenResize?.();
});

defineExpose({ toggleMaximize });
</script>

<template>
  <div class="window-controls" data-tauri-drag-region="ignore">
    <button class="window-btn" type="button" @click="minimize" aria-label="最小化">
      <svg width="12" height="12" viewBox="0 0 12 12" fill="none">
        <path d="M2 6H10" stroke="currentColor" stroke-width="1.2" stroke-linecap="round" />
      </svg>
    </button>
    <button class="window-btn" type="button" @click="toggleMaximize" aria-label="最大化/还原">
      <svg v-if="isMaximized" width="12" height="12" viewBox="0 0 12 12" fill="none">
        <path d="M3.5 1.5H9.5C10.0523 1.5 10.5 1.94772 10.5 2.5V8.5M8.5 10.5H2.5C1.94772 10.5 1.5 10.0523 1.5 9.5V3.5H8.5V10.5Z" stroke="currentColor" stroke-width="1.2" stroke-linejoin="round" />
      </svg>
      <svg v-else width="12" height="12" viewBox="0 0 12 12" fill="none">
        <rect x="1.5" y="1.5" width="9" height="9" rx="0.5" stroke="currentColor" stroke-width="1.2" />
      </svg>
    </button>
    <button class="window-btn win-close" type="button" @click="close" aria-label="关闭">
      <svg width="12" height="12" viewBox="0 0 12 12" fill="none">
        <path d="M2.5 2.5L9.5 9.5M9.5 2.5L2.5 9.5" stroke="currentColor" stroke-width="1.2" stroke-linecap="round" />
      </svg>
    </button>
  </div>
</template>

<style scoped>
.window-controls {
  display: flex;
  align-items: stretch;
  height: var(--header-height, 44px);
  flex-shrink: 0;
  align-self: stretch;
}

.window-btn {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 56px;
  height: var(--header-height, 44px);
  border: none;
  background: transparent;
  color: var(--text-secondary);
  cursor: pointer;
  transition: background-color var(--transition-fast), color var(--transition-fast);
  outline: none;
}

.window-btn:focus-visible {
  outline: 2px solid var(--accent);
  outline-offset: -2px;
}

.window-btn:hover {
  background: var(--bg-overlay);
  color: var(--text-primary);
}

.window-btn.win-close:hover {
  background: #e81123;
  color: #ffffff;
}

.window-btn.win-close:active {
  background: #f1707a;
}
</style>
