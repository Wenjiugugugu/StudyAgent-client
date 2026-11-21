import type { FocusSession } from "@/api";
import type { StudyState } from "@/types";

/** 只追加尚未累计的会话；旧记录仍按完成/打断状态读取，避免猜测历史计时来源。 */
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
    const recorded = session.task_minutes_recorded ?? session.status === "completed";
    const target = recorded ? completed : interrupted;
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
