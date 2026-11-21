//! 规划任务的上下文与提示词构造。
pub(super) struct ExclusionPromptContext<'a> {
    pub(super) state: &'a StudyState,
    pub(super) excluded_day: &'a ExcludedDay,
    pub(super) regen_days: &'a [&'a crate::data::plan::WeekDayPlan],
    pub(super) excluded_original_allocations: &'a [crate::data::plan::DaySubjectAllocation],
    pub(super) regen_start: &'a str,
    pub(super) week_end: &'a str,
    pub(super) week_start: &'a str,
    pub(super) rest_days: &'a [String],
    pub(super) subject_start_dates: &'a [(&'static str, String)],
    pub(super) subject_time_allocation: Option<&'a std::collections::HashMap<String, f64>>,
    pub(super) daily_target_hours: f64,
    pub(super) standard_granularity: f64,
    pub(super) enable_review_tasks: bool,
    pub(super) basics_done_labels: &'a [String],
    pub(super) all_excluded_days: &'a [ExcludedDay],
}

pub(super) struct RegenerationPromptContext<'a> {
    pub(super) state: &'a StudyState,
    pub(super) review: &'a crate::data::records::ReviewFile,
    pub(super) regen_days: &'a [&'a crate::data::plan::WeekDayPlan],
    pub(super) review_date: &'a str,
    pub(super) regen_start: &'a str,
    pub(super) week_end: &'a str,
    pub(super) week_start: &'a str,
    pub(super) rest_days: &'a [String],
    pub(super) subject_start_dates: &'a [(&'static str, String)],
    pub(super) subject_time_allocation: Option<&'a std::collections::HashMap<String, f64>>,
    pub(super) daily_target_hours: f64,
    pub(super) standard_granularity: f64,
    pub(super) enable_review_tasks: bool,
    pub(super) basics_done_labels: &'a [String],
    pub(super) excluded_days: &'a [ExcludedDay],
    pub(super) hidden_task_ids: &'a std::collections::HashSet<String>,
}

pub(super) struct WeekPromptContext<'a> {
    pub(super) data_dir: &'a Path,
    pub(super) state: &'a StudyState,
    pub(super) user_model: &'a crate::data::assets::UserModelIndex,
    pub(super) recent_reviews: &'a [crate::data::records::ReviewFile],
    pub(super) exam_config: &'a str,
    pub(super) knowledge_graph: &'a str,
    pub(super) week_start: &'a str,
    pub(super) week_end: &'a str,
    pub(super) rest_days: &'a [String],
    pub(super) subject_start_dates: &'a [(&'static str, String)],
    pub(super) subject_time_allocation: Option<&'a std::collections::HashMap<String, f64>>,
    pub(super) daily_target_hours: f64,
    pub(super) standard_granularity: f64,
    pub(super) enable_review_tasks: bool,
    pub(super) basics_done_labels: &'a [String],
    pub(super) prev_week_daily_plans: &'a [DailyPlanFile],
    pub(super) prev_week_reviews: &'a [crate::data::records::ReviewFile],
    pub(super) excluded_days: &'a [ExcludedDay],
    pub(super) workload_adjustment: Option<&'a WorkloadAdjustment>,
    pub(super) adaptive_parameters: &'a AdaptivePlanParameters,
}

