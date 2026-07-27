/**
 * StudyAgent Core — Knowledge Types
 * 对应 assets/knowledge/objects/ (Markdown)
 */

import type { SubjectKey } from "./state";

/** 知识对象类型 */
export type KnowledgeType =
  | "concept"
  | "theorem"
  | "algorithm"
  | "formula"
  | "method"
  | "example"
  | "summary";

/** 知识对象状态 */
export type KnowledgeStatus = "learning" | "reviewing" | "mastered" | "weak";

/** 知识对象章节索引项 */
export interface KnowledgeIndexEntry {
  id: string;
  name: string;
  order: string;
}

/** 知识对象章节 */
export interface KnowledgeChapter {
  title: string;
  entries: KnowledgeIndexEntry[];
}

/** 知识对象学科索引 */
export interface KnowledgeSubjectIndex {
  subject: SubjectKey;
  name: string;
  chapters: KnowledgeChapter[];
}

/** 知识对象详情 */
export interface KnowledgeObject {
  id: string;
  subject: SubjectKey;
  title: string;
  type: KnowledgeType;
  status: KnowledgeStatus;
  content: string;
  prerequisites: string[];
  dependents: string[];
  textbook_ref?: {
    name: string;
    chapter: string;
    page?: string;
  };
  exam_ref?: {
    year: string;
    question: string;
  };
  notes?: string;
  tags: string[];
}

/** 依赖关系图节点 */
export interface KnowledgeGraphNode {
  id: string;
  label: string;
  subject: SubjectKey;
  status: KnowledgeStatus;
}

/** 依赖关系图边 */
export interface KnowledgeGraphEdge {
  source: string;
  target: string;
  type: "prerequisite" | "related";
}

/** 知识图谱 */
export interface KnowledgeGraph {
  nodes: KnowledgeGraphNode[];
  edges: KnowledgeGraphEdge[];
}

/** Mapping 条目（教材/真题映射） */
export interface KnowledgeMapping {
  knowledge_id: string;
  textbook?: {
    name: string;
    chapter: string;
    section: string;
    page?: string;
  };
  exam?: {
    year: string;
    type: string;
    question: string;
    difficulty: "easy" | "medium" | "hard";
  };
}
