<script setup lang="ts">
import { computed } from "vue";
import Card from "@/components/ui/Card.vue";
import Button from "@/components/ui/Button.vue";
import MarkdownText from "@/components/MarkdownText.vue";
import { useUpdateStore } from "@/stores/update";
import { useAppVersion } from "@/version";
import { RefreshCw, AlertCircle, CheckCircle, Download, Package, HardDriveDownload } from "lucide-vue-next";

const updateStore = useUpdateStore();
const { version } = useAppVersion();

// 从 store 获取响应式状态与方法（模板中直接引用这些名称，保持与原实现兼容）
const checking = computed(() => updateStore.checking);
const updateResult = computed(() => updateStore.updateResult);
const updateError = computed(() => updateStore.updateError);
const downloadState = computed(() => updateStore.downloadState);
const downloadProgress = computed(() => updateStore.downloadProgress);
const downloadError = computed(() => updateStore.downloadError);
const selectedAsset = computed({
  get: () => updateStore.selectedAsset,
  set: (v) => { updateStore.selectedAsset = v; },
});
const installing = computed(() => updateStore.installing);

// 代理方法
const assetLabel = (kind: string) => updateStore.assetLabel(kind);
const formatSize = (bytes: number) => updateStore.formatSize(bytes);
const handleCheckUpdate = () => updateStore.checkUpdate();
const handleDownload = () => updateStore.handleDownload();
const handleInstall = () => updateStore.handleInstall();
const resetUpdate = () => updateStore.resetUpdate();
</script>

<template>
  <!-- 检查更新 -->
  <Card id="settings-update" padding="lg" class="settings-section">
    <div class="section-head">
      <div class="section-title">
        <RefreshCw :size="18" />
        <span>检查更新</span>
      </div>
      <div class="current-version">
        <span class="version-label-text">当前版本</span>
        <span class="version-value text-mono">{{ version }}</span>
      </div>
    </div>

    <!-- 初始状态：检查按钮 -->
    <div v-if="!updateResult && !checking && !updateError" class="update-idle">
      <Button
        variant="primary"
        size="md"
        :loading="checking"
        @click="handleCheckUpdate"
      >
        <RefreshCw :size="14" />
        <span>检查更新</span>
      </Button>
      <p class="field-hint">
        点击检查是否有新版本，发现新版本后可在应用内下载并安装
      </p>
    </div>

    <!-- 检查中 -->
    <div v-else-if="checking" class="update-checking">
      <RefreshCw :size="16" class="spin" />
      <span>正在检查更新...</span>
    </div>

    <!-- 检查失败 -->
    <div v-else-if="updateError" class="update-error">
      <AlertCircle :size="16" />
      <span>检查更新失败：{{ updateError }}</span>
      <Button variant="secondary" size="sm" @click="handleCheckUpdate">
        <RefreshCw :size="13" />
        <span>重试</span>
      </Button>
    </div>

    <!-- 检查结果：无更新 -->
    <div v-else-if="updateResult && !updateResult.has_update" class="update-no-update">
      <div class="update-status-row">
        <CheckCircle :size="16" class="status-icon-ok" />
        <span class="status-text">{{ updateResult.message }}</span>
      </div>
      <Button variant="secondary" size="sm" @click="handleCheckUpdate">
        <RefreshCw :size="13" />
        <span>重新检查</span>
      </Button>
    </div>

    <!-- 检查结果：有更新 -->
    <div v-else-if="updateResult && updateResult.has_update" class="update-available">
      <!-- 版本信息 -->
      <div class="update-info">
        <div class="update-info-row">
          <span class="info-label">最新版本</span>
          <span class="info-value text-mono highlight">{{ updateResult.latest_version }}</span>
        </div>
        <div v-if="updateResult.release_name" class="update-info-row">
          <span class="info-label">Release</span>
          <span class="info-value">{{ updateResult.release_name }}</span>
        </div>
        <div v-if="updateResult.published_at" class="update-info-row">
          <span class="info-label">发布时间</span>
          <span class="info-value text-mono">
            {{ updateResult.published_at.replace('T', ' ').replace('Z', ' UTC') }}
          </span>
        </div>
      </div>

      <!-- Release notes -->
      <div v-if="updateResult.release_notes" class="release-notes-block">
        <div class="release-notes-head">更新说明</div>
        <div class="release-notes-content"><MarkdownText :content="updateResult.release_notes" /></div>
      </div>

      <!-- 安装包选择 -->
      <div v-if="updateResult.assets.length > 0" class="asset-selector">
        <label class="form-label">安装包</label>
        <div class="asset-options">
          <button
            v-for="asset in updateResult.assets"
            :key="asset.download_url"
            class="asset-option"
            :class="{ active: selectedAsset?.download_url === asset.download_url }"
            @click="selectedAsset = asset"
          >
            <Package :size="14" />
            <span class="asset-name">{{ assetLabel(asset.kind) }}</span>
            <span class="asset-size">{{ formatSize(asset.size) }}</span>
          </button>
        </div>
      </div>

      <!-- 下载进度 -->
      <div v-if="downloadState === 'downloading' && downloadProgress" class="download-progress-block">
        <div class="progress-head">
          <span class="progress-label">正在下载{{ selectedAsset ? assetLabel(selectedAsset.kind) : '安装包' }}</span>
          <span class="progress-percent">{{ downloadProgress.percent.toFixed(1) }}%</span>
        </div>
        <div class="progress-bar-track">
          <div
            class="progress-bar-fill"
            :style="{ width: `${downloadProgress.percent}%` }"
          ></div>
        </div>
        <div class="progress-detail">
          <span>{{ formatSize(downloadProgress.downloaded) }}</span>
          <span v-if="downloadProgress.total > 0">/ {{ formatSize(downloadProgress.total) }}</span>
        </div>
      </div>

      <!-- 下载完成 -->
      <div v-if="downloadState === 'downloaded'" class="download-complete">
        <CheckCircle :size="16" class="status-icon-ok" />
        <span>下载完成，正在启动安装程序…</span>
      </div>

      <!-- 安装中 -->
      <div v-if="downloadState === 'installing'" class="installing-state">
        <RefreshCw :size="16" class="spin" />
        <span>正在启动安装程序，应用即将退出...</span>
      </div>

      <!-- 下载/安装失败 -->
      <div v-if="downloadState === 'error' && downloadError" class="download-error-msg">
        <AlertCircle :size="14" />
        <span>{{ downloadError }}</span>
      </div>

      <!-- 操作按钮 -->
      <div class="update-actions">
        <Button
          v-if="downloadState === 'idle' || downloadState === 'error'"
          variant="primary"
          size="md"
          :disabled="!selectedAsset"
          @click="handleDownload"
        >
          <Download :size="14" />
          <span>下载安装包</span>
        </Button>
        <Button
          v-if="downloadState === 'downloaded'"
          variant="primary"
          size="md"
          :loading="installing"
          @click="handleInstall"
        >
          <HardDriveDownload :size="14" />
          <span>立即安装</span>
        </Button>
        <Button
          v-if="downloadState === 'downloading'"
          variant="secondary"
          size="md"
          disabled
        >
          <span>下载中...</span>
        </Button>
        <Button variant="ghost" size="md" @click="resetUpdate">
          <span>稍后再说</span>
        </Button>
      </div>
    </div>
  </Card>
</template>
