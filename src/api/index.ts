/**
 * StudyAgent — API Service Layer
 * 前端统一 API 入口，所有数据请求通过此层
 * Tauri 环境调用 Rust 后端，浏览器环境回退 Mock 数据
 */

import { invokeWithFallback, invokeDirect, isTauri } from "./tauri";
import { todayString } from "@/utils/date";
import {
  mockState,
  mockTodayPlan,
  mockReview,
  mockDashboardSummary,
  mockSettings,
  mockMCPServerStatus,
} from "./mock-data";
import { useAiDebugStore } from "@/stores/aiDebug";

/** AI 请求取消键（与后端 agent 类型小写对应） */
export const AI_CANCEL_KEYS = {
  planner: "planner",
  reviewer: "reviewer",
  briefing: "briefing",
  teacher: "teacher",
  assistant: "assistant",
  doubt: "doubt",
} as const;

export type AiCancelKey = (typeof AI_CANCEL_KEYS)[keyof typeof AI_CANCEL_KEYS];

/**
 * AI 调用统一编排入口（状态管理统一化重构，2026-08-04）
 *
 * 收敛三个横切关注点：
 * 1. **trace**：调用前后记录到 aiDebug store（继承原 invokeWithAiTrace 行为）
 * 2. **超时**：按场景配置 timeoutMs，到点 reject 并**自动触发后端取消**
 *    （解决 H21：超时后后端 AI 请求仍在执行导致状态不一致）
 * 3. **取消**：cancelKey 透传，视图层取消按钮用同一 key 即可终止
 *
 * 所有 AI 驱动的 API 函数必须走此入口，禁止再手写 withTimeout / Promise.race。
 */
export interface AiInvokeOptions<T = unknown> {
  command: string;
  label: string;
  args: Record<string, unknown>;
  cancelKey: AiCancelKey;
  /** 超时毫秒。默认 120s（对齐后端 default_timeout）。到点自动 cancelAiRequest */
  timeoutMs?: number;
  timeoutMessage?: string;
  /** 浏览器环境 mock 回退（仅开发用，生产环境不传） */
  fallback?: () => Promise<T>;
}

/**
 * 从 AI 调用结果中提取推理模型的思考过程（reasoning_content）。
 * 仅对话类命令（chat / chatDoubt）的返回里带 reasoning 字段。
 */
function extractReasoning(result: unknown): string | null {
  if (result && typeof result === "object" && "reasoning" in result) {
    const r = (result as { reasoning?: unknown }).reasoning;
    if (typeof r === "string" && r.length > 0) return r;
  }
  return null;
}

export async function aiInvoke<T = unknown>(opts: AiInvokeOptions<T>): Promise<T> {
  const { command, label, args, cancelKey, timeoutMs = 120_000, fallback } = opts;
  const aiDebug = useAiDebugStore();
  const finish = aiDebug.startCall(command, label, args);

  let timer: ReturnType<typeof setTimeout> | undefined;
  let settled = false;

  const cleanup = () => {
    if (timer) clearTimeout(timer);
    timer = undefined;
  };

  try {
    // 超时计时器：到点 reject，并通知后端取消（避免后台请求继续写文件）
    const timeoutPromise = new Promise<never>((_, reject) => {
      timer = setTimeout(async () => {
        settled = true;
        cleanup();
        try {
          await cancelAiRequest(cancelKey);
        } catch {
          // 取消请求失败不阻塞超时错误返回
        }
        reject(
          new Error(opts.timeoutMessage ?? `AI 请求超时（超过 ${Math.round(timeoutMs / 1000)} 秒）`)
        );
      }, timeoutMs);
    });

    const result = await Promise.race([
      fallback
        ? invokeWithFallback<T>(command, args, fallback)
        : invokeDirect<T>(command, args),
      timeoutPromise,
    ]);

    if (settled) throw new Error("AI 请求已被取消");
    cleanup();
    // 修复：后端部分命令（如 test_ai_provider）失败时仍返回 Ok，但带 `success:false` 语义字段。
    // 不能仅以"是否抛异常"判定成败，否则调试页会把失败误记为成功。
    if (
      result &&
      typeof result === "object" &&
      "success" in (result as object) &&
      (result as { success?: unknown }).success === false
    ) {
      const msg =
        (result as { message?: string }).message || "AI 调用失败（未提供错误详情）";
      finish("error", null, msg);
      throw new Error(msg);
    }
    finish("success", result, null, extractReasoning(result));
    return result;
  } catch (e) {
    const message = e instanceof Error ? e.message : String(e);
    finish("error", null, message);
    throw e;
  }
}
import type {
  StudyState,
  DailyPlan,
  WeekPlan,
  ReviewRecord,
  DashboardSummary,
  AppSettings,
  AIProviderConfig,
  ModelInfo,
  MCPServerStatus,
  ChatRequest,
  ChatResponse,
  ToolCallResult,
  TextbookInfo,
  TextbookContent,
  TextbookSearchHit,
  PlanSummary,
  UpdateCheckResult,
  DownloadProgress,
  AiUsageEntry,
  BriefingFile,
  GetBriefingResult,
  ProviderBalanceResult,
} from "@/types";

