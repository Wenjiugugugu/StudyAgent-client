//! Planner — 调用 AI Service 生成周计划，再通过 DailyScheduler 生成日计划
//!
//! 数据契约：统一 JSON 格式 { version, meta, data, view? }
//! - 周计划：plan/YYYY-Www_week.json
//! - 日计划：plan/YYYY-MM-DD_day.json

use std::path::Path;

use crate::ai::provider::{AgentType, ChatMessage, ChatRequest, MessageRole};
use crate::ai::service::AiService;
use crate::core::scheduler::DailyScheduler;
use crate::data::plan::{
    BasedOn, DailyPlanFile, WeekDayPlan, WeekPlanData, WeekPlanFile,
};
use crate::data::state::StudyState;
use crate::data::{
    add_days, days_between, get_week_end, get_week_start, iso_week_string, now_string, today_string,
    weekday_name, DataResult,
};

/// Planner — 计划生成器
pub struct Planner<'a> {
    ai_service: &'a AiService,
}

impl<'a> Planner<'a> {
    /// 创建新的 Planner
    pub fn new(ai_service: &'a AiService) -> Self {
        Self { ai_service }
    }

    /// 生成日计划
    ///
    /// 流程：
    /// 1. 校验今天/休息日
    /// 2. 通过 DailyScheduler 从当前周计划生成日计划
    /// 3. 保存日计划 JSON
    /// 4. 同步 State.current_task
    /// 5. 返回 DailyPlanFile
    pub async fn generate_daily_plan(
        &self,
        data_dir: &Path,
        date: &str,
    ) -> DataResult<DailyPlanFile> {
        // 0. 只允许生成今天的计划
        let today = today_string();
        if date != today {
            return Err(format!(
                "只能生成今天的学习计划（今天是 {}），不支持生成过去或未来的计划",
                today
            ));
        }

        // 0.1 检查是否为休息日
        let settings = crate::load_settings(data_dir);
        let weekday = weekday_name(date)?;
        if settings.rest_days().contains(&weekday) {
            return Err(format!("{}（{}）是休息日，不生成学习计划", date, weekday));
        }

        self.generate_daily_plan_internal(data_dir, date).await
    }

    /// 内部：生成日计划（不校验今天/休息日，供周计划生成使用）
    async fn generate_daily_plan_internal(
        &self,
        data_dir: &Path,
        date: &str,
    ) -> DataResult<DailyPlanFile> {
        // 1. 通过 Scheduler 从周计划生成日计划（不调用 AI）
        let plan = DailyScheduler::generate_daily_plan(data_dir, date)?;

        // 2. 保存日计划
        crate::data::plan::save_daily_plan(data_dir, &plan)?;

        // 3. 同步到 State.current_task
        if let Err(e) = Self::sync_current_task(data_dir, date, &plan) {
            log::warn!("同步 current_task 失败: {}", e);
        }

        Ok(plan)
    }

    /// 将生成的日计划同步到 State.current_task
    /// 仅在 state 中当前日期任务为空或日期不匹配时才写入，避免覆盖已有的完成状态
    fn sync_current_task(data_dir: &Path, date: &str, plan: &DailyPlanFile) -> DataResult<()> {
        let mut state = crate::data::state::read_state(data_dir).unwrap_or_default();

        let need_init = state.current_task.date != date || state.current_task.tasks.is_empty();
        if !need_init {
            return Ok(()); // 已存在今天任务（可能已有完成标记），不覆盖
        }

        let tasks: Vec<crate::data::state::StateTask> = plan
            .data
            .tasks
            .iter()
            .map(|task| crate::data::state::StateTask {
                task_id: Some(task.id.clone()),
                subject: format!("{:?}", task.subject).to_lowercase(),
                task: task.title.clone(),
                priority: task.priority.clone(),
                status: task.status.clone(),
            })
            .collect();

        state.current_task = crate::data::state::CurrentTask {
            date: date.to_string(),
            focus: plan.data.strategy.clone(),
            total_hours: Some(plan.data.total_hours),
            tasks,
            note: String::new(),
        };

        crate::data::state::save_state(data_dir, &state)
    }

