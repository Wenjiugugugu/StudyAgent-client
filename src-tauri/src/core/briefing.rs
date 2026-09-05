//! Briefing Agent — 调用 AI Service 生成每日简报
//!
//! 数据契约：{ version, meta, data }
//! - 简报：records/YYYY-MM-DD_briefing.json
//!
//! 流程：
//! 1. 读取昨日复盘 + 当前 State + 目标日期日计划
//! 2. 构建简报 prompt，要求 AI 输出 JSON
//! 3. 解析、兜底填充、保存 JSON
//! 4. 返回 BriefingFile
//!
//! 触发时机：
//! - 用户提交复盘后自动生成「次日」简报（容错：失败不阻塞复盘）
//! - 用户在 Dashboard 手动重新生成「今日」简报

use std::path::Path;

use crate::ai::provider::{AgentType, ChatMessage, ChatRequest, MessageRole};
use crate::ai::service::AiService;
use crate::data::briefing::{BriefingData, BriefingFile, BriefingMeta, SubjectEstimation};
use crate::data::progress_tables::{NodeLevel, NodeStatus, ProgressIndex};
use crate::data::records::ReviewFile;
use crate::data::state::StudyState;
use crate::data::{now_string, DataResult};

/// Briefing Agent — 每日简报生成器
pub struct BriefingAgent<'a> {
    ai_service: &'a AiService,
}

impl<'a> BriefingAgent<'a> {
    /// 创建新的 Briefing Agent
    pub fn new(ai_service: &'a AiService) -> Self {
        Self { ai_service }
    }

    /// 生成每日简报
    ///
    /// - `target_date`: 简报对应的日期（通常为次日或今日）
    /// - `based_on_review_date`: 生成依据的复盘日期（通常为 target_date 的前一天）
    pub async fn generate_briefing(
        &self,
        data_dir: &Path,
        target_date: &str,
        based_on_review_date: &str,
        source: &str,
    ) -> DataResult<BriefingFile> {
        // 1. 读取昨日复盘（生成依据）
        // M10：区分「文件不存在」（正常，简报可基于空依据生成）与「解析失败」（异常，记录日志）
        let yesterday_review: Option<ReviewFile> =
            match crate::data::records::read_review(data_dir, based_on_review_date) {
                Ok(r) => Some(r),
                Err(e) => {
                    if crate::data::records::review_file_path(data_dir, based_on_review_date)
                        .exists()
                    {
                        log::error!(
                            "[Briefing] 昨日复盘文件存在但解析失败，简报将基于空依据生成: {}",
                            e
                        );
                    }
                    None
                }
            };

        // 2. 读取当前 State
        let state = crate::data::state::read_state_or_default(data_dir);

        // 3. 读取目标日期的日计划（用于让 AI 感知今日任务重点）
        let target_plan =
            match crate::data::plan::read_daily_plan_with_merged_status(data_dir, target_date) {
                Ok(f) => Some(f),
                Err(e) => {
                    if crate::data::plan::daily_plan_path(data_dir, target_date).exists() {
                        log::error!(
                            "[Briefing] 目标日计划文件存在但解析失败，将基于空计划生成: {}",
                            e
                        );
                    }
                    None
                }
            };

        // 4. 读取设置（用于考试日期、每周学习时长等）
        let settings = crate::load_settings(data_dir);

        // 4.5 读取进度表索引（供「阶段估时」按已完成/剩余知识点数估时）
        let progress_index = crate::data::progress_tables::load_progress_index(data_dir);

        // 5. 构建简报 prompt
        let prompt = self.build_briefing_prompt(
            &state,
            yesterday_review.as_ref(),
            target_plan.as_ref().map(|f| &f.data),
            target_date,
            based_on_review_date,
            &settings,
            &progress_index,
        );

        crate::data::write_ai_debug_log(
            data_dir,
            "briefing_prompt_ready",
            &format!(
                "简报 prompt 已构建, target_date={}, based_on_review={}, 长度={} 字符",
                target_date,
                based_on_review_date,
                prompt.len()
            ),
        );

        // 6. 调用 AI Service
        let request = ChatRequest {
            messages: vec![ChatMessage {
                role: MessageRole::User,
                content: prompt,
                ..Default::default()
            }],
            agent: Some(AgentType::Briefing),
            temperature: Some(0.6),
            // 简报生成相对轻量，但给足超时避免慢模型失败
            timeout_override: Some(180),
            ..Default::default()
        };

        crate::data::write_ai_debug_log(
            data_dir,
            "briefing_ai_request",
            &format!(
                "即将发送简报 AI 请求, target_date={}, timeout=180s",
                target_date
            ),
        );

        let response = self.ai_service.chat(request).await.map_err(|e| {
            crate::data::write_ai_debug_log(
                data_dir,
                "briefing_ai_call_error",
                &format!("简报 AI 调用失败: {}", e),
            );
            format!("AI 生成简报失败: {}", e)
        })?;

        crate::data::write_ai_debug_log(
            data_dir,
            "briefing_ai_response",
            &format!(
                "简报 AI 返回, 长度={} 字符, 前 300 字符: {}",
                response.content.len(),
                response.content.chars().take(300).collect::<String>()
            ),
        );

        // 7. 解析 AI 返回的 JSON
        let briefing_data = parse_briefing_json(&response.content)?;

        // 8. 组装 BriefingFile
        let briefing = BriefingFile {
            version: "1.0.0".to_string(),
            meta: BriefingMeta {
                date: target_date.to_string(),
                generated_at: now_string(),
                based_on_review: based_on_review_date.to_string(),
                source: source.to_string(),
            },
            data: briefing_data,
        };

        // 9. 保存
        crate::data::briefing::save_briefing(data_dir, &briefing)?;

        log::info!(
            "每日简报已生成: target_date={}, based_on_review={}",
            target_date,
            based_on_review_date
        );

        Ok(briefing)
    }

