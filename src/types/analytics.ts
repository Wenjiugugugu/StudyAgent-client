/** 学习数据分析相关类型定义 */

/** 分析时间范围 */
export type AnalyticsRange = "last_7_days" | "last_30_days" | "all";

/** 每日学习量数据点 */
export interface DailyTrendPoint {
  date: string;
  /** 完成率（0-100） */
  completion_rate: number;
  /** 计划任务数 */
  planned_tasks: number;
  /** 已完成任务数 */
  completed_tasks: number;
  /** 计划学习时长（小时） */
  planned_hours: number;
  /** 实际学习时长（小时） */
  actual_hours: number;
}

/** 学习量趋势统计 */
export interface LearningTrend {
  points: DailyTrendPoint[];
  /** 平均完成率 */
  avg_completion_rate: number;
  /** 累计学习时长 */
  total_actual_hours: number;
  /** 累计计划学习时长 */
  total_planned_hours: number;
  /** 累计任务数 */
  total_planned_tasks: number;
  /** 累计完成任务数 */
  total_completed_tasks: number;
  /** 学习天数（有实际时长>0 的天数） */
  study_days: number;
}

/** 掌握度分布 */
export interface MasteryDistribution {
  mastered: number;
  basic: number;
  weak: number;
  not_marked: number;
}

/** 阻碍因素统计项 */
export interface BlockerItem {
  key: string;
  label: string;
  count: number;
}

/** 感受曲线数据点 */
export interface FeelingPoint {
  date: string;
  /** smooth=3, normal=2, hard=1 */
  score: number;
  label: string;
}

/** 困难类型分布项 */
export interface DifficultyItem {
  key: string;
  label: string;
  count: number;
}

/** 复盘质量分析 */
export interface ReviewQuality {
  mastery: MasteryDistribution;
  blockers: BlockerItem[];
  feelings: FeelingPoint[];
  difficulties: DifficultyItem[];
  /** 有效复盘天数 */
  review_count: number;
}

/** 周期对比指标 */
export interface PeriodMetrics {
  /** 平均完成率 */
  avg_completion_rate: number;
  /** 总学习时长 */
  total_hours: number;
  /** 总任务数 */
  total_tasks: number;
  /** 总完成任务数 */
  total_completed: number;
  /** 学习天数 */
  study_days: number;
}

/** 周期对比结果 */
export interface PeriodComparison {
  current: PeriodMetrics;
  previous: PeriodMetrics;
  /** 当前周期标签（如 "本周" / "本月"） */
  current_label: string;
  previous_label: string;
  /** 完成率变化（百分点，正数表示提升） */
  completion_rate_delta: number;
  /** 学习时长变化（小时） */
  hours_delta: number;
  /** 任务量变化 */
  tasks_delta: number;
}

/** 目标达成预测 */
export interface GoalPrediction {
  /** 近7天平均完成率 */
  recent_avg_completion_rate: number;
  /** 近7天平均每日学习时长 */
  recent_avg_daily_hours: number;
  /** 基于近7天完成率推算的预期完成率 */
  expected_completion_rate: number;
  /** 预测状态：on_track / at_risk / off_track / no_data */
  status: string;
  /** 状态描述 */
  description: string;
}

/** 周期对比与预测 */
export interface ComparisonAndPrediction {
  week_comparison: PeriodComparison;
  month_comparison: PeriodComparison;
  prediction: GoalPrediction;
}

/** 完整分析数据 */
export interface AnalyticsSummary {
  range: string;
  learning_trend: LearningTrend;
  review_quality: ReviewQuality;
  comparison: ComparisonAndPrediction;
}