export { isTauri } from "./tauri";

// ── Dashboard ──

export async function getDashboardSummary(): Promise<DashboardSummary> {
  return invokeWithFallback("get_dashboard_summary", undefined, async () => mockDashboardSummary);
}

// ── Analytics ──

/** 获取学习数据分析数据
 *  `excludeExemptDates`：是否在分析中排除休息日和特殊情况排除日（默认 true）
 */
export async function getAnalytics(
  range: import("@/types").AnalyticsRange = "last_30_days",
  excludeExemptDates: boolean = true,
): Promise<import("@/types").AnalyticsSummary> {
  return invokeDirect("get_analytics", {
    range,
    excludeExemptDates,
  });
}

// ── State ──

export async function getState(): Promise<StudyState> {
  return invokeWithFallback("get_state", undefined, async () => mockState);
}

// ── Plan ──

export async function getTodayPlan(): Promise<DailyPlan> {
  return invokeWithFallback("get_today_plan", undefined, async () => mockTodayPlan);
}

export async function getPlanByDate(date: string): Promise<DailyPlan> {
  // 必须使用真实后端数据，避免无计划的日期回退到 mock 数据
  return invokeDirect<DailyPlan>("get_plan_by_date", { date });
}

export async function listPlanDates(): Promise<string[]> {
  return invokeWithFallback("list_plan_dates", undefined, async () => {
    // Mock fallback: use today and a few past dates
    const today = new Date();
    const dates: string[] = [];
    for (let i = 6; i >= 0; i--) {
      const d = new Date(today);
      d.setDate(d.getDate() - i);
      dates.push(d.toISOString().split("T")[0]);
    }
    return dates;
  });
}

/** 列出所有日计划摘要（含 review 完成度） */
export async function listPlanSummaries(): Promise<PlanSummary[]> {
  return invokeWithFallback("list_plan_summaries", undefined, async () => {
    // Mock fallback: 生成 7 天示例数据
    const today = new Date();
    const summaries: PlanSummary[] = [];
    for (let i = 6; i >= 0; i--) {
      const d = new Date(today);
      d.setDate(d.getDate() - i);
      const date = d.toISOString().split("T")[0];
      const isRest = d.getDay() === 0;
      summaries.push({
        date,
        has_plan: !isRest,
        has_review: i > 0 && !isRest,
        planned_tasks: isRest ? 0 : 3,
        planned_hours: isRest ? 0 : 4.5,
        completed_tasks: isRest ? 0 : (i > 0 ? 2 : 0),
        completion_rate: isRest ? 0 : (i > 0 ? 67 : 0),
        actual_hours: isRest ? 0 : (i > 0 ? 3.5 : 0),
        is_rest_day: isRest,
        is_excluded: false,
      });
    }
    return summaries;
  });
}

/** 获取指定周的日计划摘要（7 天） */
export async function getWeekSummaries(weekStart: string): Promise<PlanSummary[]> {
  return invokeWithFallback("get_week_summaries", { weekStart }, async () => {
    const summaries: PlanSummary[] = [];
    for (let i = 0; i < 7; i++) {
      const d = new Date(weekStart);
      d.setDate(d.getDate() + i);
      const date = d.toISOString().split("T")[0];
      const isRest = d.getDay() === 0;
      summaries.push({
        date,
        has_plan: !isRest,
        has_review: false,
        planned_tasks: isRest ? 0 : 3,
        planned_hours: isRest ? 0 : 4.5,
        completed_tasks: 0,
        completion_rate: 0,
        actual_hours: 0,
        is_rest_day: isRest,
        is_excluded: false,
      });
    }
    return summaries;
  });
}

