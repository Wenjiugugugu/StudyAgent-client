/**
 * 调试页 — 数据目录检查逻辑（原 DebugView「数据文件检查」区块）
 *
 * 通过后端 Rust 命令（debug_list_dir / debug_read_file）绕开前端 fs 插件
 * 的作用域限制；传相对路径（"state"/"plan" 等），后端 resolve_debug_path
 * 会拒绝绝对路径（H4）。
 */
import { ref } from "vue";
import { debugApi } from "../api";
import { joinPath } from "../utils/formatters";
import type { DirCheck } from "../types";

/** 数据目录检查项的默认清单 */
const DEFAULT_DIRS: Array<Omit<DirCheck, "exists" | "loading" | "error" | "entries">> = [
  { name: "state", label: "state（状态）" },
  { name: "plan", label: "plan（计划）" },
  { name: "records", label: "records（复盘记录）" },
  { name: "config", label: "config（配置）" },
];

export function useDataDirectoryCheck(dataDir: () => string) {
  const dataDirs = ref<DirCheck[]>(DEFAULT_DIRS.map((d) => ({
    ...d,
    exists: null,
    loading: false,
    error: null,
    entries: [],
  })));
  const expandedDir = ref<string | null>(null);
  const fileContent = ref<{ dir: string; name: string; content: string; error: string | null } | null>(null);
  const loadingFile = ref(false);

  async function checkDataDirs() {
    if (!dataDir()) return;
    for (const dir of dataDirs.value) {
      dir.loading = true;
      dir.error = null;
      try {
        // H4：传相对路径（dir.name 即 "state"/"plan" 等），后端 resolve_debug_path 拒绝绝对路径
        const entries = await debugApi.debugListDir(dir.name);
        dir.exists = true;
        dir.entries = entries.map((e) => ({
          name: e.name,
          path: joinPath(dataDir(), dir.name, e.name),
          isDirectory: e.is_directory,
        }));
      } catch (e) {
        dir.exists = false;
        dir.error = e instanceof Error ? e.message : String(e);
        dir.entries = [];
      } finally {
        dir.loading = false;
      }
    }
  }

  function toggleDir(name: string) {
    expandedDir.value = expandedDir.value === name ? null : name;
  }

  async function viewFile(dirName: string, entry: { name: string; isDirectory: boolean }) {
    if (entry.isDirectory) return;
    loadingFile.value = true;
    fileContent.value = null;
    try {
      // H4：传相对路径（dirName 为 "state"/"plan" 等），避免后端拒绝绝对路径
      const content = await debugApi.debugReadFile(joinPath(dirName, entry.name));
      fileContent.value = { dir: dirName, name: entry.name, content, error: null };
    } catch (e) {
      fileContent.value = {
        dir: dirName,
        name: entry.name,
        content: "",
        error: e instanceof Error ? e.message : String(e),
      };
    } finally {
      loadingFile.value = false;
    }
  }

  return {
    dataDirs,
    expandedDir,
    fileContent,
    loadingFile,
    checkDataDirs,
    toggleDir,
    viewFile,
  };
}
