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
  /** 本周特殊情况排除日期（不生成计划，自动免复盘） */
  excluded_days?: ExcludedDay[];
  /** 本周任务量调整（相对上周） */
  workload_adjustment?: WorkloadAdjustment;
  /** 本周任务量自校准（基于上周完成率自动下调每日任务数） */
  calibration?: WeekCalibration;
}

/** 每周任务量自校准元数据（记录自动减量原因） */
export interface WeekCalibration {
  /** 基准每日任务数（用户设置） */
  base_daily_task_count: number;
  /** 自动校准后的有效每日任务数 */
  effective_daily_task_count: number;
  /** 自校准系数（<1.0 表示减量） */
  coefficient: number;
  /** 上周复盘平均完成率（0-100） */
  avg_completion_rate: number;
}

/** 特殊情况排除日类型 */
export type ExcludedReasonType = "travel" | "sick" | "exam" | "other";

/** 特殊情况排除日（用户主动声明本周某天不学习） */
export interface ExcludedDay {
  /** YYYY-MM-DD */
  date: string;
  /** 预设类型 */
  reason_type: ExcludedReasonType;
  /** 自由备注（可空） */
  note?: string;
}

/** 任务量调整方向 */
export type WorkloadDirection = "increase" | "decrease" | "unchanged";

/** 任务量调整幅度档位 */
export type WorkloadLevel = "small" | "large";

/** 周任务量调整（相对上周） */
export interface WorkloadAdjustment {
  /** 方向 */
  direction: WorkloadDirection;
  /** 幅度档位（仅 direction != unchanged 时有意义） */
  level?: WorkloadLevel;
  /** 用户备注（可空） */
  note?: string;
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
