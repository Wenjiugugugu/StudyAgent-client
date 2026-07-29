/**
 * StudyAgent Core — State Types
 * 对应 state/current.state (TOML)
 */

export type SubjectKey = "math" | "english" | "politics" | "professional";

export type StudyPhase = "foundation" | "strengthen" | "sprint" | "mock" | "complete";

export type TaskStatus = "pending" | "in_progress" | "done" | "abandoned";

export type TaskPriority = "A" | "B" | "C";

export type RiskLevel = "low" | "medium" | "high" | "critical";

export type RiskSubject = SubjectKey | "overall";

/** 单个学习任务（State.current_task.tasks 项） */
export interface StateTask {
  /** 任务 ID（与 PlanTask.id 对应，格式 YYYY-MM-DD-NN）
   * 旧版 state 文件无此字段，反序列化为 undefined */
  task_id?: string;
  subject: SubjectKey;
  task: string;
  priority: TaskPriority;
  status: TaskStatus;
  /** 计时开始时间戳（ISO 8601, +0800），仅当任务正在计时时存在
   * 仅在启用 enable_time_tracking 时使用；旧 state 文件无此字段 */
  started_at?: string;
  /** 累计已计时分钟数（不含当前正在进行中的时段）
   * 仅在启用 enable_time_tracking 时维护；旧 state 文件无此字段 */
  accumulated_minutes?: number;
}

/** 风险项（State.risks.items 项） */
export interface StateRisk {
  subject: SubjectKey | "overall";
  level: RiskLevel;
  description: string;
  suggested_action: string;
}

/** 科目状态 */
export interface SubjectState {
  active: boolean;
  name?: string;
  phase: StudyPhase;
  version?: string;
  textbook?: string;
  textbook_note?: string;
  target_score: number;
  current_score: number;
  weekly_hours: number;
  weak_chapters: string[];
  strong_chapters: string[];
  completed: string[];
  current_focus: string;
  note?: string;
}

/** 用户学习画像 */
export interface UserModel {
  preferred_study_time: string;
  avg_focus_hours_per_day: number;
  best_subjects: string[];
  worst_subjects: string[];
  learning_style: string;
  common_error_types: string[];
  review_compliance_rate: number;
}

/** 全局进度 */
export interface GlobalProgress {
  total_study_days: number;
  last_study_date: string;
  streak_days: number;
  total_practice_questions: number;
  note: string;
}

/** 完整 State */
export interface StudyState {
  meta: {
    last_updated: string;
    exam_date: string;
    target_school: string;
    target_major: string;
  };
  subjects: {
    math: SubjectState;
    english: SubjectState;
    politics: SubjectState;
    professional: SubjectState;
  };
  current_task: {
    date: string;
    focus: string;
    total_hours: number;
    tasks: StateTask[];
    note: string;
  };
  /** @deprecated 0.3.0 风险项已移除，仅为兼容旧 state 文件保留 */
  risks?: {
    items: StateRisk[];
  };
  user_model: UserModel;
  progress: GlobalProgress;
}