/** 获取确定性周计划自适应分析 */
export async function getWeekPlanningAnalysis(
  weekStart: string,
): Promise<import("@/types").WeekPlanningAnalysis> {
  return invokeDirect("get_week_planning_analysis", { weekStart });
}

export async function getWeekPlan(weekStart: string): Promise<WeekPlan> {
  // 必须使用真实后端数据，避免无周计划时回退到 mock 数据（mock 默认周末休息，与用户设置冲突）
  return invokeDirect<WeekPlan>("get_week_plan", { weekStart });
}

export async function generateDailyPlan(date: string): Promise<DailyPlan> {
  return aiInvoke<DailyPlan>({
    command: "generate_daily_plan",
    label: `生成日计划 ${date}`,
    args: { date },
    cancelKey: AI_CANCEL_KEYS.planner,
    timeoutMs: 60_000,
    timeoutMessage: `生成日计划超时（超过 60 秒）。请检查 AI Provider 配置或网络连接。`,
    fallback: async () => {
      await new Promise((r) => setTimeout(r, 1500));
      return mockTodayPlan;
    },
  });
}

export async function generateWeekPlan(
  weekStart: string,
  excludedDays: import("@/types").ExcludedDay[] = [],
  workloadAdjustment?: import("@/types").WorkloadAdjustment,
): Promise<WeekPlan> {
  return aiInvoke<WeekPlan>({
    command: "generate_week_plan",
    label: `生成周计划 ${weekStart}`,
    args: { weekStart, excludedDays, workloadAdjustment },
    cancelKey: AI_CANCEL_KEYS.planner,
    timeoutMs: 290_000,
    timeoutMessage: `生成周计划超时（超过 290 秒）。可能网络较慢或模型响应过久，可点击「取消」终止。`,
  });
}

export async function updateTaskStatus(taskId: string, status: string): Promise<void> {
  return invokeWithFallback("update_task_status", { taskId, status }, async () => {
    console.log(`[Mock] Task ${taskId} status updated to ${status}`);
  });
}

/** 更新指定科目的教材信息（传空字符串或 null 清除） */
export async function updateSubjectTextbook(
  subject: "math" | "english" | "politics" | "professional",
  textbook: string | null,
): Promise<void> {
  return invokeWithFallback("update_subject_textbook", { subject, textbook }, async () => {
    console.log(`[Mock] Updated ${subject} textbook to ${textbook}`);
  });
}

/** 开始任务计时（设置 started_at 为当前时间） */
export async function startTaskTimer(taskId: string): Promise<void> {
  return invokeWithFallback("start_task_timer", { taskId }, async () => {
    console.log(`[Mock] Started timer for task ${taskId}`);
  });
}

/** 暂停任务计时（累加本次时长到 accumulated_minutes，清空 started_at）
 * 返回本次新增的计时分钟数 */
export async function pauseTaskTimer(taskId: string): Promise<number> {
  return invokeWithFallback("pause_task_timer", { taskId }, async () => {
    console.log(`[Mock] Paused timer for task ${taskId}`);
    return 0;
  });
}

/** 获取任务累计专注分钟数（含正在进行的时段） */
export async function getTaskTotalMinutes(taskId: string): Promise<number> {
  return invokeWithFallback("get_task_total_minutes", { taskId }, async () => 0);
}

/** 番茄钟：把完成的学习会话分钟数累加到关联任务（今日计划/复盘会累计实际用时） */
export async function focusAddMinutes(taskId: string, minutes: number): Promise<void> {
  return invokeWithFallback("focus_add_minutes", { taskId, minutes }, async () => {
    console.log(`[Mock] Focus added ${minutes} min to ${taskId}`);
  });
}

// ── Focus（番茄钟）会话记录 ──

export type FocusSessionType = "focus" | "short_break" | "long_break" | "stopwatch";
export type FocusSessionStatus = "completed" | "interrupted";

export interface FocusSession {
  id: string;
  type: FocusSessionType;
  started_at: string;
  ended_at: string;
  duration_minutes: number;
  task_id: string | null;
  status: FocusSessionStatus;
}

