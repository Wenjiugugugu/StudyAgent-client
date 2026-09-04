/** 周级学习计划自适应分析（对应后端 adaptive_planner.rs） */
export interface WorkloadFeedbackSummary {
  too_little_days: number;
  reasonable_days: number;
  too_much_days: number;
  valid_days: number;
  weighted_score: number;
  ema: number;
}

export interface SubjectPlanningAnalysis {
  subject: string;
  planned_hours: number;
  actual_hours: number;
  time_ratio?: number;
  planned_completion_rate: number;
  task_count: number;
  reviewed_task_count: number;
  completed_task_count: number;
  unfinished_task_count: number;
  valid_time_tasks: number;
  blockers: Record<string, number>;
}

export interface WeekPlanningAnalysis {
  version: string;
  week_start: string;
  week_end: string;
  planned_total_hours: number;
  eligible_planned_hours: number;
  actual_total_hours: number;
  eligible_actual_hours: number;
  completed_planned_hours: number;
  planned_completion_rate: number;
  task_completion_rate: number;
  planned_task_count: number;
  eligible_task_count: number;
  reviewed_task_count: number;
  completed_task_count: number;
  unfinished_task_count: number;
  unfinished_reasons: Record<string, number>;
  valid_review_days: number;
  actual_data_days: number;
  valid_days: number;
  external_day_count: number;
  feedback: WorkloadFeedbackSummary;
  subjects: SubjectPlanningAnalysis[];
  manual_override: boolean;
  confidence: number;
  capacity_before: number;
  capacity_observation: number;
  capacity_after: number;
  capacity_adjustment: number;
  workload_adjustment: number;
  reasons: string[];
  warnings: string[];
}
