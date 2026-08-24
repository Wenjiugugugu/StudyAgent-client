/**
 * 调试页 — 状态徽章 / 文案工具
 */
import type { TestStatus } from "../types";

/** 测试状态 → Badge variant */
export function statusBadge(status: TestStatus): "success" | "danger" | "info" | "default" {
  if (status === "success") return "success";
  if (status === "error") return "danger";
  if (status === "loading") return "info";
  return "default";
}

/** 测试状态 → 中文文案 */
export function statusLabel(status: TestStatus): string {
  const map: Record<string, string> = {
    idle: "待测试",
    loading: "测试中",
    success: "成功",
    error: "失败",
  };
  return map[status] ?? status;
}

/** AI 调用状态 → Badge variant */
export function aiCallStatusBadge(status: "pending" | "success" | "error"): "info" | "success" | "danger" {
  if (status === "success") return "success";
  if (status === "error") return "danger";
  return "info";
}

/** AI 调用状态 → 中文文案 */
export function aiCallStatusLabel(status: "pending" | "success" | "error"): string {
  if (status === "success") return "成功";
  if (status === "error") return "失败";
  return "进行中";
}

/** 用量记录状态 → Badge variant */
export function usageStatusBadge(status: string): "success" | "danger" | "info" {
  if (status === "success") return "success";
  if (status === "error") return "danger";
  return "info";
}

/** 用量记录状态 → 中文文案 */
export function usageStatusLabel(status: string): string {
  if (status === "success") return "成功";
  if (status === "error") return "失败";
  return "未知";
}
