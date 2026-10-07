import type { FocusSession } from "@/api";
import type { StudyState } from "@/types";

/** 不把未记录当成零；已完成番茄已写入任务累计时长，避免再次相加。 */
export function reviewTaskMinutes(
  date: string,
  sessions: FocusSession[],
  state?: StudyState,
  now = Date.now()
): Record<string, number> {
  const completed: Record<string, number> = {};
  const interrupted: Record<string, number> = {};
  for (const session of sessions) {
    if (!session.task_id || (session.type !== "focus" && session.type !== "stopwatch")) continue;
    const target = session.status === "completed" ? completed : interrupted;
    target[session.task_id] =
      (target[session.task_id] ?? 0) + Math.max(0, session.duration_minutes);
  }
  if (state?.current_task.date === date) {
    for (const task of state.current_task.tasks) {
      if (!task.task_id) continue;
      let minutes = task.accumulated_minutes ?? 0;
      if (task.started_at) {
        const start = new Date(task.started_at).getTime();
        if (Number.isFinite(start)) minutes += Math.max(0, Math.floor((now - start) / 60000));
      }
      if (minutes > 0 || task.started_at)
        completed[task.task_id] = Math.max(completed[task.task_id] ?? 0, minutes);
    }
  }
  for (const [id, minutes] of Object.entries(interrupted))
    completed[id] = (completed[id] ?? 0) + minutes;
  return completed;
}