    /// 构建简报 prompt
    #[allow(clippy::too_many_arguments)]
    fn build_briefing_prompt(
        &self,
        state: &StudyState,
        yesterday_review: Option<&ReviewFile>,
        target_plan: Option<&crate::data::plan::DailyPlanData>,
        target_date: &str,
        based_on_review_date: &str,
        settings: &crate::AppSettings,
        progress_index: &ProgressIndex,
    ) -> String {
        let mut prompt = String::new();

        prompt.push_str(&format!(
            "请为 {} 生成每日简报。该简报将在用户打开应用时展示，作为当日的学习指引。\n\n",
            target_date
        ));

        // 考试倒计时
        let remaining_days =
            crate::data::days_between(&state.meta.exam_date, target_date).unwrap_or(0);
        prompt.push_str("## 考试信息\n");
        prompt.push_str(&format!(
            "- 考试日期: {}\n- 距离考研还有: {} 天\n- 目标院校: {} {}\n\n",
            state.meta.exam_date, remaining_days, state.meta.target_school, state.meta.target_major
        ));

        // 昨日复盘摘要
        prompt.push_str("## 昨日复盘（生成依据）\n");
        prompt.push_str(&format!("- 复盘日期: {}\n", based_on_review_date));
        if let Some(review) = yesterday_review {
            // 完成率：优先从 task_reviews 聚合（兼容旧版 data.completion 全零的复盘文件）
            let (a_total, a_done, b_total, b_done, comp_rate) =
                crate::data::records::review_completion_stats(review);
            let rate = comp_rate.round() as i64;
            prompt.push_str(&format!(
                "- 完成情况: A 级 {}/{}, B 级 {}/{}, 总完成率 {}%\n",
                a_done, a_total, b_done, b_total, rate
            ));

            // 实际学习时长（无任何计时数据时不写入 prompt，避免 AI 据 0 值编造「昨日未计时」类表述）
            let has_time_data = review.data.total_hours > 0.0
                || review
                    .task_reviews
                    .iter()
                    .any(|tr| tr.actual_minutes.unwrap_or(0) > 0);
            if has_time_data {
                let actual_hours = crate::data::records::review_actual_hours(review);
                prompt.push_str(&format!("- 实际学习时长: {:.1} 小时\n", actual_hours));
            }

            // 整体感受
            if let Some(daily) = &review.daily_review {
                let feeling_label = match daily.overall_feeling.as_str() {
                    "smooth" => "顺利",
                    "normal" => "一般",
                    "hard" => "困难",
                    _ => "未填写",
                };
                prompt.push_str(&format!("- 整体感受: {}\n", feeling_label));
                if !daily.main_difficulty.is_empty() {
                    let diff_label = match daily.main_difficulty.as_str() {
                        "understanding" => "理解困难",
                        "problems" => "解题困难",
                        "memorization" => "记忆困难",
                        "attention" => "注意力不集中",
                        "time_management" => "时间管理",
                        "environment" => "环境干扰",
                        _ => "其他",
                    };
                    prompt.push_str(&format!("- 主要困难: {}\n", diff_label));
                }
            }

            // 任务掌握度摘要
            let mastered_count = review
                .task_reviews
                .iter()
                .filter(|t| t.mastery == "mastered")
                .count();
            let weak_count = review
                .task_reviews
                .iter()
                .filter(|t| t.mastery == "weak")
                .count();
            if !review.task_reviews.is_empty() {
                prompt.push_str(&format!(
                    "- 任务掌握: {} 项已掌握, {} 项需巩固\n",
                    mastered_count, weak_count
                ));
            }

            // 计划外进展（用户实际进度声明）
            if !review.overcompletion.is_empty() {
                prompt.push_str("- 计划外进展: ");
                let oc_summary: Vec<String> = review
                    .overcompletion
                    .iter()
                    .map(|oc| format!("{}-{}", oc.subject, oc.chapter_reached))
                    .collect();
                prompt.push_str(&oc_summary.join(", "));
                prompt.push('\n');
            }
        } else {
            prompt.push_str("- 未找到昨日复盘记录（用户可能未提交复盘）\n");
        }
        prompt.push('\n');

        // 今日任务重点（来自目标日期日计划）
        prompt.push_str("## 今日任务重点\n");
        if let Some(plan) = target_plan {
            if plan.tasks.is_empty() {
                prompt.push_str("- 今日暂无计划任务\n");
            } else {
                for task in &plan.tasks {
                    let subj_cn = match task.subject {
                        crate::data::state::SubjectKey::Math => "数学",
                        crate::data::state::SubjectKey::English => "英语",
                        crate::data::state::SubjectKey::Politics => "政治",
                        crate::data::state::SubjectKey::Professional => "专业课",
                    };
                    let priority_label = match task.priority {
                        crate::data::state::TaskPriority::A => "A",
                        crate::data::state::TaskPriority::B => "B",
                        crate::data::state::TaskPriority::C => "C",
                    };
                    prompt.push_str(&format!(
                        "- [P{}] {}：{}\n",
                        priority_label, subj_cn, task.title
                    ));
                }
            }
        } else {
            prompt.push_str("- 今日暂无日计划\n");
        }
        prompt.push('\n');

        // 各科当前进度
        prompt.push_str("## 各科当前进度\n");
        let daily_target_hours = settings.daily_target_hours();
        let study_days_per_week = settings.study_days_per_week();

        let push_subject =
            |key: &str, name: &str, subj: &crate::data::state::SubjectState, p: &mut String| {
                if !subj.active {
                    return;
                }
                p.push_str(&format!("### {}（{}）\n", name, key));
                p.push_str(&format!("- 阶段: {:?}\n", subj.phase));
                p.push_str(&format!("- 每周时长: {}h\n", subj.weekly_hours));
                p.push_str(&format!("- 当前重点: {}\n", subj.current_focus));
                p.push_str(&format!(
                    "- 已完成章节 ({}): {:?}\n",
                    subj.completed.len(),
                    subj.completed
                ));
                // 进度表统计：已完成/剩余知识点数 + 剩余预估总时长（供 AI 更准确地估时）
                if let Some((done, total)) = progress_table_summary(progress_index, key) {
                    let remain_hours = progress_table_remaining_hours(progress_index, key)
                        .map(|h| format!("，预估共需约 {} 小时", h))
                        .unwrap_or_default();
                    p.push_str(&format!(
                        "- 进度表: 已完成 {}/{} 个知识点（剩余 {} 个{}）\n",
                        done,
                        total,
                        total.saturating_sub(done),
                        remain_hours
                    ));
                }
                if !subj.weak_chapters.is_empty() {
                    p.push_str(&format!("- 薄弱章节: {:?}\n", subj.weak_chapters));
                }
                p.push_str(&format!(
                    "- 教材: {}\n\n",
                    subj.textbook.as_deref().unwrap_or("未指定")
                ));
            };

        push_subject("math", "数学", &state.subjects.math, &mut prompt);
        push_subject("english", "英语", &state.subjects.english, &mut prompt);
        push_subject("politics", "政治", &state.subjects.politics, &mut prompt);
        push_subject(
            "professional",
            "专业课",
            &state.subjects.professional,
            &mut prompt,
        );

        // 学习节奏参考
        prompt.push_str("## 学习节奏参考\n");
        prompt.push_str(&format!(
            "- 每日目标学习时长: {} 小时\n- 每周学习天数: {} 天\n\n",
            daily_target_hours, study_days_per_week
        ));

        // 生成要求
        prompt.push_str("## 生成要求\n");
        prompt.push_str("1. **estimations**：为每个 active 科目估算「学完当前阶段还需多少天」。");
        prompt.push_str(&format!(
            "参考因素：剩余备考天数（{} 天）、每周学习天数（{}）、各科每周时长、各科进度表的已完成/剩余知识点数（优先按剩余知识点数与每知识点所需的专注时长估算）。",
            remaining_days, study_days_per_week
        ));
        prompt.push_str("estimated_days_to_finish 为正整数；note 用一句话说明依据（如「按每周 6h 推进，约 2 周完成基础阶段」）。\n");
        prompt.push_str("2. 严格输出 JSON，不包裹 ```json 代码块。结构：{\"estimations\": [{\"subject\": \"...\", \"current_chapter\": \"...\", \"estimated_days_to_finish\": N, \"note\": \"...\"}]}。\n");

        prompt
    }
}

