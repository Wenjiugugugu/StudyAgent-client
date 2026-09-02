<script setup lang="ts">
/**
 * 工作台 — 发现新版本弹窗
 *
 * 由原 DashboardView 中的更新弹窗拆分而来：展示新版本信息、选择安装包、
 * 下载进度与安装操作，全部状态来自 updateStore。
 *
 * 强制更新模式（当前版本被远端策略禁用）：弹窗不可关闭，仅可"立即更新"
 * 或"退出应用"，普通更新的"3 天后再提醒"与"查看详情"按钮隐藏。
 */
import { computed } from "vue";
import { useRouter } from "vue-router";
import { useUpdateStore } from "@/stores/update";
import * as api from "@/api";
import Modal from "@/components/ui/Modal.vue";
import Button from "@/components/ui/Button.vue";
import ProgressBar from "@/components/ui/ProgressBar.vue";
import MarkdownText from "@/components/MarkdownText.vue";
import { Package, Download, HardDriveDownload, AlertTriangle, LogOut } from "lucide-vue-next";

const router = useRouter();
const updateStore = useUpdateStore();

const forceUpdate = computed(() => updateStore.forceUpdate);
const forceReason = computed(() => updateStore.updateResult?.force_update_reason ?? "");
</script>

<template>
  <Modal
    :open="updateStore.showUpdateModal"
    :title="forceUpdate ? '必须更新' : '发现新版本'"
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
        <span>{{
          forceReason || "当前版本存在已知问题，必须更新到最新版本后才能继续使用。"
        }}</span>
      </div>

      <p class="update-modal-version">
        新版本：<strong>v{{ updateStore.updateResult.latest_version }}</strong>
        <span v-if="updateStore.updateResult.release_name" class="update-modal-name">
          — {{ updateStore.updateResult.release_name }}
        </span>
      </p>

      <p v-if="!forceUpdate" class="update-modal-tip">建议保持应用更新到最新版本，以便第一时间体验新功能与各类修复。</p>

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
            :value="updateStore.downloadProgress.percent || 0"
            :max="100"
          />
          <span class="update-progress-text">
            {{ updateStore.downloadProgress.percent?.toFixed(0) ?? 0 }}%
          </span>
        </div>

        <p v-if="updateStore.downloadError" class="update-download-error">
          {{ updateStore.downloadError }}
        </p>
      </div>
    </div>

    <template #footer>
      <!-- 非强制：3 天后再提醒（写入静默标记） -->
      <Button
        v-if="!forceUpdate"
        variant="ghost"
        size="sm"
        @click="updateStore.dismissUpdate()"
      >
        3 天后再提醒
      </Button>

      <!-- 非强制：查看详情（跳转设置页，不写入静默标记） -->
      <Button
        v-if="!forceUpdate"
        variant="ghost"
        size="sm"
        @click="router.push('/settings#settings-update')"
      >
        查看详情
      </Button>

      <!-- 强制：退出应用（唯一不更新的出口） -->
      <Button
        v-if="forceUpdate"
        variant="ghost"
        size="sm"
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
        <span>下载安装包</span>
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