/** 单日专注统计 */
export interface FocusDayStats {
  date: string;
  pomodoros: number;
  focus_minutes: number;
  breaks: number;
}

/** 记录一条专注会话（学习/休息/长休息） */
export async function recordFocusSession(session: FocusSession): Promise<void> {
  return invokeWithFallback("record_focus_session", { session }, async () => {
    console.log("[Mock] Focus session recorded", session.type);
  });
}

/** 番茄钟：为某条未关联的专注会话手动绑定任务 */
export async function linkFocusSession(
  sessionId: string,
  taskId: string,
  date: string
): Promise<void> {
  return invokeWithFallback("link_focus_session", { sessionId, taskId, date }, async () => {
    console.log(`[Mock] Focus session ${sessionId} linked to task ${taskId}`);
  });
}

/** 读取某天的专注会话列表 */
export async function getFocusSessions(date: string): Promise<FocusSession[]> {
  return invokeDirect<FocusSession[]>("get_focus_sessions", { date });
}

/** 读取 [start, end] 日期区间内的全部专注会话 */
export async function getFocusSessionsRange(start: string, end: string): Promise<FocusSession[]> {
  return invokeDirect<FocusSession[]>("get_focus_sessions_range", { start, end });
}

/** 今日专注统计（番茄数 / 专注分钟 / 休息次数） */
export async function getFocusTodayStats(): Promise<FocusDayStats> {
  return invokeWithFallback<FocusDayStats>("get_focus_today_stats", undefined, async () => ({
    date: todayString(),
    pomodoros: 0,
    focus_minutes: 0,
    breaks: 0,
  }));
}

// ── Review ──

export async function getReview(date: string): Promise<ReviewRecord> {
  // 必须使用真实后端数据，避免未生成复盘的日期回退到 mock 数据
  return invokeDirect<ReviewRecord>("get_review", { date });
}

/** 列出所有复盘日期（YYYY-MM-DD，升序） */
export async function listReviewDates(): Promise<string[]> {
  return invokeWithFallback("list_review_dates", undefined, async () => []);
}

export async function generateReview(date: string): Promise<ReviewRecord> {
  return aiInvoke<ReviewRecord>({
    command: "generate_review",
    label: `生成复盘 ${date}`,
    args: { date },
    cancelKey: AI_CANCEL_KEYS.reviewer,
    timeoutMs: 300_000,
    timeoutMessage: `生成复盘超时（超过 300 秒）。请检查 AI Provider 配置或网络连接。`,
    fallback: async () => {
      await new Promise((r) => setTimeout(r, 2000));
      return mockReview;
    },
  });
}

/** 提交结构化复盘（新版，无需 AI） */
export async function submitReview(payload: import("@/types").SubmitReviewPayload): Promise<import("@/types").SubmitReviewResult> {
  return invokeDirect("submit_review", { payload });
}

/** 复盘后重新生成本周剩余天数计划（AI 驱动） */
export async function regenerateRemainingDays(reviewDate: string): Promise<import("@/types").RegenerateResult> {
  return aiInvoke<import("@/types").RegenerateResult>({
    command: "regenerate_remaining_days",
    label: `复盘后重排剩余计划 ${reviewDate}`,
    args: { reviewDate },
    cancelKey: AI_CANCEL_KEYS.planner,
    timeoutMs: 290_000,
    timeoutMessage: "AI 调整超时（290 秒），可能网络较慢或模型响应过久",
  });
}

/** 周中新增排除日并重排剩余天数（AI 驱动） */
export async function addExcludedDayAndRegenerate(
  weekStart: string,
  excludedDay: import("@/types").ExcludedDay,
): Promise<import("@/types").RegenerateResult> {
  return aiInvoke<import("@/types").RegenerateResult>({
    command: "add_excluded_day_and_regenerate",
    label: `周中排除日并重排 ${excludedDay.date}`,
    args: { weekStart, excludedDay },
    cancelKey: AI_CANCEL_KEYS.planner,
    timeoutMs: 290_000,
    timeoutMessage: "AI 调整超时（290 秒），可能网络较慢或模型响应过久",
  });
}

// ── Briefing ──

/** 获取指定日期的每日简报（含状态判断字段） */
export async function getBriefing(date: string): Promise<GetBriefingResult> {
  return invokeDirect<GetBriefingResult>("get_briefing", { date });
}

