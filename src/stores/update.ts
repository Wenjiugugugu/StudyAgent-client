import { defineStore } from "pinia";
import { computed, ref } from "vue";
import * as api from "@/api";
import type { UpdateAsset, UpdateCheckResult, DownloadProgress } from "@/types";

export type DownloadState = "idle" | "downloading" | "downloaded" | "installing" | "error";

export const useUpdateStore = defineStore("update", () => {
  // ── 检查更新状态 ──
  const checking = ref(false);
  const updateResult = ref<UpdateCheckResult | null>(null);
  const updateError = ref<string | null>(null);

  // ── 下载/安装状态 ──
  const downloadState = ref<DownloadState>("idle");
  const downloadProgress = ref<DownloadProgress | null>(null);
  const downloadedFilePath = ref<string | null>(null);
  const downloadError = ref<string | null>(null);
  const selectedAsset = ref<UpdateAsset | null>(null);
  const installing = ref(false);

  // ── 启动检查标记（避免重复检查）──
  let startupChecked = false;
  let progressUnlisten: (() => void) | null = null;

  // ── 首页更新弹窗 ──
  const showUpdateModal = ref(false);

  const hasUpdate = computed(() => updateResult.value?.has_update ?? false);

  // 当前选中的安装包（默认 Inno Setup 生成的 Windows 安装包）
  const preferredAsset = computed(() => {
    const assets = updateResult.value?.assets ?? [];
    if (assets.length === 0) return null;
    return (
      assets.find((a) => a.kind === "inno") ??
      assets.find((a) => a.kind === "nsis") ??
      assets.find((a) => a.kind === "exe") ??
      assets.find((a) => a.kind === "msi") ??
      assets[0]
    );
  });

  // 安装包类型展示名
  function assetLabel(kind: string): string {
    switch (kind) {
      case "inno":
        return "Windows 安装包（推荐）";
      case "nsis":
        return "Windows 安装包（旧版）";
      case "msi":
        return "MSI 安装包";
      case "exe":
        return "可执行文件";
      default:
        return "安装包";
    }
  }

  // 文件大小格式化
  function formatSize(bytes: number): string {
    if (!bytes) return "—";
    if (bytes < 1024) return `${bytes} B`;
    if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
    return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
  }

  async function ensureProgressListener() {
    if (progressUnlisten) return;
    progressUnlisten = await api.onDownloadProgress((p) => {
      downloadProgress.value = p;
    });
  }

  async function checkUpdate() {
    if (checking.value) return;
    checking.value = true;
    updateError.value = null;
    updateResult.value = null;
    downloadState.value = "idle";
    downloadProgress.value = null;
    downloadedFilePath.value = null;
    downloadError.value = null;
    selectedAsset.value = null;

    try {
      const result = await api.checkForUpdates();
      updateResult.value = result;
      selectedAsset.value = preferredAsset.value;
    } catch (e) {
      updateError.value = e instanceof Error ? e.message : String(e);
    } finally {
      checking.value = false;
    }
  }

  /** 启动时检查更新（仅一次），发现新版本时弹出首页弹窗 */
  async function checkOnStartup() {
    if (startupChecked) return;
    startupChecked = true;
    try {
      await ensureProgressListener();
      await checkUpdate();
      if (hasUpdate.value) {
        showUpdateModal.value = true;
      }
    } catch (e) {
      console.warn("[Update] 启动检查更新失败:", e);
    }
  }

  async function handleDownload() {
    const asset = selectedAsset.value;
    if (!asset) return;

    downloadState.value = "downloading";
    downloadProgress.value = null;
    downloadError.value = null;
    downloadedFilePath.value = null;

    try {
      await ensureProgressListener();
      const path = await api.downloadUpdate(asset.download_url, asset.name, asset.sha256);
      downloadedFilePath.value = path;
      downloadState.value = "downloaded";
    } catch (e) {
      downloadError.value = e instanceof Error ? e.message : String(e);
      downloadState.value = "error";
    }
  }

  async function handleInstall() {
    if (!downloadedFilePath.value) return;
    installing.value = true;
    downloadState.value = "installing";
    try {
      await api.installUpdate(downloadedFilePath.value);
    } catch (e) {
      downloadError.value = e instanceof Error ? e.message : String(e);
      downloadState.value = "error";
      installing.value = false;
    }
  }

  function resetUpdate() {
    updateResult.value = null;
    updateError.value = null;
    downloadState.value = "idle";
    downloadProgress.value = null;
    downloadedFilePath.value = null;
    downloadError.value = null;
    selectedAsset.value = null;
  }

  /** 关闭首页更新弹窗（不重置下载状态） */
  function dismissUpdate() {
    showUpdateModal.value = false;
  }

  return {
    checking,
    updateResult,
    updateError,
    downloadState,
    downloadProgress,
    downloadedFilePath,
    downloadError,
    selectedAsset,
    installing,
    hasUpdate,
    showUpdateModal,
    preferredAsset,
    checkUpdate,
    checkOnStartup,
    handleDownload,
    handleInstall,
    resetUpdate,
    dismissUpdate,
    assetLabel,
    formatSize,
  };
});
