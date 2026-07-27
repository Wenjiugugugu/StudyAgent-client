/**
 * StudyAgent — API Service Layer
 * 前端统一 API 入口，所有数据请求通过此层
 * Tauri 环境调用 Rust 后端，浏览器环境回退 Mock 数据
 */

import { invoke, invokeWithFallback, invokeDirect, isTauri } from "./tauri";
import {
  mockState,
  mockTodayPlan,
  mockReview,
  mockDashboardSummary,
  mockKnowledgeIndex,
  mockKnowledgeObject,
  mockSettings,
  mockMCPServerStatus,
} from "./mock-data";
import { useAiDebugStore } from "@/stores/aiDebug";

/**
 * 包装 AI 相关的 invoke 调用，将请求/响应/错误记录到 aiDebug store。
 * 用于在「调试」视图中查看 AI 调用历史与原始返回数据。
 */
async function invokeWithAiTrace<T>(
  command: string,
  label: string,
  args: Record<string, unknown>,
  fallback?: () => Promise<T>,
): Promise<T> {
  const aiDebug = useAiDebugStore();
  const finish = aiDebug.startCall(command, label, args);
  try {
    const result = fallback
      ? await invokeWithFallback<T>(command, args, fallback)
      : await invokeDirect<T>(command, args);
    finish("success", result, null);
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
  KnowledgeSubjectIndex,
  KnowledgeObject,
  KnowledgeGraph,
  AppSettings,
  AIProviderConfig,
  ModelInfo,
  MCPServerStatus,
  ChatRequest,
  ChatResponse,
  ToolCallResult,
  SubjectKey,
  TextbookInfo,
  TextbookContent,
  TextbookSearchHit,
  PlanSummary,
  UpdateCheckResult,
  UpdateAsset,
  DownloadProgress,
} from "@/types";

export { isTauri } from "./tauri";

// ── Dashboard ──

export async function getDashboardSummary(): Promise<DashboardSummary> {
  return invokeWithFallback("get_dashboard_summary", undefined, async () => mockDashboardSummary);
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
      });
    }
    return summaries;
  });
}

export async function getWeekPlan(weekStart: string): Promise<WeekPlan> {
  // 必须使用真实后端数据，避免无周计划时回退到 mock 数据（mock 默认周末休息，与用户设置冲突）
  return invokeDirect<WeekPlan>("get_week_plan", { weekStart });
}

export async function generateDailyPlan(date: string): Promise<DailyPlan> {
  return invokeWithAiTrace(
    "generate_daily_plan",
    `生成日计划 ${date}`,
    { date },
    async () => {
      await new Promise((r) => setTimeout(r, 1500));
      return mockTodayPlan;
    },
  );
}

export async function generateWeekPlan(weekStart: string): Promise<WeekPlan> {
  return invokeWithAiTrace<WeekPlan>(
    "generate_week_plan",
    `生成周计划 ${weekStart}`,
    { weekStart },
  );
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
  return invokeWithAiTrace(
    "generate_review",
    `生成复盘 ${date}`,
    { date },
    async () => {
      await new Promise((r) => setTimeout(r, 2000));
      return mockReview;
    },
  );
}

/** 提交结构化复盘（新版，无需 AI） */
export async function submitReview(payload: import("@/types").SubmitReviewPayload): Promise<void> {
  return invokeDirect("submit_review", { payload });
}

// ── Knowledge ──

export async function listKnowledge(subject?: string): Promise<KnowledgeSubjectIndex[]> {
  return invokeWithFallback("list_knowledge", { subject }, async () => mockKnowledgeIndex);
}

export async function getKnowledge(id: string): Promise<KnowledgeObject> {
  return invokeWithFallback("get_knowledge", { id }, async () => mockKnowledgeObject);
}

export async function searchKnowledge(query: string): Promise<KnowledgeObject[]> {
  return invokeWithFallback("search_knowledge", { query }, async () => [mockKnowledgeObject]);
}

export async function getKnowledgeGraph(subject: string): Promise<KnowledgeGraph> {
  return invokeWithFallback("get_knowledge_graph", { subject }, async () => ({
    nodes: [
      { id: "408-ds-03-tree-basics", label: "树的基本概念", subject: "professional" as SubjectKey, status: "mastered" },
      { id: "408-ds-03-bst", label: "二叉搜索树", subject: "professional" as SubjectKey, status: "mastered" },
      { id: "408-ds-03-avl", label: "AVL树", subject: "professional" as SubjectKey, status: "reviewing" },
      { id: "408-ds-04-bf-match", label: "BF简单匹配", subject: "professional" as SubjectKey, status: "mastered" },
      { id: "408-ds-04-kmp", label: "KMP字符串匹配", subject: "professional" as SubjectKey, status: "mastered" },
    ],
    edges: [
      { source: "408-ds-03-tree-basics", target: "408-ds-03-bst", type: "prerequisite" },
      { source: "408-ds-03-bst", target: "408-ds-03-avl", type: "prerequisite" },
      { source: "408-ds-04-bf-match", target: "408-ds-04-kmp", type: "prerequisite" },
    ],
  }));
}

// ── AI ──

export async function chat(request: ChatRequest): Promise<ChatResponse> {
  return invokeWithAiTrace<ChatResponse>(
    "chat",
    `AI 对话（${request.messages.length} 条消息）`,
    { request },
  );
}

export async function testAIProvider(config: AIProviderConfig): Promise<{ success: boolean; message: string }> {
  return invokeWithAiTrace<{ success: boolean; message: string }>(
    "test_ai_provider",
    `测试 Provider ${config.name}`,
    { config },
  );
}

/** 获取 AI Provider 可用模型列表（传入 config 测试特定配置，不传则从默认 provider 获取） */
export async function listAIModels(config?: AIProviderConfig): Promise<ModelInfo[]> {
  return invokeWithAiTrace<ModelInfo[]>(
    "list_ai_models",
    "获取模型列表",
    { config: config ?? null },
  );
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
 */
export async function downloadUpdate(url: string, filename: string): Promise<string> {
  return invokeDirect<string>("download_update", { url, filename });
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
 * 获取应用版本号（来自 tauri.conf.json）
 */
export async function getAppVersion(): Promise<string> {
  return invokeDirect<string>("get_app_version");
}