/** 重新生成指定日期的每日简报（AI 驱动，需存在昨日复盘） */
export async function regenerateBriefing(date: string): Promise<BriefingFile> {
  return aiInvoke<BriefingFile>({
    command: "regenerate_briefing",
    label: `重新生成简报 ${date}`,
    args: { date },
    cancelKey: AI_CANCEL_KEYS.briefing,
    timeoutMs: 180_000,
    timeoutMessage: "重新生成简报超时（超过 180 秒）。请检查 AI Provider 配置或网络连接。",
  });
}

/** 列出所有简报日期（YYYY-MM-DD，升序） */
export async function listBriefingDates(): Promise<string[]> {
  return invokeDirect<string[]>("list_briefing_dates");
}

// ── AI ──

export async function chat(request: ChatRequest): Promise<ChatResponse> {
  return aiInvoke<ChatResponse>({
    command: "chat",
    label: `AI 对话（${request.messages.length} 条消息）`,
    args: { request },
    cancelKey: AI_CANCEL_KEYS.assistant,
    timeoutMs: 120_000,
    timeoutMessage: "AI 对话超时（超过 120 秒）。请检查 AI Provider 配置或网络连接。",
  });
}

export async function chatDoubt(request: ChatRequest): Promise<ChatResponse> {
  return aiInvoke<ChatResponse>({
    command: "chat",
    label: `解惑对话（${request.messages.length} 条消息）`,
    args: { request },
    cancelKey: AI_CANCEL_KEYS.doubt,
    timeoutMs: 120_000,
    timeoutMessage: "解惑对话超时（超过 120 秒）。请检查 AI Provider 配置或网络连接。",
  });
}

/**
 * 取消指定 agent 的进行中 AI 请求（M9：超时过长且无取消机制）
 *
 * 后端收到取消信号后以「AI 请求已被用户取消」提前结束，
 * 不会等待到 300s 超时，也不会切换 fallback provider。
 * 返回是否找到了对应请求。
 */
export async function cancelAiRequest(key: AiCancelKey): Promise<boolean> {
  return invokeDirect<boolean>("cancel_ai_request", { key });
}

export async function testAIProvider(config: AIProviderConfig): Promise<{ success: boolean; message: string }> {
  return aiInvoke<{ success: boolean; message: string }>({
    command: "test_ai_provider",
    label: `测试 Provider ${config.name}`,
    args: { config },
    cancelKey: AI_CANCEL_KEYS.teacher,
    timeoutMs: 30_000,
    timeoutMessage: `测试 Provider ${config.name} 超时（超过 30 秒）`,
  });
}

/** 获取 AI Provider 可用模型列表（传入 config 测试特定配置，不传则从默认 provider 获取） */
export async function listAIModels(config?: AIProviderConfig): Promise<ModelInfo[]> {
  return aiInvoke<ModelInfo[]>({
    command: "list_ai_models",
    label: "获取模型列表",
    args: { config: config ?? null },
    cancelKey: AI_CANCEL_KEYS.teacher,
    timeoutMs: 60_000,
    timeoutMessage: "获取模型列表超时（超过 60 秒）",
  });
}

/**
 * 查询 AI Provider 余额/用量（参考 cc-Switch 用量查询模板）
 *
 * 后端按 Provider 类型 / base_url 域名自动选择查询端点（OpenRouter / SiliconFlow /
 * DeepSeek / Moonshot），未识别时依次尝试通用端点。始终返回结果对象（失败时
 * success: false），不抛异常，便于界面区分「网络失败」与「端点不支持」。
 */
export async function queryProviderBalance(config: AIProviderConfig): Promise<ProviderBalanceResult> {
  return invokeDirect<ProviderBalanceResult>("query_provider_balance", { config });
}

// ── AI 用量日志 ──

/**
 * 读取 AI 用量日志（持久化记录，重启后不丢失）
 *
 * 返回所有历史 AI 调用的 token 消耗记录，按时间升序排列。
 * 用于在「调试」视图中展示历史用量与估算费用。
 */
export async function getAiUsageLog(): Promise<AiUsageEntry[]> {
  return invokeDirect<AiUsageEntry[]>("get_ai_usage_log");
}

/** 清空 AI 用量日志（不可恢复） */
export async function clearAiUsageLog(): Promise<void> {
  return invokeDirect<void>("clear_ai_usage_log");
}

