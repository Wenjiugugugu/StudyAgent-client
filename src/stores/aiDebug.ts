/**
 * AI 调用调试记录器
 *
 * 用于在「调试」视图中查看 AI 调用历史：
 *   - 调用时间、命令名、参数、响应或错误、耗时
 *   - 保留最近 50 条记录，避免内存占用过大
 *   - 同时通过 console.info 输出到「日志」面板
 *   - 持久化到 localStorage，重启后不丢失
 */
import { defineStore } from "pinia";
import { ref, computed, watch } from "vue";

/** localStorage 存储键 */
const STORAGE_KEY = "studyagent-ai-debug-records";
/** 持久化记录上限（localStorage 容量有限，需要控制） */
const MAX_PERSISTED_RECORDS = 30;

/** 单条 AI 调用记录 */
export interface AiCallRecord {
  /** 唯一 ID（自增计数器） */
  id: number;
  /** 调用命令名（如 generate_daily_plan、chat） */
  command: string;
  /** 简短标签（中文，用于展示） */
  label: string;
  /** 调用时间 ISO 字符串 */
  timestamp: string;
  /** 调用耗时（毫秒），未完成时为 null */
  durationMs: number | null;
  /** 状态：进行中 / 成功 / 失败 */
  status: "pending" | "success" | "error";
  /** 请求参数（已深拷贝并裁剪） */
  request: unknown;
  /** 响应数据（成功时）；错误信息（失败时） */
  response: unknown;
  /** 错误信息（仅失败时） */
  error: string | null;
}

/** 最多保留的记录数 */
const MAX_RECORDS = 50;
/** 单个字段字符串最大长度（超过截断） */
const MAX_STRING_LEN = 4000;

/** 自增 ID 计数器（进程内） */
let nextId = 1;

/** 需要脱敏的敏感字段名（小写匹配） */
const SENSITIVE_KEYS = new Set([
  "api_key",
  "apikey",
  "api-key",
  "secret",
  "token",
  "authorization",
  "password",
  "access_token",
  "refresh_token",
]);

/** 对敏感字段的值进行脱敏：保留前 4 位 + 后 4 位，中间用 *** 代替 */
function maskSensitive(value: string): string {
  if (value.length <= 8) return "***";
  return value.slice(0, 4) + "***" + value.slice(-4);
}

/** 安全序列化：避免循环引用 / 过大字符串 / 敏感字段泄露 */
function safeClone(value: unknown): unknown {
  if (value === null || value === undefined) return value;
  if (typeof value === "string") {
    return value.length > MAX_STRING_LEN
      ? value.slice(0, MAX_STRING_LEN) + `…[+${value.length - MAX_STRING_LEN} 字符]`
      : value;
  }
  if (typeof value === "number" || typeof value === "boolean") return value;
  if (typeof value === "function") return "[Function]";
  try {
    // 先 JSON 序列化以剥离 Proxy / Vue ref，同时对敏感字段脱敏
    const json = JSON.stringify(value, (key, v) => {
      if (typeof v === "function") return "[Function]";
      if (typeof v === "string") {
        // 敏感字段脱敏（key 不区分大小写）
        if (SENSITIVE_KEYS.has(key.toLowerCase()) && v.length > 0) {
          return maskSensitive(v);
        }
        if (v.length > MAX_STRING_LEN) {
          return v.slice(0, MAX_STRING_LEN) + `…[+${v.length - MAX_STRING_LEN} 字符]`;
        }
      }
      return v;
    });
    return JSON.parse(json);
  } catch {
    return String(value);
  }
}

/** 从 localStorage 加载已保存的记录 */
function loadPersistedRecords(): AiCallRecord[] {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) return [];
    const parsed = JSON.parse(raw);
    if (!Array.isArray(parsed)) return [];
    // 仅恢复已完成的记录（pending 状态在重启后无意义）
    const restored = parsed.filter(
      (r: unknown): r is AiCallRecord =>
        typeof r === "object" && r !== null && "id" in r && "status" in r,
    );
    // 更新 nextId 以避免 ID 冲突
    const maxId = restored.reduce((max, r) => Math.max(max, r.id), 0);
    nextId = maxId + 1;
    return restored;
  } catch {
    return [];
  }
}

/** 将记录持久化到 localStorage（仅保存已完成的，且限制数量） */
function persistRecords(records: AiCallRecord[]) {
  try {
    const toSave = records
      .filter((r) => r.status !== "pending")
      .slice(0, MAX_PERSISTED_RECORDS);
    localStorage.setItem(STORAGE_KEY, JSON.stringify(toSave));
  } catch {
    // localStorage 满或不可用时静默忽略
  }
}

export const useAiDebugStore = defineStore("aiDebug", () => {
  const records = ref<AiCallRecord[]>(loadPersistedRecords());

  /** 最近一次调用（用于顶层快速访问） */
  const latestRecord = computed<AiCallRecord | null>(() => records.value[0] ?? null);

  /** 当前进行中的调用数（用于显示加载状态） */
  const pendingCount = computed(() => records.value.filter((r) => r.status === "pending").length);

  /** 结束回调的类型：传入状态、响应数据、错误信息 */
  type FinishFn = (status: "success" | "error", response: unknown, error: string | null) => void;

  /** 开始一次 AI 调用记录，返回用于结束记录的回调 */
  function startCall(command: string, label: string, requestArgs: unknown): FinishFn {
    const id = nextId++;
    const record: AiCallRecord = {
      id,
      command,
      label,
      timestamp: new Date().toISOString(),
      durationMs: null,
      status: "pending",
      request: safeClone(requestArgs),
      response: null,
      error: null,
    };
    records.value.unshift(record);
    if (records.value.length > MAX_RECORDS) {
      records.value = records.value.slice(0, MAX_RECORDS);
    }
    // 同时输出到 console（便于在「日志」面板查看）
    console.info(`[AI 调用] 开始 ${command} — ${label}`, requestArgs);

    const startedAt = performance.now();
    return (status: "success" | "error", response: unknown, error: string | null) => {
      const duration = Math.round(performance.now() - startedAt);
      record.durationMs = duration;
      record.status = status;
      record.response = status === "success" ? safeClone(response) : null;
      record.error = error;
      // M34：record 为 store 内 reactive 数组元素，直接修改属性已触发响应式更新，
      // 无需通过整体替换数组强制重渲染（原写法会造成全量重渲染）
      if (status === "success") {
        console.info(`[AI 调用] 完成 ${command} 耗时 ${duration}ms`, response);
      } else {
        console.error(`[AI 调用] 失败 ${command} 耗时 ${duration}ms 错误: ${error ?? ""}`);
      }
    };
  }

  /** 清空所有记录（同时清除 localStorage） */
  function clearAll() {
    records.value = [];
    try {
      localStorage.removeItem(STORAGE_KEY);
    } catch {
      // 忽略
    }
  }

  // 监听 records 变化，自动持久化
  watch(
    records,
    (newRecords) => persistRecords(newRecords),
    { deep: true },
  );

  return {
    records,
    latestRecord,
    pendingCount,
    startCall,
    clearAll,
  };
});