use super::*;
impl Planner<'_> {
    /// 生成数学考纲约束文本（按用户实际卷种动态注入，不再硬编码「数学二」）
    ///
    /// 返回可直接嵌入 prompt 的一句话约束。数二排除伯努利方程/全微分方程；
    /// 数一/数三/其他卷种仅要求遵循对应考纲；未考数学或未指定时返回中性提示。
    pub(super) fn math_syllabus_constraint(state: &StudyState) -> String {
        match state.subjects.math.version.as_deref() {
            Some("数二") => {
                "数学任务必须严格遵循「数学二」考纲，排除伯努利方程、全微分方程相关内容".to_string()
            }
            Some(v) if !v.is_empty() => format!("数学任务必须严格遵循「{}」考纲", v),
            // 未指定卷种或未考数学：不强加任何数学考纲
            _ => "若用户考数学，数学任务必须遵循其实际考试卷种对应考纲；若不考数学则忽略本条"
                .to_string(),
        }
    }

    /// 生成「各科待学知识点预估时长」参考文本（隐藏数据，仅进 prompt，不展示给用户）。
    ///
    /// 从各科启用进度表取「待学（pending/learning）」知识点，按表内顺序每科最多取前
    /// `MAX_PER_SUBJECT` 个；预估值缺失时用确定性估算回退，并按自适应周计划学到的
    /// 用户效率做**复合调整**：融合「实际/预估时间比效率系数 + 任务量反馈信号 +
    /// 学科完成率」三个信号，再按置信度收缩、按知识点难度调整敏感性（详见
    /// `crate::core::estimated_time::EstimateAdjustment`）。
    pub(super) fn progress_estimate_prompt_block(
        data_dir: &Path,
        adaptive_parameters: &AdaptivePlanParameters,
        standard_granularity: f64,
    ) -> String {
        use crate::core::estimated_time::{
            adjust_hours, estimate_knowledge_hours, EstimateAdjustment,
        };
        use crate::data::progress_tables::{NodeLevel, NodeStatus};
        const MAX_PER_SUBJECT: usize = 15;

        let index = crate::data::load_progress_index(data_dir);
        let subjects = crate::data::state::SubjectKey::ALL.map(|key| key.key());
        let mut lines: Vec<String> = Vec::new();
        for subject in subjects {
            let Some(table) = crate::data::progress_tables::active_progress_table(&index, subject)
            else {
                continue;
            };
            // 复合调整参数：效率系数 + 反馈信号 + 该科完成率 + 置信度
            let adjustment = EstimateAdjustment {
                efficiency_factor: adaptive_parameters
                    .estimation_factors
                    .get(subject)
                    .copied()
                    .unwrap_or(1.0),
                feedback_signal: adaptive_parameters.feedback_signal,
                completion_rate: adaptive_parameters
                    .subject_completion_rates
                    .get(subject)
                    .copied()
                    .unwrap_or(0.85),
                confidence: adaptive_parameters.confidence,
            };
            // 待学知识点全量统计（表尾守护）：取前 MAX_PER_SUBJECT 条进 prompt，
            // 但剩余条数用于判断「即将学空 / 已学空」
            const LOW_PENDING_THRESHOLD: usize = 5;
            let pending: Vec<&crate::data::progress_tables::ProgressNode> = table
                .nodes
                .iter()
                .filter(|n| n.level == NodeLevel::Knowledge)
                .filter(|n| matches!(n.status, NodeStatus::Pending | NodeStatus::Learning))
                .collect();
            let pending_count = pending.len();

            let cn = crate::data::subject_label(subject);
            let factor_pct = (adjustment.combined_factor() * 100.0).round();

            if pending_count == 0 {
                // 已学空：明确告知 AI 安排滚动巩固任务，不要虚构新进度
                lines.push(format!(
                    "- {}：该科进度表知识点已全部推进完毕（无待学节点）。本周任务请安排滚动巩固类内容（真题二刷、错题重做、限时模考、背诵复习等），不要虚构新的章节/知识点进度。旧计划中仍未完成的首次学习/补学任务，若关联知识点已达到基础，不得继续补排；基础完成状态优先于旧任务的待完成状态，但用户在复盘中明确保留的剩余范围仍须保留。",
                    cn
                ));
                continue;
            }

            let points: Vec<String> = pending
                .iter()
                .take(MAX_PER_SUBJECT)
                .map(|n| {
                    let base = n
                        .estimated_hours
                        .unwrap_or_else(|| estimate_knowledge_hours(subject, &n.title));
                    let adjusted = adjust_hours(base, &adjustment, &n.title);
                    format!("{} ≈{:.1}h", n.title, adjusted)
                })
                .collect();
            let mut line = format!(
                "- {}（综合校准系数 ≈{:.0}%，效率 {:.2}/反馈 {:.1}/完成率 {:.0}%）：{}",
                cn,
                factor_pct,
                adjustment.efficiency_factor,
                adjustment.feedback_signal,
                adjustment.completion_rate * 100.0,
                points.join("；")
            );
            if pending_count < LOW_PENDING_THRESHOLD {
                // 即将学空：新内容为主但需提示用户生成二轮/强化表
                line.push_str(&format!(
                    "（注意：该科待学知识点仅剩 {} 条，进度表即将学空——本周巩固类任务应占该科大头，并在 style_tips 中建议用户生成二轮/强化进度表）",
                    pending_count
                ));
            }
            lines.push(line);
        }
        if lines.is_empty() {
            return String::new();
        }
        let mut out = String::from(
            "## 各科待学知识点预估时长（隐藏校准数据，仅用于安排任务时长参考，不展示给用户）\n\
             以下为各科启用进度表中待学知识点的预估学习时长，已按用户近期学习效率（时间比、任务量反馈、完成率）自动校准：\n",
        );
        out.push_str(&lines.join("\n"));
        out.push_str(&format!(
            "\n要求：安排单条任务 estimated_hours 时，优先以上述预估值为主要依据，并按用户设置的任务粒度 {:.2}h/条 合并/拆分（单条不超过 {:.2}h）；未列出的内容仍按原规则估算。\n\n",
            standard_granularity,
            standard_granularity * 1.5
        ));
        out
    }

    /// 内容来源优先级说明（内置表 ↔ AI 知识库）。
    ///
    /// 用户规则：AI 知识库认为内置表缺少某章节/知识点时以 AI 为准（补齐）；
    /// 内置表提供的内容比 AI 更全时以内置表为准（不得漏）。二者合起来即「取并集」，
    /// 因此这里同时写明两个方向，避免模型只保留其中一侧。
    pub(super) fn content_source_priority_block() -> String {
        String::from(
            "## 内容来源优先级（内置表 ↔ AI 知识库，重要）\n\
             - 上文「各科待学知识点」来自用户启用的进度表（下称**内置表**，可能是内置官方考纲表，也可能是用户/AI 生成的表），是本周排任务的主要依据。\n\
             - 若提供的可信考纲来源明确表明内置表遗漏了章节或知识点，将其补入整体待学范围；没有来源时仅列为待核实建议，不声称这是最新官方考点。\n\
             - 反之，若内置表列出的内容比你能覆盖的更全（条目更多、更细），以内置表为准：表内待学章节/知识点应纳入整体学习范围，按顺序在时间预算内推进；超出本周预算的内容留待后续周，不强迫一周完成。\n\
             - 最终两侧取并集：既不丢弃内置表已列内容，也不被它的范围限制而漏掉考纲真实要求的章节/知识点。\n\
             - 某科没有内置表时，完全以你的考纲知识为准组织该科内容。\n\n",
        )
    }

    /// 构建周计划 prompt
    #[allow(clippy::too_many_arguments)]
    pub(super) fn build_week_plan_prompt(&self, context: WeekPromptContext<'_>) -> String {
        let WeekPromptContext {
            data_dir,
            state,
            user_model,
            recent_reviews,
            exam_config,
            knowledge_graph,
            week_start,
            week_end,
            rest_days,
            subject_start_dates,
            subject_time_allocation,
            daily_target_hours,
            standard_granularity,
            enable_review_tasks,
            basics_done_labels,
            prev_week_daily_plans,
            prev_week_reviews,
            excluded_days,
            workload_adjustment,
            adaptive_parameters,
        } = context;
        let available_dates = crate::core::planning::constraints::available_dates(
            week_start,
            week_end,
            rest_days,
            excluded_days,
            &today_string(),
        )
        .unwrap_or_default();
        let available_day_count = available_dates.len();
        let remaining = days_between(&state.meta.exam_date, week_start).unwrap_or(0);
        let iso_week = iso_week_string(week_start).unwrap_or_else(|_| "YYYY-Www".to_string());
        let week_number = iso_week
            .rsplit('W')
            .next()
            .and_then(|value| value.parse::<u32>().ok())
            .unwrap_or(1);

        // 任务数量只是固定时间预算下的软提示，不能再作为唯一自校准对象。
        let effective_daily_task_count = adaptive_parameters.daily_task_count.clamp(1, 8);
        let study_days = available_day_count.max(1) as f64;
        let effective_target_hours = adaptive_parameters.nominal_total_hours / study_days;
        let self_coeff = adaptive_parameters.workload_factor;

        let mut prompt = String::new();
        prompt.push_str(&format!(
            "请为 {}（{} 至 {}）生成考研学习周计划。请严格遵循以下数据和输出规范。\n\n",
            iso_week, week_start, week_end
        ));

        // 周中生成提示：明确告知今天日期，要求已过去的日期不安排任务
        let today = today_string();
        let today_weekday = weekday_name(&today).unwrap_or_default();
        prompt.push_str(&format!(
            "## 当前时间\n- 今天是 {}（{}）。\n- **已过去的日期（早于今天）不得安排任何任务**：这些天的 subject_allocations 必须为空数组，任务只从今天（{}）起安排到本周日（{}）。\n- 若今天是周一，则整周从周一到周日正常安排。\n\n",
            today, today_weekday, today, week_end
        ));

        // 确定性算法给出数字预算；AI 不得自行改变总量和学科预算。
        prompt.push_str("## 确定性自适应计划参数（必须遵守）\n");
        prompt.push_str(&format!(
            "- 下一周名义计划总时长目标：{:.2} 小时；有效容量目标：{:.2} 小时。\n",
            adaptive_parameters.nominal_total_hours, adaptive_parameters.capacity_hours
        ));
        prompt.push_str(&format!(
            "- 每日任务数量仅作为软提示：约 {} 个；真正硬约束是每日时长上限和下列学科预算。\n",
            effective_daily_task_count
        ));
        if !adaptive_parameters.subject_hours.is_empty() {
            let mut budgets: Vec<String> = adaptive_parameters
                .subject_hours
                .iter()
                .map(|(subject, hours)| {
                    format!(
                        "{} {:.2}h（占比 {:.0}% ，估时系数×{:.2}）",
                        planner_subject_cn_from_str(subject),
                        hours,
                        adaptive_parameters
                            .subject_shares
                            .get(subject)
                            .copied()
                            .unwrap_or(0.0)
                            * 100.0,
                        adaptive_parameters
                            .estimation_factors
                            .get(subject)
                            .copied()
                            .unwrap_or(1.0)
                    )
                })
                .collect();
            budgets.sort();
            prompt.push_str(&format!("- 学科预算：{}。\n", budgets.join("，")));
        }
        prompt.push_str("- 不得因为自然语言判断而额外增加总时长；预算不足时优先保留高优先级和必要的复习任务。\n");
        if !adaptive_parameters.reasons.is_empty() {
            prompt.push_str(&format!(
                "- 参数依据：{}\n",
                adaptive_parameters.reasons.join("；")
            ));
        }
        prompt.push('\n');

        // 考试信息
        prompt.push_str("## 考试信息\n");
        if !exam_config.is_empty() {
            prompt.push_str(exam_config);
            prompt.push('\n');
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
            // M1：用 saturating_sub 防止 rest_days 超过 7 时下溢
            prompt.push_str(&format!("- 每周学习天数：{} 天\n", available_day_count));
        }
        prompt.push_str(&format!(
            "- 每日任务数量：{} 个（由每日有效目标学时 {:.2}h ÷ 用户设置的任务粒度 {:.2}h/条 派生；单条任务目标时长 ≈ {:.2}h；**每科每天至少 1 条任务**，任务量下调只允许调整任务时长/合并粒度，不得把某科的当日任务清零；同时遵循各科开始学习日期，未开始的科目不安排任务，相应减少当日任务数）\n",
            effective_daily_task_count, effective_target_hours, standard_granularity, standard_granularity
        ));
        prompt.push_str(&format!(
            "- 是否安排总结/复习任务：{}（{}）\n\n",
            if enable_review_tasks { "允许" } else { "禁止" },
            if enable_review_tasks {
                "可在 task_templates 中安排'回顾'/'总结'/'复习'类任务以巩固知识".to_string()
            } else if basics_done_labels.is_empty() {
                "严禁安排任何形式的复习/巩固类任务，包括但不限于'回顾'/'总结'/'复习'/'梳理'/'练习'/'巩固'/'强化'/'温习'/'复盘'/'巩固练习'等；每日任务必须推进新知识点、新章节或新习题".to_string()
            } else {
                // 开关仅约束基础阶段：基础已学完的科目已进入强化阶段，不受该开关限制
                format!(
                    "此开关仅对处于基础阶段的科目生效。以下科目的启用进度表知识点已全部达到「基础」，已进入强化阶段，不受本开关限制——**必须为其安排巩固/复习/强化类任务**（真题二刷、错题重做、限时模考、背诵复习等），且**严禁为其虚构新的章节/知识点进度**：{}。其余科目仍严格禁止安排任何形式的复习/巩固类任务（包括但不限于'回顾'/'总结'/'复习'/'梳理'/'练习'/'巩固'/'强化'/'温习'/'复盘'等），每日任务必须推进新知识点、新章节或新习题",
                    basics_done_labels.join("、")
                )
            }
        ));
        if enable_review_tasks && !basics_done_labels.is_empty() {
            prompt.push_str(&format!(
                "- 上述科目已进入强化阶段（知识点全部达「基础」），本周巩固类任务应占其任务大头：{}\n\n",
                basics_done_labels.join("、")
            ));
        }

        // 任务粒度与学时约束（确定性硬约束，替代仅"大致等于"的软约束）
        prompt.push_str("## 任务粒度与学时约束（重要）\n");
        prompt.push_str(&format!(
            "- 每日有效目标学时：{:.2}h（用户基础目标 {:.2}h；由近期有效容量、任务量反馈和估时校准共同计算；任务量系数 {:.2}）\n",
            effective_target_hours, daily_target_hours, self_coeff
        ));
        prompt.push_str(&format!(
            "- 每日任务数约 {} 个；每天所有任务 estimated_hours 总和须 ∈ [{:.2}h, {:.2}h]\n",
            effective_daily_task_count,
            effective_target_hours * 0.95,
            effective_target_hours * 1.05
        ));
        prompt.push_str(
            "- 单条任务 estimated_hours ∈ [1.0, 2.0]h（推荐），超过 3.0h 必须拆分为多条\n",
        );
        prompt.push_str("- 一条任务 = 单一主旨的同动作学习单元。不同学习动作（如背诵/阅读/分析/做题/听课）即使总时长合理也必须拆为独立任务，不得用 + 拼接；判定：该单元完成后能否单独勾选完成？能 → 独立成条。\n\n");

        // 各科待学知识点预估时长参考（隐藏数据，已按用户效率系数校准）
        let progress_estimate_block = Self::progress_estimate_prompt_block(
            data_dir,
            adaptive_parameters,
            standard_granularity,
        );
        if !progress_estimate_block.is_empty() {
            prompt.push_str(&progress_estimate_block);
        }
        // 内容来源优先级（内置表 ↔ AI 知识库）：按「并集」取舍，两侧都不丢。
        prompt.push_str(&Self::content_source_priority_block());

        // 各科学时分配（用户配置占比，重要约束）：按占比给出每科每周/每日学时，
        // 并要求各科 subject_allocations 的 hours 按占比分配
        if let Some(allocation) = subject_time_allocation {
            let study_days = available_day_count;
            prompt.push_str("## 各科学时分配（用户配置，重要）\n");
            prompt.push_str(&format!(
                "- 用户为各科配置了学习时间占比（合计 100%，基准：每日有效目标学时 {:.2}h × 剩余 {} 个可学习日）：\n",
                effective_target_hours, study_days
            ));
            for (key, cn) in [
                ("math", "数学"),
                ("english", "英语"),
                ("politics", "政治"),
                ("professional", "专业课"),
            ] {
                if let Some(share) = allocation.get(key) {
                    let weekly = effective_target_hours * study_days as f64 * share / 100.0;
                    let daily = effective_target_hours * share / 100.0;
                    prompt.push_str(&format!(
                        "- {}：占比 {:.0}%（每周约 {:.1}h，每日约 {:.1}h）\n",
                        cn, share, weekly, daily
                    ));
                }
            }
            prompt.push_str("- 要求：各科 subject_allocations 的 hours 之和应按上述占比分配；占比为 0 的科目本周不安排任务；未开始的科目（开始日期晚于本周日）即使占比 > 0 也不得安排。\n\n");
        }

        // 每科任务数确定性预算（每科至少 1 条，条数多了才按时长权重分散）
        let per_subject_budget = subject_task_budget(
            state,
            effective_daily_task_count,
            week_end,
            subject_start_dates,
            subject_time_allocation,
        );
        if !per_subject_budget.is_empty() {
            // 去掉分配数为 0 的科目，避免给 AI 造成"竟有空科"的误导
            let nonzero: Vec<String> = per_subject_budget
                .iter()
                .filter(|(_, n)| *n > 0)
                .map(|(k, n)| format!("{} {} 条", k.label(), n))
                .collect();
            if !nonzero.is_empty() {
                let distribution_note = if subject_time_allocation.is_some() {
                    "（按用户配置的占比分配；占比为 0 的科目不安排；**每科每天至少 1 条任务**，任务量下调不得把某科清零）"
                } else {
                    "（**每科每天至少 1 条任务**，任务量下调不得把某科清零；条数多了才按时长权重分散）"
                };
                prompt.push_str(&format!(
                    "- 每日任务在各科的分布参考：{}\n",
                    distribution_note
                ));
                prompt.push_str(&format!("  具体条数：{}\n", nonzero.join("，")));
            }
        }

        // 记忆曲线复习调度（B）：把掌握不足内容的到期复习点按日期注入 prompt
        let memory_items = memory_curve_review_items(recent_reviews, week_start, week_end);
        if !memory_items.is_empty() {
            prompt.push_str("## 本周待复习（记忆曲线，务必安排到对应日期）\n");
            prompt.push_str("以下复习点基于近期掌握不足（weak）内容确定性生成，必须在对应日期安排复习任务（作为当日对应科目的一条任务）：\n");
            let mut cur: Option<String> = None;
            for it in &memory_items {
                if cur.as_deref() != Some(it.due_date.as_str()) {
                    if cur.is_some() {
                        prompt.push('\n');
                    }
                    let _cn = weekday_name(&it.due_date).unwrap_or_default();
                    prompt.push_str(&format!("- {}（{}）:\n", it.due_date, _cn));
                    cur = Some(it.due_date.clone());
                }
                prompt.push_str(&format!("  - {}：{}\n", it.subject.label(), it.title));
            }
            prompt.push('\n');
        }

        // 近期强度建议（E）：用复盘完成率 + 精力值给出本周强度提示
        if !recent_reviews.is_empty() {
            let intensity = today_intensity_label(recent_reviews);
            if !intensity.is_empty() {
                prompt.push_str("## 近期状态提示\n");
                prompt.push_str(&format!("{}\n\n", intensity));
            }
        }

        // 本周任务量调整（相对上周）
        if let Some(adj) = workload_adjustment {
            if adj.direction != "unchanged" {
                prompt.push_str("## 本周任务量调整（相对上周）\n");
                let dir_label = match adj.direction.as_str() {
                    "increase" => "增加",
                    "decrease" => "减少",
                    _ => "调整",
                };
                let level_label = match adj.level.as_deref() {
                    Some("small") => "小幅",
                    Some("large") => "大幅",
                    _ => "适度",
                };
                prompt.push_str(&format!("- 方向: {}（{}）\n", dir_label, level_label));
                prompt.push_str("- 该调整已经由程序计入本次预算。请直接使用给定学时和任务数，不再乘 20%/40% 系数；预算外的待学范围留待后续日期。\n");
                if let Some(note) = &adj.note {
                    if !note.is_empty() {
                        prompt.push_str(&format!("- 用户备注: {}\n", note));
                    }
                }
                prompt.push('\n');
            }
        }

        // 本周特殊情况排除日期
        if !excluded_days.is_empty() {
            prompt.push_str("## 本周特殊情况排除日期（重要）\n");
            prompt.push_str("以下日期用户已声明不学习（外出/生病/考试等），本周计划中这些日期：\n");
            prompt.push_str("- is_rest_day 必须为 true\n");
            prompt.push_str("- subject_allocations 必须为空数组\n");
            prompt.push_str(
                "- 原任务保留为待学范围，在给定剩余预算内顺延；超出本周预算的部分留待后续周\n\n",
            );
            let type_label = |t: &str| -> &'static str {
                match t {
                    "travel" => "外出旅行",
                    "sick" => "生病",
                    "exam" => "考试",
                    _ => "其他",
                }
            };
            for ex in excluded_days {
                let weekday = weekday_name(&ex.date).unwrap_or_else(|_| "未知".to_string());
                prompt.push_str(&format!(
                    "- {}（{}）: {}",
                    ex.date,
                    weekday,
                    type_label(&ex.reason_type)
                ));
                if let Some(note) = &ex.note {
                    if !note.is_empty() {
                        prompt.push_str(&format!(" — {}", note));
                    }
                }
                prompt.push('\n');
            }
            prompt.push_str(&format!("\n本周剩余可学习日期（已合并休息日、排除日与过去日期）：{}；共 {} 天。按给定预算推进，不额外挤入超量任务。\n\n", available_dates.join("、"), available_day_count));
        }

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
            prompt.push('\n');
        }

        // 各科状态
        prompt.push_str("## 当前各科状态\n\n");
        // 每周时长展示：配置了占比时按「每日有效目标学时 × 学习日 × 占比」推导；
        // 未配置时回退 State 中的周学时（旧行为）
        let study_days_total = available_day_count;
        let weekly_display = |key: &str, fallback: f64| -> String {
            subject_time_allocation
                .and_then(|a| a.get(key))
                .map(|share| {
                    format!(
                        "{:.1}h（占比 {:.0}%）",
                        effective_target_hours * study_days_total as f64 * share / 100.0,
                        share
                    )
                })
                .unwrap_or_else(|| format!("{}h", fallback))
        };
        let math_label = state
            .subjects
            .math
            .version
            .as_deref()
            .filter(|v| !v.is_empty())
            .map(|v| format!("数学（{}）", v))
            .unwrap_or_else(|| "数学".to_string());
        prompt.push_str(&format!(
            "### {}\n- 阶段: {:?}\n- 每周时长: {}\n- 目标分数: {}\n- 当前重点: {}\n- 薄弱章节: {:?}\n- 已完成: {:?}\n- 教材: {}\n- 状态: {}\n\n",
            math_label,
            state.subjects.math.phase,
            weekly_display("math", state.subjects.math.weekly_hours),
            state.subjects.math.target_score,
            state.subjects.math.current_focus,
            state.subjects.math.weak_chapters,
            state.subjects.math.completed,
            state.subjects.math.textbook.as_deref().unwrap_or("未指定"),
            if state.subjects.math.active { "活跃" } else { "未启动" }
        ));
        prompt.push_str(&format!(
            "### 英语\n- 阶段: {:?}\n- 每周时长: {}\n- 目标分数: {}\n- 当前重点: {}\n- 薄弱章节: {:?}\n- 已完成: {:?}\n- 教材: {}\n- 状态: {}\n\n",
            state.subjects.english.phase,
            weekly_display("english", state.subjects.english.weekly_hours),
            state.subjects.english.target_score,
            state.subjects.english.current_focus,
            state.subjects.english.weak_chapters,
            state.subjects.english.completed,
            state.subjects.english.textbook.as_deref().unwrap_or("未指定"),
            if state.subjects.english.active { "活跃" } else { "未启动" }
        ));
        prompt.push_str(&format!(
            "### 专业课（{}）\n- 阶段: {:?}\n- 每周时长: {}\n- 目标分数: {}\n- 当前重点: {}\n- 薄弱章节: {:?}\n- 已完成: {:?}\n- 教材: {}\n- 状态: {}\n\n",
            state.subjects.professional.name.as_ref().unwrap_or(&"专业课".to_string()),
            state.subjects.professional.phase,
            weekly_display("professional", state.subjects.professional.weekly_hours),
            state.subjects.professional.target_score,
            state.subjects.professional.current_focus,
            state.subjects.professional.weak_chapters,
            state.subjects.professional.completed,
            state.subjects.professional.textbook.as_deref().unwrap_or("未指定"),
            if state.subjects.professional.active { "活跃" } else { "未启动" }
        ));
        if state.subjects.politics.active {
            prompt.push_str(&format!(
                "### 政治\n- 阶段: {:?}\n- 每周时长: {}\n- 目标分数: {}\n- 当前重点: {}\n- 薄弱章节: {:?}\n- 已完成: {:?}\n- 教材: {}\n- 状态: 活跃\n\n",
                state.subjects.politics.phase,
                weekly_display("politics", state.subjects.politics.weekly_hours),
                state.subjects.politics.target_score,
                state.subjects.politics.current_focus,
                state.subjects.politics.weak_chapters,
                state.subjects.politics.completed,
                state.subjects.politics.textbook.as_deref().unwrap_or("未指定")
            ));
        }

        // 已完成内容防重复提示
        let has_any_completed = !state.subjects.math.completed.is_empty()
            || !state.subjects.english.completed.is_empty()
            || !state.subjects.professional.completed.is_empty()
            || (state.subjects.politics.active && !state.subjects.politics.completed.is_empty());
        if has_any_completed {
            prompt.push_str("## 已完成内容防重复（重要）\n");
            prompt.push_str("上述各科「已完成」列表中的章节/任务均已完成，本周计划**严禁重复**这些内容。具体要求：\n");
            prompt.push_str("1. 不得在 task_templates 的 title/goal/focus 中再次安排「已完成」列表中的章节或知识点；\n");
            prompt.push_str(
                "2. 各科目必须从「已完成」列表之后的下一个章节/知识点继续推进，不得回退；\n",
            );
            prompt.push_str("3. 若「当前重点」与「已完成」有重叠，以「已完成」为准并向前推进；\n");
            prompt.push_str(
                "4. 英语若已完成某些真题/单元，本周应安排后续真题/单元，不重做已完成题目。\n\n",
            );
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
            prompt.push_str("## 教材章节核验（只使用提供的来源）\n");
            prompt.push_str("本次请求没有联网检索工具。针对下列教材，只使用已提供的目录、章节结构与小节划分；没有来源时不得杜撰章节编号或声称已经核实。\n\n");
            prompt.push_str("已配置教材：\n");
            for (subject, textbook) in &textbooks {
                prompt.push_str(&format!("- {}：{}\n", subject, textbook));
            }
            prompt.push_str("\n检索与使用规则：\n");
            prompt.push_str("1. 若教材名带有版本/版次（如「张宇高数18讲」「王道408 2026版」），请按对应版本检索目录；\n");
            prompt.push_str("2. 检索后请在 task_templates 中使用教材原文章节名作为 focus 和 goal（例如「高等数学 第三章 中值定理」），避免编造不存在的章节；\n");
            prompt.push_str("3. 若某教材没有已提供的可靠目录来源，请在 task_templates 的 textbook 字段回填原书名，并在 focus 中标注「（章节待用户确认）」；\n");
            prompt.push_str("4. 严禁臆造章节编号或小节名；若不确定，宁可使用泛化描述（如「线性代数行列式部分」）也不要编造具体编号。\n\n");
        }

        // 当前各科薄弱章节与关注点（替代原 risks 段，AI 据此感知滞后科目）
        let has_weak = !state.subjects.math.weak_chapters.is_empty()
            || !state.subjects.english.weak_chapters.is_empty()
            || !state.subjects.professional.weak_chapters.is_empty();
        let has_focus = !state.subjects.math.current_focus.is_empty()
            || !state.subjects.english.current_focus.is_empty()
            || !state.subjects.professional.current_focus.is_empty();
        if has_weak || has_focus {
            prompt.push_str("## 当前需关注科目（薄弱章节与当前重点）\n");
            for (subj_name, subj) in [
                ("数学", &state.subjects.math),
                ("英语", &state.subjects.english),
                ("专业课", &state.subjects.professional),
            ] {
                if !subj.weak_chapters.is_empty() || !subj.current_focus.is_empty() {
                    prompt.push_str(&format!("- {}", subj_name));
                    if !subj.weak_chapters.is_empty() {
                        prompt.push_str(&format!("｜薄弱: {}", subj.weak_chapters.join("、")));
                    }
                    if !subj.current_focus.is_empty() {
                        prompt.push_str(&format!("｜当前重点: {}", subj.current_focus));
                    }
                    prompt.push('\n');
                }
            }
            prompt.push('\n');
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
                        cap.id,
                        cap.title,
                        cap.category,
                        cap.confidence * 100.0,
                        cap.description
                    ));
                }
            }
            prompt.push('\n');
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
                let (_, _, _, _, rate) = crate::data::records::review_completion_stats(review);
                prompt.push_str(&format!(
                    "- {}: 完成率 {:.0}%, 总时长 {:.1}h\n",
                    review.meta.date,
                    rate,
                    crate::data::records::review_actual_hours(review)
                ));
            }
            prompt.push('\n');
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
                let weekday = weekday_name(date).unwrap_or_else(|_| "未知".to_string());
                // 用户关闭「允许 AI 参考」的任务不计入参考口径
                let ai_tasks: Vec<&crate::data::plan::PlanTask> =
                    plan.data.tasks.iter().filter(|t| t.ai_visible()).collect();
                let is_rest = ai_tasks.is_empty();
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
                        ai_tasks.len(),
                        ai_tasks.iter().map(|t| t.estimated_hours).sum::<f64>()
                    ));
                    // 按科目分组任务标题
                    use std::collections::BTreeMap;
                    let mut by_subject: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
                    for task in ai_tasks {
                        let subj = task.subject.label();
                        by_subject
                            .entry(subj)
                            .or_default()
                            .push(task.title.as_str());
                    }
                    for (subj, titles) in &by_subject {
                        prompt.push_str(&format!("  - {}: {}\n", subj, titles.join("、")));
                    }
                }
                // 用户关闭「允许 AI 参考」的任务：即使出现在复盘中，也不写入提示词
                let hidden_ids: std::collections::HashSet<&str> = plan
                    .data
                    .tasks
                    .iter()
                    .filter(|t| !t.ai_visible())
                    .map(|t| t.id.as_str())
                    .collect();
                let hidden_titles: std::collections::HashSet<&str> = plan
                    .data
                    .tasks
                    .iter()
                    .filter(|t| !t.ai_visible())
                    .map(|t| t.title.as_str())
                    .collect();
                // 对应复盘
                if let Some(review) = review_by_date.get(date.as_str()) {
                    let (a_total, _, b_total, _, rate) =
                        crate::data::records::review_completion_stats(review);
                    let (done, _) = crate::data::records::review_total_completion(review);
                    let rate_label = if a_total + b_total > 0 {
                        format!("{rate:.0}%")
                    } else {
                        "未记录".into()
                    };
                    let has_time = review.data.total_hours > 0.0
                        || !review.data.time_spent.is_empty()
                        || review
                            .task_reviews
                            .iter()
                            .any(|task| task.actual_minutes.is_some());
                    let time_label = if has_time {
                        format!("{:.1}h", crate::data::records::review_actual_hours(review))
                    } else {
                        "未记录".into()
                    };
                    let energy_label = if (1..=5).contains(&review.data.energy_level) {
                        format!("{}/5", review.data.energy_level)
                    } else {
                        "未记录".into()
                    };
                    prompt.push_str(&format!(
                        "- 复盘: 完成率 {}, 实际时长 {}, 精力 {}, 已完成 {}/{}\n",
                        rate_label,
                        time_label,
                        energy_label,
                        done,
                        a_total + b_total
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
                        prompt.push_str(&format!(
                            "  - 后续: {}\n",
                            review.data.next_steps.join("；")
                        ));
                    }
                    // 收集未完成任务（incomplete / partial），格式: (date, subject, title, status)
                    for tr in &review.task_reviews {
                        let st = tr.status.as_str();
                        // 跳过用户禁止 AI 参考的任务（按 task_id 命中，标题兜底旧数据）
                        if hidden_ids.contains(tr.task_id.as_str())
                            || (!tr.title.is_empty() && hidden_titles.contains(tr.title.as_str()))
                        {
                            continue;
                        }
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
                                if tr.title.is_empty() {
                                    "(未命名任务)".to_string()
                                } else {
                                    tr.title.clone()
                                },
                                st.to_string(),
                            ));
                        }
                    }
                }
            }
            prompt.push('\n');

            // 上周未完成任务清单 — 要求 AI 在本周重新安排
            if !uncompleted_tasks.is_empty() {
                prompt.push_str("## 上周未完成任务（必须在本周重新安排，重要）\n");
                prompt.push_str("以下是上一周复盘中标记为「未完成」或「部分完成」的任务。");
                prompt.push_str(
                    "这些任务纳入待学范围，只在剩余预算内安排；优先接续未完成知识点，不补排过去日期：\n\n",
                );
                prompt.push_str("| 日期 | 科目 | 任务 | 状态 |\n");
                prompt.push_str("|------|------|------|------|\n");
                for (date, subj, title, status) in &uncompleted_tasks {
                    let status_label = if status == "partial" {
                        "部分完成（继续推进剩余部分）"
                    } else {
                        "未完成（需重新安排）"
                    };
                    prompt.push_str(&format!(
                        "| {} | {} | {} | {} |\n",
                        date, subj, title, status_label
                    ));
                }
                prompt.push('\n');
                prompt.push_str("重新安排规则：\n");
                prompt
                    .push_str("1. 将上述未完成任务作为本周该科目的重点，优先安排在周一至周三；\n");
                prompt.push_str(
                    "2. 若未完成任务涉及某个章节，本周该科目应继续推进该章节，而非跳到后续章节；\n",
                );
                prompt.push_str("3. 「部分完成」的任务应继续推进剩余部分，避免重复已完成内容；\n");
                prompt.push_str("4. 在 task_templates 中可适度合并未完成任务与新任务，但确保未完成任务的核心知识点被覆盖；\n");
                prompt.push_str(
                    "5. 不要因为「上周未完成」就降低本周任务量，应在保持总量的基础上重新排程。\n\n",
                );
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
    "week_number": {week_number},
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
    ]
  }},
  "view": "用于人类阅读的 Markdown 摘要（可选，可为空字符串）"
}}