    /// 生成周计划
    ///
    /// 流程：
    /// 1. 读取 State、User Model
    /// 2. 调用 AI 生成 WeekPlanFile JSON（包含 data 和 view）
    /// 3. 保存为 plan/YYYY-Www_week.json
    /// 4. 逐天通过 DailyScheduler 生成日计划（跳过休息日）
    /// 5. 返回 WeekPlanFile
    pub async fn generate_week_plan(
        &self,
        data_dir: &Path,
        week_start: &str,
    ) -> DataResult<WeekPlanFile> {
        let week_end = get_week_end(week_start)?;

        // 0. 只允许生成本周计划（包含今天的周）
        let today = today_string();
        let today_week_start = get_week_start(&today)?;
        if week_start != &today_week_start {
            return Err(format!(
                "只能生成本周（{} 至 {}）的周计划，不支持生成过去或未来的周计划",
                today_week_start,
                get_week_end(&today_week_start)?
            ));
        }

        // 1. 读取 State 和 User Model
        let state = crate::data::state::read_state(data_dir).unwrap_or_default();
        let user_model = crate::data::assets::read_user_model_index(data_dir).unwrap_or_default();
        let recent_reviews = Self::read_recent_reviews(data_dir, week_start, 5);
        let exam_config = Self::read_exam_config(data_dir);
        let knowledge_graph = Self::read_knowledge_graph_summary(data_dir);
        let settings = crate::load_settings(data_dir);

        // 1.1 读取上一周的 7 个日计划及其对应复盘，作为本周排程参考
        let prev_week_start = add_days(week_start, -7)?;
        let prev_week_end = get_week_end(&prev_week_start)?;
        let prev_week_daily_plans =
            crate::data::plan::read_week_daily_plans(data_dir, &prev_week_start).unwrap_or_default();
        let prev_week_reviews = Self::read_reviews_in_range(data_dir, &prev_week_start, &prev_week_end);

        // 2. 构建周计划 prompt
        let rest_days = settings.rest_days();
        let subject_start_dates = settings.subject_start_dates();
        let daily_task_count = settings.daily_task_count();
        let enable_review_tasks = settings.enable_review_tasks();
        let prompt = self.build_week_plan_prompt(
            &state,
            &user_model,
            &recent_reviews,
            &exam_config,
            &knowledge_graph,
            week_start,
            &week_end,
            &rest_days,
            &subject_start_dates,
            daily_task_count,
            enable_review_tasks,
            &prev_week_daily_plans,
            &prev_week_reviews,
        );

        // 3. 调用 AI 生成周计划 JSON
        let request = ChatRequest {
            messages: vec![ChatMessage {
                role: MessageRole::User,
                content: prompt,
                ..Default::default()
            }],
            agent: Some(AgentType::Planner),
            temperature: Some(0.7),
            // 周计划 JSON 生成可能较慢（大 prompt + 长输出），
            // 覆盖 Provider 默认 timeout 至 300s
            timeout_override: Some(300),
            ..Default::default()
        };

        let response = self
            .ai_service
            .chat(request)
            .await
            .map_err(|e| format!("AI 生成周计划失败: {}", e))?;

        // 调试日志：AI 返回的原始周计划 JSON（解析前）
        log::info!(
            "[AI-DEBUG] 周计划原始响应长度: {} 字符, 前 500 字符: {}",
            response.content.len(),
            response.content.chars().take(500).collect::<String>()
        );
        log::debug!(
            "[AI-DEBUG] 周计划原始响应全文:\n{}",
            response.content
        );

        let mut week_plan = parse_week_plan_json(&response.content, week_start, &week_end)?;

        // 3.1 后置校验：强制周计划的 is_rest_day 与用户设置一致
        enforce_rest_days(&mut week_plan, &rest_days, week_start, &week_end)?;

        // 4. 保存周计划 JSON
        crate::data::plan::save_week_plan(data_dir, &week_plan)?;

        // 5. 逐天生成日计划（跳过休息日）
        let mut current_date = week_start.to_string();

        loop {
            let weekday = weekday_name(&current_date)?;
            if !settings.rest_days().contains(&weekday) {
                match self.generate_daily_plan_internal(data_dir, &current_date).await {
                    Ok(_) => {}
                    Err(e) => log::warn!("生成 {} 的日计划失败: {}", current_date, e),
                }
            } else {
                log::info!("{}（{}）是休息日，跳过生成", current_date, weekday);
            }

            if current_date == week_end {
                break;
            }
            current_date = add_days(&current_date, 1)?;
        }

        Ok(week_plan)
    }

