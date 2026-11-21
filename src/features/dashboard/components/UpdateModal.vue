<script setup lang="ts">
/**
 * 全局强制更新提示
 *
 * 当前版本被禁用时在任意页面展示，下载及安装自动进行，失败后允许重试。
 */
import { computed } from "vue";
import { useUpdateStore } from "@/stores/update";
import * as api from "@/api";
import Modal from "@/components/ui/Modal.vue";
import Button from "@/components/ui/Button.vue";
import ProgressBar from "@/components/ui/ProgressBar.vue";
import MarkdownText from "@/components/MarkdownText.vue";
import { Package, Download, HardDriveDownload, AlertTriangle, LogOut } from "lucide-vue-next";

const updateStore = useUpdateStore();

const forceUpdate = computed(() => updateStore.forceUpdate);
const forceReason = computed(() => updateStore.updateResult?.force_update_reason ?? "");
</script>

<template>
  <Modal
    :open="updateStore.showUpdateModal"
    :title="forceUpdate ? '需要更新' : '发现新版本'"
    :show-close="!forceUpdate"
    :close-on-overlay="false"
    :close-on-esc="!forceUpdate"
    :width="520"
    @close="updateStore.dismissUpdate()"
  >
    <div v-if="updateStore.updateResult" class="update-modal-body">
      <!-- 强制更新警告横幅 -->
      <div v-if="forceUpdate" class="update-modal-force-banner">
        <AlertTriangle :size="16" />
        <span>{{ forceReason || "当前版本存在已知问题，必须更新到最新版本后才能继续使用。" }}</span>
      </div>
      <p v-if="forceUpdate" class="update-modal-tip">
        更新包将自动下载，校验通过后保存复盘草稿、暂停计时并安装更新。
      </p>

      <p class="update-modal-version">
        新版本：<strong>v{{ updateStore.updateResult.latest_version }}</strong>
        <span v-if="updateStore.updateResult.release_name" class="update-modal-name">
          — {{ updateStore.updateResult.release_name }}
        </span>
      </p>

      <p v-if="!forceUpdate" class="update-modal-tip">
        建议保持应用更新到最新版本，以便第一时间体验新功能与各类修复。
      </p>

      <!-- Release notes -->
      <div v-if="updateStore.updateResult.release_notes" class="update-modal-notes">
        <MarkdownText :content="updateStore.updateResult.release_notes" />
      </div>

      <!-- 安装包选择 + 下载 -->
      <div class="update-modal-actions">
        <div v-if="updateStore.updateResult.assets.length > 1" class="update-modal-assets">
          <button
            v-for="asset in updateStore.updateResult.assets"
            :key="asset.download_url"
            class="update-asset-btn"
            :disabled="updateStore.busy || !!updateStore.downloadedFilePath"
            :class="{ active: updateStore.selectedAsset?.download_url === asset.download_url }"
            @click="updateStore.selectedAsset = asset"
          >
            <Package :size="13" />
            <span>{{ updateStore.assetLabel(asset.kind) }}</span>
            <span class="update-asset-size">{{ updateStore.formatSize(asset.size) }}</span>
          </button>
        </div>

        <!-- 下载进度条 -->
        <div
          v-if="updateStore.downloadState === 'downloading' && updateStore.downloadProgress"
          class="update-download-progress"
        >
          <ProgressBar
            v-if="updateStore.downloadProgress.total > 0"
            :value="updateStore.downloadProgress.percent || 0"
            :max="100"
          />
          <span class="update-progress-text">
            {{
              updateStore.downloadProgress.total > 0
                ? `${updateStore.downloadProgress.percent.toFixed(0)}%`
                : "正在下载…"
            }}
          </span>
        </div>

        <p v-if="updateStore.downloadError" class="update-download-error">
          {{ updateStore.downloadError }}
        </p>
      </div>
    </div>

    <template #footer>
      <!-- 强制：退出应用（唯一不更新的出口） -->
      <Button
        v-if="forceUpdate"
        variant="ghost"
        size="sm"
        :disabled="updateStore.installing"
        @click="api.quitApp()"
      >
        <LogOut :size="13" />
        退出应用
      </Button>

      <Button
        v-if="updateStore.downloadState === 'idle' || updateStore.downloadState === 'error'"
        variant="primary"
        size="sm"
        :disabled="!updateStore.selectedAsset"
        @click="updateStore.handleDownload()"
      >
        <Download :size="13" />
        <span>{{ updateStore.downloadedFilePath ? "重试安装" : "重试下载" }}</span>
      </Button>

      <Button
        v-if="updateStore.downloadState === 'downloaded'"
        variant="primary"
        size="sm"
        :loading="updateStore.installing"
        @click="updateStore.handleInstall()"
      >
        <HardDriveDownload :size="13" />
        <span>立即安装</span>
      </Button>

      <Button
        v-if="updateStore.downloadState === 'downloading'"
        variant="secondary"
        size="sm"
        disabled
      >
        <span>下载中…</span>
      </Button>

      <Button v-if="updateStore.preparing" variant="secondary" size="sm" disabled
        ><span>正在等待当前操作并保存现场…</span></Button
      >

      <Button
        v-if="updateStore.downloadState === 'installing'"
        variant="secondary"
        size="sm"
        disabled
      >
        <span>安装中…</span>
      </Button>
    </template>
  </Modal>
</template>

<style scoped>
.update-modal-force-banner {
  display: flex;
  align-items: flex-start;
  gap: 8px;
  padding: 10px 12px;
  margin-bottom: 12px;
  border-radius: 8px;
  background: rgba(239, 68, 68, 0.1);
  border: 1px solid rgba(239, 68, 68, 0.35);
  color: #b91c1c;
  font-size: 13px;
  line-height: 1.5;
}

.update-modal-force-banner :deep(svg) {
  flex-shrink: 0;
  margin-top: 1px;
}
</style>
