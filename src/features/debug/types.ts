/**
 * 调试页 — 共享类型定义
 */
import type { AIProviderConfig } from "@/types";

/** 诊断测试的统一状态 */
export type TestStatus = "idle" | "loading" | "success" | "error";

/** 诊断测试结果容器（State / Plan / Review / Dashboard / Settings 探测共用） */
export interface TestResult<T> {
  status: TestStatus;
  data: T | null;
  error: string | null;
}

/** 数据目录下的条目 */
export interface DirEntry {
  name: string;
  path: string;
  isDirectory: boolean;
}

/** 数据目录检查项 */
export interface DirCheck {
  name: string;
  label: string;
  /** null = 未检测 */
  exists: boolean | null;
  loading: boolean;
  error: string | null;
  entries: DirEntry[];
}

/** AI Provider 单条诊断状态 */
export interface ProviderTestState {
  provider: AIProviderConfig;
  status: TestStatus;
  message: string;
}