// ============================================================================
// 进度表统计（供「阶段估时」参考）
// ============================================================================

/// 从进度表索引统计某科进度，返回 `(已完成知识点数, 总知识点数)`。
///
/// 统计该科全部进度表（专业课含总表与各教材表）的 knowledge 节点；
/// "已完成" = 状态达「基础」及以上（basic/reinforcing/mastered），
/// learning/pending 视为剩余。无进度表或无知识点时返回 None。
fn progress_table_summary(index: &ProgressIndex, subject: &str) -> Option<(usize, usize)> {
    let set = index.subjects.get(subject)?;
    if set.tables.is_empty() {
        return None;
    }
    let mut done = 0usize;
    let mut total = 0usize;
    for table in &set.tables {
        for node in &table.nodes {
            if node.level != NodeLevel::Knowledge {
                continue;
            }
            total += 1;
            if node.status.rank() >= NodeStatus::Basic.rank() {
                done += 1;
            }
        }
    }
    if total == 0 {
        None
    } else {
        Some((done, total))
    }
}

/// 统计某科**启用**进度表剩余（pending/learning）知识点的预估总时长（小时）。
///
/// 依据节点的隐藏预估值 `estimated_hours`（缺失时按标题确定性估算回退），
/// 供简报 AI prompt 与无 AI 兜底估时共用，保证「阶段估时」口径一致。
/// 无启用表或无剩余知识点时返回 None。
fn progress_table_remaining_hours(index: &ProgressIndex, subject: &str) -> Option<f64> {
    use crate::data::progress_tables::ProgressNode;

    let set = index.subjects.get(subject)?;
    let active_id = if set.active_id.is_empty() {
        set.tables.first()?.id.clone()
    } else {
        set.active_id.clone()
    };
    let table = set.tables.iter().find(|t| t.id == active_id)?;
    let remaining: Vec<&ProgressNode> = table
        .nodes
        .iter()
        .filter(|n| n.level == NodeLevel::Knowledge)
        .filter(|n| matches!(n.status, NodeStatus::Pending | NodeStatus::Learning))
        .collect();
    if remaining.is_empty() {
        return None;
    }
    let total: f64 = remaining
        .iter()
        .map(|n| {
            n.estimated_hours.unwrap_or_else(|| {
                crate::core::estimated_time::estimate_knowledge_hours(subject, &n.title)
            })
        })
        .sum();
    Some(crate::core::estimated_time::round1(total))
}

