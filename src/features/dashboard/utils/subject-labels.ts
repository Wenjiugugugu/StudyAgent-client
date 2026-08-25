/**
 * 科目展示标签与徽章变体（纯函数）
 */

import type { SubjectKey } from "@/types";

const SUBJECT_LABELS: Record<string, string> = {
  math: "数学",
  english: "英语",
  politics: "政治",
  professional: "专业课",
};

/** 科目 key → 中文名称 */
export function subjectLabel(subject: SubjectKey | string): string {
  return SUBJECT_LABELS[subject] ?? subject;
}

const SUBJECT_BADGE_VARIANTS: Record<string, "math" | "english" | "politics" | "professional"> = {
  math: "math",
  english: "english",
  politics: "politics",
  professional: "professional",
};

/** 科目 key → Badge 组件变体 */
export function subjectBadgeVariant(
  subject: string
): "default" | "math" | "english" | "politics" | "professional" {
  return SUBJECT_BADGE_VARIANTS[subject] ?? "default";
}
