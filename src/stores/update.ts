import { defineStore } from "pinia";
import { computed, ref } from "vue";
import * as api from "@/api";
import type { UpdateAsset, UpdateCheckResult, DownloadProgress } from "@/types";
import { useFocusStore } from "@/stores/focus";
import {
  prepareViewsForUpdate,
  taskTimerPauseVersion,
  updatePreparationActive,
} from "@/utils/updatePreparation";

export type DownloadState =
  "idle" | "downloading" | "downloaded" | "preparing" | "installing" | "error";

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
  let progressListening: Promise<void> | null = null;
  let checkInterval: ReturnType<typeof setInterval> | null = null;
  const downloadedVersion = ref("");

  // ── 首页更新弹窗 ──
  const showUpdateModal = ref(false);

  const hasUpdate = computed(() => updateResult.value?.has_update ?? false);

  /** 是否强制更新（当前版本被远端策略禁用）——强制更新无视 3 天静默期 */
  const forceUpdate = computed(() => updateResult.value?.force_update ?? false);
  const preparing = computed(() => downloadState.value === "preparing");
  const busy = computed(() =>
    ["downloading", "preparing", "installing"].includes(downloadState.value)
  );
  const showIndicator = computed(() => hasUpdate.value || busy.value || !!downloadError.value);
  const targetVersion = computed(
    () => downloadedVersion.value || updateResult.value?.latest_version || ""
  );

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
    progressListening ??= api
      .onDownloadProgress((p) => {
        if (downloadState.value === "downloading") downloadProgress.value = p;
      })
      .then((unlisten) => {
        progressUnlisten = unlisten;
      })
      .finally(() => {
        progressListening = null;
      });
    await progressListening;
  }

  async function checkUpdate() {
    if (checking.value || busy.value) return;
    checking.value = true;
    updateError.value = null;
    try {
      const result = await api.checkForUpdates();
      checking.value = false;
      // 检查失败不会撤销已确认的禁用策略或清空已下载的包。
      if (!result.has_update && !result.latest_version) {
        updateError.value = result.message;
        return;
      }
      const oldAsset = selectedAsset.value;
      updateResult.value = result;
      showUpdateModal.value = forceUpdate.value;
      if (!downloadedFilePath.value) selectedAsset.value = preferredAsset.value;
      if (downloadedFilePath.value && oldAsset && result.has_update) {
        const sameAsset = result.assets.some(
          (asset) => asset.name === oldAsset.name && asset.sha256 === oldAsset.sha256
        );
        if (!sameAsset) {
          downloadedFilePath.value = null;
          downloadedVersion.value = "";
          downloadState.value = "idle";
          downloadProgress.value = null;
          selectedAsset.value = preferredAsset.value;
        }
      }
      if (hasUpdate.value) {
        if (downloadedFilePath.value) {
          // 自动安装失败后不在定时检查中反复退出；留给用户重试。
          if (forceUpdate.value && downloadState.value === "downloaded") await handleInstall();
        } else {
          await handleDownload();
        }
      }
    } catch (e) {
      updateError.value = e instanceof Error ? e.message : String(e);
    } finally {
      checking.value = false;
    }
  }

  /** 启动及每小时自动检查；普通更新仅通过侧边栏显示，不弹窗打断。 */
  async function checkOnStartup() {
    if (startupChecked) return;
    startupChecked = true;
    try {
      checkInterval = setInterval(
        () => {
          void checkUpdate();
        },
        60 * 60 * 1000
      );
      await checkUpdate();
    } catch (e) {
      console.warn("[Update] 启动检查更新失败:", e);
    }
  }

  async function handleDownload() {
    if (busy.value) return;
    if (downloadedFilePath.value) {
      await handleInstall();
      return;
    }
    const asset = selectedAsset.value;
    if (!asset) {
      downloadError.value = "暂时没有可下载的更新包，请重新检查更新或稍后重试";
      downloadState.value = "error";
      return;
    }

    downloadState.value = "downloading";
    downloadProgress.value = null;
    downloadError.value = null;
    downloadedFilePath.value = null;

    // 防御：后端已过滤缺 SHA-256 的资源，正常情况下走到这里 asset.sha256 必有值；
    // 若仍缺失（如数据异常），给出明确提示而不是把 null 传给后端被笼统拒绝。
    if (!asset.sha256) {
      downloadError.value =
        "该安装包缺少校验信息（SHA-256），暂时无法下载，请尝试重新检查更新或稍后再试";
      downloadState.value = "error";
      return;
    }

    try {
      await ensureProgressListener();
      const path = await api.downloadUpdate(asset.download_url, asset.name, asset.sha256);
      downloadedFilePath.value = path;
      downloadedVersion.value = updateResult.value?.latest_version ?? "";
      downloadState.value = "downloaded";
      if (forceUpdate.value) await handleInstall();
    } catch (e) {
      downloadError.value = e instanceof Error ? e.message : String(e);
      downloadState.value = "error";
    }
  }

  async function handleInstall() {
    if (!downloadedFilePath.value || installing.value) return;
    installing.value = true;
    downloadError.value = null;
    downloadState.value = "preparing";
    updatePreparationActive.value = true;
    try {
      await prepareViewsForUpdate();
      await useFocusStore().prepareForUpdate();
      await api.prepareUpdateTimers();
      taskTimerPauseVersion.value++;
      downloadState.value = "installing";
      await api.installUpdate(downloadedFilePath.value);
    } catch (e) {
      downloadError.value = e instanceof Error ? e.message : String(e);
      downloadState.value = "error";
      updatePreparationActive.value = false;
      installing.value = false;
      if (
        /安装包不存在|安装包未通过本次运行|安装前完整性复核失败/.test(downloadError.value ?? "")
      ) {
        downloadedFilePath.value = null;
        downloadedVersion.value = "";
      }
    }
  }

  function resetUpdate() {
    if (busy.value || forceUpdate.value) return;
    updateResult.value = null;
    updateError.value = null;
    downloadState.value = "idle";
    downloadProgress.value = null;
    downloadedFilePath.value = null;
    downloadedVersion.value = "";
    downloadError.value = null;
    selectedAsset.value = null;
  }

  /** 关闭首页更新弹窗（强制更新模式下无效）。
   *  非强制关闭时记录 3 天静默期，避免打扰用户。 */
  function dismissUpdate() {
    if (forceUpdate.value) return; // 强制更新不可关闭
    showUpdateModal.value = false;
  }

  function stopAutomaticChecks() {
    if (checkInterval) clearInterval(checkInterval);
    checkInterval = null;
    progressUnlisten?.();
    progressUnlisten = null;
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
    preparing,
    busy,
    showIndicator,
    targetVersion,
    hasUpdate,
    forceUpdate,
    showUpdateModal,
    preferredAsset,
    checkUpdate,
    checkOnStartup,
    handleDownload,
    handleInstall,
    resetUpdate,
    dismissUpdate,
    stopAutomaticChecks,
    assetLabel,
    formatSize,
  };
});