// ============================================================================
// 确定性「阶段估时」（无 AI 兜底）
// ============================================================================

/// 无 AI 时的确定性「阶段估时」兜底，供首页在未配置 AI / AI 不可用 / 未生成简报时正常显示估时。
///
/// 依据内置/启用进度表的**隐藏预估时长**（`ProgressNode.estimated_hours`，缺失时按标题
/// 确定性估算），逐知识点按自适应周计划学到的复合调整系数（`EstimateAdjustment`：
/// 实际/预估时间比效率 + 任务量反馈 + 完成率 + 置信度）校准后求和得到「剩余预估总时长」，
/// 再除以该科每日学习时长，得到「预计还需多少天学完剩余内容」。
///
/// 与 AI 简报 `estimations` 结构一致，前端无需区分来源即可展示。
/// 仅在某科存在启用进度表且有剩余知识点时返回该科；无任何数据时返回空。
pub fn deterministic_estimations(data_dir: &Path) -> Vec<SubjectEstimation> {
    use crate::core::estimated_time::{adjust_hours, estimate_knowledge_hours, EstimateAdjustment};
    use crate::data::progress_tables::{load_progress_index, NodeLevel, NodeStatus, ProgressNode};

    let state = crate::data::state::read_state_or_default(data_dir);
    let settings = crate::load_settings(data_dir);
    let index = load_progress_index(data_dir);
    let adaptive = crate::core::adaptive_planner::read_adaptive_state(data_dir).unwrap_or_default();
    let study_days = (7usize.saturating_sub(settings.rest_days().len())).max(1) as f64;

    let subjects = [
        ("math", &state.subjects.math),
        ("english", &state.subjects.english),
        ("politics", &state.subjects.politics),
        ("professional", &state.subjects.professional),
    ];
    let mut out = Vec::new();
    for (key, subj) in subjects {
        if !subj.active {
            continue;
        }
        let Some(set) = index.subjects.get(key) else {
            continue;
        };
        let active_id = if set.active_id.is_empty() {
            match set.tables.first() {
                Some(t) => t.id.clone(),
                None => continue,
            }
        } else {
            set.active_id.clone()
        };
        let Some(table) = set.tables.iter().find(|t| t.id == active_id) else {
            continue;
        };
        // 待学知识点（pending/learning 视为剩余）
        let pending: Vec<&ProgressNode> = table
            .nodes
            .iter()
            .filter(|n| n.level == NodeLevel::Knowledge)
            .filter(|n| matches!(n.status, NodeStatus::Pending | NodeStatus::Learning))
            .collect();
        if pending.is_empty() {
            continue;
        }
        // 复合调整系数：从自适应周计划状态读取（效率 + 反馈），完成率无周分析上下文时取中性 0.85
        let entry = adaptive.subjects.get(key).cloned().unwrap_or_default();
        let adjustment = EstimateAdjustment {
            efficiency_factor: entry.estimation_factor,
            feedback_signal: adaptive.workload_ema,
            completion_rate: 0.85,
            confidence: if entry.estimation_samples > 0.0 {
                0.6
            } else {
                0.35
            },
        };
        // 剩余预估总时长（逐知识点调整后求和）
        let remaining_hours: f64 = pending
            .iter()
            .map(|n| {
                let base = n
                    .estimated_hours
                    .unwrap_or_else(|| estimate_knowledge_hours(key, &n.title));
                adjust_hours(base, &adjustment, &n.title)
            })
            .sum();
        // 该科每日学习时长：周学时 / 每周学习天数
        let daily_hours = if subj.weekly_hours > 0.0 {
            subj.weekly_hours / study_days
        } else {
            0.0
        };
        if daily_hours <= 0.0 || remaining_hours <= 0.0 {
            continue;
        }
        let days = (remaining_hours / daily_hours).ceil() as i32;
        let current_chapter = pending
            .iter()
            .find(|n| !n.phase.trim().is_empty())
            .map(|n| n.phase.trim().to_string())
            .unwrap_or_default();
        let note = format!(
            "按内置考纲预估（含效率校准），剩余约 {:.0}h，日均约 {:.1}h",
            remaining_hours, daily_hours
        );
        out.push(SubjectEstimation {
            subject: key.to_string(),
            current_chapter,
            estimated_days_to_finish: days,
            note,
        });
    }
    out
}