/** 读取应用日志文件（logs/ai-debug.log），返回末尾 maxChars 字符的文本 */
export async function readAppLog(maxChars?: number): Promise<string> {
  return invokeDirect<string>("read_app_log", { maxChars: maxChars ?? 200_000 });
}

/** 清空应用日志文件（logs/ai-debug.log，不可恢复） */
export async function clearAppLog(): Promise<void> {
  return invokeDirect<void>("clear_app_log");
}

// ── 调试页：数据文件检查 ──

export interface DebugDirEntry {
  name: string;
  is_directory: boolean;
}

/** 列出数据目录下某相对路径的条目（目录不存在时返回空列表） */
export async function debugListDir(relativePath: string): Promise<DebugDirEntry[]> {
  return invokeDirect<DebugDirEntry[]>("debug_list_dir", { relativePath });
}

/** 读取数据目录下某相对路径的文件文本内容 */
export async function debugReadFile(relativePath: string): Promise<string> {
  return invokeDirect<string>("debug_read_file", { relativePath });
}

// ── MCP / Tools ──

export async function listMCPServers(): Promise<MCPServerStatus[]> {
  return invokeWithFallback("list_mcp_servers", undefined, async () => mockMCPServerStatus);
}

export async function callTool(toolName: string, args: Record<string, unknown>): Promise<ToolCallResult> {
  return invokeWithFallback("call_tool", { toolName, args }, async () => ({
    success: true,
    data: { message: `Tool "${toolName}" called successfully (Mock)` },
  }));
}

// ── Settings ──

export async function getSettings(): Promise<AppSettings> {
  return invokeWithFallback("get_settings", undefined, async () => mockSettings);
}

export async function saveSettings(settings: AppSettings): Promise<void> {
  return invokeWithFallback("save_settings", { settings }, async () => {
    console.log("[Mock] Settings saved");
  });
}

/** 切换数据目录（重启后生效；旧目录数据不自动迁移） */
export async function changeDataDirectory(newPath: string): Promise<string> {
  return invokeDirect<string>("change_data_directory", { newPath });
}

/** 导出数据备份（zip）到指定路径，返回导出的文件数 */
export async function exportBackup(destPath: string, includeLogs: boolean = false): Promise<number> {
  return invokeDirect<number>("export_backup", { destPath, includeLogs });
}

/** 导入数据备份（zip），覆盖前自动备份原数据目录；返回恢复摘要 */
export async function importBackup(filePath: string): Promise<import("@/types").ImportSummary> {
  return invokeDirect<import("@/types").ImportSummary>("import_backup", { filePath });
}

// ── Textbooks ──

export async function listTextbooks(): Promise<TextbookInfo[]> {
  return invokeWithFallback("list_textbooks", undefined, async () => {
    // Mock 数据
    return [
      { id: "408-wangdao-co", subject: "408", title: "计算机组成原理", filename: "wangdao-co.md", file_path: "" },
      { id: "408-wangdao-os", subject: "408", title: "操作系统", filename: "wangdao-os.md", file_path: "" },
    ];
  });
}

export async function readTextbook(id: string): Promise<TextbookContent> {
  return invokeWithFallback("read_textbook", { id }, async () => {
    return {
      id,
      content: "# 教材内容\n\n这是 Mock 数据，请在桌面应用中查看真实教材内容。\n\n## 第一节\n\n示例段落内容。\n\n## 第二节\n\n- 列表项一\n- 列表项二\n",
      file_path: "",
    };
  });
}

/** 导入教材文件（调用 Tauri 文件对话框选择 .md 文件） */
export async function importTextbook(subject: string, title?: string): Promise<TextbookInfo> {
  const { open } = await import("@tauri-apps/plugin-dialog");
  const filePath = await open({
    multiple: false,
    filters: [{ name: "Markdown", extensions: ["md", "markdown", "txt"] }],
  });
  if (!filePath || typeof filePath !== "string") {
    throw new Error("未选择文件");
  }
  return invokeDirect<TextbookInfo>("import_textbook", {
    subject,
    filePath,
    title: title ?? null,
  });
}

/** 删除已导入的教材 */
export async function deleteTextbook(id: string): Promise<void> {
  return invokeDirect<void>("delete_textbook", { id });
}

