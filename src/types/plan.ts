/**
 * StudyAgent Core — Plan Types
 * 对应 plan/YYYY-MM-DD_day.json、plan/YYYY-Www_week.json
 *
 * 统一数据契约：{ version, meta, data, view? }
 * - data: 业务数据，所有逻辑层必须读取 data
 * - view: 仅供人类阅读的 Markdown，程序不得解析
 */

import type { SubjectKey, TaskPriority, TaskStatus } from "./state";

/** 计划依赖的数据源 */
export interface BasedOn {
  state: string;
  user_model: string;
  exam_config: string;
  review_ref?: string;
  week_plan?: string;
}

/**
 * 风险提示项（已废弃，仅为兼容旧版 plan JSON 文件保留反序列化能力）
 * @deprecated 0.3.0 风险项功能已移除，不再生成也不再展示
 */
export interface PlanRisk {
  subject: import("./state").RiskSubject;
  item: string;
  level: import("./state").RiskLevel;
  suggestion: string;
}

/** 任务模板（Week Plan 中使用） */
export interface TaskTemplate {
  title: string;
  priority: TaskPriority;
  estimated_hours: number;
  goal: string;
  completion_criteria: string[];
  textbook?: string;
  style_tips?: string;
  fallback_plan?: string;
}

/** 单日某科目的任务分配 */
export interface DaySubjectAllocation {
  subject: SubjectKey;
  hours: number;
  focus: string;
  task_templates: TaskTemplate[];
}

/** 周计划中的单日安排 */
export interface WeekDayPlan {
  date: string;
  weekday: string;
  is_rest_day: boolean;
  subject_allocations: DaySubjectAllocation[];
}

/** 周计划中的科目安排 */
export interface WeekSubjectPlan {
  subject: SubjectKey;
  weekly_hours: number;
  focus: string;
  milestones: string[];
}

/** 日计划元信息 */
export interface DailyPlanMeta {
  date: string;
  generated_at: string;
  type: "daily";
  based_on: BasedOn;
}

/** 日计划业务数据 */
export interface DailyPlanData {
  remaining_days: number;
  target: string;
  strategy: string;
  tasks: PlanTask[];
  /** @deprecated 0.3.0 风险项已移除，仅为兼容旧数据保留 */
  risks?: PlanRisk[];
  style_tips: string[];
  after_today: string;
  /** @deprecated 0.3.0 今日提醒已移除，仅为兼容旧数据保留 */
  reminders?: string[];
  total_hours: number;
  total_tasks: number;
}

/** 日计划文件（完整 JSON） */
export interface DailyPlanFile {
  version: string;
  meta: DailyPlanMeta;
  data: DailyPlanData;
  view?: string;
}

/** 周计划元信息 */
export interface WeekPlanMeta {
  week_start: string;
  week_end: string;
  week_number: number;
  generated_at: string;
  based_on: BasedOn;
}

/** 周计划业务数据 */
export interface WeekPlanData {
  goals: string[];
  subjects: WeekSubjectPlan[];
  days: WeekDayPlan[];
  /** @deprecated 0.3.0 风险项已移除，仅为兼容旧数据保留 */
  risks?: PlanRisk[];
  /** @deprecated 0.3.0 今日提醒已移除，仅为兼容旧数据保留 */
  reminders?: string[];
}

/** 周计划文件（完整 JSON） */
export interface WeekPlanFile {
  version: string;
  meta: WeekPlanMeta;
  data: WeekPlanData;
  view?: string;
}

/** 单个计划任务（日计划中使用） */
export interface PlanTask {
  id: string;
  subject: SubjectKey;
  title: string;
  priority: TaskPriority;
  estimated_hours: number;
  goal: string;
  completion_criteria: string[];
  textbook?: string;
  style_tips?: string;
  fallback_plan?: string;
  status: TaskStatus;
}

/** 兼容别名：DailyPlan 等价于 DailyPlanFile（完整文件） */
export type DailyPlan = DailyPlanFile;

/** 兼容别名：WeekPlan 等价于 WeekPlanFile（完整文件） */
export type WeekPlan = WeekPlanFile;