// ============================================================================
// JSON 解析
// ============================================================================

/// 解析 AI 返回的简报 JSON
///
/// 兼容 AI 可能包裹 ```json 代码块的情况
fn parse_briefing_json(content: &str) -> DataResult<BriefingData> {
    let trimmed = content.trim();

    // 去除可能的 ```json ... ``` 包裹
    let json_str = if trimmed.starts_with("```") {
        let inner = trimmed
            .trim_start_matches("```json")
            .trim_start_matches("```")
            .trim_end_matches("```")
            .trim();
        inner
    } else {
        trimmed
    };

    // 尝试直接解析为 BriefingData
    if let Ok(data) = serde_json::from_str::<BriefingData>(json_str) {
        return Ok(data);
    }

    // 兼容：AI 可能返回完整 { version, meta, data } 结构
    if let Ok(full) = serde_json::from_str::<serde_json::Value>(json_str) {
        if let Some(data_val) = full.get("data") {
            if let Ok(data) = serde_json::from_value::<BriefingData>(data_val.clone()) {
                return Ok(data);
            }
        }
        // 兼容：AI 可能将 greeting/estimations 直接放在顶层
        if full.get("greeting").is_some() || full.get("estimations").is_some() {
            if let Ok(data) = serde_json::from_value::<BriefingData>(full.clone()) {
                return Ok(data);
            }
        }
    }

    // 解析失败：返回错误，避免保存无效的空简报文件
    let preview = content.chars().take(500).collect::<String>();
    log::warn!("简报 JSON 解析失败。原始内容前 500 字符: {}", preview);
    Err(format!(
        "简报 JSON 解析失败，AI 返回内容无法识别。前 500 字符: {}",
        preview
    ))
}