重要约束：
1. 必须包含 data 字段；view 为可选展示摘要，不会被程序解析。
2. 只给 active=true 的科目安排任务；政治未启动则安排为 rest_day 或空 allocations。
3. {math}
4. 任务已不再分级（Priority A/B 已废弃）：task_templates 中不要输出 priority 字段，任务不做 A/B 区分，所有任务同等对待。
5. subject 字段只能是 "math" / "english" / "politics" / "professional" 之一，严禁使用 "general" 或其他值。出现在 data.subjects[].subject、data.days[].subject_allocations[].subject 中的所有取值都必须严格属于这四个枚举值之一。
6. 休息日 is_rest_day=true，且 subject_allocations 为空数组。
7. 每条任务必须输出 estimated_hours（单位小时，数值型，取值 ∈ [0.5, 3.0]，超过 3.0h 拆成多条）；每天所有任务的 estimated_hours 总和应大致等于当天预期学习时长，严禁省略该字段或填 0。
8. 必须严格遵守「学习日程」节中声明的休息日配置。weekday 字段（如"周日"）与用户休息日列表匹配的，is_rest_day 必须为 true，且不分配任何任务；weekday 不在休息日列表的，必须有 subject_allocations。
9. 必须严格遵守「各科开始学习日期」节中的约束：若某科目开始日期晚于本周日（{}），该科目不得出现在 subjects、subject_allocations 中，本周完全不为其安排任务。
10. 参考「上一周任务参考」节调整本周任务量，避免任务量与上周实际完成情况严重偏离。
11. 每天的 task_templates 数量应大致等于「用户期望每日任务数量」（{} 个），每科约一条；未开始的科目不安排，相应减少当日任务数，不得为了凑数而强行安排。
12. {}若用户禁止总结任务，task_templates 的标题和 goal 不得出现"回顾"/"总结"/"复习"/"梳理"/"练习"/"巩固"/"强化"/"温习"/"复盘"等字样，每个任务必须推进新的知识点、章节或新习题（新习题指未做过的题目，不含已做题目的重做）；若用户允许总结任务，可酌情安排 1 个总结/复习类任务以巩固知识。{}
13. 若存在「上周未完成任务」节，将未完成任务纳入待学范围，在今天及之后的剩余预算内接续；超量部分留待后续周。未完成任务的状态由复盘时的勾决定定，不再自动标记为「已放弃」，因此「未完成」和「部分完成」的任务都需要在本周重新排程。
14. **不得重复已完成内容**：各科「已完成」列表中的章节/任务严禁再次出现在本周计划中，必须从已完成之后的下一个章节/知识点继续推进。同时以各科「当前重点」作为实际进度基准：不得在用户尚未到达的章节安排任务，计划的推进顺序必须以教材章节先后为准，不得跳过用户尚未学习的章节跳级到后面（若「当前重点」显示的进度落后于本周计划，以「当前重点」为准相应调整，而非沿用旧计划）。
15. **按天推进切分（受排除日影响）**：本周计划必须将每个科目的学习内容切分到每一天，每天推进不同的章节/知识点/习题，逐日向前递进。同一章节可以跨天安排，但每天推进的知识点或练习范围必须明确不同（休息日/排除日除外），避免一天内塞满整周内容或每天重复同一内容。**{}应作为本周的起始点**，从各科「已完成」之后的章节开始，逐天分配到剩余学习日（若为周中生成，起点为今天而非周一，已过去的日期不安排任务）。**注意排除日不分配任务**，排除日应占用的任务量必须分摊到本周其他学习日，因此实际可学习天数按程序提供的可学习日期集合计算（休息日和排除日重叠只扣一次），每天的 task_templates 数量限制（约束11）仍须遵守。
"#,
            week_start, week_end, week_end, effective_daily_task_count,
            if enable_review_tasks { "" } else { "严禁安排总结/复习类任务。" },
            if basics_done_labels.is_empty() {
                String::new()
            } else {
                format!(
                    "**例外（优先级高于本条禁令）**：{} 的启用进度表知识点已全部达「基础」，已进入强化阶段，不受本开关限制，必须为其安排巩固/复习/强化类任务（真题二刷、错题重做、限时模考、背诵复习等），并严禁为其虚构新的章节/知识点进度；上述禁令只对处于基础阶段的其余科目生效。",
                    basics_done_labels.join("、")
                )
            },
            if today == week_start { "周一" } else { "今天" },
            math = Self::math_syllabus_constraint(state),
        ));

        // 英语单词每日安排约束：凡当天安排了英语任务，须至少包含 1 条背单词任务
        if state.subjects.english.active {
            prompt.push_str(&format!(
                "\n补充约束（英语单词每天安排）：只要当天 subject_allocations 中安排了英语科目（english）的任务，则该英语科目 task_templates 中必须至少包含 1 条「背单词」任务（如：{}），并计入当天任务数；休息日/排除日以及当天未安排英语任务的日子不受此约束。\n",
                if enable_review_tasks || basics_done_labels.iter().any(|l| l == "英语") {
                    "背 N 个新词（可含单词复习巩固）"
                } else {
                    "背 N 个新词（推进新词，不含复习字样）"
                }
            ));
        }

        prompt
    }

    /// 构建「JSON 语法修复」prompt：解析失败后的自动重试。
    ///
    /// 把上一次的完整输出与解析错误位置交回给 AI，只允许修语法、
    /// 不得改动任何内容——周计划的内容（目标/任务/时长）全部保留。
    pub(super) fn build_json_repair_prompt(original_content: &str, parse_error: &str) -> String {
        format!(
            r#"你上一轮被要求输出一份考研周计划的 JSON，但你输出的 JSON 存在语法错误，程序解析失败：

解析错误：
{}

你上一轮的完整输出如下：

<<<BEGIN_PREVIOUS_OUTPUT
{}
END_PREVIOUS_OUTPUT>>>

请修复 JSON 的语法错误并重新输出。要求：
1. 常见语法错误包括：字符串值中出现未转义的英文双引号（必须改为转义 \" 或替换为中文引号「」“”）、尾随逗号、漏写逗号、括号/方括号不匹配、字符串未闭合等。重点检查解析错误提示的位置附近。
2. 只修正语法，严禁增删改任何字段名与字段内容（标题、目标、任务、时长、数值、文本都必须与原输出完全一致）。
3. 直接输出修正后的完整 JSON（一个完整的 JSON 对象），不要包裹 ```json 代码块，不要输出任何解释或多余文字。
"#,
            parse_error, original_content
        )
    }

    /// 读取考试配置
    pub(super) fn read_exam_config(data_dir: &Path) -> String {
        let path = data_dir
            .join("assets")
            .join("config")
            .join("exam-config.md");
        if path.exists() {
            crate::data::read_file_content(&path)
                .unwrap_or_default()
                .chars()
                .take(12_000)
                .collect()
        } else {
            String::new()
        }
    }

    /// 读取最近 N 条复盘记录
    pub(super) fn read_recent_reviews(
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
    pub(super) fn read_knowledge_graph_summary(data_dir: &Path) -> String {
        let path = data_dir.join("assets").join("knowledge").join("GRAPH.md");
        if path.exists() {
            let content = crate::data::read_file_content(&path).unwrap_or_default();
            let mut summary: String = content.chars().take(12_000).collect();
            if content.chars().count() > 12_000 {
                summary.push_str("\n[知识图谱仅提供节选，未展示的内容不能视为不存在或已学完。]");
            }
            summary
        } else {
            String::new()
        }
    }

    /// 读取指定日期区间 [start, end] 内的所有复盘记录（含两端）
    ///
    /// 用于周计划生成时参考上一周的复盘情况。
    pub(super) fn read_reviews_in_range(
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

    /// 构建复盘后重排剩余天数的 AI prompt
    ///
    /// 输入：复盘数据、当前 State、需要重排的日期范围及原安排
    /// 输出：AI 重新生成这些日期的 subject_allocations
    #[allow(clippy::too_many_arguments)]
    pub(super) fn build_regenerate_prompt(&self, context: RegenerationPromptContext<'_>) -> String {
        let RegenerationPromptContext {
            state,
            review,
            regen_days,
            review_date,
            regen_start,
            week_end,
            week_start,
            rest_days,
            subject_start_dates,
            subject_time_allocation,
            daily_target_hours,
            standard_granularity,
            enable_review_tasks,
            basics_done_labels,
            excluded_days,
            hidden_task_ids,
        } = context;
        let remaining = days_between(&state.meta.exam_date, regen_start).unwrap_or(0);
        let iso_week = iso_week_string(week_start).unwrap_or_else(|_| "YYYY-Www".to_string());
        // 任务条数由「每日目标学时 ÷ 用户设置的任务粒度」确定性派生（重排沿用基准学时，不叠加自校准）
        let effective_daily_task_count =
            crate::core::planning::pure::derive_task_count_with_granularity(
                daily_target_hours,
                standard_granularity,
                1.0,
                crate::core::planning::pure::active_subject_count_for(
                    state,
                    week_end,
                    subject_start_dates,
                ),
            );

        let mut prompt = String::new();
        prompt.push_str(&format!(
            "用户刚完成 {} 的复盘，需要根据复盘结果重新安排本周（{}）剩余天数（{} 至 {}）的学习计划。\n\n",
            review_date, iso_week, regen_start, week_end
        ));

        // 考试信息
        prompt.push_str("## 考试信息\n");
        prompt.push_str(&format!(
            "- 考试日期: {}\n- 距今剩余: {} 天\n- 目标院校: {} {}\n\n",
            state.meta.exam_date, remaining, state.meta.target_school, state.meta.target_major
        ));

        // 复盘结果（核心输入）
        prompt.push_str("## 昨日复盘结果（重排的核心依据）\n");
        prompt.push_str(&format!("- 复盘日期: {}\n", review_date));

        // 整体感受
        if let Some(dr) = &review.daily_review {
            let feeling_label = match dr.overall_feeling.as_str() {
                "smooth" => "顺利",
                "normal" => "一般",
                "hard" => "困难",
                _ => "未知",
            };
            prompt.push_str(&format!("- 整体感受: {}\n", feeling_label));
            if !dr.main_difficulty.is_empty() {
                prompt.push_str(&format!("- 主要困难类型: {}\n", dr.main_difficulty));
            }
        }

        // 任务完成情况
        if !review.task_reviews.is_empty() {
            prompt.push_str("\n### 任务复盘明细\n");
            prompt.push_str("| 科目 | 任务 | 状态 | 掌握程度 | 未完成原因 |\n");
            prompt.push_str("|------|------|------|----------|------------|\n");
            // 用户关闭「允许 AI 参考」的任务不写入提示词
            for tr in review
                .task_reviews
                .iter()
                .filter(|tr| !hidden_task_ids.contains(&tr.task_id))
            {
                let subj_label = match tr.subject.as_str() {
                    "math" => "数学",
                    "english" => "英语",
                    "politics" => "政治",
                    "professional" => "专业课",
                    _ => "其他",
                };
                let status_label = match tr.status.as_str() {
                    "completed" => "已完成",
                    "partial" => "部分完成",
                    "incomplete" => "未完成",
                    "abandoned" => "已放弃",
                    _ => "未知",
                };
                let mastery_label = match tr.mastery.as_str() {
                    "mastered" => "已掌握",
                    "basic" => "基本掌握",
                    "weak" => "薄弱",
                    _ => "",
                };
                let blockers = if tr.blockers.is_empty() {
                    String::new()
                } else {
                    tr.blockers.join("、")
                };
                let remaining = crate::core::task_progress::remaining_title(tr);
                let title = if tr.status == "partial" {
                    remaining.as_str()
                } else if tr.title.is_empty() {
                    "(未命名)"
                } else {
                    &tr.title
                };
                prompt.push_str(&format!(
                    "| {} | {} | {} | {} | {} |\n",
                    subj_label, title, status_label, mastery_label, blockers
                ));
            }
        }

        // 计划外学习内容 / 额外进度
        if !review.overcompletion.is_empty() {
            prompt.push_str("\n### 用户实际进度声明（作为各科的进度基准）\n");
            prompt.push_str("用户反馈了各科当前实际学习的位置（可能是超前完成，也可能是对计划的进度修正）。该位置已更新到 State，重排时必须作为对应科目的**实际进度起点**，据此双向调整剩余计划：\n");
            for oc in &review.overcompletion {
                let subj_label = match oc.subject.as_str() {
                    "math" => "数学",
                    "english" => "英语",
                    "politics" => "政治",
                    "professional" => "专业课",
                    _ => "其他",
                };
                prompt.push_str(&format!(
                    "- {}: 实际进度位于「{}」",
                    subj_label, oc.chapter_reached
                ));
                if let Some(note) = &oc.note {
                    prompt.push_str(&format!("（备注: {}）", note));
                }
                prompt.push('\n');
            }
            prompt.push_str("\n**重要**：以用户声明的实际进度作为该科目的起点，剩余计划只能从这个位置之后继续推进，不得重复已学内容。若原计划把任务安排在用户尚未到达的位置，应视为进度修正而非超前，需相应调整。计划的推进顺序必须以教材章节先后为准，既不得跳过用户尚未学习的章节跳级到后面，也不得回退。\n");
        }

        // 未完成任务处理指引
        let uncompleted: Vec<&crate::data::records::TaskReviewEntry> = review
            .task_reviews
            .iter()
            .filter(|tr| !hidden_task_ids.contains(&tr.task_id))
            .filter(|tr| tr.status == "incomplete" || tr.status == "partial")
            .collect();
        if !uncompleted.is_empty() {
            prompt.push_str("\n### 未完成任务处理指引\n");
            prompt.push_str("以下任务未完成或部分完成，必须在剩余天数中优先安排继续学习：\n");
            for tr in &uncompleted {
                let subj_label = match tr.subject.as_str() {
                    "math" => "数学",
                    "english" => "英语",
                    "politics" => "政治",
                    "professional" => "专业课",
                    _ => "其他",
                };
                let status_label = if tr.status == "partial" {
                    "部分完成（继续推进剩余部分）"
                } else {
                    "未完成（需重新安排）"
                };
                prompt.push_str(&format!(
                    "- {}「{}」: {}\n",
                    subj_label,
                    crate::core::task_progress::remaining_title(tr),
                    status_label
                ));
            }
            prompt.push_str("\n规则：\n1. 未完成任务应尽快安排在剩余天数的开头几天，但**仅限位于用户实际进度起点之后**的任务；\n2. 若某项未完成任务位于用户实际进度起点**之前**（即用户尚未学到那一步，被实际进度声明覆盖），则不重复安排，改排起点之后的后续章节；\n3. 不得跳过用户实际进度之后的未完成任务直接学更新的内容；\n4. 部分完成的任务继续推进剩余部分，不重复已学内容。\n");
        }

        // 当前各科状态
        prompt.push_str("\n## 当前各科状态（已反映额外进度）\n");
        prompt.push_str(&format!(
            "### 数学\n- 阶段: {:?}\n- 当前重点: {}\n- 已完成: {:?}\n- 薄弱章节: {:?}\n\n",
            state.subjects.math.phase,
            state.subjects.math.current_focus,
            state.subjects.math.completed,
            state.subjects.math.weak_chapters
        ));
        prompt.push_str(&format!(
            "### 英语\n- 阶段: {:?}\n- 当前重点: {}\n- 已完成: {:?}\n\n",
            state.subjects.english.phase,
            state.subjects.english.current_focus,
            state.subjects.english.completed
        ));
        prompt.push_str(&format!(
            "### 专业课\n- 阶段: {:?}\n- 当前重点: {}\n- 已完成: {:?}\n\n",
            state.subjects.professional.phase,
            state.subjects.professional.current_focus,
            state.subjects.professional.completed
        ));
        if state.subjects.politics.active {
            prompt.push_str(&format!(
                "### 政治\n- 阶段: {:?}\n- 当前重点: {}\n- 已完成: {:?}\n\n",
                state.subjects.politics.phase,
                state.subjects.politics.current_focus,
                state.subjects.politics.completed
            ));
        }

        // 学习日程配置
        prompt.push_str("## 学习日程约束\n");
        prompt.push_str(&format!(
            "- 休息日: {}（这些日子不安排任务）\n",
            rest_days.join("、")
        ));
        prompt.push_str(&format!(
            "- 每日任务数量: {} 个（由每日目标学时 {:.2}h ÷ 用户设置的任务粒度 {:.2}h/条 派生）；单条任务 estimated_hours 目标 ≈ {:.2}h，不得超过 {:.2}h；每科约一条；未开始的科目不安排）\n",
            effective_daily_task_count,
            daily_target_hours,
            standard_granularity,
            standard_granularity,
            standard_granularity * 1.5
        ));
        prompt.push_str(&format!(
            "- 总结/复习任务: {}\n",
            if enable_review_tasks {
                "允许安排".to_string()
            } else if basics_done_labels.is_empty() {
                "禁止安排（严禁任何形式的复习/巩固类任务，包括「回顾」「总结」「复习」「梳理」「练习」「巩固」「强化」「温习」「复盘」等）".to_string()
            } else {
                format!(
                    "此开关仅对基础阶段科目生效。以下科目已进入强化阶段（启用进度表知识点全部达「基础」），不受限制，必须安排巩固/复习/强化类任务且严禁虚构新进度：{}；其余科目禁止安排（严禁任何形式的复习/巩固类任务，包括「回顾」「总结」「复习」「梳理」「练习」「巩固」「强化」「温习」「复盘」等）",
                    basics_done_labels.join("、")
                )
            }
        ));

        // 各科学时分配（用户配置占比，重要约束）：重排剩余天数时按占比分配各科学时
        if let Some(allocation) = subject_time_allocation {
            let study_days = crate::core::planning::constraints::available_dates(
                regen_start,
                week_end,
                rest_days,
                excluded_days,
                &today_string(),
            )
            .unwrap_or_default()
            .len();
            prompt.push_str("## 各科学时分配（用户配置，重要）\n");
            prompt.push_str(&format!(
                "- 用户为各科配置了学习时间占比（合计 100%，基准：每日目标学时 {:.2}h × 剩余 {} 个可学习日）：\n",
                daily_target_hours, study_days
            ));
            for (key, cn) in [
                ("math", "数学"),
                ("english", "英语"),
                ("politics", "政治"),
                ("professional", "专业课"),
            ] {
                if let Some(share) = allocation.get(key) {
                    let weekly = daily_target_hours * study_days as f64 * share / 100.0;
                    let daily = daily_target_hours * share / 100.0;
                    prompt.push_str(&format!(
                        "- {}：占比 {:.0}%（每周约 {:.1}h，每日约 {:.1}h）\n",
                        cn, share, weekly, daily
                    ));
                }
            }
            prompt.push_str("- 要求：重排后各科 subject_allocations 的 hours 之和应按上述占比分配；占比为 0 的科目不安排任务；未开始的科目（开始日期晚于本周日）即使占比 > 0 也不得安排。\n\n");
        }

        // 本周特殊情况排除日期（重排时也必须遵守）
        if !excluded_days.is_empty() {
            prompt.push_str("\n### 本周特殊情况排除日期\n");
            prompt.push_str("以下日期用户已声明不学习，重排时这些日期必须：is_rest_day=true、subject_allocations 为空数组。原本安排在这些日期的任务量分摊到剩余学习日。\n");
            let type_label = |t: &str| -> &'static str {
                match t {
                    "travel" => "外出旅行",
                    "sick" => "生病",
                    "exam" => "考试",
                    _ => "其他",
                }
            };
            for ex in excluded_days {
                let weekday = weekday_name(&ex.date).unwrap_or_else(|_| "未知".to_string());
                prompt.push_str(&format!(
                    "- {}（{}）: {}",
                    ex.date,
                    weekday,
                    type_label(&ex.reason_type)
                ));
                if let Some(note) = &ex.note {
                    if !note.is_empty() {
                        prompt.push_str(&format!(" — {}", note));
                    }
                }
                prompt.push('\n');
            }
        }

        // 各科开始日期约束
        let has_unstarted = subject_start_dates.iter().any(|(k, d)| {
            !d.is_empty()
                && d.as_str() > week_end
                && (*k == "math" || *k == "english" || *k == "politics" || *k == "professional")
        });
        if has_unstarted {
            prompt.push_str("\n## 各科开始日期约束\n");
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
                if !date.is_empty() && date.as_str() > week_end {
                    prompt.push_str(&format!(
                        "- {}（{}）: 开始日期 {}，本周剩余天数不得安排任务\n",
                        subject_cn(key),
                        key,
                        date
                    ));
                }
            }
        }

        // 原安排参考
        prompt.push_str("\n## 剩余天数原安排（仅供参考，可调整）\n");
        for day in regen_days {
            let weekday = weekday_name(&day.date).unwrap_or_else(|_| "未知".to_string());
            prompt.push_str(&format!(
                "\n**{}（{}）**{}\n",
                day.date,
                weekday,
                if day.is_rest_day {
                    "【休息日】"
                } else {
                    ""
                }
            ));
            if !day.is_rest_day && !day.subject_allocations.is_empty() {
                for alloc in &day.subject_allocations {
                    let subj_label = alloc.subject.label();
                    prompt.push_str(&format!(
                        "- {}（{}h）: {}",
                        subj_label, alloc.hours, alloc.focus
                    ));
                    if !alloc.task_templates.is_empty() {
                        let titles: Vec<&str> = alloc
                            .task_templates
                            .iter()
                            .map(|t| t.title.as_str())
                            .collect();
                        prompt.push_str(&format!(" → {}", titles.join("、")));
                    }
                    prompt.push('\n');
                }
            }
        }

        // 输出要求
        prompt.push_str(&format!(
            r#"

## 输出要求

请直接输出一个合法的 JSON 对象（不要包裹 ```json 代码块），严格符合以下结构：

{{
  "days": [
    {{
      "date": "YYYY-MM-DD",
      "subject_allocations": [
        {{
          "subject": "math",
          "hours": 2.0,
          "focus": "当天该科重点",
          "task_templates": [
            {{
              "title": "任务标题",
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
  ]
}}

重要约束：
1. 必须覆盖 {} 至 {} 的所有学习日（休息日无需输出 subject_allocations，但 date 必须包含）。
2. subject 只能是 "math" / "english" / "politics" / "professional" 之一。
3. 任务已不再分级（Priority A/B 已废弃）：task_templates 中不要输出 priority 字段，任务不做 A/B 区分，所有任务同等对待。
4. {math}
5. 未完成任务必须尽快安排在剩余天数的前几天（仅限位于用户实际进度之后的未完成任务；已被实际进度声明覆盖的按已学习处理，不再安排）。
6. 用户声明的实际进度是各科的进度基准，重排允许双向调整（超前或修正）。所有科目的任务必须从用户实际进度**之后**继续推进，不得在用户尚未到达的章节安排任务；若原计划凌驾于用户实际进度之前，应删除或后置。
7. 每天的 task_templates 数量约 {} 个，每科约一条。
8. {}若用户禁止总结任务，task_templates 的标题和 goal 不得出现"回顾"/"总结"/"复习"/"梳理"/"练习"/"巩固"/"强化"/"温习"/"复盘"等字样，每个任务必须推进新知识点、新章节或新习题。{}
9. 休息日的 subject_allocations 为空数组。
10. **不得重复已完成内容**：各科「已完成」列表中的章节/任务严禁再次出现，必须从已完成之后的下一个章节/知识点继续推进。
11. **按天推进切分（受排除日影响）**：每个科目的学习内容必须切分到剩余的每个学习日，每天推进不同的章节/知识点/习题，逐日向前递进。同一章节可以跨天安排，但每天推进的知识点或练习范围必须明确不同（休息日/排除日除外）。若存在排除日，排除日不分配任务，其任务量分摊到其他学习日，每天的 task_templates 数量限制（约束7）仍须遵守。
"#,
            regen_start, week_end, effective_daily_task_count,
            if enable_review_tasks { "" } else { "严禁安排总结/复习类任务。" },
            if basics_done_labels.is_empty() {
                String::new()
            } else {
                format!(
                    "**例外（优先级高于本条禁令）**：{} 的启用进度表知识点已全部达「基础」，已进入强化阶段，不受本开关限制，必须为其安排巩固/复习/强化类任务（真题二刷、错题重做、限时模考、背诵复习等），并严禁为其虚构新的章节/知识点进度；上述禁令只对处于基础阶段的其余科目生效。",
                    basics_done_labels.join("、")
                )
            },
            math = Self::math_syllabus_constraint(state),
        ));

        // 英语单词每日安排约束：凡当天安排了英语任务，须至少包含 1 条背单词任务
        if state.subjects.english.active {
            prompt.push_str(&format!(
                "\n补充约束（英语单词每天安排）：只要当天 subject_allocations 中安排了英语科目（english）的任务，则该英语科目 task_templates 中必须至少包含 1 条「背单词」任务（如：{}），并计入当天任务数；休息日/排除日以及当天未安排英语任务的日子不受此约束。\n",
                if enable_review_tasks || basics_done_labels.iter().any(|l| l == "英语") {
                    "背 N 个新词（可含单词复习巩固）"
                } else {
                    "背 N 个新词（推进新词，不含复习字样）"
                }
            ));
        }

        prompt
    }

    /// 构建排除日重排 prompt（周中新增排除日时使用，不依赖复盘）
    #[allow(clippy::too_many_arguments)]
    pub(super) fn build_exclusion_regenerate_prompt(
        &self,
        context: ExclusionPromptContext<'_>,
    ) -> String {
        let ExclusionPromptContext {
            state,
            excluded_day,
            regen_days,
            excluded_original_allocations,
            regen_start,
            week_end,
            week_start,
            rest_days,
            subject_start_dates,
            subject_time_allocation,
            daily_target_hours,
            standard_granularity,
            enable_review_tasks,
            basics_done_labels,
            all_excluded_days,
        } = context;
        let remaining = days_between(&state.meta.exam_date, regen_start).unwrap_or(0);
        let iso_week = iso_week_string(week_start).unwrap_or_else(|_| "YYYY-Www".to_string());
        // 任务条数由「每日目标学时 ÷ 用户设置的任务粒度」确定性派生（重排沿用基准学时，不叠加自校准）
        let effective_daily_task_count =
            crate::core::planning::pure::derive_task_count_with_granularity(
                daily_target_hours,
                standard_granularity,
                1.0,
                crate::core::planning::pure::active_subject_count_for(
                    state,
                    week_end,
                    subject_start_dates,
                ),
            );

        let type_label = match excluded_day.reason_type.as_str() {
            "travel" => "外出旅行",
            "sick" => "生病",
            "exam" => "考试",
            _ => "其他",
        };

        let mut prompt = String::new();
        prompt.push_str(&format!(
            "用户在本周（{}）新增了一个特殊情况排除日：{}（{}，{}）。需要重新安排本周 {} 至 {} 的学习计划，把原本安排在排除日的任务量分摊到剩余学习日。\n\n",
            iso_week, excluded_day.date, type_label,
            excluded_day.note.as_deref().unwrap_or(""),
            regen_start, week_end
        ));

        // 考试信息
        prompt.push_str("## 考试信息\n");
        prompt.push_str(&format!(
            "- 考试日期: {}\n- 距今剩余: {} 天\n- 目标院校: {} {}\n\n",
            state.meta.exam_date, remaining, state.meta.target_school, state.meta.target_major
        ));

        // 重排原因
        prompt.push_str("## 重排原因（新增排除日）\n");
        prompt.push_str(&format!(
            "- 排除日期: {}\n- 类型: {}\n",
            excluded_day.date, type_label
        ));
        if let Some(note) = &excluded_day.note {
            if !note.is_empty() {
                prompt.push_str(&format!("- 备注: {}\n", note));
            }
        }
        prompt.push_str(
            "- 该日不安排任何任务，is_rest_day 必须为 true，subject_allocations 为空数组。\n",
        );
        prompt.push_str("- 原本安排在该日的任务量必须分摊到本周其他学习日。\n\n");

        // 排除日原有任务清单（AI 精确承接原任务的依据）
        if !excluded_original_allocations.is_empty() {
            prompt.push_str("## 排除日原有任务清单（必须承接分摊到剩余学习日）\n");
            prompt.push_str(&format!(
                "排除日 {} 原本安排了以下任务，重排时必须把这些任务量分摊到剩余学习日：\n",
                excluded_day.date
            ));
            for alloc in excluded_original_allocations {
                let subj_label = alloc.subject.label();
                prompt.push_str(&format!(
                    "- {}（{}h）: {}",
                    subj_label, alloc.hours, alloc.focus
                ));
                if !alloc.task_templates.is_empty() {
                    let titles: Vec<&str> = alloc
                        .task_templates
                        .iter()
                        .map(|t| t.title.as_str())
                        .collect();
                    prompt.push_str(&format!(" → {}", titles.join("、")));
                }
                prompt.push('\n');
            }
            prompt.push('\n');
        }

        // 当前各科状态
        prompt.push_str("## 当前各科状态\n");
        prompt.push_str(&format!(
            "### 数学\n- 阶段: {:?}\n- 当前重点: {}\n- 已完成: {:?}\n- 薄弱章节: {:?}\n\n",
            state.subjects.math.phase,
            state.subjects.math.current_focus,
            state.subjects.math.completed,
            state.subjects.math.weak_chapters
        ));
        prompt.push_str(&format!(
            "### 英语\n- 阶段: {:?}\n- 当前重点: {}\n- 已完成: {:?}\n\n",
            state.subjects.english.phase,
            state.subjects.english.current_focus,
            state.subjects.english.completed
        ));
        prompt.push_str(&format!(
            "### 专业课\n- 阶段: {:?}\n- 当前重点: {}\n- 已完成: {:?}\n\n",
            state.subjects.professional.phase,
            state.subjects.professional.current_focus,
            state.subjects.professional.completed
        ));
        if state.subjects.politics.active {
            prompt.push_str(&format!(
                "### 政治\n- 阶段: {:?}\n- 当前重点: {}\n- 已完成: {:?}\n\n",
                state.subjects.politics.phase,
                state.subjects.politics.current_focus,
                state.subjects.politics.completed
            ));
        }

        // 已完成内容防重复提示
        let has_any_completed = !state.subjects.math.completed.is_empty()
            || !state.subjects.english.completed.is_empty()
            || !state.subjects.professional.completed.is_empty()
            || (state.subjects.politics.active && !state.subjects.politics.completed.is_empty());
        if has_any_completed {
            prompt.push_str("\n## 已完成内容防重复（重要）\n");
            prompt.push_str("上述各科「已完成」列表中的章节/任务均已完成，重排时**严禁重复**这些内容。各科目必须从「已完成」之后的下一个章节/知识点继续推进，不得回退。\n\n");
        }

        // 学习日程约束
        prompt.push_str("## 学习日程约束\n");
        prompt.push_str(&format!(
            "- 休息日: {}（这些日子不安排任务）\n",
            rest_days.join("、")
        ));
        prompt.push_str(&format!(
            "- 每日任务数量: {} 个（由每日目标学时 {:.2}h ÷ 用户设置的任务粒度 {:.2}h/条 派生）；单条任务 estimated_hours 目标 ≈ {:.2}h，不得超过 {:.2}h；每科约一条；未开始的科目不安排）\n",
            effective_daily_task_count,
            daily_target_hours,
            standard_granularity,
            standard_granularity,
            standard_granularity * 1.5
        ));
        prompt.push_str(&format!(
            "- 总结/复习任务: {}\n",
            if enable_review_tasks {
                "允许安排"
            } else {
                "禁止安排（严禁任何形式的复习/巩固类任务，包括「回顾」「总结」「复习」「梳理」「练习」「巩固」「强化」「温习」「复盘」等）"
            }
        ));

        // 各科学时分配（用户配置占比，重要约束）：分摊排除日任务时也须按占比分配各科学时
        if let Some(allocation) = subject_time_allocation {
            let study_days = crate::core::planning::constraints::available_dates(
                regen_start,
                week_end,
                rest_days,
                all_excluded_days,
                &today_string(),
            )
            .unwrap_or_default()
            .len();
            prompt.push_str("## 各科学时分配（用户配置，重要）\n");
            prompt.push_str(&format!(
                "- 用户为各科配置了学习时间占比（合计 100%，基准：每日目标学时 {:.2}h × 剩余 {} 个可学习日）：\n",
                daily_target_hours, study_days
            ));
            for (key, cn) in [
                ("math", "数学"),
                ("english", "英语"),
                ("politics", "政治"),
                ("professional", "专业课"),
            ] {
                if let Some(share) = allocation.get(key) {
                    let weekly = daily_target_hours * study_days as f64 * share / 100.0;
                    let daily = daily_target_hours * share / 100.0;
                    prompt.push_str(&format!(
                        "- {}：占比 {:.0}%（每周约 {:.1}h，每日约 {:.1}h）\n",
                        cn, share, weekly, daily
                    ));
                }
            }
            prompt.push_str("- 要求：分摊后各科 subject_allocations 的 hours 之和应按上述占比分配；占比为 0 的科目不安排任务；未开始的科目（开始日期晚于本周日）即使占比 > 0 也不得安排。\n\n");
        }

        // 本周所有排除日（重排时必须遵守）
        if !all_excluded_days.is_empty() {
            prompt.push_str("\n### 本周所有特殊情况排除日期\n");
            prompt.push_str("以下日期用户已声明不学习，重排时这些日期必须：is_rest_day=true、subject_allocations 为空数组。原本安排在这些日期的任务量分摊到剩余学习日。\n");
            let type_label_fn = |t: &str| -> &'static str {
                match t {
                    "travel" => "外出旅行",
                    "sick" => "生病",
                    "exam" => "考试",
                    _ => "其他",
                }
            };
            for ex in all_excluded_days {
                let weekday = weekday_name(&ex.date).unwrap_or_else(|_| "未知".to_string());
                prompt.push_str(&format!(
                    "- {}（{}）: {}",
                    ex.date,
                    weekday,
                    type_label_fn(&ex.reason_type)
                ));
                if let Some(note) = &ex.note {
                    if !note.is_empty() {
                        prompt.push_str(&format!(" — {}", note));
                    }
                }
                prompt.push('\n');
            }
        }

        // 各科开始日期约束
        let has_unstarted = subject_start_dates.iter().any(|(k, d)| {
            !d.is_empty()
                && d.as_str() > week_end
                && (*k == "math" || *k == "english" || *k == "politics" || *k == "professional")
        });
        if has_unstarted {
            prompt.push_str("\n## 各科开始日期约束\n");
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
                if !date.is_empty() && date.as_str() > week_end {
                    prompt.push_str(&format!(
                        "- {}（{}）: 开始日期 {}，本周剩余天数不得安排任务\n",
                        subject_cn(key),
                        key,
                        date
                    ));
                }
            }
        }

        // 原安排参考
        prompt.push_str("\n## 剩余天数原安排（仅供参考，可调整）\n");
        for day in regen_days {
            let weekday = weekday_name(&day.date).unwrap_or_else(|_| "未知".to_string());
            prompt.push_str(&format!(
                "\n**{}（{}）**{}\n",
                day.date,
                weekday,
                if day.is_rest_day {
                    "【休息日/排除日】"
                } else {
                    ""
                }
            ));
            if !day.is_rest_day && !day.subject_allocations.is_empty() {
                for alloc in &day.subject_allocations {
                    let subj_label = alloc.subject.label();
                    prompt.push_str(&format!(
                        "- {}（{}h）: {}",
                        subj_label, alloc.hours, alloc.focus
                    ));
                    if !alloc.task_templates.is_empty() {
                        let titles: Vec<&str> = alloc
                            .task_templates
                            .iter()
                            .map(|t| t.title.as_str())
                            .collect();
                        prompt.push_str(&format!(" → {}", titles.join("、")));
                    }
                    prompt.push('\n');
                }
            }
        }

        // 输出要求
        prompt.push_str(&format!(
            r#"

## 输出要求

请直接输出一个合法的 JSON 对象（不要包裹 ```json 代码块），严格符合以下结构：

{{
  "days": [
    {{
      "date": "YYYY-MM-DD",
      "subject_allocations": [
        {{
          "subject": "math",
          "hours": 2.0,
          "focus": "当天该科重点",
          "task_templates": [
            {{
              "title": "任务标题",
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
  ]
}}

重要约束：
1. 必须覆盖 {} 至 {} 的所有学习日（休息日和排除日无需输出 subject_allocations，但 date 必须包含）。
2. 排除日（{}）的 subject_allocations 必须为空数组。
3. subject 只能是 "math" / "english" / "politics" / "professional" 之一。
4. 任务已不再分级（Priority A/B 已废弃）：task_templates 中不要输出 priority 字段，任务不做 A/B 区分，所有任务同等对待。
5. {math}
6. 每天的 task_templates 数量约 {} 个，每科约一条。
7. 排除日原本的任务量必须分摊到剩余学习日，可通过适当增加每日任务数或难度来实现。
8. {}若用户禁止总结任务，task_templates 的标题和 goal 不得出现"回顾"/"总结"/"复习"/"梳理"/"练习"/"巩固"/"强化"/"温习"/"复盘"等字样，每个任务必须推进新知识点、新章节或新习题。{}
9. 休息日的 subject_allocations 为空数组。
10. **不得重复已完成内容**：各科「已完成」列表中的章节/任务严禁再次出现，必须从已完成之后的下一个章节/知识点继续推进。
11. **按天推进切分（受排除日影响）**：每个科目的学习内容必须切分到剩余的每个学习日，每天推进不同的章节/知识点/习题，逐日向前递进。同一章节可以跨天安排，但每天推进的知识点或练习范围必须明确不同（休息日/排除日除外）。实际可学习天数按程序提供的可学习日期集合计算（休息日和排除日重叠只扣一次），排除日不分配任务，其任务量分摊到其他学习日，每天的 task_templates 数量限制（约束6）仍须遵守。
"#,
            regen_start, week_end,
            all_excluded_days.iter().map(|d| d.date.as_str()).collect::<Vec<_>>().join("、"),
            effective_daily_task_count,
            if enable_review_tasks { "" } else { "严禁安排总结/复习类任务。" },
            if basics_done_labels.is_empty() {
                String::new()
            } else {
                format!(
                    "**例外（优先级高于本条禁令）**：{} 的启用进度表知识点已全部达「基础」，已进入强化阶段，不受本开关限制，必须为其安排巩固/复习/强化类任务（真题二刷、错题重做、限时模考、背诵复习等），并严禁为其虚构新的章节/知识点进度；上述禁令只对处于基础阶段的其余科目生效。",
                    basics_done_labels.join("、")
                )
            },
            math = Self::math_syllabus_constraint(state),
        ));

        // 英语单词每日安排约束：凡当天安排了英语任务，须至少包含 1 条背单词任务
        if state.subjects.english.active {
            prompt.push_str(&format!(
                "\n补充约束（英语单词每天安排）：只要当天 subject_allocations 中安排了英语科目（english）的任务，则该英语科目 task_templates 中必须至少包含 1 条「背单词」任务（如：{}），并计入当天任务数；休息日/排除日以及当天未安排英语任务的日子不受此约束。\n",
                if enable_review_tasks {
                    "背 N 个新词（可含单词复习巩固）"
                } else {
                    "背 N 个新词（推进新词，不含复习字样）"
                }
            ));
        }

        prompt
    }
}
