import type { ActualStudyEntry } from "@/types";

export function actualStudyError(entries: ActualStudyEntry[]): string {
  const subjects = new Set<string>();
  for (const entry of entries) {
    if (subjects.has(entry.subject)) return "同一科目请合并成一条说明。";
    subjects.add(entry.subject);
    if (entry.scope !== "end" && !entry.content.trim()) return "请填写实际学习内容或后续方向。";
    if (entry.content.length > 2000) return "每科的学习说明请控制在 2000 字以内。";
  }
  return "";
}
