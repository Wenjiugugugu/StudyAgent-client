/**
 * AI 调用调试记录器
 *
 * 用于在「调试」视图中查看 AI 调用历史：
 *   - 调用时间、命令名、参数、响应或错误、耗时
 *   - 保留最近 50 条记录，避免内存占用过大
 *   - 同时通过 console.info 输出到「日志」面板
 */
import { defineStore } from "pinia";
import { ref, computed } from "vue";

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

/** 安全序列化：避免循环引用 / 过大字符串 */
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
    // 先 JSON 序列化以剥离 Proxy / Vue ref
    const json = JSON.stringify(value, (_k, v) => {
      if (typeof v === "function") return "[Function]";
      if (typeof v === "string" && v.length > MAX_STRING_LEN) {
        return v.slice(0, MAX_STRING_LEN) + `…[+${v.length - MAX_STRING_LEN} 字符]`;
      }
      return v;
    });
    return JSON.parse(json);
  } catch {
    return String(value);
  }
}

export const useAiDebugStore = defineStore("aiDebug", () => {
  const records = ref<AiCallRecord[]>([]);

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
      // 触发响应式更新（直接修改对象属性 Pinia 也能感知，但保险起见）
      records.value = [...records.value];
      if (status === "success") {
        console.info(`[AI 调用] 完成 ${command} 耗时 ${duration}ms`, response);
      } else {
        console.error(`[AI 调用] 失败 ${command} 耗时 ${duration}ms 错误: ${error ?? ""}`);
      }
    };
  }

  /** 清空所有记录 */
  function clearAll() {
    records.value = [];
  }

  return {
    records,
    latestRecord,
    pendingCount,
    startCall,
    clearAll,
  };
});