/// 计算目标日期的「昨日」日期（用于读取复盘）
///
/// 返回基于 target_date 前一天的日期字符串
pub fn yesterday_of(target_date: &str) -> DataResult<String> {
    crate::data::add_days(target_date, -1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_briefing_json_handles_plain_json() {
        let json = r#"{"greeting": "今天加油", "estimations": []}"#;
        let data = parse_briefing_json(json).unwrap();
        assert_eq!(data.greeting, "今天加油");
        assert!(data.estimations.is_empty());
    }

    #[test]
    fn parse_briefing_json_handles_code_block() {
        let json = r#"```json
{"greeting": "今天加油", "estimations": [{"subject": "math", "current_chapter": "微分方程", "estimated_days_to_finish": 10, "note": "约2周"}]}
```"#;
        let data = parse_briefing_json(json).unwrap();
        assert_eq!(data.greeting, "今天加油");
        assert_eq!(data.estimations.len(), 1);
        assert_eq!(data.estimations[0].subject, "math");
        assert_eq!(data.estimations[0].estimated_days_to_finish, 10);
    }

    #[test]
    fn parse_briefing_json_returns_err_on_invalid() {
        let json = "not a json at all";
        let result = parse_briefing_json(json);
        assert!(result.is_err());
    }

    #[test]
    fn parse_briefing_json_handles_subject_estimation_default() {
        let json = r#"{"greeting": "测试", "estimations": [{"subject": "english"}]}"#;
        let data = parse_briefing_json(json).unwrap();
        assert_eq!(data.estimations[0].subject, "english");
        assert_eq!(data.estimations[0].estimated_days_to_finish, 0);
    }

    #[test]
    fn progress_table_summary_counts_done_and_total() {
        use crate::data::progress_tables::{
            ProgressNode, ProgressTable, SubjectProgressSet, WebSearchConfig,
        };
        use std::collections::HashMap;

        let node = |title: &str, level: NodeLevel, status: NodeStatus| ProgressNode {
            id: title.to_string(),
            title: title.to_string(),
            level,
            parent_id: None,
            phase: "章".to_string(),
            status,
            planned_date: None,
            note: String::new(),
            estimated_hours: None,
        };
        let table = ProgressTable {
            id: "t1".into(),
            subject: "math".into(),
            variant: "数二".into(),
            name: "测试".into(),
            origin: crate::data::progress_tables::TableOrigin::Custom,
            created_at: String::new(),
            updated_at: String::new(),
            nodes: vec![
                node("章节", NodeLevel::Chapter, NodeStatus::Pending),
                node("k1", NodeLevel::Knowledge, NodeStatus::Basic),
                node("k2", NodeLevel::Knowledge, NodeStatus::Pending),
                node("k3", NodeLevel::Knowledge, NodeStatus::Mastered),
            ],
        };
        let mut subjects = HashMap::new();
        subjects.insert(
            "math".to_string(),
            SubjectProgressSet {
                active_variant: "数二".into(),
                active_id: "t1".into(),
                tables: vec![table],
            },
        );
        let index = ProgressIndex {
            subjects,
            web_search: WebSearchConfig::default(),
        };
        // basic + mastered = 2 个已完成；章节节点不计入总数
        let (done, total) = progress_table_summary(&index, "math").expect("应有统计");
        assert_eq!(done, 2);
        assert_eq!(total, 3);
        // 无进度表的科目返回 None
        assert!(progress_table_summary(&index, "english").is_none());
    }

    #[test]
    fn deterministic_estimations_falls_back_without_ai() {
        // 构造临时数据目录：state（math 启用，周学时 12）+ 内置考纲进度表
        let dir =
            std::env::temp_dir().join(format!("studyagent_briefing_test_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("state")).unwrap();
        std::fs::create_dir_all(dir.join("progress_tables")).unwrap();

        std::fs::write(
            dir.join("state").join("current.state"),
            "[subjects.math]\nactive = true\nweekly_hours = 12.0\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("progress_tables").join("progress_index.json"),
            r#"{
  "subjects": {
    "math": {
      "active_variant": "数二",
      "active_id": "t1",
      "tables": [
        {
          "id": "t1", "subject": "math", "variant": "数二", "name": "数学内置",
          "origin": "builtin", "created_at": "", "updated_at": "",
          "nodes": [
            {"id":"c1","title":"第一章","level":"chapter","parent_id":null,"phase":"第一章","status":"pending","planned_date":null,"note":"","estimated_hours":4.5},
            {"id":"n1","title":"函数的概念","level":"knowledge","parent_id":"c1","phase":"第一章","status":"pending","planned_date":null,"note":"","estimated_hours":1.0},
            {"id":"n2","title":"数列极限","level":"knowledge","parent_id":"c1","phase":"第一章","status":"basic","planned_date":null,"note":"","estimated_hours":1.5},
            {"id":"n3","title":"微分中值定理","level":"knowledge","parent_id":"c1","phase":"第一章","status":"pending","planned_date":null,"note":"","estimated_hours":2.0}
          ]
        }
      ]
    }
  },
  "web_search": {"enabled": false, "provider": "", "base_url": "", "api_key": ""}
}"#,
        )
        .unwrap();

        let est = deterministic_estimations(&dir);
        let _ = std::fs::remove_dir_all(&dir);

        // 仅 math 有启用表且有剩余知识点；n2 为 basic（已完成）不计入剩余
        let math = est
            .iter()
            .find(|e| e.subject == "math")
            .expect("应有 math 估时");
        assert_eq!(math.current_chapter, "第一章");
        // 剩余 = n1(1.0) + n3(2.0) = 3.0h；默认学习天数 6，math 日均 12/6=2h → 2 天
        assert_eq!(math.estimated_days_to_finish, 2);
        assert!(math.note.contains("内置考纲"));
        // 无进度表的科目不输出
        assert!(!est.iter().any(|e| e.subject == "english"));
    }
}
