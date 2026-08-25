/**
 * 设置页 — 存储操作（数据目录切换 + 备份导出/导入）
 *
 * 原 SettingsView 中 handleChangeDataDir / handleExportBackup / handleImportBackup 逻辑。
 * 数据目录切换后需要刷新表单缓冲与「未保存」快照，因此依赖 useSettingsForm 的
 * syncFormFromStore / syncSavedSnapshot，以参数形式注入。
 */
import { ref } from "vue";
import { useSettingsStore } from "@/stores/settings";
import { settingsApi } from "../api";
import { todayString } from "@/utils/date";

export interface BackupActionsDeps {
  syncFormFromStore: () => void;
  syncSavedSnapshot: () => void;
}

export function useBackupActions(deps: BackupActionsDeps) {
  const settingsStore = useSettingsStore();

  // ── 数据目录切换 ──
  const changingDir = ref(false);
  const dirChangeMsg = ref<string | null>(null);
  const dirChangeError = ref(false);

  async function handleChangeDataDir() {
    dirChangeMsg.value = null;
    dirChangeError.value = false;

    let selected: string | null = null;
    try {
      const { open } = await import("@tauri-apps/plugin-dialog");
      const result = await open({ directory: true, multiple: false });
      selected = typeof result === "string" ? result : null;
    } catch (e) {
      dirChangeMsg.value = `打开目录对话框失败：${e instanceof Error ? e.message : String(e)}`;
      dirChangeError.value = true;
      return;
    }

    if (!selected) return;

    changingDir.value = true;
    try {
      const msg = await settingsApi.changeDataDirectory(selected);
      dirChangeMsg.value = msg;
      dirChangeError.value = false;
      await settingsStore.load();
      deps.syncFormFromStore();
      deps.syncSavedSnapshot();
    } catch (e) {
      dirChangeMsg.value = e instanceof Error ? e.message : String(e);
      dirChangeError.value = true;
    } finally {
      changingDir.value = false;
    }
  }

  // ── 数据备份 / 导出 / 导入 ──
  const exporting = ref(false);
  const importing = ref(false);
  const backupMsg = ref<string | null>(null);
  const backupError = ref(false);

  async function handleExportBackup() {
    backupMsg.value = null;
    backupError.value = false;

    let dest: string | null = null;
    try {
      const { save } = await import("@tauri-apps/plugin-dialog");
      const result = await save({
        defaultPath: `StudyAgent-backup-${todayString()}.zip`,
        filters: [{ name: "ZIP", extensions: ["zip"] }],
      });
      dest = typeof result === "string" ? result : null;
    } catch (e) {
      backupMsg.value = `打开保存对话框失败：${e instanceof Error ? e.message : String(e)}`;
      backupError.value = true;
      return;
    }
    if (!dest) return;

    exporting.value = true;
    try {
      const count = await settingsApi.exportBackup(dest, false);
      backupMsg.value = `导出成功：共 ${count} 个文件，已保存到 ${dest}`;
      backupError.value = false;
    } catch (e) {
      backupMsg.value = `导出失败：${e instanceof Error ? e.message : String(e)}`;
      backupError.value = true;
    } finally {
      exporting.value = false;
    }
  }

  async function handleImportBackup() {
    backupMsg.value = null;
    backupError.value = false;

    let selected: string | null = null;
    try {
      const { open } = await import("@tauri-apps/plugin-dialog");
      const result = await open({
        multiple: false,
        filters: [{ name: "ZIP", extensions: ["zip"] }],
      });
      selected = typeof result === "string" ? result : null;
    } catch (e) {
      backupMsg.value = `打开文件对话框失败：${e instanceof Error ? e.message : String(e)}`;
      backupError.value = true;
      return;
    }
    if (!selected) return;

    // M13：覆盖式导入前二次确认（原数据会自动备份到 bak 目录，但需明确提醒）
    if (!window.confirm("导入备份将覆盖当前全部数据（原数据会自动备份到 bak 目录，可恢复）。确定继续？")) {
      return;
    }

    importing.value = true;
    try {
      const summary = await settingsApi.importBackup(selected);
      backupMsg.value = `导入成功：恢复 ${summary.files_restored} 个文件。原数据已备份到 ${summary.backup_dir}，重启应用后生效。`;
      backupError.value = false;
    } catch (e) {
      backupMsg.value = `导入失败：${e instanceof Error ? e.message : String(e)}`;
      backupError.value = true;
    } finally {
      importing.value = false;
    }
  }

  return {
    changingDir,
    dirChangeMsg,
    dirChangeError,
    handleChangeDataDir,
    exporting,
    importing,
    backupMsg,
    backupError,
    handleExportBackup,
    handleImportBackup,
  };
}
