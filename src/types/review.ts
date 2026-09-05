/**
 * StudyAgent Core — Review Types
 * 对应 records/YYYY-MM-DD_review.json
 *
 * 统一数据契约：{ version, meta, data, view? }
 * - data: 业务数据，所有逻辑层必须读取 data
 * - view: 仅供人类阅读的 Markdown，程序不得解析
 */

import type { SubjectKey, TaskPriority } from "./state";

/** 复盘元信息 */
export interface ReviewMeta {
  date: string;
  type: "review";
  plan_ref: string;
  generated_at: string;
}

/** 完成任务记录 */
export interface ReviewCompletedTask {
  task_id?: string;
  subject: SubjectKey;
  title: string;
  priority: TaskPriority;
  completed: boolean;
  completion_time?: string;
  note?: string;
}

/** 计划外任务 */
export interface ReviewUnplannedTask {
  subject: SubjectKey;
  title: string;
  hours: number;
  note?: string;
}

/** 遇到的困难 */
export interface ReviewDifficulty {
  description: string;
  root_cause?: string;
  resolution?: string;
}

/** 实际用时 */
export interface ReviewTimeSpent {
  subject: SubjectKey;
  hours: number;
  planned_hours?: number;
}

/** 完成情况统计 */
export interface ReviewCompletion {
  priority_a_total: number;
  priority_a_done: number;
  priority_b_total: number;
  priority_b_done: number;
  completion_rate: number;
}

/** 复盘业务数据 */
export interface ReviewData {
  completed_tasks: ReviewCompletedTask[];
  unplanned_tasks: ReviewUnplannedTask[];
  difficulties: ReviewDifficulty[];
  time_spent: ReviewTimeSpent[];
  total_hours: number;
  completion: ReviewCompletion;
  energy_level: number;
  external_interference: string;
  key_achievements: string[];
  /** @deprecated 已废弃，仅为兼容旧 review JSON 保留 */
  risks_resolved?: string[];
  next_steps: string[];
}

/** 复盘文件（完整 JSON） */
export interface ReviewFile {
  version: string;
  meta: ReviewMeta;
  data: ReviewData;
  view?: string;
  /** 新版：每个任务的结构化复盘 */
  task_reviews?: TaskReviewEntry[];
  /** 新版：每日整体回顾 */
  daily_review?: DailyReviewInput;
  /** 计划外学习记录（用户实际进度领先计划时填写） */
  overcompletion?: OvercompletionEntry[];
}

/** 兼容别名：ReviewRecord 等价于 ReviewFile（完整文件） */
export type ReviewRecord = ReviewFile;

// ============================================================================
// 新版 Review 类型（结构化问答）
// ============================================================================

/** 任务复盘条目 */
export interface TaskReviewEntry {
  task_id: string;
  /** completed | partial | incomplete | abandoned */
  status: string;
  /** 0.0 - 1.0 */
  completion: number;
  /** mastered | basic | weak */
  mastery: string;
  /** 未完成原因标签 */
  blockers: string[];
  /** 其他原因说明 */
  blocker_note?: string;
  /** 任务标题（自包含字段，便于复盘记录独立展示） */
  title?: string;
  /** 科目：math / english / politics / professional */
  subject?: string;
  /** 优先级：A / B */
  priority?: string;
  /** AI 估时（小时），仅在启用「记录学习时长」时持久化 */
  estimated_hours?: number;
  /** 实际用时（分钟），仅在启用「记录学习时长」时持久化 */
  actual_minutes?: number;
}

/** 每日整体回顾 */
export interface DailyReviewInput {
  /** smooth | normal | hard */
  overall_feeling: string;
  /** understanding | problems | memorization | attention | time_management | environment | other */
  main_difficulty: string;
  /** too_little | reasonable | too_much */
  workload_feedback?: string;
  /** none | sick | travel | exam | family | environment | other */
  external_interference?: string;
}

/** 计划外学习记录条目（用户实际进度领先计划时填写） */
export interface OvercompletionEntry {
  /** 科目：math / english / politics / professional */
  subject: string;
  /** 实际已学习到的章节（如 "多元函数微分学"） */
  chapter_reached: string;
  /** 对应进度表节点 id（可选，用于追溯与回读勾选状态） */
  node_id?: string;
  /** 备注（可选） */
  note?: string;
}

/** 提交复盘请求 */
export interface SubmitReviewPayload {
  date: string;
  task_reviews: TaskReviewEntry[];
  daily_review: DailyReviewInput;
  /** 计划外学习记录（可选） */
  overcompletion?: OvercompletionEntry[];
}

/** 提交复盘的返回结果 */
export interface SubmitReviewResult {
  /** 是否需要调用 AI 重新生成本周剩余天数计划 */
  needs_regeneration: boolean;
  /** 触发重排的原因（用于前端展示） */
  regen_reasons: string[];
  /** 次日简报是否已在后台开始生成（fire-and-forget） */
  briefing_generating?: boolean;
}

/** 单个受影响日期的任务变动摘要（重排前后对比） */
export interface RegenDayChange {
  /** 受影响日期 (YYYY-MM-DD) */
  date: string;
  /** 该日新增的任务标题 */
  added: string[];
  /** 该日被移除的任务标题 */
  removed: string[];
  /** 该日标题变化的任务（原标题 → 新标题） */
  adjusted: [string, string][];
}

/** 重排剩余天数的返回结果 */
export interface RegenerateResult {
  /** 是否实际执行了重排 */
  regenerated: boolean;
  /** 受影响的日期列表 */
  affected_dates: string[];
  /** AI 调用失败时是否启用了确定性兜底安排（用于前端提示用户） */
  used_fallback?: boolean;
  /** 一致性校验警告：声明了计划外进度的科目重排后未生效时给出提示 */
  consistency_warnings?: string[];
  /** 各受影响日期的任务变动明细（悬停提示展示） */
  changes?: RegenDayChange[];
}
