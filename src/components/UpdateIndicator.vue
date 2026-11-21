<script setup lang="ts">
import { computed, ref } from "vue";
import { useRouter } from "vue-router";
import { ArrowDown, ArrowUp, AlertTriangle, LoaderCircle } from "lucide-vue-next";
import { useUpdateStore } from "@/stores/update";

const update = useUpdateStore();
const router = useRouter();
const button = ref<HTMLButtonElement | null>(null);
const tooltipVisible = ref(false);
const tooltipStyle = ref<Record<string, string>>({});
const downloading = computed(() => update.downloadState === "downloading");
const indeterminate = computed(() => downloading.value && !update.downloadProgress?.total);
const progress = computed(() => Math.max(0, Math.min(100, update.downloadProgress?.percent ?? 0)));
const label = computed(() => {
  if (update.preparing) return "正在保存复盘草稿并暂停计时…";
  if (update.installing) return "正在安装更新，应用即将重启…";
  if (update.downloadError)
    return `${update.downloadError}；点击重试${update.downloadedFilePath ? "安装" : "下载"}`;
  if (downloading.value) {
    const size = update.formatSize(update.downloadProgress?.downloaded ?? 0);
    return indeterminate.value
      ? `正在下载更新 · ${size}`
      : `正在下载更新 · ${progress.value.toFixed(0)}% · ${size} / ${update.formatSize(update.downloadProgress!.total)}`;
  }
  if (update.downloadedFilePath) return "点击安装更新，应用将退出并重启";
  return "点击下载更新";
});

function showTooltip() {
  if (!button.value) return;
  const bounds = button.value.getBoundingClientRect();
  tooltipStyle.value = {
    left: `${Math.min(bounds.right + 10, window.innerWidth - 290)}px`,
    bottom: `${Math.max(12, window.innerHeight - bounds.bottom)}px`,
  };
  tooltipVisible.value = true;
}
async function activate() {
  tooltipVisible.value = false;
  if (downloading.value) {
    await router.push("/settings#settings-update");
    return;
  }
  if (update.downloadedFilePath) await update.handleInstall();
  else await update.handleDownload();
}
</script>

<template>
  <div v-if="update.showIndicator" class="update-indicator" @pointerdown.stop>
    <button
      ref="button"
      type="button"
      class="update-circle"
      :class="{
        ready: !!update.downloadedFilePath && !update.installing,
        failed: !!update.downloadError,
      }"
      :disabled="update.installing"
      :aria-label="`更新 ${update.targetVersion}：${label}`"
      @click="activate"
      @mouseenter="showTooltip"
      @mouseleave="tooltipVisible = false"
      @focus="showTooltip"
      @blur="tooltipVisible = false"
    >
      <svg
        class="update-ring"
        :class="{ spinning: indeterminate }"
        viewBox="0 0 32 32"
        aria-hidden="true"
      >
        <circle class="ring-track" cx="16" cy="16" r="14" />
        <circle
          class="ring-value"
          cx="16"
          cy="16"
          r="14"
          pathLength="100"
          :stroke-dasharray="indeterminate ? '25 75' : '100'"
          :stroke-dashoffset="downloading && !indeterminate ? 100 - progress : 0"
        />
      </svg>
      <AlertTriangle v-if="update.downloadError" :size="14" />
      <LoaderCircle v-else-if="update.installing" :size="14" class="spinning" />
      <ArrowDown v-else-if="downloading" :size="15" />
      <ArrowUp v-else :size="15" />
    </button>
    <Teleport to="body">
      <div v-if="tooltipVisible" class="update-circle-tooltip" :style="tooltipStyle" role="tooltip">
        <strong>更新至 {{ update.targetVersion }}</strong>
        <span>{{ label }}</span>
      </div>
    </Teleport>
  </div>
</template>

<style scoped>
.update-indicator {
  display: flex;
  justify-content: center;
  padding: 6px 0;
}
.update-circle {
  position: relative;
  display: grid;
  place-items: center;
  width: 32px;
  height: 32px;
  padding: 0;
  border: 0;
  border-radius: 50%;
  background: var(--bg-secondary);
  color: var(--text-secondary);
  cursor: pointer;
}
.update-circle:hover {
  color: var(--text-primary);
  background: var(--bg-tertiary);
}
.update-circle:focus-visible {
  outline: 2px solid var(--accent);
  outline-offset: 3px;
}
.update-circle.ready {
  color: var(--accent);
}
.update-circle.failed {
  color: var(--error-color, #e05b5b);
}
.update-circle:disabled {
  cursor: wait;
}
.update-ring {
  position: absolute;
  inset: 0;
  width: 32px;
  height: 32px;
  transform: rotate(-90deg);
  fill: none;
  stroke-width: 2;
}
.ring-track {
  stroke: var(--border-color);
}
.ring-value {
  stroke: currentColor;
  stroke-linecap: round;
  transition: stroke-dashoffset 0.2s ease;
}
.spinning {
  animation: update-spin 1.2s linear infinite;
}
.update-circle-tooltip {
  position: fixed;
  z-index: 10000;
  display: flex;
  flex-direction: column;
  gap: 5px;
  width: 270px;
  padding: 12px;
  border: 1px solid var(--border-color);
  border-radius: var(--radius-md);
  background: var(--bg-primary);
  color: var(--text-primary);
  box-shadow: var(--shadow-lg);
  font-size: var(--text-sm);
  pointer-events: none;
  overflow-wrap: anywhere;
}
@keyframes update-spin {
  to {
    transform: rotate(270deg);
  }
}
@media (prefers-reduced-motion: reduce) {
  .spinning {
    animation: none;
  }
  .ring-value {
    transition: none;
  }
}
</style>
