import type { ActualStudyEntry, KnowledgePointRef, OvercompletionEntry, PlanTask } from "@/types";

export interface ReviewDraftForm {
  taskSignatures: Record<string, string>;
  taskCompleted: Record<string, boolean>;
  taskCompletedPoints: Record<string, KnowledgePointRef[]>;
  taskCompletionTouched: Record<string, boolean>;
  taskBlockers: Record<string, string[]>;
  blockerNotes: Record<string, string>;
  overallFeeling: string;
  mainDifficulty: string;
  workloadFeedback: string;
  externalInterference: string;
  actualStudy: ActualStudyEntry[];
  hasOvercompletion: boolean;
  overcompletions: OvercompletionEntry[];
  step: number;
  quickMode: boolean;
  activeSubject: string;
  viewTableIds: Record<string, string>;
  ocOrig: Record<string, string>;
  ocTail: Record<string, number>;
}

export function reviewTaskSignatures(tasks: PlanTask[]) {
  return Object.fromEntries(
    tasks.map((task) => [
      task.id,
      JSON.stringify([task.subject, task.title, task.knowledge_points ?? []]),
    ])
  );
}

/** 只恢复身份和学习范围均未变化的任务选择，文字记录仍完整保留。 */
export function matchingDraftTasks(form: ReviewDraftForm, tasks: PlanTask[]) {
  const current = reviewTaskSignatures(tasks);
  return Object.keys(form.taskSignatures).filter((id) => current[id] === form.taskSignatures[id]);
}

export function isReviewDraftForm(value: unknown): value is ReviewDraftForm {
  if (!value || typeof value !== "object") return false;
  const form = value as Record<string, unknown>;
  const recordFields = [
    "taskSignatures",
    "taskCompleted",
    "taskCompletedPoints",
    "taskCompletionTouched",
    "taskBlockers",
    "blockerNotes",
    "viewTableIds",
    "ocOrig",
    "ocTail",
  ];
  const stringFields = [
    "overallFeeling",
    "mainDifficulty",
    "workloadFeedback",
    "externalInterference",
    "activeSubject",
  ];
  return (
    recordFields.every(
      (key) => !!form[key] && typeof form[key] === "object" && !Array.isArray(form[key])
    ) &&
    stringFields.every((key) => typeof form[key] === "string") &&
    Array.isArray(form.actualStudy) &&
    form.actualStudy.every(
      (entry) =>
        entry &&
        typeof entry.subject === "string" &&
        typeof entry.content === "string" &&
        ["today", "ongoing", "end"].includes(entry.scope)
    ) &&
    Array.isArray(form.overcompletions) &&
    form.overcompletions.every(
      (entry) =>
        entry && typeof entry.subject === "string" && typeof entry.chapter_reached === "string"
    ) &&
    Object.values(form.taskCompleted as Record<string, unknown>).every(
      (value) => typeof value === "boolean"
    ) &&
    Object.values(form.taskCompletedPoints as Record<string, unknown>).every(
      (value) =>
        Array.isArray(value) &&
        value.every(
          (point) =>
            point && typeof point.table_id === "string" && typeof point.node_id === "string"
        )
    ) &&
    typeof form.quickMode === "boolean" &&
    typeof form.hasOvercompletion === "boolean" &&
    Number.isInteger(form.step) &&
    (form.step as number) >= 0 &&
    (form.step as number) <= 4
  );
}