    /// 构建周计划 prompt
    fn build_week_plan_prompt(
        &self,
        state: &StudyState,
        user_model: &crate::data::assets::UserModelIndex,
        recent_reviews: &[crate::data::records::ReviewFile],
        exam_config: &str,
        knowledge_graph: &str,
        week_start: &str,
        week_end: &str,
        rest_days: &[String],
        subject_start_dates: &[(&'static str, String)],
        daily_task_count: i64,
        enable_review_tasks: bool,
        prev_week_daily_plans: &[DailyPlanFile],
        prev_week_reviews: &[crate::data::records::ReviewFile],
    ) -> String {
        let remaining = days_between(&state.meta.exam_date, week_start).unwrap_or(0);
        let iso_week = iso_week_string(week_start).unwrap_or_else(|_| "YYYY-Www".to_string());

        let mut prompt = String::new();
        prompt.push_str(&format!(
            "请为 {}（{} 至 {}）生成考研学习周计划。请严格遵循以下数据和输出规范。\n\n",
            iso_week, week_start, week_end
        ));

        // 考试信息
        prompt.push_str("## 考试信息\n");
        if !exam_config.is_empty() {
            prompt.push_str(exam_config);
            prompt.push_str("\n");
        }
        prompt.push_str(&format!(
            "- 考试日期: {}\n- 剩余天数: {} 天\n- 目标院校: {} {}\n- 总分目标: {} / 500\n\n",
            state.meta.exam_date,
            remaining,
            state.meta.target_school,
            state.meta.target_major,
            state.subjects.math.target_score
                + state.subjects.english.target_score
                + state.subjects.politics.target_score
                + state.subjects.professional.target_score
        ));

        // 学习日程配置
        prompt.push_str("## 学习日程\n");
        if !rest_days.is_empty() {
            prompt.push_str(&format!(
                "- 用户休息日：{}（这些日子 is_rest_day 必须为 true，且不分配任何任务）\n",
                rest_days.join("、")
            ));
            prompt.push_str(&format!(
                "- 每周学习天数：{} 天\n",
                7 - rest_days.len()
            ));
        }
        prompt.push_str(&format!(
            "- 用户期望每日任务数量：{} 个（每科约一条；同时遵循各科开始学习日期，未开始的科目不安排任务，相应减少当日任务数）\n",
            daily_task_count
        ));
        prompt.push_str(&format!(
            "- 是否安排总结/复习任务：{}（{}）\n\n",
            if enable_review_tasks { "允许" } else { "禁止" },
            if enable_review_tasks {
                "可在 task_templates 中安排'回顾'/'总结'/'复习'类任务以巩固知识"
            } else {
                "严禁安排'回顾'/'总结'/'复习'/'梳理'类任务，每日任务必须推进新知识点或新章节"
            }
        ));

        // 各科开始学习日期（未到开始日期的科目本周不得安排任务）
        if !subject_start_dates.is_empty() {
            prompt.push_str("## 各科开始学习日期（重要约束）\n");
            prompt.push_str("以下科目设有开始学习日期。本周（");
            prompt.push_str(week_start);
            prompt.push_str(" 至 ");
            prompt.push_str(week_end);
            prompt.push_str("）若某科目开始日期晚于本周日（");
            prompt.push_str(week_end);
            prompt.push_str("），则该科目本周不得安排任何任务，不出现在 subjects 与 subject_allocations 中。\n\n");
            let subject_cn = |key: &str| -> &'static str {
                match key {
                    "math" => "数学",
                    "english" => "英语",
                    "politics" => "政治",
                    "professional" => "专业课",
                    _ => "未知",
                }
            };
            for (key, date) in subject_start_dates {
                let not_started = date.as_str() > week_end;
                prompt.push_str(&format!(
                    "- {}（{}）：开始日期 {}{}。若未开始，本周禁止为其安排任务。\n",
                    subject_cn(key),
                    key,
                    date,
                    if not_started {
                        "（本周尚未开始）"
                    } else {
                        "（本周已开始，可正常安排）"
                    }
                ));
            }
            prompt.push_str("\n");
        }

        // 各科状态
        prompt.push_str("## 当前各科状态\n\n");
        prompt.push_str(&format!(
            "### 数学（数二）\n- 阶段: {:?}\n- 每周时长: {}h\n- 目标分数: {}\n- 当前重点: {}\n- 薄弱章节: {:?}\n- 已完成: {:?}\n- 教材: {}\n- 状态: {}\n\n",
            state.subjects.math.phase,
            state.subjects.math.weekly_hours,
            state.subjects.math.target_score,
            state.subjects.math.current_focus,
            state.subjects.math.weak_chapters,
            state.subjects.math.completed,
            state.subjects.math.textbook.as_deref().unwrap_or("未指定"),
            if state.subjects.math.active { "活跃" } else { "未启动" }
        ));
        prompt.push_str(&format!(
            "### 英语\n- 阶段: {:?}\n- 每周时长: {}h\n- 目标分数: {}\n- 当前重点: {}\n- 薄弱章节: {:?}\n- 教材: {}\n- 状态: {}\n\n",
            state.subjects.english.phase,
            state.subjects.english.weekly_hours,
            state.subjects.english.target_score,
            state.subjects.english.current_focus,
            state.subjects.english.weak_chapters,
            state.subjects.english.textbook.as_deref().unwrap_or("未指定"),
            if state.subjects.english.active { "活跃" } else { "未启动" }
        ));
        prompt.push_str(&format!(
            "### 专业课（{}）\n- 阶段: {:?}\n- 每周时长: {}h\n- 目标分数: {}\n- 当前重点: {}\n- 薄弱章节: {:?}\n- 教材: {}\n- 状态: {}\n\n",
            state.subjects.professional.name.as_ref().unwrap_or(&"专业课".to_string()),
            state.subjects.professional.phase,
            state.subjects.professional.weekly_hours,
            state.subjects.professional.target_score,
            state.subjects.professional.current_focus,
            state.subjects.professional.weak_chapters,
            state.subjects.professional.textbook.as_deref().unwrap_or("未指定"),
            if state.subjects.professional.active { "活跃" } else { "未启动" }
        ));
        if state.subjects.politics.active {
            prompt.push_str(&format!(
                "### 政治\n- 阶段: {:?}\n- 每周时长: {}h\n- 目标分数: {}\n- 当前重点: {}\n- 薄弱章节: {:?}\n- 教材: {}\n- 状态: 活跃\n\n",
                state.subjects.politics.phase,
                state.subjects.politics.weekly_hours,
                state.subjects.politics.target_score,
                state.subjects.politics.current_focus,
                state.subjects.politics.weak_chapters,
                state.subjects.politics.textbook.as_deref().unwrap_or("未指定")
            ));
        }

        // 教材联网检索指令
        let textbooks: Vec<(&str, &str)> = [
            ("数学", state.subjects.math.textbook.as_deref()),
            ("英语", state.subjects.english.textbook.as_deref()),
            ("专业课", state.subjects.professional.textbook.as_deref()),
            ("政治", state.subjects.politics.textbook.as_deref()),
        ]
        .into_iter()
        .filter_map(|(s, t)| t.filter(|v| !v.is_empty()).map(|v| (s, v)))
        .collect();

        if !textbooks.is_empty() {
            prompt.push_str("## 教材章节核验（重要：请联网检索）\n");
            prompt.push_str("请针对下列教材，使用联网搜索能力查询其官方目录、章节结构与小节划分，确保本周排程任务涉及的章节名称、小节编号与教材实际目录一致。\n\n");
            prompt.push_str("已配置教材：\n");
            for (subject, textbook) in &textbooks {
                prompt.push_str(&format!("- {}：{}\n", subject, textbook));
            }
            prompt.push_str("\n检索与使用规则：\n");
            prompt.push_str("1. 若教材名带有版本/版次（如「张宇高数18讲」「王道408 2026版」），请按对应版本检索目录；\n");
            prompt.push_str("2. 检索后请在 task_templates 中使用教材原文章节名作为 focus 和 goal（例如「高等数学 第三章 中值定理」），避免编造不存在的章节；\n");
            prompt.push_str("3. 若某教材联网检索失败或结果不可靠，请在 task_templates 的 textbook 字段回填原书名，并在 focus 中标注「（章节待用户确认）」；\n");
            prompt.push_str("4. 严禁臆造章节编号或小节名；若不确定，宁可使用泛化描述（如「线性代数行列式部分」）也不要编造具体编号。\n\n");
        }


        // 当前风险
        if !state.risks.items.is_empty() {
            prompt.push_str("## 当前风险\n");
            for risk in &state.risks.items {
                prompt.push_str(&format!(
                    "- [{:?}] {}: {}\n  建议: {}\n",
                    risk.level, risk.subject, risk.description, risk.suggested_action
                ));
            }
            prompt.push_str("\n");
        }

        // 用户画像
        prompt.push_str("## 用户学习画像\n");
        prompt.push_str(&format!(
            "- 平均每日专注时长: {}h\n- 擅长科目: {:?}\n- 薄弱科目: {:?}\n- 常见错误类型: {:?}\n- 复盘完成率: {:.0}%\n",
            state.user_model.avg_focus_hours_per_day,
            state.user_model.best_subjects,
            state.user_model.worst_subjects,
            state.user_model.common_error_types,
            state.user_model.review_compliance_rate * 100.0
        ));
        if !user_model.capabilities.is_empty() {
            prompt.push_str("### 活跃能力特征\n");
            for cap in &user_model.capabilities {
                if cap.activity == "active" {
                    prompt.push_str(&format!(
                        "- {} ({} {}, 置信度 {:.0}%): {}\n",
                        cap.id, cap.title, cap.category, cap.confidence * 100.0, cap.description
                    ));
                }
            }
            prompt.push_str("\n");
        }

        // 知识图谱摘要
        if !knowledge_graph.is_empty() {
            prompt.push_str("## 知识图谱摘要\n");
            prompt.push_str(knowledge_graph);
            prompt.push_str("\n\n");
        }

        // 最近复盘
        if !recent_reviews.is_empty() {
            prompt.push_str("## 最近复盘记录\n");
            for review in recent_reviews.iter().take(5) {
                prompt.push_str(&format!(
                    "- {}: 完成率 {:.0}%, 总时长 {:.1}h\n",
                    review.meta.date,
                    review.data.completion.completion_rate,
                    review.data.total_hours
                ));
            }
            prompt.push_str("\n");
        }

        // 上一周任务参考（日计划 + 复盘），用于校准本周任务量
        if !prev_week_daily_plans.is_empty() {
            prompt.push_str("## 上一周任务参考（用于校准本周任务量）\n");
            prompt.push_str("以下是上一周每天的日计划与复盘情况。请据此调整本周任务量：\n");
            prompt.push_str("- 若某天完成率偏低或总时长不足，本周相应科目可适度减量或延后进度；\n");
            prompt.push_str("- 若某天超额完成且精力充足，可适度加重；\n");
            prompt.push_str("- 注意保持每天任务量与上一周实际完成情况相匹配，避免任务量突增。\n\n");

            // 按日期索引复盘
            let review_by_date: std::collections::HashMap<&str, &crate::data::records::ReviewFile> =
                prev_week_reviews
                    .iter()
                    .map(|r| (r.meta.date.as_str(), r))
                    .collect();

            prompt.push_str("### 上一周每日明细\n");
            // 同时收集所有未完成任务（incomplete / partial），用于后续「未完成任务重排」段
            let mut uncompleted_tasks: Vec<(String, String, String, String)> = Vec::new();
            for plan in prev_week_daily_plans.iter() {
                let date = &plan.meta.date;
                let weekday = crate::data::weekday_name(date).unwrap_or_default();
                let is_rest = plan.data.tasks.is_empty();
                prompt.push_str(&format!(
                    "\n**{}（{}）**{}",
                    date,
                    weekday,
                    if is_rest { "【休息日】\n" } else { "\n" }
                ));
                if !is_rest {
                    // 计划任务
                    prompt.push_str(&format!(
                        "- 计划任务数: {}, 计划总时长: {:.1}h\n",
                        plan.data.total_tasks, plan.data.total_hours
                    ));
                    // 按科目分组任务标题
                    use std::collections::BTreeMap;
                    let mut by_subject: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
                    for task in &plan.data.tasks {
                        let subj = match task.subject {
                            crate::data::state::SubjectKey::Math => "数学",
                            crate::data::state::SubjectKey::English => "英语",
                            crate::data::state::SubjectKey::Politics => "政治",
                            crate::data::state::SubjectKey::Professional => "专业课",
                        };
                        by_subject.entry(subj).or_default().push(task.title.as_str());
                    }
                    for (subj, titles) in &by_subject {
                        prompt.push_str(&format!("  - {}: {}\n", subj, titles.join("、")));
                    }
                }
                // 对应复盘
                if let Some(review) = review_by_date.get(date.as_str()) {
                    prompt.push_str(&format!(
                        "- 复盘: 完成率 {:.0}%, 实际时长 {:.1}h, 精力 {}, A类 {}/{}, B类 {}/{}\n",
                        review.data.completion.completion_rate,
                        review.data.total_hours,
                        review.data.energy_level,
                        review.data.completion.priority_a_done,
                        review.data.completion.priority_a_total,
                        review.data.completion.priority_b_done,
                        review.data.completion.priority_b_total,
                    ));
                    if !review.data.difficulties.is_empty() {
                        let diffs: Vec<&str> = review
                            .data
                            .difficulties
                            .iter()
                            .map(|d| d.description.as_str())
                            .collect();
                        prompt.push_str(&format!("  - 困难: {}\n", diffs.join("；")));
                    }
                    if !review.data.next_steps.is_empty() {
                        prompt.push_str(&format!("  - 后续: {}\n", review.data.next_steps.join("；")));
                    }
                    // 收集未完成任务（incomplete / partial），格式: (date, subject, title, status)
                    for tr in &review.task_reviews {
                        let st = tr.status.as_str();
                        if st == "incomplete" || st == "partial" {
                            let subj_label = match tr.subject.as_str() {
                                "math" => "数学",
                                "english" => "英语",
                                "politics" => "政治",
                                "professional" => "专业课",
                                _ => "其他",
                            };
                            uncompleted_tasks.push((
                                date.clone(),
                                subj_label.to_string(),
                                if tr.title.is_empty() { "(未命名任务)".to_string() } else { tr.title.clone() },
                                st.to_string(),
                            ));
                        }
                    }
                }
            }
            prompt.push_str("\n");

            // 上周未完成任务清单 — 要求 AI 在本周重新安排
            if !uncompleted_tasks.is_empty() {
                prompt.push_str("## 上周未完成任务（必须在本周重新安排，重要）\n");
                prompt.push_str("以下是上一周复盘中标记为「未完成」或「部分完成」的任务。");
                prompt.push_str("这些任务**不得跳过**，必须在本周计划中重新安排，优先放在周一至周三：\n\n");
                prompt.push_str("| 日期 | 科目 | 任务 | 状态 |\n");
                prompt.push_str("|------|------|------|------|\n");
                for (date, subj, title, status) in &uncompleted_tasks {
                    let status_label = if status == "partial" {
                        "部分完成（继续推进剩余部分）"
                    } else {
                        "未完成（需重新安排）"
                    };
                    prompt.push_str(&format!("| {} | {} | {} | {} |\n", date, subj, title, status_label));
                }
                prompt.push_str("\n");
                prompt.push_str("重新安排规则：\n");
                prompt.push_str("1. 将上述未完成任务作为本周该科目的重点，优先安排在周一至周三；\n");
                prompt.push_str("2. 若未完成任务涉及某个章节，本周该科目应继续推进该章节，而非跳到后续章节；\n");
                prompt.push_str("3. 「部分完成」的任务应继续推进剩余部分，避免重复已完成内容；\n");
                prompt.push_str("4. 在 task_templates 中可适度合并未完成任务与新任务，但确保未完成任务的核心知识点被覆盖；\n");
                prompt.push_str("5. 不要因为「上周未完成」就降低本周任务量，应在保持总量的基础上重新排程。\n\n");
            }
        }

        // 输出要求
        prompt.push_str("## 输出要求\n");
        prompt.push_str(&format!(
            r#"请直接输出一个合法的 JSON 对象（不要包裹 ```json 代码块），严格符合以下结构：

{{
  "version": "1.0.0",
  "meta": {{
    "week_start": "{}",
    "week_end": "{}",
    "week_number": <从 1 开始的周序号>,
    "generated_at": "YYYY-MM-DDTHH:mm",
    "based_on": {{
      "state": "state/current.state",
      "user_model": "assets/user_model/_index.md",
      "exam_config": "assets/config/exam-config.md",
      "review_ref": null
    }}
  }},
  "data": {{
    "goals": ["本周目标1", "本周目标2"],
    "subjects": [
      {{
        "subject": "math",
        "weekly_hours": 10.0,
        "focus": "本周该科重点",
        "milestones": ["里程碑1"]
      }}
    ],
    "days": [
      {{
        "date": "YYYY-MM-DD",
        "weekday": "周一",
        "is_rest_day": false,
        "subject_allocations": [
          {{
            "subject": "math",
            "hours": 2.0,
            "focus": "当天该科重点",
            "task_templates": [
              {{
                "title": "任务标题",
                "priority": "A",
                "estimated_hours": 1.5,
                "goal": "任务目标",
                "completion_criteria": ["完成标准1"],
                "textbook": "教材（可选）",
                "style_tips": "学习风格提示（可选）",
                "fallback_plan": "失败回退方案（可选）"
              }}
            ]
          }}
        ]
      }}
    ],
    "risks": [
      {{
        "subject": "math",
        "item": "风险项",
        "level": "high",
        "suggestion": "建议"
      }}
    ],
    "reminders": ["提醒1"]
  }},
  "view": "用于人类阅读的 Markdown 摘要（可选，可为空字符串）"
}}

重要约束：
1. 必须包含 data 和 view 两个字段；view 仅用于展示，不会被程序解析。
2. 只给 active=true 的科目安排任务；政治未启动则安排为 rest_day 或空 allocations。
3. 数学任务必须严格遵循「数学二」考纲，排除伯努利方程、全微分方程相关内容。
4. 优先级使用 "A"（必须完成）或 "B"（建议完成）。
5. 风险 level 只能是 "low" / "medium" / "high" / "critical"。
6. subject 字段只能是 "math" / "english" / "politics" / "professional" 之一，严禁使用 "general" 或其他值。出现在 data.subjects[].subject、data.days[].subject_allocations[].subject、data.risks[].subject 中的所有取值都必须严格属于这四个枚举值之一。
7. 休息日 is_rest_day=true，且 subject_allocations 为空数组。
8. 任务 estimated_hours 总和应大致等于当天预期学习时长。
9. 必须严格遵守「学习日程」节中声明的休息日配置。weekday 字段（如"周日"）与用户休息日列表匹配的，is_rest_day 必须为 true，且不分配任何任务；weekday 不在休息日列表的，必须有 subject_allocations。
10. 必须严格遵守「各科开始学习日期」节中的约束：若某科目开始日期晚于本周日（{}），该科目不得出现在 subjects、subject_allocations、risks 中，本周完全不为其安排任务。
11. 参考「上一周任务参考」节调整本周任务量，避免任务量与上周实际完成情况严重偏离。
12. 每天的 task_templates 数量应大致等于「用户期望每日任务数量」（{} 个），每科约一条；未开始的科目不安排，相应减少当日任务数，不得为了凑数而强行安排。
13. {}若用户禁止总结任务，task_templates 的标题和 goal 不得出现"回顾"/"总结"/"复习"/"梳理"等字样，每个任务必须推进新的知识点、章节或习题；若用户允许总结任务，可酌情安排 1 个总结/复习类任务以巩固知识。
14. 若存在「上周未完成任务」节，必须在本周计划中重新安排这些任务（不得跳过），并优先放在周一至周三。未完成任务的状态由复盘时的勾解决定，不再自动标记为「已放弃」，因此「未完成」和「部分完成」的任务都需要在本周重新排程。
"#,
            week_start, week_end, week_end, daily_task_count,
            if enable_review_tasks { "" } else { "严禁安排总结/复习类任务。" }
        ));

        prompt
    }

    /// 读取考试配置
    fn read_exam_config(data_dir: &Path) -> String {
        let path = data_dir.join("assets").join("config").join("exam-config.md");
        if path.exists() {
            crate::data::read_file_content(&path).unwrap_or_default()
        } else {
            String::new()
        }
    }

    /// 读取最近 N 条复盘记录
    fn read_recent_reviews(
        data_dir: &Path,
        _week_start: &str,
        count: usize,
    ) -> Vec<crate::data::records::ReviewFile> {
        let today = today_string();
        let mut dates = match crate::data::records::list_review_dates(data_dir) {
            Ok(d) => d,
            Err(_) => return Vec::new(),
        };

        dates.sort();
        dates
            .into_iter()
            .filter(|d| d != &today)
            .rev()
            .take(count)
            .filter_map(|d| crate::data::records::read_review(data_dir, &d).ok())
            .collect()
    }

    /// 读取知识图谱摘要
    fn read_knowledge_graph_summary(data_dir: &Path) -> String {
        let path = data_dir.join("assets").join("knowledge").join("GRAPH.md");
        if path.exists() {
            crate::data::read_file_content(&path).unwrap_or_default()
        } else {
            String::new()
        }
    }

    /// 读取指定日期区间 [start, end] 内的所有复盘记录（含两端）
    ///
    /// 用于周计划生成时参考上一周的复盘情况。
    fn read_reviews_in_range(
        data_dir: &Path,
        start: &str,
        end: &str,
    ) -> Vec<crate::data::records::ReviewFile> {
        let mut result = Vec::new();
        let mut current = match add_days(start, 0) {
            Ok(d) => d,
            Err(_) => return result,
        };
        loop {
            if let Ok(review) = crate::data::records::read_review(data_dir, &current) {
                result.push(review);
            }
            if current == end {
                break;
            }
            match add_days(&current, 1) {
                Ok(next) => current = next,
                Err(_) => break,
            }
        }
        result
    }
}

/// 从 AI 响应中提取并解析周计划 JSON
fn parse_week_plan_json(
    content: &str,
    expected_week_start: &str,
    expected_week_end: &str,
) -> DataResult<WeekPlanFile> {
    let cleaned = clean_ai_json(content);
    let mut plan: WeekPlanFile = serde_json::from_str(&cleaned)
        .map_err(|e| format!("解析 AI 返回的周计划 JSON 失败: {}\n内容片段: {}", e, &cleaned[..cleaned.len().min(200)]))?;

    // 兜底填充版本与 meta 日期
    if plan.version.is_empty() {
        plan.version = "1.0.0".to_string();
    }
    if plan.meta.week_start.is_empty() {
        plan.meta.week_start = expected_week_start.to_string();
    }
    if plan.meta.week_end.is_empty() {
        plan.meta.week_end = expected_week_end.to_string();
    }
    if plan.meta.generated_at.is_empty() {
        plan.meta.generated_at = now_string();
    }
    if plan.meta.based_on.state.is_empty() {
        plan.meta.based_on = BasedOn {
            state: "state/current.state".to_string(),
            user_model: "assets/user_model/_index.md".to_string(),
            exam_config: "assets/config/exam-config.md".to_string(),
            review_ref: None,
            week_plan: None,
        };
    }

    Ok(plan)
}

/// 清理 AI 可能包裹的代码块，提取纯 JSON
fn clean_ai_json(content: &str) -> String {
    let trimmed = content.trim();

    // 尝试提取 ```json ... ``` 或 ``` ... ``` 包裹的内容
    if trimmed.starts_with("```") {
        let start = trimmed.find('\n').map(|p| p + 1).unwrap_or(0);
        let end = trimmed.rfind("```").unwrap_or(trimmed.len());
        return trimmed[start..end].trim().to_string();
    }

    // 尝试找到第一个 '{' 和最后一个 '}'
    if let Some(start) = trimmed.find('{') {
        if let Some(end) = trimmed.rfind('}') {
            return trimmed[start..=end].to_string();
        }
    }

    trimmed.to_string()
}

/// 后置校验：强制周计划中每天的 is_rest_day 与用户设置一致
///
/// 规则：
/// - 用户休息日列表中的日期：is_rest_day = true，subject_allocations 清空
/// - 不在用户休息日列表中的日期：is_rest_day = false
///
/// 同时补全缺失的日期（若 AI 漏掉了某些天，会自动补上空分配）
fn enforce_rest_days(
    plan: &mut WeekPlanFile,
    rest_days: &[String],
    week_start: &str,
    week_end: &str,
) -> DataResult<()> {
    // 收集已存在的日期
    let existing_dates: std::collections::HashSet<String> = plan
        .data
        .days
        .iter()
        .map(|d| d.date.clone())
        .collect();

    // 遍历整周，校验/补全
    let mut current_date = week_start.to_string();
    loop {
        let weekday = weekday_name(&current_date)?;
        let should_rest = rest_days.contains(&weekday);

        if let Some(day) = plan.data.days.iter_mut().find(|d| d.date == current_date) {
            // 已存在：强制覆盖
            if should_rest && !day.is_rest_day {
                log::warn!(
                    "周计划校验: {}（{}）应为休息日但 AI 标记为学习日，已修正",
                    current_date,
                    weekday
                );
                day.is_rest_day = true;
                day.subject_allocations.clear();
            } else if !should_rest && day.is_rest_day {
                log::warn!(
                    "周计划校验: {}（{}）应为学习日但 AI 标记为休息日，已修正",
                    current_date,
                    weekday
                );
                day.is_rest_day = false;
            }
        } else if !existing_dates.contains(&current_date) {
            // 缺失：补全
            log::warn!(
                "周计划校验: {}（{}）缺失，已补全（is_rest_day={}）",
                current_date,
                weekday,
                should_rest
            );
            plan.data.days.push(WeekDayPlan {
                date: current_date.clone(),
                weekday,
                is_rest_day: should_rest,
                subject_allocations: Vec::new(),
            });
        }

        if current_date == week_end {
            break;
        }
        current_date = add_days(&current_date, 1)?;
    }

    // 按日期排序
    plan.data.days.sort_by(|a, b| a.date.cmp(&b.date));

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clean_ai_json_removes_fences() {
        let raw = r#"```json
{"version":"1.0.0","meta":{"week_start":"2026-07-20"},"data":{"goals":[]},"view":""}
```"#;
        let cleaned = clean_ai_json(raw);
        assert!(cleaned.starts_with('{'));
        assert!(cleaned.ends_with('}'));
        assert!(!cleaned.contains("```"));
    }

    #[test]
    fn test_parse_week_plan_json() {
        let raw = r#"{"version":"1.0.0","meta":{"week_start":"2026-07-20","week_end":"2026-07-26","week_number":30,"generated_at":"2026-07-20T04:00","based_on":{"state":"state/current.state","user_model":"assets/user_model/_index.md","exam_config":"assets/config/exam-config.md"}},"data":{"goals":[],"subjects":[],"days":[],"risks":[],"reminders":[]},"view":""}"#;
        let plan = parse_week_plan_json(raw, "2026-07-20", "2026-07-26").unwrap();
        assert_eq!(plan.meta.week_start, "2026-07-20");
        assert_eq!(plan.meta.week_number, 30);
    }

    #[test]
    fn test_enforce_rest_days_corrects_ai_mistakes() {
        // 模拟 AI 错误地把周六标记为休息日，但用户设置只有周日休息
        let raw = r#"{"version":"1.0.0","meta":{"week_start":"2026-07-20","week_end":"2026-07-26","week_number":30,"generated_at":"2026-07-20T04:00","based_on":{"state":"","user_model":"","exam_config":""}},"data":{"goals":[],"subjects":[],"days":[{"date":"2026-07-25","weekday":"周六","is_rest_day":true,"subject_allocations":[]},{"date":"2026-07-26","weekday":"周日","is_rest_day":false,"subject_allocations":[{"subject":"math","hours":2.0,"focus":"测试","task_templates":[]}]}],"risks":[],"reminders":[]},"view":""}"#;
        let mut plan = parse_week_plan_json(raw, "2026-07-20", "2026-07-26").unwrap();

        // 用户设置：仅周日休息
        let rest_days = vec!["周日".to_string()];
        enforce_rest_days(&mut plan, &rest_days, "2026-07-20", "2026-07-26").unwrap();

        // 验证：周六应为学习日（is_rest_day=false）
        let saturday = plan.data.days.iter().find(|d| d.date == "2026-07-25").unwrap();
        assert!(!saturday.is_rest_day, "周六应为学习日");

        // 验证：周日应为休息日（is_rest_day=true），且 allocations 被清空
        let sunday = plan.data.days.iter().find(|d| d.date == "2026-07-26").unwrap();
        assert!(sunday.is_rest_day, "周日应为休息日");
        assert!(sunday.subject_allocations.is_empty(), "周日的 allocations 应被清空");

        // 验证：补全了缺失的日期（周一到周五）
        assert_eq!(plan.data.days.len(), 7, "应补全为 7 天");
    }
}
