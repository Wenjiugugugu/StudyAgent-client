/**
 * 每日简报（Daily Briefing）类型定义
 *
 * 对应后端 `data/briefing.rs` 与 `api/commands.rs` 中的 Briefing 相关结构。
 */

/** 单科进度估算 */
export interface SubjectEstimation {
  /** 科目 key：math / english / politics / professional */
  subject: string;
  /** 当前正在学习的章节/阶段 */
  current_chapter: string;
  /** 预计还需多少天学完当前教材/阶段 */
  estimated_days_to_finish: number;
  /** AI 给出的简短说明 */
  note: string;
}

/** 简报数据体 */
export interface BriefingData {
  /** AI 生成的「今日寄语」 */
  greeting: string;
  /** 各科进度估算 */
  estimations: SubjectEstimation[];
}

/** 简报元信息 */
export interface BriefingMeta {
  /** 简报对应的日期 (YYYY-MM-DD) */
  date: string;
  /** 生成时间 (YYYY-MM-DDTHH:mm) */
  generated_at: string;
  /** 生成该简报所依据的复盘日期 (YYYY-MM-DD) */
  based_on_review: string;
  /** 生成方式：auto（复盘后自动）/ manual（手动重新生成） */
  source: string;
}

/** 简报文件（完整 JSON） */
export interface BriefingFile {
  version: string;
  meta: BriefingMeta;
  data: BriefingData;
}

/**
 * get_briefing 命令的返回结构
 *
 * 包含简报本体与多项判断标志，供前端决定展示哪种状态：
 * - briefing 缺失 + yesterday_review_exists=false → 提示先去复盘
 * - briefing 缺失 + within_makeup_window=false → 错过补复盘窗口，不提供 AI 建议
 * - briefing 存在 → 正常展示
 */
export interface GetBriefingResult {
  /** 简报文件（若存在） */
  briefing?: BriefingFile;
  /** 简报是否存在 */
  exists: boolean;
  /** 昨日复盘是否存在（决定能否提供 AI 建议） */
  yesterday_review_exists: boolean;
  /** 今日是否为休息日 */
  is_rest_day: boolean;
  /** 今日是否为周计划排除日 */
  is_excluded_day: boolean;
  /** 昨日是否为休息日或排除日（若是，则不要求补复盘） */
  yesterday_exempt: boolean;
  /** 是否在补复盘窗口内（今日） */
  within_makeup_window: boolean;
}
