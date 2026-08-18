/**
 * StudyAgent Core — Settings Types
 */

// H24：直接导入具体模块，避免与 index.ts barrel 形成循环引用
import type { AIProviderConfig } from "./ai";
import type { MCPServerConfig } from "./mcp";

/** 主题模式 */
export type ThemeMode = "light" | "dark" | "system";

/** 视觉模式：标准 | 液态玻璃 */
export type VisualMode = "standard" | "liquid-glass";

/** 应用设置 */
export interface AppSettings {
  /** 数据目录路径 */
  data_directory: string;
  /** 主题模式 */
  theme: ThemeMode;
  /** 视觉模式（标准 / 液态玻璃） */
  visual_mode?: VisualMode;
  /** 语言 */
  language: "zh-CN" | "en-US";
  /** 用户称呼（用于首页问候） */
  user_name: string;
  /** 首页问候显示开关 */
  show_greeting: boolean;
  /** 考试类型（如 数学一/数学二/数学三/408计算机 等） */
  exam_type: string;
  /** 目标院校 */
  target_school?: string;
  /** 目标专业 */
  target_major?: string;
  /** 考试日期 (YYYY-MM-DD) */
  exam_date: string;
  /** 目标分数 */
  target_score: number;
  /** 引导完成标记 */
  onboarding_completed: boolean;
  /** 学习时间偏好 */
  study_schedule: StudySchedule;
  /** AI Provider 列表 */
  ai_providers: AIProviderConfig[];
  /** 默认 AI Provider ID */
  default_provider_id: string;
  /** MCP Server 列表 */
  mcp_servers: MCPServerConfig[];
  /** 启用的 MCP Server IDs */
  enabled_mcp_ids: string[];
  /** TickTick 配置 */
  ticktick: TickTickConfig;
  /** 窗口配置 */
  window: WindowConfig;
  /** 自定义主色调（hex 格式如 "#5b8def"，空字符串表示使用默认蓝色） */
  accent_color?: string;
  /** 是否显示左上角 Logo */
  show_logo?: boolean;
  /** 自定义背景图相对路径（相对于 data_dir，如 "assets/backgrounds/xxx.png"，空字符串表示无背景图） */
  background_image?: string;
  /** 背景图模糊度（0-20 px，0 为不模糊） */
  background_blur?: number;
  /** 背景图不透明度（0-1，1 为完全不透明） */
  background_opacity?: number;
}

/** 学习时间配置 */
export interface StudySchedule {
  /** 每日学习开始时间 (HH:mm) */
  start_time: string;
  /** 每日学习结束时间 (HH:mm) */
  end_time: string;
  /** 每日目标学习时长（小时） */
  daily_target_hours: number;
  /** 每周学习天数 */
  study_days_per_week: number;
  /** 每周休息日（如 ['周日']） */
  rest_days: string[];
  /** 复盘提醒时间 (HH:mm) */
  review_reminder_time: string;
  /** 各科开始学习日期 (YYYY-MM-DD)，未到该日期前不为该科安排任务 */
  subject_start_dates?: SubjectStartDates;
  /** 用户期望每日任务数量（默认 3，每科约一条，未开始的科目不安排） */
  daily_task_count?: number;
  /** 是否允许 AI 安排总结/复习任务（默认 true，关闭时 AI 只推进新知识点） */
  enable_review_tasks?: boolean;
  /** 是否启用任务计时（默认 false，关闭时不显示计时 UI，State 不写入计时字段） */
  enable_time_tracking?: boolean;
}

/** 各科开始学习日期 */
export interface SubjectStartDates {
  /** 数学开始日期 */
  math?: string;
  /** 英语开始日期 */
  english?: string;
  /** 政治开始日期 */
  politics?: string;
  /** 专业课开始日期 */
  professional?: string;
}

/** TickTick 配置 */
export interface TickTickConfig {
  enabled: boolean;
  /** OAuth 或 Cookie 认证信息 */
  access_token?: string;
  /** 默认项目 ID（学习列表） */
  default_project_id?: string;
  /** 任务标签前缀 */
  tag_prefix: string;
}

/** 窗口配置 */
export interface WindowConfig {
  width: number;
  height: number;
  x?: number;
  y?: number;
  maximized: boolean;
}