/**
 * 选择并保存背景图到应用数据目录
 *
 * 调用 Tauri 文件对话框让用户选择图片，复制到 data_dir/assets/backgrounds/，
 * 返回相对于 data_dir 的路径（如 "assets/backgrounds/bg_xxx.png"）。
 */
export async function saveBackgroundImage(): Promise<string> {
  const { open } = await import("@tauri-apps/plugin-dialog");
  const filePath = await open({
    multiple: false,
    filters: [{ name: "图片", extensions: ["png", "jpg", "jpeg", "webp", "gif", "bmp"] }],
  });
  if (!filePath || typeof filePath !== "string") {
    throw new Error("未选择文件");
  }
  return invokeDirect<string>("save_background_image", { filePath });
}

/** 删除已保存的背景图文件（传入相对路径） */
export async function deleteBackgroundImage(relativePath: string): Promise<void> {
  return invokeDirect<void>("delete_background_image", { relativePath });
}

/**
 * 读取背景图文件并返回 base64 data URL
 *
 * 通过后端命令读取文件字节并编码为 data URL，避免 Tauri assetProtocol scope 配置问题。
 * 返回的 URL 可直接用于 CSS `background-image: url(...)` 或 `<img src="...">`。
 */
export async function readBackgroundAsDataUrl(relativePath: string): Promise<string> {
  if (!relativePath) return "";
  return invokeDirect<string>("read_background_as_data_url", { relativePath });
}

/** 重命名已导入的教材 */
export async function renameTextbook(id: string, newTitle: string): Promise<TextbookInfo> {
  return invokeDirect<TextbookInfo>("rename_textbook", { id, newTitle });
}

/** 在已导入教材中进行全文搜索 */
export async function searchInTextbook(query: string): Promise<TextbookSearchHit[]> {
  if (!query.trim()) return [];
  return invokeWithFallback("search_in_textbook", { query }, async () => []);
}

// ── Onboarding ──

/** 标记引导流程已完成 */
export async function completeOnboarding(): Promise<void> {
  return invokeWithFallback("complete_onboarding", undefined, async () => {});
}

/** 引导流程初始化数据 */
export interface InitStatePayload {
  target_school: string;
  target_major: string;
  exam_date: string;
  subjects: {
    subject: string;
    version?: string;
    active: boolean;
    phase: string;
    weekly_hours: number;
    target_score: number;
    textbook?: string;
  }[];
  professional_name?: string;
}

/** 初始化 State 文件（引导流程完成时调用） */
export async function initState(payload: InitStatePayload): Promise<void> {
  return invokeDirect("init_state", { payload });
}

// ── Update ──

/**
 * 检查更新
 *
 * 获取远端最新 release 信息并与当前版本比较。
 * 任何错误情况均返回 `has_update: false` + 友好提示，不抛出异常。
 */
export async function checkForUpdates(): Promise<UpdateCheckResult> {
  return invokeDirect<UpdateCheckResult>("check_for_updates");
}

/**
 * 下载更新
 *
 * 流式下载安装包到临时目录，期间通过 `update-download-progress` 事件
 * 推送下载进度。下载完成后返回本地文件路径，供 `installUpdate` 使用。
 *
 * @param url 下载地址（来自 UpdateAsset.download_url）
 * @param filename 保存到本地的文件名（来自 UpdateAsset.name）
 * @param expectedSha256 期望的 SHA-256（来自 UpdateAsset.sha256）。
 *  后端下载完成后及安装前都会复核完整性；传 null 会被后端拒绝。
 */
export async function downloadUpdate(
  url: string,
  filename: string,
  expectedSha256: string | null,
): Promise<string> {
  return invokeDirect<string>("download_update", {
    url,
    filename,
    expectedSha256,
  });
}

/**
 * 安装更新
 *
 * 启动下载好的安装包并退出当前应用。
 * 安装程序启动后应用立即退出。
 *
 * @param filePath 安装包本地路径（来自 `downloadUpdate` 返回值）
 */
export async function installUpdate(filePath: string): Promise<void> {
  return invokeDirect<void>("install_update", { filePath });
}

/**
 * 订阅下载进度事件
 *
 * @param callback 接收 `DownloadProgress` 的回调
 * @returns 取消订阅函数（在非 Tauri 环境下为空函数）
 */
