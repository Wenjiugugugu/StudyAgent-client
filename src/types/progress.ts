/**
 * StudyAgent — 各科「进度表」类型
 *
 * 每科可保存多份个性化进度表（如「数二全程」「数二强化」），
 * 同一时刻每科仅一份启用（active_id）。节点含学习状态，用于长期学习打卡。
 */

/** 节点学习状态 */
export type ProgressNodeStatus = "pending" | "learning" | "mastered";

/** 进度表节点 */
export interface ProgressNode {
  id: string;
  /** 知识点/章节标题 */
  title: string;
  /** 所属阶段/章节（如「第一章 函数、极限、连续」） */
  phase: string;
  /** 学习状态 */
  status: ProgressNodeStatus;
  /** 计划学习日期（YYYY-MM-DD），可为空 */
  planned_date: string | null;
  /** 备注 */
  note: string;
}

/** 进度表 */
export interface ProgressTable {
  id: string;
  subject: string;
  name: string;
  created_at: string;
  updated_at: string;
  nodes: ProgressNode[];
}

/** 单一科目的进度表集合 */
export interface SubjectProgressSet {
  /** 当前启用的进度表 id（空表示该科暂无启用表） */
  active_id: string;
  tables: ProgressTable[];
}

/** 联网搜索配置（AI 生成进度表时可选拉取最新考研大纲） */
export interface ProgressWebSearchConfig {
  enabled: boolean;
  /** 搜索厂商：当前支持 "bocha"（博查查） */
  provider: string;
  base_url: string;
  api_key: string;
}

/** 全部进度表索引 */
export interface ProgressIndex {
  subjects: Record<string, SubjectProgressSet>;
  web_search: ProgressWebSearchConfig;
}

/** 导出/分享用便携格式 */
export interface ProgressTableExport {
  /** 固定标记，用于导入校验 */
  type: "studyagent.progress_table";
  version: 1;
  subject: string;
  name: string;
  nodes: ProgressNode[];
}