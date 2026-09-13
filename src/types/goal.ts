/**
 * StudyAgent — Goal & Deadline Plan Types
 * 对应 plan/goals.json（目标与截止日规划区间）
 *
 * **每书唯一**：同一 (subject, book) 组合只允许一条目标计划；
 * 跨书允许多条并行推进。`book` 必填。旧数据（无 `book` 字段）按「每科一条」兼容。
 *
 * 每个科目每个书/板块可拥有一条独立的「截止日 + 目标章节」区间。区间生效期内，
 * 该书/板块的每日任务由后端用章节顺序表确定性倒排生成；区间外/已达标/
 * 已过截止日则回退到默认的「按学习时长」安排。
 */

import type { SubjectKey } from "./state";

/** 目标区间状态 */
export type GoalStatus = "active" | "completed" | "expired";

/** 一条截止日规划区间（绑定单一科目 + 书/板块） */
export interface Goal {
  /** 唯一标识，如 "goal-math-线性代数-1" */
  id: string;
  /** 关联科目 */
  subject: SubjectKey;
  /** 目标描述（用户自定义），如 "9/20 前完成线性方程组" */
  title: string;
  /** 截止日期 YYYY-MM-DD */
  deadline: string;
  /** 目标知识点（用当前版本顺序表定位），如 "线性方程组" */
  target_chapter: string;
  /** 生效起点章节（创建时自动取当前进度，仅展示用，可空） */
  start_chapter?: string;
  /** 目标所属的书/板块（进度表 chapter 节点标题），如「高等数学」「线性代数」 */
  book: string;
  /** 当前进度在顺序表中的位置（自动维护） */
  current_position?: number;
  /** 目标在顺序表中的位置 */
  target_position?: number;
  /** 是否仍生效（未到截止日且未达标） */
  active: boolean;
  /** active | completed | expired */
  status: GoalStatus;
}

/** 目标清单文件（完整 JSON） */
export interface GoalPlanFile {
  version: string;
  meta: {
    generated_at: string;
    based_on?: { state?: string; user_model?: string; exam_config?: string };
  };
  data: {
    goals: Goal[];
  };
}