export async function onDownloadProgress(
  callback: (progress: DownloadProgress) => void,
): Promise<() => void> {
  if (!isTauri()) {
    return () => {};
  }
  const { listen } = await import("@tauri-apps/api/event");
  const unlisten = await listen<DownloadProgress>("update-download-progress", (event) => {
    callback(event.payload);
  });
  return unlisten;
}

// ── 通用：关闭动作 / 开机启动 / 应用版本 ──

export type CloseAction = "ask" | "tray" | "quit";

/**
 * 获取关闭窗口时的动作设置
 *
 * 后端返回值: "ask" | "tray" | "quit"
 */
export async function getCloseAction(): Promise<CloseAction> {
  return invokeDirect<CloseAction>("get_close_action");
}

/**
 * 设置关闭窗口时的动作（并持久化）
 *
 * action: "ask" | "tray" | "quit"
 */
export async function setCloseAction(action: CloseAction): Promise<void> {
  return invokeDirect<void>("set_close_action", { action });
}

/**
 * 立即退出整个应用进程（包括托盘图标）
 *
 * 用于「关闭窗口询问弹窗」中选择"退出应用"时调用。
 * 不能仅调用 `window.destroy()`：存在 tray icon 时，销毁窗口后进程仍会驻留。
 */
export async function quitApp(): Promise<void> {
  return invokeDirect<void>("quit_app");
}

/**
 * 查询开机启动是否启用
 */
export async function getAutostart(): Promise<boolean> {
  return invokeDirect<boolean>("get_autostart");
}

/**
 * 启用或禁用开机启动
 */
export async function setAutostart(enabled: boolean): Promise<void> {
  return invokeDirect<void>("set_autostart", { enabled });
}

/**
 * 滴答清单同步 — 设置页
 */
/** 滴答 Token 是否已配置（系统凭据库或环境变量 DIDA_TOKEN） */
export async function getDidaTokenStatus(): Promise<boolean> {
  return invokeDirect<boolean>("get_dida_token_status");
}

/** 保存滴答 Token 到系统凭据库（不落 settings.json 明文） */
export async function setDidaToken(token: string): Promise<void> {
  return invokeDirect<void>("set_dida_token", { token });
}

/** 立即对今天执行一次滴答同步对账，返回 (created/updated/deleted) 摘要 */
export async function syncDidaNow(): Promise<string> {
  return invokeDirect<string>("sync_dida_now");
}

/** 清理滴答中过往（已过期）未完成的 studyagent 任务，返回清理数量摘要（复盘提交流程末尾调用） */
export async function cleanupDidaStale(): Promise<string> {
  return invokeDirect<string>("cleanup_dida_stale");
}

/** 回读滴答清单指定日期已完成任务标题（复盘页加载时用于自动勾选完成状态） */
export async function fetchDidaCompletedTitles(date: string): Promise<string[]> {
  return invokeDirect<string[]>("fetch_dida_completed_titles", { date });
}

/** 滴答清单项目（设置页选择归属清单用） */
export interface DidaProject {
  id: string;
  name: string;
}

/** 列出滴答清单项目（需已启用同步并配置 Token；失败返回空数组） */
export async function listDidaProjects(): Promise<DidaProject[]> {
  return invokeDirect<DidaProject[]>("list_dida_projects");
}

/**
 * 获取应用版本号（来自 tauri.conf.json）
 */
export async function getAppVersion(): Promise<string> {
  return invokeDirect<string>("get_app_version");
}

// ── UI 状态标记（跨重启持久化） ──

/**
 * 读取 UI 状态标记（如「更新日志已读版本」「简报已提示日期」），不存在返回空字符串。
 * 优先读后端文件持久化；浏览器开发模式回退 localStorage。
 */
export async function getUiFlag(key: string): Promise<string> {
  if (!isTauri()) {
    return localStorage.getItem(`studyagent.${key}`) ?? "";
  }
  return invokeDirect<string>("get_ui_flag", { key });
}

/**
 * 写入 UI 状态标记（Tauri 环境原子落盘，重启保留；同时写 localStorage 兜底）。
 */
export async function setUiFlag(key: string, value: string): Promise<void> {
  try {
    localStorage.setItem(`studyagent.${key}`, value);
  } catch {
    /* localStorage 不可用时不阻塞后端写入 */
  }
  if (!isTauri()) return;
  return invokeDirect<void>("set_ui_flag", { key, value });
}
