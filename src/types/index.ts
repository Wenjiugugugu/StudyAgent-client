/**
 * StudyAgent Core — Type Definitions Barrel Export
 */

export * from "./state";
export * from "./plan";
export * from "./review";
export * from "./ai";
export * from "./mcp";
export * from "./settings";
export * from "./analytics";
export * from "./briefing";

/** 统一 API 响应包装 */
export interface ApiResponse<T = unknown> {
  success: boolean;
  data?: T;
  error?: string;
}

/** 数据备份导入恢复摘要 */
export interface ImportSummary {
  files_restored: number;
  backup_dir: string;
}

/** 教材元信息 */
export interface TextbookInfo {
  id: string;
  subject: string;
  title: string;
  filename: string;
  file_path: string;
}

/** 教材内容 */
export interface TextbookContent {
  id: string;
  content: string;
  file_path: string;
}

/** 教材全文搜索命中 */
export interface TextbookSearchHit {
  textbook_id: string;
  textbook_title: string;
  subject: string;
  line_number: number;
  snippet: string;
  /** 该行实际命中的关键词（供前端高亮片段与正文） */
  matched_terms?: string[];
}

/** 日计划摘要（聚合 plan + review 数据） */
export interface PlanSummary {
  date: string;
  has_plan: boolean;
  has_review: boolean;
  planned_tasks: number;
  planned_hours: number;
  completed_tasks: number;
  completion_rate: number;
  actual_hours: number;
  is_rest_day: boolean;
  /** 是否为周计划中手动添加的特殊情况排除日（出差/生病/考试等） */
  is_excluded: boolean;
  /** 排除日类型：travel / sick / exam / other（仅当 is_excluded=true 时有值） */
  excluded_type?: string;
  /** 排除日备注（仅当 is_excluded=true 时有值） */
  excluded_note?: string;
}

/** 单个 Release 资源（一个安装包） */
export interface UpdateAsset {
  /** 文件名，如 `StudyAgent_0.1.2_x64-setup.exe` */
  name: string;
  /** 直链下载地址 */
  download_url: string;
  /** 文件大小（字节） */
  size: number;
  /** 资源类型推测：`inno` / `nsis`（旧版）/ `msi` / `exe` / `unknown` */
  kind: string;
  /** 必填的文件 SHA-256；后端会过滤缺少校验值的资产 */
  sha256: string;
}

/** 检查更新结果（与后端 `UpdateCheckResult` 对应） */
export interface UpdateCheckResult {
  /** 是否有新版本 */
  has_update: boolean;
  /** 当前版本（如 "0.1.2"） */
  current_version: string;
  /** 远端最新版本号 */
  latest_version: string;
  /** Release 名称（标题） */
  release_name: string;
  /** 发布时间（ISO 8601 字符串） */
  published_at: string;
  /** Release notes（Markdown） */
  release_notes: string;
  /** 可下载的安装包列表 */
  assets: UpdateAsset[];
  /** 用户可读的提示信息（不包含技术细节） */
  message: string;
  /** 是否强制更新（当前版本被远端策略清单禁用时为 true） */
  force_update?: boolean;
  /** 强制更新原因（force_update=true 时展示给用户） */
  force_update_reason?: string;
}

/** 下载进度事件 payload（`update-download-progress` 事件） */
export interface DownloadProgress {
  /** 已下载字节 */
  downloaded: number;
  /** 文件总字节（若服务端未返回 content-length 则为 0） */
  total: number;
  /** 进度百分比 0-100 */
  percent: number;
}

/** Dashboard 汇总数据 */
export interface DashboardSummary {
  date: string;
  remaining_days: number;
  today_tasks: {
    total: number;
    done: number;
    in_progress: number;
    pending: number;
  };
  week_progress: {
    week_start: string;
    week_end: string;
    completed_hours: number;
    target_hours: number;
    daily_breakdown: {
      date: string;
      hours: number;
      tasks_done: number;
    }[];
  };
  current_phase: string;
  streak_days: number;
  total_study_days: number;
  upcoming_deadlines: {
    date: string;
    title: string;
    subject: string;
    priority: string;
  }[];
  review_reminder: {
    last_review_date: string;
    pending_review: boolean;
  };
  subject_progress: {
    subject: string;
    name: string;
    phase: string;
    weekly_hours: number;
    target_score: number;
    completion_percentage: number;
    current_topic: string;
  }[];
}
