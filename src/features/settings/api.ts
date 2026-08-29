/**
 * 设置页 — 领域化 API 层
 *
 * 仅 re-export 设置页用到的 API 函数，避免各组件/composable 直接耦合全局 @/api。
 * 命令名与后端保持一致，未做任何改动，仅做转发。
 */
import {
  getState,
  updateSubjectTextbook,
  changeDataDirectory,
  exportBackup,
  importBackup,
  saveBackgroundImage,
  deleteBackgroundImage,
  testAIProvider,
  listAIModels,
  getAutostart,
  setAutostart,
  getCloseAction,
  setCloseAction,
  getDidaTokenStatus,
  setDidaToken,
  syncDidaNow,
  listDidaProjects,
} from "@/api";

export type { CloseAction } from "@/api";
export type { DidaProject } from "@/api";

export const settingsApi = {
  getState,
  updateSubjectTextbook,
  changeDataDirectory,
  exportBackup,
  importBackup,
  saveBackgroundImage,
  deleteBackgroundImage,
  testAIProvider,
  listAIModels,
  getAutostart,
  setAutostart,
  getCloseAction,
  setCloseAction,
  getDidaTokenStatus,
  setDidaToken,
  syncDidaNow,
  listDidaProjects,
};
