import type { PlanTask, ExcludedDay } from "./plan";

export interface AdjustmentTaskChoice {
  key: string;
  date: string;
  task: PlanTask;
  default_kept: boolean;
  required: boolean;
}
export interface AdjustmentTaskChange {
  subject: string;
  title: string;
  before_date: string | null;
  after_date: string | null;
  before_hours: number | null;
  after_hours: number | null;
  reason: string;
  before_content: string | null;
  after_content: string | null;
}
export interface AdjustmentPreview {
  id: string;
  created_at: string;
  kind: "review" | "exclusion";
  source_date: string;
  excluded_day: ExcludedDay | null;
  affected_dates: string[];
  reason: string;
  used_fallback: boolean;
  warnings: string[];
  conflicts: string[];
  tasks: AdjustmentTaskChoice[];
  kept_keys: string[];
  changes: AdjustmentTaskChange[];
  days: {
    date: string;
    before_hours: number;
    after_hours: number;
    before_rest: boolean;
    after_rest: boolean;
  }[];
  goals: {
    title: string;
    before_deadline: string;
    after_deadline: string;
    before_start: string;
    after_start: string;
  }[];
}
export interface AdjustmentStatus {
  preview: AdjustmentPreview | null;
  undo_id: string | null;
  undo_created_at: string | null;
  applied_preview: AdjustmentPreview | null;
  warnings: string[];
  sync_pending: boolean;
}
export interface AdjustmentOutcome {
  affected_dates: string[];
  warnings: string[];
}
