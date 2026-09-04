//! Planner — 调用 AI Service 生成周计划，再通过 DailyScheduler 生成日计划
//!
//! 数据契约：统一 JSON 格式 { version, meta, data, view? }
//! - 周计划：plan/YYYY-Www_week.json
//! - 日计划：plan/YYYY-MM-DD_day.json

use std::path::Path;

use crate::ai::provider::{AgentType, ChatMessage, ChatRequest, MessageRole};
use crate::ai::service::AiService;
use crate::api::commands::legacy::RegenDayChange;
use crate::core::adaptive_planner::AdaptivePlanParameters;
use crate::core::scheduler::DailyScheduler;
use crate::data::plan::{DailyPlanFile, ExcludedDay, WeekPlanFile, WorkloadAdjustment};
use crate::data::state::StudyState;
use crate::data::{
    add_days, clean_ai_json, days_between, get_week_end, get_week_start, iso_week_string,
    today_string, weekday_name, DataResult,
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

    /// 发送一次「复盘后重排」的 AI 请求并把响应解析为剩余天数安排。
    ///
    /// - 失败或解析为空数组时返回 Err（错误已写入 ai-debug.log）。
    /// - `escalation = true` 表示「进度未生效」时的校正重排，日志使用独立的 tag。
    async fn chat_regen_pass(
        &self,
        data_dir: &Path,
        review_date: &str,
        prompt: &str,
        math_version: Option<String>,
        escalation: bool,
    ) -> Result<Vec<RegenDayPlan>, String> {
        let request = ChatRequest {
            messages: vec![ChatMessage {
                role: MessageRole::User,
                content: prompt.to_string(),
                ..Default::default()
            }],
            agent: Some(AgentType::Planner),
            temperature: Some(0.6),
            // 重排剩余天数的工作量与生成周计划相当，给足 300s
            timeout_override: Some(300),
            math_version,
            ..Default::default()
        };
        let tag = if escalation {
            "regenerate_escalated"
        } else {
            "regenerate"
        };
        crate::data::write_ai_debug_log(
            data_dir,
            &format!("{}_ai_request", tag),
            &format!(
                "即将发送 AI 请求{}, review_date={}, timeout=300s",
                if escalation {
                    "(进度校正重试)"
                } else {
                    ""
                },
                review_date
            ),
        );
        self.ai_service
            .chat(request)
            .await
            .map_err(|e| {
                crate::data::write_ai_debug_log(
                    data_dir,
                    &format!("{}_ai_call_error", tag),
                    &format!("AI 调用失败: {}", e),
                );
                format!("AI 重排剩余天数失败: {}", e)
            })
            .and_then(|response| {
                let resp_preview: String = response.content.chars().take(500).collect();
                log::info!(
                    "[AI-DEBUG] 重排响应长度: {} 字符, 前 500 字符: {}",
                    response.content.len(),
                    resp_preview
                );
                crate::data::write_ai_debug_log(
                    data_dir,
                    &format!("{}_ai_response", tag),
                    &format!(
                        "AI 响应已返回, 长度={} 字符, 前 500 字符:\n{}",
                        response.content.len(),
                        resp_preview
                    ),
                );
                parse_regenerate_response(&response.content, data_dir)
            })
    }

    /// 构建「进度未生效」时的校正重排 prompt。
    ///
    /// 在基础 prompt 之后追加一段强制指令：让 AI 依据考研/统考教纲章节先后顺序
    /// （必要时借助自身联网查证能力核对章节顺序），判断并纠正超前于用户实际进度的任务，
    /// 从用户实际进度之后重新排布剩余天数，从而真正改写周计划文件而非静默通过。
    ///
    /// `anchors` 为 (科目 key, 用户声明的实际进度章节) 列表。
    fn build_escalation_prompt(&self, base: &str, anchors: &[(String, String)]) -> String {
        let mut p = base.to_string();
        p.push_str("\n\n### 上次重排未生效，必须按用户实际进度强制校正\n");
        p.push_str("以下科目声明了用户实际学习进度，但上次重排的剩余安排与重排前完全一致，说明超前于实际进度的任务仍被原样保留，未真正按实际进度调整：\n");
        for (key, chapter) in anchors {
            let label = match key.as_str() {
                "math" => "数学",
                "english" => "英语",
                "politics" => "政治",
                "professional" => "专业课",
                _ => key.as_str(),
            };
            p.push_str(&format!("- {}: 用户实际进度位于「{}」\n", label, chapter));
        }
        p.push_str("要求：\n");
        p.push_str("1. 你应依据该科目考研/统考教纲的章节先后顺序判断这些任务是否超前于用户实际进度（必要时借助自身联网查证能力核对章节顺序）。\n");
        p.push_str("2. 若原计划把任务排在了用户尚未到达的章节（超前），必须删除或后置这些超前任务，并从用户实际进度【之后】的下一个未学章节开始，重新排布剩余学习日，逐日向前推进。\n");
        p.push_str("3. 本次输出的每科 subject_allocations 必须与重排前明显不同，严禁再次原样返回未完成的前置任务。\n");
        p.push_str("4. 保持 JSON 的 days 数组结构不变，仅调整各科任务内容；休息日 subject_allocations 保持空数组。\n");
        p
    }

    /// 生成日计划
    ///
    /// 流程：
    /// 1. 校验今天/休息日
    /// 2. 调用 AI 生成今日任务 + 重排周计划剩余天数
    /// 3. 保存日计划 JSON + 更新周计划 JSON
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

        self.generate_daily_plan_from_week(data_dir, date).await
    }

    /// 从周计划生成日计划（不调用 AI）
    ///
    /// 流程：
    /// 1. 通过 DailyScheduler 从周计划生成日计划
    /// 2. 保存日计划 JSON
    /// 3. 同步到 State.current_task
    async fn generate_daily_plan_from_week(
        &self,
        data_dir: &Path,
        date: &str,
    ) -> DataResult<DailyPlanFile> {
        let plan = DailyScheduler::generate_daily_plan(data_dir, date, true)?;
        crate::data::plan::save_daily_plan(data_dir, &plan)?;
        if let Err(e) = Self::sync_current_task(data_dir, date, &plan) {
            log::warn!("同步 current_task 失败: {}", e);
        }
        Ok(plan)
    }

    /// 复盘后重新生成本周剩余天数的周计划安排（AI 驱动）
    ///
    /// 在用户提交复盘后调用。AI 会根据：
    /// - 昨日复盘（未完成/困难/额外进度）
    /// - 当前 State 进度
    /// - 周计划剩余天数的原安排
    ///   重新生成剩余天数（review_date+1 至 week_end）的 subject_allocations。
    ///
    /// 如果 review_date 的次日（review_date+1）在剩余天数中且其日计划已存在，
    /// 也会一并重新生成该日的日计划。
    ///
    /// 返回 (是否实际重排, 重排影响的日期列表, 是否启用了确定性兜底[AI 失败], 一致性警告列表)
    pub async fn regenerate_remaining_days_after_review(
        &self,
        data_dir: &Path,
        review_date: &str,
    ) -> DataResult<(bool, Vec<String>, bool, Vec<String>, Vec<RegenDayChange>)> {
        crate::data::write_ai_debug_log(
            data_dir,
            "regenerate_start",
            &format!(
                "开始复盘后重排, review_date={}, data_dir={:?}",
                review_date, data_dir
            ),
        );

        // 1. 读取复盘
        let review = crate::data::records::read_review(data_dir, review_date)?;

        // 2. 判断是否需要重排
        let needs_regen = check_review_needs_regeneration(&review);
        if !needs_regen {
            log::info!(
                "复盘 {} 无需重排剩余天数（无未完成/困难/额外进度）",
                review_date
            );
            return Ok((false, Vec::new(), false, Vec::new(), Vec::new()));
        }

        // 3. 读取本周周计划
        let iso_week = iso_week_string(review_date)?;
        let mut week_plan = crate::data::plan::read_week_plan(data_dir, &iso_week)
            .map_err(|e| format!("读取周计划失败: {}。请先生成周计划。", e))?;

        // 重排前切片：用于一致性校验比对「计划外进度是否生效」
        let original_before = week_plan.clone();

        let week_end = week_plan.meta.week_end.clone();

        // 4. 确定需要重排的日期范围：review_date+1 至 week_end
        let regen_start = add_days(review_date, 1)?;
        if regen_start > week_end {
            log::info!(
                "复盘 {} 之后已无剩余天数需要重排（本周已结束）",
                review_date
            );
            return Ok((false, Vec::new(), false, Vec::new(), Vec::new()));
        }

        // 收集需要重排的日期
        let regen_dates: Vec<String> = {
            let mut dates = Vec::new();
            let mut current = regen_start.clone();
            loop {
                dates.push(current.clone());
                if current == week_end {
                    break;
                }
                current = add_days(&current, 1)?;
            }
            dates
        };

        // 收集这些日期在周计划中的原安排（作为参考给 AI）
        let regen_days: Vec<&crate::data::plan::WeekDayPlan> = week_plan
            .data
            .days
            .iter()
            .filter(|d| regen_dates.contains(&d.date))
            .collect();

        // 5. 读取 State 和设置
        let state = crate::data::state::read_state_or_default(data_dir);
        let settings = crate::load_settings(data_dir);
        let rest_days = settings.rest_days();
        let subject_start_dates = settings.subject_start_dates();
        let daily_target_hours = settings.daily_target_hours();
        let standard_granularity = settings.standard_granularity();
        let enable_review_tasks = settings.enable_review_tasks();

        // 6. 构建 prompt
        let prompt = self.build_regenerate_prompt(
            &state,
            &review,
            &regen_days,
            review_date,
            &regen_start,
            &week_end,
            &week_plan.meta.week_start,
            &rest_days,
            &subject_start_dates,
            daily_target_hours,
            standard_granularity,
            enable_review_tasks,
            &week_plan.data.excluded_days,
        );
        crate::data::write_ai_debug_log(
            data_dir,
            "regenerate_prompt_ready",
            &format!(
                "prompt 已构建, 长度={} 字符, regen_dates={:?}",
                prompt.len(),
                regen_dates
            ),
        );

        // 7. 调用 AI（首次重排）。日志时间线由 chat_regen_pass 统一写入。
        log::info!(
            "[AI-DEBUG] 复盘后重排请求开始, review_date={}, regen范围={}-{}",
            review_date,
            regen_start,
            week_end
        );

        // 7-9. 调用 AI 并解析剩余天数安排；若 AI 失败或返回空，启用确定性兜底，
        // 把复盘中标记的「未完成 / 部分完成」任务落到剩余学习日，保证不丢失。
        let mut used_fallback = false;
        let updated_days = match self
            .chat_regen_pass(
                data_dir,
                review_date,
                &prompt,
                state.subjects.math.version.clone(),
                false,
            )
            .await
        {
            Ok(days) if !days.is_empty() => days,
            other => {
                let reason = match other {
                    Ok(_) => "AI 未返回 days 数组".to_string(),
                    Err(e) => e,
                };
                log::warn!(
                    "[AI-DEBUG] 复盘重排未生效（{}），启用未完成任务确定性兜底",
                    reason
                );
                crate::data::write_ai_debug_log(
                    data_dir,
                    "regenerate_fallback",
                    &format!("AI 重排失败，启用兜底: {}", reason),
                );
                let placed = uncompleted_tasks_fallback(&mut week_plan, &review, &regen_dates);
                used_fallback = placed;
                Vec::new()
            }
        };

        // 9. 更新周计划（兜底路径已直接改写 week_plan，跳过 update）
        if !used_fallback {
            update_week_plan_remaining_days(&mut week_plan, &updated_days);
        }

        // 9.5 一致性校验与确定性修正：
        //   - 剔除与新「已完成」重复的计划任务（无论 AI 是否遵守约束都兜底生效）
        //   - 检测声明了计划外进度的科目重排后是否真正生效，未生效则记入警告
        let declared_subjects: std::collections::HashSet<String> = review
            .overcompletion
            .iter()
            .map(|oc| oc.subject.clone())
            .collect();
        let mut consistency_warnings = consistency_check_and_correct(
            &mut week_plan,
            &state,
            &regen_dates,
            &original_before,
            &declared_subjects,
        );

        // 9.55 确定性超前剔除：按内置章节顺序表，去掉剩余计划中「排在实际进度之前（已学过）」的任务。
        // 这是不依赖 AI、确定能改写周计划文件的进度锚定兜底。
        if filter_ahead_of_progress(&mut week_plan, &state, &regen_dates, &review.overcompletion) {
            log::info!("一致性校验：已按内置章节顺序表剔除超前于实际进度的计划任务");
            crate::data::write_ai_debug_log(
                data_dir,
                "regenerate_progress_filter",
                "已按章节顺序表剔除超前于实际进度的任务",
            );
        }

        // 9.6 进度未生效 -> 联网/教纲自查 + 强制按实际进度校正（二次重排）
        // 若声明了实际进度的科目，重排后剩余安排与重排前完全一致（静默 no-op），
        // 不当作成功通过：追加一次校正重排，让 AI 依据教纲自查章节顺序，
        // 删除/后置超前任务，并从用户实际进度之后重新排布（真正改写周计划文件）。
        if !used_fallback && !declared_subjects.is_empty() {
            let unchanged = find_declared_subjects_unchanged(
                &week_plan,
                &original_before,
                &regen_dates,
                &declared_subjects,
            );
            if !unchanged.is_empty() {
                let anchors: Vec<(String, String)> = review
                    .overcompletion
                    .iter()
                    .filter(|oc| unchanged.contains(&oc.subject))
                    .map(|oc| (oc.subject.clone(), oc.chapter_reached.clone()))
                    .collect();
                crate::data::write_ai_debug_log(
                    data_dir,
                    "regenerate_escalation_start",
                    &format!("进度未生效({:?})，发起按实际进度校正重排", anchors),
                );
                let escalated_prompt = self.build_escalation_prompt(&prompt, &anchors);
                match self
                    .chat_regen_pass(
                        data_dir,
                        review_date,
                        &escalated_prompt,
                        state.subjects.math.version.clone(),
                        true,
                    )
                    .await
                {
                    Ok(days) if !days.is_empty() => {
                        update_week_plan_remaining_days(&mut week_plan, &days);
                        consistency_warnings = consistency_check_and_correct(
                            &mut week_plan,
                            &state,
                            &regen_dates,
                            &original_before,
                            &declared_subjects,
                        );
                        let still = find_declared_subjects_unchanged(
                            &week_plan,
                            &original_before,
                            &regen_dates,
                            &declared_subjects,
                        );
                        crate::data::write_ai_debug_log(
                            data_dir,
                            "regenerate_escalation_result",
                            &format!("校正后仍未生效科目: {:?}", still),
                        );
                    }
                    Err(e) => {
                        log::warn!("[AI-DEBUG] 进度校正重排失败: {}", e);
                        crate::data::write_ai_debug_log(
                            data_dir,
                            "regenerate_escalation_error",
                            &format!("进度校正重排失败: {}", e),
                        );
                    }
                    Ok(_) => {
                        crate::data::write_ai_debug_log(
                            data_dir,
                            "regenerate_escalation_error",
                            "进度校正重排返回空 days",
                        );
                    }
                }
            }
        }

        // 去重/校正可能改写了周计划，持久化后再生成日计划
        crate::data::plan::save_week_plan(data_dir, &week_plan)?;
        log::info!("周计划剩余天数已更新, 影响日期: {:?}", regen_dates);
        crate::data::write_ai_debug_log(
            data_dir,
            "regenerate_week_plan_saved",
            &format!("周计划已保存, 影响日期: {:?}", regen_dates),
        );

        if !consistency_warnings.is_empty() {
            log::warn!(
                "一致性校验：计划外进度未反映到计划 -> {}",
                consistency_warnings.join("；")
            );
            crate::data::write_ai_debug_log(
                data_dir,
                "regenerate_consistency_warning",
                &format!("计划外进度未生效: {}", consistency_warnings.join("；")),
            );
        }

        // 10. 重新生成所有受影响日期的日计划文件
        Self::regenerate_daily_plans_for_dates(data_dir, &week_plan, &regen_dates, "review_regen")?;

        // 10.5 对比重排前后周计划，汇总各受影响日期的任务变动明细（供前端悬停展示）
        let changes = compute_regen_changes(&original_before, &week_plan, &regen_dates);

        crate::data::write_ai_debug_log(
            data_dir,
            "regenerate_complete",
            &format!(
                "复盘后重排完成, review_date={}, 影响日期: {:?}",
                review_date, regen_dates
            ),
        );

        Ok((
            true,
            regen_dates,
            used_fallback,
            consistency_warnings,
            changes,
        ))
    }

    /// 周中新增排除日后，重新生成本周剩余天数的周计划安排（AI 驱动）
    ///
    /// 在用户于周计划页点击"今天及之后"的日期标记为排除日时调用。
    /// AI 会根据：
    /// - 新增的排除日（该日不排任务）
    /// - 当前 State 进度
    /// - 周计划剩余天数的原安排
    ///   重新生成重排范围（excluded_date 至 week_end）的 subject_allocations，
    ///   把原本安排在排除日的任务量分摊到剩余学习日。
    ///
    /// 返回 (是否实际重排, 重排影响的日期列表, 是否启用了确定性兜底[AI 失败])
    pub async fn regenerate_after_exclusion(
        &self,
        data_dir: &Path,
        week_start: &str,
        excluded_day: ExcludedDay,
    ) -> DataResult<(bool, Vec<String>, bool)> {
        crate::data::write_ai_debug_log(
            data_dir,
            "exclusion_regen_start",
            &format!(
                "开始排除日重排, week_start={}, excluded_date={}, reason={}",
                week_start, excluded_day.date, excluded_day.reason_type
            ),
        );

        // 1. 读取本周周计划
        let iso_week = iso_week_string(week_start)?;
        let mut week_plan = crate::data::plan::read_week_plan(data_dir, &iso_week)
            .map_err(|e| format!("读取周计划失败: {}。请先生成周计划。", e))?;

        let week_end = week_plan.meta.week_end.clone();

        // 2. 校验排除日不早于今天（不允许排除过去日期）
        let today = today_string();
        if excluded_day.date < today {
            return Err(format!(
                "不能排除过去的日期（{}），今天是 {}。只能排除今天及之后的日期。",
                excluded_day.date, today
            ));
        }

        // 3. 校验排除日在本周范围内
        if excluded_day.date.as_str() < week_start || excluded_day.date.as_str() > week_end.as_str()
        {
            return Err(format!(
                "排除日 {} 不在本周（{} 至 {}）范围内",
                excluded_day.date, week_start, week_end
            ));
        }

        // 4. 若该日已是排除日，直接返回（幂等）；但仍收敛一次该日旧计划，
        //    保证旧版本残留的日计划/滴答任务也能被清理
        if week_plan
            .data
            .excluded_days
            .iter()
            .any(|d| d.date == excluded_day.date)
        {
            log::info!("排除日 {} 已存在，跳过重排", excluded_day.date);
            if let Err(e) = crate::data::plan::clear_daily_plan(data_dir, &excluded_day.date) {
                log::warn!("清空排除日 {} 旧日计划失败: {}", excluded_day.date, e);
            }
            return Ok((false, Vec::new(), false));
        }

        // 5. 捕获排除日原有任务量（用于 AI 失败时程序化再分摊兜底，enforce 会清空它）
        let excluded_allocations: Vec<crate::data::plan::DaySubjectAllocation> = week_plan
            .data
            .days
            .iter()
            .find(|d| d.date == excluded_day.date)
            .map(|d| d.subject_allocations.clone())
            .unwrap_or_default();

        // 6. 加入排除日列表 + enforce
        week_plan.data.excluded_days.push(excluded_day.clone());
        let excluded_snapshot = week_plan.data.excluded_days.clone();
        enforce_excluded_days(&mut week_plan, &excluded_snapshot)?;

        // 7. 确定重排范围：excluded_date 至 week_end
        let regen_start = excluded_day.date.clone();
        let regen_dates: Vec<String> = {
            let mut dates = Vec::new();
            let mut current = regen_start.clone();
            loop {
                dates.push(current.clone());
                if current == week_end {
                    break;
                }
                current = add_days(&current, 1)?;
            }
            dates
        };

        // 收集这些日期在周计划中的原安排（作为参考给 AI）
        let regen_days: Vec<&crate::data::plan::WeekDayPlan> = week_plan
            .data
            .days
            .iter()
            .filter(|d| regen_dates.contains(&d.date))
            .collect();

        // 7. 读取 State 和设置
        let state = crate::data::state::read_state_or_default(data_dir);
        let settings = crate::load_settings(data_dir);
        let rest_days = settings.rest_days();
        let subject_start_dates = settings.subject_start_dates();
        let daily_target_hours = settings.daily_target_hours();
        let standard_granularity = settings.standard_granularity();
        let enable_review_tasks = settings.enable_review_tasks();

        // 8. 构建 prompt
        let prompt = self.build_exclusion_regenerate_prompt(
            &state,
            &excluded_day,
            &regen_days,
            &excluded_allocations,
            &regen_start,
            &week_end,
            &week_plan.meta.week_start,
            &rest_days,
            &subject_start_dates,
            daily_target_hours,
            standard_granularity,
            enable_review_tasks,
            &week_plan.data.excluded_days,
        );
        crate::data::write_ai_debug_log(
            data_dir,
            "exclusion_regen_prompt_ready",
            &format!(
                "prompt 已构建, 长度={} 字符, regen_dates={:?}",
                prompt.len(),
                regen_dates
            ),
        );

        // 9. 调用 AI
        let request = ChatRequest {
            messages: vec![ChatMessage {
                role: MessageRole::User,
                content: prompt,
                ..Default::default()
            }],
            agent: Some(AgentType::Planner),
            temperature: Some(0.6),
            timeout_override: Some(300),
            math_version: state.subjects.math.version.clone(),
            ..Default::default()
        };

        log::info!(
            "[AI-DEBUG] 排除日重排请求开始, excluded_date={}, regen范围={}-{}",
            excluded_day.date,
            regen_start,
            week_end
        );
        crate::data::write_ai_debug_log(
            data_dir,
            "exclusion_regen_ai_request",
            &format!(
                "即将发送 AI 请求, excluded_date={}, regen范围={}-{}, timeout=300s",
                excluded_day.date, regen_start, week_end
            ),
        );
        // 调用 AI 并解析剩余天数安排；若 AI 失败或返回空，启用确定性兜底，
        // 把原本排在排除日的任务逐条分摊到剩余学习日，保证「排除日的任务量不丢失」。
        let parse_result: Result<Vec<RegenDayPlan>, String> = self
            .ai_service
            .chat(request)
            .await
            .map_err(|e| {
                crate::data::write_ai_debug_log(
                    data_dir,
                    "exclusion_regen_ai_call_error",
                    &format!("AI 调用失败: {}", e),
                );
                format!("AI 排除日重排失败: {}", e)
            })
            .and_then(|response| {
                let resp_preview: String = response.content.chars().take(500).collect();
                log::info!(
                    "[AI-DEBUG] 排除日重排响应长度: {} 字符, 前 500 字符: {}",
                    response.content.len(),
                    resp_preview
                );
                crate::data::write_ai_debug_log(
                    data_dir,
                    "exclusion_regen_ai_response",
                    &format!(
                        "AI 响应已返回, 长度={} 字符, 前 500 字符:\n{}",
                        response.content.len(),
                        resp_preview
                    ),
                );
                parse_regenerate_response(&response.content, data_dir)
            });

        let mut used_fallback = false;
        let updated_days = match parse_result {
            Ok(days) if !days.is_empty() => days,
            other => {
                let reason = match other {
                    Ok(_) => "AI 未返回 days 数组".to_string(),
                    Err(e) => e,
                };
                log::warn!(
                    "[AI-DEBUG] 排除日重排未生效（{}），启用确定性再分摊兜底",
                    reason
                );
                crate::data::write_ai_debug_log(
                    data_dir,
                    "exclusion_regen_fallback",
                    &format!("AI 重排失败，启用排除日再分摊兜底: {}", reason),
                );
                let placed = exclusion_redistribute_fallback(
                    &mut week_plan,
                    &excluded_allocations,
                    &regen_dates,
                    &subject_start_dates,
                );
                used_fallback = placed;
                Vec::new()
            }
        };

        // M14：仅当 AI 重排或兜底分摊实际改动了计划时标记 regenerated=true，
        // 避免前端误判「重排成功」；fallback 未放置任何任务时返回 false
        let actually_regenerated = used_fallback || !updated_days.is_empty();

        // 更新周计划（兜底路径已直接改写 week_plan，跳过 update）
        if !used_fallback {
            update_week_plan_remaining_days(&mut week_plan, &updated_days);
        }
        // enforce 确保排除日仍为休息日（AI 可能在重排时误塞任务）
        let excluded_snapshot2 = week_plan.data.excluded_days.clone();
        enforce_excluded_days(&mut week_plan, &excluded_snapshot2)?;
        crate::data::plan::save_week_plan(data_dir, &week_plan)?;
        log::info!("周计划已更新（排除日重排）, 影响日期: {:?}", regen_dates);
        crate::data::write_ai_debug_log(
            data_dir,
            "exclusion_regen_week_plan_saved",
            &format!("周计划已保存, 影响日期: {:?}", regen_dates),
        );

        // 12. 重新生成所有受影响日期的日计划文件
        Self::regenerate_daily_plans_for_dates(
            data_dir,
            &week_plan,
            &regen_dates,
            "exclusion_regen",
        )?;

        crate::data::write_ai_debug_log(
            data_dir,
            "exclusion_regen_complete",
            &format!(
                "排除日重排完成, excluded_date={}, 影响日期: {:?}",
                excluded_day.date, regen_dates
            ),
        );

        Ok((actually_regenerated, regen_dates, used_fallback))
    }

    /// 将生成的日计划同步到 State.current_task
    /// 仅在 state 中当前日期任务为空或日期不匹配时才写入，避免覆盖已有的完成状态
    fn sync_current_task(data_dir: &Path, date: &str, plan: &DailyPlanFile) -> DataResult<()> {
        let mut state = crate::data::state::read_state_or_default(data_dir);

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
                started_at: None,
                accumulated_minutes: 0,
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
    /// 4. 保存原始周计划副本 plan/YYYY-Www_week_original.json（用于一周结束后对比）
    /// 5. 返回 WeekPlanFile
    ///
    /// 注意：周计划生成后不再逐天生成日计划。日计划由 generate_daily_plan
    /// 从周计划直接生成（不调用 AI）。复盘提交后，若需要调整（未完成/困难/额外进度），
    /// 由 regenerate_remaining_days_after_review 调用 AI 重新生成本周剩余天数安排。
    pub async fn generate_week_plan(
        &self,
        data_dir: &Path,
        week_start: &str,
        excluded_days: &[ExcludedDay],
        workload_adjustment: Option<&WorkloadAdjustment>,
    ) -> DataResult<WeekPlanFile> {
        let week_end = get_week_end(week_start)?;

        // 0. 只允许生成本周计划（包含今天的周）
        let today = today_string();
        let today_week_start = get_week_start(&today)?;
        if week_start != today_week_start {
            return Err(format!(
                "只能生成本周（{} 至 {}）的周计划，不支持生成过去或未来的周计划",
                today_week_start,
                get_week_end(&today_week_start)?
            ));
        }

        // 1. 读取 State 和 User Model
        let state = crate::data::state::read_state_or_default(data_dir);
        let user_model = crate::data::assets::read_user_model_index(data_dir).unwrap_or_default();
        let recent_reviews = Self::read_recent_reviews(data_dir, week_start, 5);
        let exam_config = Self::read_exam_config(data_dir);
        let knowledge_graph = Self::read_knowledge_graph_summary(data_dir);
        let settings = crate::load_settings(data_dir);

        // 1.1 读取上一周的 7 个日计划及其对应复盘，作为本周排程参考
        let prev_week_start = add_days(week_start, -7)?;
        let prev_week_end = get_week_end(&prev_week_start)?;
        let prev_week_daily_plans =
            crate::data::plan::read_week_daily_plans(data_dir, &prev_week_start)
                .unwrap_or_default();
        let prev_week_reviews =
            Self::read_reviews_in_range(data_dir, &prev_week_start, &prev_week_end);

        // 周级自适应参数由确定性算法计算；AI 只负责在固定预算内生成具体学习内容。
        let mut adaptive_parameters =
            match crate::core::adaptive_planner::prepare_next_week(
                data_dir,
                week_start,
                &prev_week_start,
                &state,
                &settings,
            ) {
                Ok(parameters) => parameters,
                Err(e) => {
                    log::warn!("周级自适应分析失败，保持用户配置: {}", e);
                    crate::core::adaptive_planner::baseline_parameters(&state, &settings)
                }
            };
        if let Some(adjustment) = workload_adjustment {
            crate::core::adaptive_planner::apply_manual_workload_override(
                &mut adaptive_parameters,
                adjustment,
                &settings,
            );
        }

        // 2. 构建周计划 prompt
        let rest_days = settings.rest_days();
        let subject_start_dates = settings.subject_start_dates();
        let daily_target_hours = settings.daily_target_hours();
        let standard_granularity = settings.standard_granularity();
        let enable_review_tasks = settings.enable_review_tasks();
        let active_count = crate::core::planning::pure::active_subject_count_for(
            &state,
            &week_end,
            &subject_start_dates,
        );
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
            daily_target_hours,
            standard_granularity,
            enable_review_tasks,
            &prev_week_daily_plans,
            &prev_week_reviews,
            excluded_days,
            workload_adjustment,
            &adaptive_parameters,
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
            math_version: state.subjects.math.version.clone(),
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
        log::debug!("[AI-DEBUG] 周计划原始响应全文:\n{}", response.content);

        let mut week_plan = parse_week_plan_json(&response.content, week_start, &week_end)?;

        // 3.1 后置校验：强制周计划的 is_rest_day 与用户设置一致
        enforce_rest_days(&mut week_plan, &rest_days, week_start, &week_end)?;
        // 3.2 后置校验：把排除日也标记为 is_rest_day=true 并清空 allocations
        enforce_excluded_days(&mut week_plan, excluded_days)?;
        // 3.3 后置校验：周中生成时清空已过去日期的任务分配（确定性兜底，避免 AI 补排历史日）
        enforce_past_days_empty(&mut week_plan);

        // 3.4 一致性校验：确定性剔除与各科「已完成」重复的任务（无论 AI 是否遵守约束均兜底）
        let week_all_dates: Vec<String> =
            week_plan.data.days.iter().map(|d| d.date.clone()).collect();
        let empty_declared = std::collections::HashSet::new();
        let orig_snapshot = week_plan.clone();
        consistency_check_and_correct(
            &mut week_plan,
            &state,
            &week_all_dates,
            &orig_snapshot,
            &empty_declared,
        );

        // 3.4b 任务粒度规范化：时长上限拆分 / 下限合并 / 原子性拆分（兜底 AI 不规范输出）
        normalize_task_granularity(&mut week_plan);

        // AI 输出之后再次应用确定性预算，避免模型自由改变自适应算法给出的数字参数。
        apply_adaptive_parameters(&mut week_plan, &adaptive_parameters);

        // 3.3 持久化用户本周配置（排除日 + 任务量调整）
        week_plan.data.excluded_days = excluded_days.to_vec();
        week_plan.data.workload_adjustment = workload_adjustment.cloned();

        // 3.5 持久化任务量调整元数据，供周计划页解释本周任务数变化。
        // coefficient 来自自适应算法的综合任务量系数；完成率仅作为历史参考展示，
        // 不再单独决定下一周计划量。
        let calib_base_count =
            crate::core::planning::pure::derive_task_count(daily_target_hours, 1.0, active_count);
        let calib_effective = adaptive_parameters.daily_task_count;
        let (_, calib_avg_rate) = prev_week_calibration_stats(&prev_week_reviews);
        if calib_effective != calib_base_count
            || adaptive_parameters.capacity_adjustment.abs() >= 0.005
            || adaptive_parameters.workload_factor != 1.0
        {
            week_plan.data.calibration = Some(crate::data::plan::WeekCalibration {
                base_daily_task_count: calib_base_count,
                effective_daily_task_count: calib_effective,
                coefficient: adaptive_parameters.workload_factor,
                avg_completion_rate: calib_avg_rate,
            });
        }

        // 4. 保存周计划 JSON
        crate::data::plan::save_week_plan(data_dir, &week_plan)?;

        // 4.1 保存原始周计划副本（用于一周结束后对比原计划与现计划的任务进度）
        if let Err(e) = save_week_plan_original(data_dir, &week_plan) {
            log::warn!("保存原始周计划副本失败（不阻塞主流程）: {}", e);
        }

        // 5. 从周计划批量生成本周所有学习日的日计划文件
        // 仅对今天同步 State.current_task，其他日期只生成文件
        Self::generate_daily_plans_for_week(data_dir, &week_plan)?;

        Ok(week_plan)
    }

    /// 从周计划批量生成本周所有学习日的日计划文件
    ///
    /// 遍历周计划的每一天，对非休息日生成日计划 JSON 并保存。
    /// 仅对今天同步 State.current_task（避免覆盖已有完成状态），
    /// 其他日期只生成文件，前端按每日开始时间控制显示。
    fn generate_daily_plans_for_week(data_dir: &Path, week_plan: &WeekPlanFile) -> DataResult<()> {
        let today = today_string();
        let mut generated = 0;
        let mut skipped_rest = 0;
        let mut errors: Vec<(String, String)> = Vec::new();

        for day in &week_plan.data.days {
            if day.is_rest_day {
                skipped_rest += 1;
                // 今天及以后的休息日/排除日：清空可能残留的旧日计划，
                // 使随后的滴答按日对账能删除该日期的原有任务；
                // 过去的日期保留历史计划不动
                if day.date.as_str() >= today.as_str() {
                    match crate::data::plan::clear_daily_plan(data_dir, &day.date) {
                        Ok(true) => log::info!("{}: 已清空旧日计划（休息日/排除日）", day.date),
                        Ok(false) => {}
                        Err(e) => log::warn!("清空 {} 旧日计划失败: {}", day.date, e),
                    }
                }
                continue;
            }
            // 周中生成周计划时，跳过早于今天的日期：
            // 过去天已有历史日计划/已完成学习，不应被本次生成覆盖或重新排程
            if day.date.as_str() < today.as_str() {
                log::info!("{} 早于今天，跳过日计划生成（保留历史计划）", day.date);
                continue;
            }
            match DailyScheduler::generate_daily_plan(data_dir, &day.date, day.date == today) {
                Ok(plan) => {
                    if let Err(e) = crate::data::plan::save_daily_plan(data_dir, &plan) {
                        errors.push((day.date.clone(), format!("保存日计划失败: {}", e)));
                    } else {
                        // 对今天额外执行温和同步（sync_current_task 不覆盖已有完成状态）
                        if day.date == today {
                            if let Err(e) = Self::sync_current_task(data_dir, &day.date, &plan) {
                                log::warn!("同步 {} 的 current_task 失败: {}", day.date, e);
                            }
                        }
                        generated += 1;
                    }
                }
                Err(e) => {
                    errors.push((day.date.clone(), e));
                }
            }
        }

        log::info!(
            "批量生成日计划完成: 生成 {} 个, 跳过休息日/排除日 {} 个, 失败 {} 个",
            generated,
            skipped_rest,
            errors.len()
        );
        for (date, err) in &errors {
            log::warn!("生成 {} 日计划失败: {}", date, err);
        }

        Ok(())
    }

    /// 重新生成指定日期的日计划文件（M8：抽取自重排逻辑，消除重复）
    ///
    /// 遍历 `regen_dates`，对每个非休息日/排除日重新生成并保存日计划；
    /// 排除日/休息日改为清空旧日计划文件，使随后的滴答按日对账删除该日期的原有任务。
    /// 仅对今天额外执行温和同步（`sync_current_task`，不覆盖已有完成状态）。
    ///
    /// `week_plan` 用于读取每日 is_rest_day 标记（排除日已被 enforce 标记为休息日）。
    fn regenerate_daily_plans_for_dates(
        data_dir: &Path,
        week_plan: &WeekPlanFile,
        regen_dates: &[String],
        tag: &str,
    ) -> DataResult<()> {
        let today = today_string();
        let today_is_excluded = week_plan.data.excluded_days.iter().any(|d| d.date == today);

        for date in regen_dates {
            // 排除日/休息日不生成日计划，但要清空该日可能残留的旧日计划：
            // 使随后的滴答按日对账能删除该日期在滴答侧的原有任务（"被更改日期的原有计划删掉"）
            let is_rest = week_plan
                .data
                .days
                .iter()
                .find(|d| &d.date == date)
                .map(|d| d.is_rest_day)
                .unwrap_or(false);
            if is_rest {
                match crate::data::plan::clear_daily_plan(data_dir, date) {
                    Ok(true) => log::info!("{}: 已清空旧日计划（排除日/休息日）", date),
                    Ok(false) => {}
                    Err(e) => log::warn!("清空 {} 旧日计划失败: {}", date, e),
                }
                continue;
            }
            match DailyScheduler::generate_daily_plan(data_dir, date, date == &today) {
                Ok(plan) => {
                    if let Err(e) = crate::data::plan::save_daily_plan(data_dir, &plan) {
                        log::warn!("保存 {} 日计划失败: {}", date, e);
                    } else {
                        log::info!("{}: 已重新生成 {} 的日计划", tag, date);
                        // 对今天额外执行温和同步
                        if date == &today && !today_is_excluded {
                            if let Err(e) = Self::sync_current_task(data_dir, &today, &plan) {
                                log::warn!("同步 current_task 失败: {}", e);
                            }
                        }
                    }
                }
                Err(e) => {
                    log::warn!("重新生成 {} 日计划失败: {}", date, e);
                }
            }
        }
        crate::data::write_ai_debug_log(
            data_dir,
            &format!("{}_daily_plans_saved", tag),
            &format!("已重新生成受影响日期的日计划, 日期列表: {:?}", regen_dates),
        );

        Ok(())
    }

    /// 生成数学考纲约束文本（按用户实际卷种动态注入，不再硬编码「数学二」）
    ///
    /// 返回可直接嵌入 prompt 的一句话约束。数二排除伯努利方程/全微分方程；
    /// 数一/数三/其他卷种仅要求遵循对应考纲；未考数学或未指定时返回中性提示。
    fn math_syllabus_constraint(state: &StudyState) -> String {
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

    /// 构建周计划 prompt
    #[allow(clippy::too_many_arguments)]
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
        daily_target_hours: f64,
        standard_granularity: f64,
        enable_review_tasks: bool,
        prev_week_daily_plans: &[DailyPlanFile],
        prev_week_reviews: &[crate::data::records::ReviewFile],
        excluded_days: &[ExcludedDay],
        workload_adjustment: Option<&WorkloadAdjustment>,
        adaptive_parameters: &AdaptivePlanParameters,
    ) -> String {
        let remaining = days_between(&state.meta.exam_date, week_start).unwrap_or(0);
        let iso_week = iso_week_string(week_start).unwrap_or_else(|_| "YYYY-Www".to_string());

        // 任务数量只是固定时间预算下的软提示，不能再作为唯一自校准对象。
        let effective_daily_task_count = adaptive_parameters
            .daily_task_count
            .clamp(1, 8);
        let study_days = 7usize.saturating_sub(rest_days.len()).max(1) as f64;
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
            prompt.push_str(&format!(
                "- 每周学习天数：{} 天\n",
                7usize.saturating_sub(rest_days.len())
            ));
        }
        prompt.push_str(&format!(
            "- 每日任务数量：约 {} 个（由每日有效目标学时 {:.2}h ÷ 标准任务粒度 {:.1}h/条 派生；每科约一条；同时遵循各科开始学习日期，未开始的科目不安排任务，相应减少当日任务数）\n",
            effective_daily_task_count, effective_target_hours, standard_granularity
        ));
        prompt.push_str(&format!(
            "- 是否安排总结/复习任务：{}（{}）\n\n",
            if enable_review_tasks { "允许" } else { "禁止" },
            if enable_review_tasks {
                "可在 task_templates 中安排'回顾'/'总结'/'复习'类任务以巩固知识"
            } else {
                "严禁安排任何形式的复习/巩固类任务，包括但不限于'回顾'/'总结'/'复习'/'梳理'/'练习'/'巩固'/'强化'/'温习'/'复盘'/'巩固练习'等；每日任务必须推进新知识点、新章节或新习题"
            }
        ));

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

        // 每科任务数确定性预算（每科至少 1 条，条数多了才按时长权重分散）
        let per_subject_budget = subject_task_budget(
            state,
            effective_daily_task_count,
            week_end,
            subject_start_dates,
        );
        if !per_subject_budget.is_empty() {
            // 去掉分配数为 0 的科目，避免给 AI 造成"竟有空科"的误导
            let nonzero: Vec<String> = per_subject_budget
                .iter()
                .filter(|(_, n)| *n > 0)
                .map(|(k, n)| format!("{} {} 条", planner_subject_cn(k), n))
                .collect();
            if !nonzero.is_empty() {
                prompt.push_str(&format!(
                    "- 每日任务在各科的分布参考：{}（每科至少 1 条；每日总数不足以覆盖所有科目时，优先高时长科目）\n",
                    nonzero.join("，")
                ));
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
                    let _cn = crate::data::weekday_name(&it.due_date).unwrap_or_default();
                    prompt.push_str(&format!("- {}（{}）:\n", it.due_date, _cn));
                    cur = Some(it.due_date.clone());
                }
                prompt.push_str(&format!(
                    "  - {}：{}\n",
                    planner_subject_cn(&it.subject),
                    it.title
                ));
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
                prompt.push_str(&format!(
                    "- 要求: 相比上一周的任务总量，本周整体任务量应{}约 20%（小幅）或 40%（大幅）。通过调整每日任务数（在每日 {} 个基准上 ±1）或任务难度来实现，不得通过删减必要章节来减量。\n",
                    dir_label, effective_daily_task_count
                ));
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
            prompt.push_str("- 原本应安排在这些日期的任务量必须分摊到本周其他学习日\n\n");
            let type_label = |t: &str| -> &'static str {
                match t {
                    "travel" => "外出旅行",
                    "sick" => "生病",
                    "exam" => "考试",
                    _ => "其他",
                }
            };
            for ex in excluded_days {
                let weekday =
                    crate::data::weekday_name(&ex.date).unwrap_or_else(|_| "未知".to_string());
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
            let study_days = 7usize.saturating_sub(rest_days.len());
            let study_days_after_exclusion = study_days.saturating_sub(excluded_days.len());
            prompt.push_str(&format!(
                "\n本周实际可学习天数 = 7 - {}（休息日）- {}（排除日）= {} 天。请确保这 {} 天的任务量合理覆盖本周目标。\n\n",
                rest_days.len(),
                excluded_days.len(),
                study_days_after_exclusion,
                study_days_after_exclusion
            ));
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
        let math_label = state
            .subjects
            .math
            .version
            .as_deref()
            .filter(|v| !v.is_empty())
            .map(|v| format!("数学（{}）", v))
            .unwrap_or_else(|| "数学".to_string());
        prompt.push_str(&format!(
            "### {}\n- 阶段: {:?}\n- 每周时长: {}h\n- 目标分数: {}\n- 当前重点: {}\n- 薄弱章节: {:?}\n- 已完成: {:?}\n- 教材: {}\n- 状态: {}\n\n",
            math_label,
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
            "### 英语\n- 阶段: {:?}\n- 每周时长: {}h\n- 目标分数: {}\n- 当前重点: {}\n- 薄弱章节: {:?}\n- 已完成: {:?}\n- 教材: {}\n- 状态: {}\n\n",
            state.subjects.english.phase,
            state.subjects.english.weekly_hours,
            state.subjects.english.target_score,
            state.subjects.english.current_focus,
            state.subjects.english.weak_chapters,
            state.subjects.english.completed,
            state.subjects.english.textbook.as_deref().unwrap_or("未指定"),
            if state.subjects.english.active { "活跃" } else { "未启动" }
        ));
        prompt.push_str(&format!(
            "### 专业课（{}）\n- 阶段: {:?}\n- 每周时长: {}h\n- 目标分数: {}\n- 当前重点: {}\n- 薄弱章节: {:?}\n- 已完成: {:?}\n- 教材: {}\n- 状态: {}\n\n",
            state.subjects.professional.name.as_ref().unwrap_or(&"专业课".to_string()),
            state.subjects.professional.phase,
            state.subjects.professional.weekly_hours,
            state.subjects.professional.target_score,
            state.subjects.professional.current_focus,
            state.subjects.professional.weak_chapters,
            state.subjects.professional.completed,
            state.subjects.professional.textbook.as_deref().unwrap_or("未指定"),
            if state.subjects.professional.active { "活跃" } else { "未启动" }
        ));
        if state.subjects.politics.active {
            prompt.push_str(&format!(
                "### 政治\n- 阶段: {:?}\n- 每周时长: {}h\n- 目标分数: {}\n- 当前重点: {}\n- 薄弱章节: {:?}\n- 已完成: {:?}\n- 教材: {}\n- 状态: 活跃\n\n",
                state.subjects.politics.phase,
                state.subjects.politics.weekly_hours,
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
                let weekday =
                    crate::data::weekday_name(date).unwrap_or_else(|_| "未知".to_string());
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
                        by_subject
                            .entry(subj)
                            .or_default()
                            .push(task.title.as_str());
                    }
                    for (subj, titles) in &by_subject {
                        prompt.push_str(&format!("  - {}: {}\n", subj, titles.join("、")));
                    }
                }
                // 对应复盘
                if let Some(review) = review_by_date.get(date.as_str()) {
                    let (a_total, a_done, b_total, b_done, rate) =
                        crate::data::records::review_completion_stats(review);
                    prompt.push_str(&format!(
                        "- 复盘: 完成率 {:.0}%, 实际时长 {:.1}h, 精力 {}, A类 {}/{}, B类 {}/{}\n",
                        rate,
                        crate::data::records::review_actual_hours(review),
                        review.data.energy_level,
                        a_done,
                        a_total,
                        b_done,
                        b_total,
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
                    "这些任务**不得跳过**，必须在本周计划中重新安排，优先放在周一至周三：\n\n",
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
1. 必须包含 data 和 view 两个字段；view 仅用于展示，不会被程序解析。
2. 只给 active=true 的科目安排任务；政治未启动则安排为 rest_day 或空 allocations。
3. {math}
4. 任务已不再分级（Priority A/B 已废弃）：task_templates 中不要输出 priority 字段，任务不做 A/B 区分，所有任务同等对待。
5. subject 字段只能是 "math" / "english" / "politics" / "professional" 之一，严禁使用 "general" 或其他值。出现在 data.subjects[].subject、data.days[].subject_allocations[].subject 中的所有取值都必须严格属于这四个枚举值之一。
6. 休息日 is_rest_day=true，且 subject_allocations 为空数组。
7. 任务 estimated_hours 总和应大致等于当天预期学习时长。
8. 必须严格遵守「学习日程」节中声明的休息日配置。weekday 字段（如"周日"）与用户休息日列表匹配的，is_rest_day 必须为 true，且不分配任何任务；weekday 不在休息日列表的，必须有 subject_allocations。
9. 必须严格遵守「各科开始学习日期」节中的约束：若某科目开始日期晚于本周日（{}），该科目不得出现在 subjects、subject_allocations 中，本周完全不为其安排任务。
10. 参考「上一周任务参考」节调整本周任务量，避免任务量与上周实际完成情况严重偏离。
11. 每天的 task_templates 数量应大致等于「用户期望每日任务数量」（{} 个），每科约一条；未开始的科目不安排，相应减少当日任务数，不得为了凑数而强行安排。
12. {}若用户禁止总结任务，task_templates 的标题和 goal 不得出现"回顾"/"总结"/"复习"/"梳理"/"练习"/"巩固"/"强化"/"温习"/"复盘"等字样，每个任务必须推进新的知识点、章节或新习题（新习题指未做过的题目，不含已做题目的重做）；若用户允许总结任务，可酌情安排 1 个总结/复习类任务以巩固知识。
13. 若存在「上周未完成任务」节，必须在本周计划中重新安排这些任务（不得跳过），并优先放在周一至周三。未完成任务的状态由复盘时的勾决定定，不再自动标记为「已放弃」，因此「未完成」和「部分完成」的任务都需要在本周重新排程。
14. **不得重复已完成内容**：各科「已完成」列表中的章节/任务严禁再次出现在本周计划中，必须从已完成之后的下一个章节/知识点继续推进。同时以各科「当前重点」作为实际进度基准：不得在用户尚未到达的章节安排任务，计划的推进顺序必须以教材章节先后为准，不得跳过用户尚未学习的章节跳级到后面（若「当前重点」显示的进度落后于本周计划，以「当前重点」为准相应调整，而非沿用旧计划）。
15. **按天推进切分（受排除日影响）**：本周计划必须将每个科目的学习内容切分到每一天，每天推进不同的章节/知识点/习题，逐日向前递进。同一科目相邻两天的 focus 不得完全相同（休息日/排除日除外），避免一天内塞满整周内容或每天重复同一内容。**{}应作为本周的起始点**，从各科「已完成」之后的章节开始，逐天分配到剩余学习日（若为周中生成，起点为今天而非周一，已过去的日期不安排任务）。**注意排除日不分配任务**，排除日应占用的任务量必须分摊到本周其他学习日，因此实际可学习天数 = 7 - 休息日 - 排除日，每天的 task_templates 数量限制（约束11）仍须遵守。
"#,
            week_start, week_end, week_end, effective_daily_task_count,
            if enable_review_tasks { "" } else { "严禁安排总结/复习类任务。" },
            if today == week_start { "周一" } else { "今天" },
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

    /// 读取考试配置
    fn read_exam_config(data_dir: &Path) -> String {
        let path = data_dir
            .join("assets")
            .join("config")
            .join("exam-config.md");
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

    /// 构建复盘后重排剩余天数的 AI prompt
    ///
    /// 输入：复盘数据、当前 State、需要重排的日期范围及原安排
    /// 输出：AI 重新生成这些日期的 subject_allocations
    #[allow(clippy::too_many_arguments)]
    fn build_regenerate_prompt(
        &self,
        state: &StudyState,
        review: &crate::data::records::ReviewFile,
        regen_days: &[&crate::data::plan::WeekDayPlan],
        review_date: &str,
        regen_start: &str,
        week_end: &str,
        week_start: &str,
        rest_days: &[String],
        subject_start_dates: &[(&'static str, String)],
        daily_target_hours: f64,
        standard_granularity: f64,
        enable_review_tasks: bool,
        excluded_days: &[ExcludedDay],
    ) -> String {
        let remaining = days_between(&state.meta.exam_date, regen_start).unwrap_or(0);
        let iso_week = iso_week_string(week_start).unwrap_or_else(|_| "YYYY-Www".to_string());
        // 任务条数由「每日目标学时 ÷ 标准粒度」确定性派生（重排沿用基准学时，不叠加自校准）
        let effective_daily_task_count = crate::core::planning::pure::derive_task_count(
            daily_target_hours,
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
            for tr in &review.task_reviews {
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
                let title = if tr.title.is_empty() {
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
                    subj_label, tr.title, status_label
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
            "- 每日任务数量: {} 个（由每日目标学时 {:.2}h ÷ 标准任务粒度 {:.1}h/条 派生；每科约一条；未开始的科目不安排）\n",
            effective_daily_task_count, daily_target_hours, standard_granularity
        ));
        prompt.push_str(&format!(
            "- 总结/复习任务: {}\n",
            if enable_review_tasks {
                "允许安排"
            } else {
                "禁止安排（严禁任何形式的复习/巩固类任务，包括「回顾」「总结」「复习」「梳理」「练习」「巩固」「强化」「温习」「复盘」等）"
            }
        ));

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
                let weekday =
                    crate::data::weekday_name(&ex.date).unwrap_or_else(|_| "未知".to_string());
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
            let weekday =
                crate::data::weekday_name(&day.date).unwrap_or_else(|_| "未知".to_string());
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
                    let subj_label = match alloc.subject {
                        crate::data::state::SubjectKey::Math => "数学",
                        crate::data::state::SubjectKey::English => "英语",
                        crate::data::state::SubjectKey::Politics => "政治",
                        crate::data::state::SubjectKey::Professional => "专业课",
                    };
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
8. {}若用户禁止总结任务，task_templates 的标题和 goal 不得出现"回顾"/"总结"/"复习"/"梳理"/"练习"/"巩固"/"强化"/"温习"/"复盘"等字样，每个任务必须推进新知识点、新章节或新习题。
9. 休息日的 subject_allocations 为空数组。
10. **不得重复已完成内容**：各科「已完成」列表中的章节/任务严禁再次出现，必须从已完成之后的下一个章节/知识点继续推进。
11. **按天推进切分（受排除日影响）**：每个科目的学习内容必须切分到剩余的每个学习日，每天推进不同的章节/知识点/习题，逐日向前递进。同一科目相邻两天的 focus 不得完全相同（休息日/排除日除外）。若存在排除日，排除日不分配任务，其任务量分摊到其他学习日，每天的 task_templates 数量限制（约束7）仍须遵守。
"#,
            regen_start, week_end, effective_daily_task_count,
            if enable_review_tasks { "" } else { "严禁安排总结/复习类任务。" },
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

    /// 构建排除日重排 prompt（周中新增排除日时使用，不依赖复盘）
    #[allow(clippy::too_many_arguments)]
    fn build_exclusion_regenerate_prompt(
        &self,
        state: &StudyState,
        excluded_day: &ExcludedDay,
        regen_days: &[&crate::data::plan::WeekDayPlan],
        excluded_original_allocations: &[crate::data::plan::DaySubjectAllocation],
        regen_start: &str,
        week_end: &str,
        week_start: &str,
        rest_days: &[String],
        subject_start_dates: &[(&'static str, String)],
        daily_target_hours: f64,
        standard_granularity: f64,
        enable_review_tasks: bool,
        all_excluded_days: &[ExcludedDay],
    ) -> String {
        let remaining = days_between(&state.meta.exam_date, regen_start).unwrap_or(0);
        let iso_week = iso_week_string(week_start).unwrap_or_else(|_| "YYYY-Www".to_string());
        // 任务条数由「每日目标学时 ÷ 标准粒度」确定性派生（重排沿用基准学时，不叠加自校准）
        let effective_daily_task_count = crate::core::planning::pure::derive_task_count(
            daily_target_hours,
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
                let subj_label = match alloc.subject {
                    crate::data::state::SubjectKey::Math => "数学",
                    crate::data::state::SubjectKey::English => "英语",
                    crate::data::state::SubjectKey::Politics => "政治",
                    crate::data::state::SubjectKey::Professional => "专业课",
                };
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
            "- 每日任务数量: {} 个（由每日目标学时 {:.2}h ÷ 标准任务粒度 {:.1}h/条 派生；每科约一条；未开始的科目不安排）\n",
            effective_daily_task_count, daily_target_hours, standard_granularity
        ));
        prompt.push_str(&format!(
            "- 总结/复习任务: {}\n",
            if enable_review_tasks {
                "允许安排"
            } else {
                "禁止安排（严禁任何形式的复习/巩固类任务，包括「回顾」「总结」「复习」「梳理」「练习」「巩固」「强化」「温习」「复盘」等）"
            }
        ));

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
                let weekday =
                    crate::data::weekday_name(&ex.date).unwrap_or_else(|_| "未知".to_string());
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
            let weekday =
                crate::data::weekday_name(&day.date).unwrap_or_else(|_| "未知".to_string());
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
                    let subj_label = match alloc.subject {
                        crate::data::state::SubjectKey::Math => "数学",
                        crate::data::state::SubjectKey::English => "英语",
                        crate::data::state::SubjectKey::Politics => "政治",
                        crate::data::state::SubjectKey::Professional => "专业课",
                    };
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
8. {}若用户禁止总结任务，task_templates 的标题和 goal 不得出现"回顾"/"总结"/"复习"/"梳理"/"练习"/"巩固"/"强化"/"温习"/"复盘"等字样，每个任务必须推进新知识点、新章节或新习题。
9. 休息日的 subject_allocations 为空数组。
10. **不得重复已完成内容**：各科「已完成」列表中的章节/任务严禁再次出现，必须从已完成之后的下一个章节/知识点继续推进。
11. **按天推进切分（受排除日影响）**：每个科目的学习内容必须切分到剩余的每个学习日，每天推进不同的章节/知识点/习题，逐日向前递进。同一科目相邻两天的 focus 不得完全相同（休息日/排除日除外）。实际可学习天数 = 7 - 休息日 - 排除日，排除日不分配任务，其任务量分摊到其他学习日，每天的 task_templates 数量限制（约束6）仍须遵守。
"#,
            regen_start, week_end,
            all_excluded_days.iter().map(|d| d.date.as_str()).collect::<Vec<_>>().join("、"),
            effective_daily_task_count,
            if enable_review_tasks { "" } else { "严禁安排总结/复习类任务。" },
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

/// 从 AI 响应中提取并解析周计划 JSON
fn parse_week_plan_json(
    content: &str,
    expected_week_start: &str,
    expected_week_end: &str,
) -> DataResult<WeekPlanFile> {
    crate::core::planning::parsing::parse_week_plan_json(
        content,
        expected_week_start,
        expected_week_end,
    )
}

/// 每周自校准统计：返回 (系数, 上周复盘平均完成率%)。
/// 实现见 `pure::prev_week_calibration_stats_impl`（按任务数加权的平均完成率）。
fn prev_week_calibration_stats(
    prev_week_reviews: &[crate::data::records::ReviewFile],
) -> (f64, f64) {
    crate::core::planning::pure::prev_week_calibration_stats_impl(prev_week_reviews)
}

/// 今日/近期强度预测（E）：基于最近的复盘完成率与精力值，得出今日强度建议。
///
/// 规则（确定性）：
/// - 无复盘 → 返回空串；
/// - 平均完成率 < 60% 或精力均值 ≤ 1.5 → 偏轻（优先完成而非加量）；
/// - 完成率 ≥ 90% 且精力均值 ≥ 4 → 可加量；
/// - 完成率 < 75% → 适中；否则 → 正常。
///   完成率取有效复盘 completion rate（0-100）均值，精力取 `data.energy_level` 均值。
pub fn today_intensity_label(reviews: &[crate::data::records::ReviewFile]) -> String {
    crate::core::planning::pure::today_intensity_label(reviews)
}
fn planner_subject_key_str(subject: &crate::data::state::SubjectKey) -> &'static str {
    crate::core::planning::pure::subject_key_str(subject)
}
fn planner_subject_cn(subject: &crate::data::state::SubjectKey) -> &'static str {
    crate::core::planning::pure::subject_cn(subject)
}
fn planner_subject_cn_from_str(subject: &str) -> &'static str {
    match subject {
        "math" => "数学",
        "english" => "英语",
        "politics" => "政治",
        "professional" => "专业课",
        _ => "其他",
    }
}

/// 将确定性算法给出的学科预算和估时系数应用到 AI 的内容结果。
///
/// AI 仍然负责选择具体章节/任务，但不能通过返回更大的 estimated_hours
/// 绕过 AdaptivePlanner 的周预算。系数只应用一次，最后只做全周归一化，
/// 从而保留不同学科之间的估时差异，不把 estimation_factor 在学科内抵消掉。
fn apply_adaptive_parameters(
    plan: &mut WeekPlanFile,
    parameters: &AdaptivePlanParameters,
) {
    use std::collections::HashMap;

    let mut current_total = 0.0f64;

    for day in &mut plan.data.days {
        if day.is_rest_day {
            continue;
        }
        for allocation in &mut day.subject_allocations {
            let subject = planner_subject_key_str(&allocation.subject).to_string();
            let estimation_factor = parameters
                .estimation_factors
                .get(&subject)
                .copied()
                .unwrap_or(1.0)
                .clamp(0.8, 1.25);
            for template in &mut allocation.task_templates {
                if template.estimated_hours > 0.0 {
                    template.estimated_hours *= estimation_factor;
                }
            }
            let total: f64 = allocation
                .task_templates
                .iter()
                .map(|task| task.estimated_hours.max(0.0))
                .sum();
            current_total += total;
        }
    }

    // 只做全周归一化，确保 sum(subject hours) 不膨胀，同时保留学科估时系数
    // 造成的相对时长差异。AI 已在 prompt 中拿到各科预算，异常偏离时由这一
    // 步骤将全周总量拉回确定性预算。
    let global_scale = if current_total > 0.0 && parameters.nominal_total_hours > 0.0 {
        // AI 低于预算时保持保守，不为了填满预算而人为拉长任务；
        // 只有超预算时才做确定性缩减。
        (parameters.nominal_total_hours / current_total).min(1.0)
    } else {
        1.0
    };
    let mut final_subject_totals: HashMap<String, f64> = HashMap::new();
    for day in &mut plan.data.days {
        if day.is_rest_day {
            continue;
        }
        for allocation in &mut day.subject_allocations {
            let subject = planner_subject_key_str(&allocation.subject).to_string();
            for template in &mut allocation.task_templates {
                template.estimated_hours = (template.estimated_hours * global_scale).max(0.0);
            }
            allocation.hours = allocation
                .task_templates
                .iter()
                .map(|task| task.estimated_hours.max(0.0))
                .sum();
            *final_subject_totals.entry(subject).or_insert(0.0) += allocation.hours;
        }
    }

    // 对明显超出学科预算的科目只做向下投影；预算未填满时不人为拉长，
    // 贯彻“宁可保守，不制造新的计划压力”。
    let subject_scales: HashMap<String, f64> = final_subject_totals
        .iter()
        .filter_map(|(subject, current)| {
            let target = parameters.subject_hours.get(subject).copied()?;
            if *current > target * 1.05 && *current > 0.0 {
                Some((subject.clone(), target / current))
            } else {
                Some((subject.clone(), 1.0))
            }
        })
        .collect();
    if subject_scales.values().any(|scale| *scale < 0.999_999) {
        final_subject_totals.clear();
        for day in &mut plan.data.days {
            if day.is_rest_day {
                continue;
            }
            for allocation in &mut day.subject_allocations {
                let subject = planner_subject_key_str(&allocation.subject).to_string();
                let scale = subject_scales.get(&subject).copied().unwrap_or(1.0);
                for template in &mut allocation.task_templates {
                    template.estimated_hours = (template.estimated_hours * scale).max(0.0);
                }
                allocation.hours = allocation
                    .task_templates
                    .iter()
                    .map(|task| task.estimated_hours.max(0.0))
                    .sum();
                *final_subject_totals.entry(subject).or_insert(0.0) += allocation.hours;
            }
        }
    }

    for subject_plan in &mut plan.data.subjects {
        let subject = planner_subject_key_str(&subject_plan.subject);
        if let Some(final_total) = final_subject_totals.get(subject) {
            subject_plan.weekly_hours = *final_total;
        } else if let Some(target) = parameters.subject_hours.get(subject) {
            subject_plan.weekly_hours = *target;
        }
    }
}

/// 每科每日任务数确定性分配（"每科至少一条，条数多了才按时长权重分散"）。
///
/// 规则：
/// - 仅计入本周已开课科目（active 且开始日期 ≤ week_end，或无开始日期约束）；
/// - 若每日任务总数 >= 科目数：先给每科保底 1 条，多余条数按 weekly_hours 权重分摊；
/// - 若总数 < 科目数：只按权重分配总数（高时长科目优先占名额）。
fn subject_task_budget(
    state: &StudyState,
    total: i64,
    week_end: &str,
    subject_start_dates: &[(&'static str, String)],
) -> Vec<(crate::data::state::SubjectKey, i64)> {
    crate::core::planning::pure::subject_task_budget(state, total, week_end, subject_start_dates)
}
type MemoryReviewItem = crate::core::planning::pure::MemoryReviewItem;
fn memory_curve_review_items(
    reviews: &[crate::data::records::ReviewFile],
    week_start: &str,
    week_end: &str,
) -> Vec<MemoryReviewItem> {
    crate::core::planning::pure::memory_curve_review_items(reviews, week_start, week_end)
}
fn enforce_rest_days(
    plan: &mut WeekPlanFile,
    rest_days: &[String],
    week_start: &str,
    week_end: &str,
) -> DataResult<()> {
    crate::core::planning::constraints::enforce_rest_days(plan, rest_days, week_start, week_end)
}

/// 后置校验：把排除日标记为 is_rest_day=true 并清空 subject_allocations
///
/// 与 enforce_rest_days 不同，排除日是用户主动声明的"本周临时跳过"日期，
/// 语义上区别于每周固定休息日。但为了复用 scheduler 的"休息日不生成日计划"逻辑，
/// 排除日也设置 is_rest_day=true。前端通过 week_plan.data.excluded_days 区分展示。
fn enforce_excluded_days(plan: &mut WeekPlanFile, excluded_days: &[ExcludedDay]) -> DataResult<()> {
    crate::core::planning::constraints::enforce_excluded_days(plan, excluded_days)
}

/// 后置校验：清空「早于今天」的日期的任务分配（周中生成周计划的确定性兜底）
///
/// 周计划正常在周一生成，覆盖整周 7 天。但当用户**周中**才首次生成周计划时，
/// 之前已过去的几天（如周一、周二）本应已有计划或已完成学习，AI 若按整周
/// 生成会把这些过去的日期也补上任务，造成与历史进度/已完成的日计划冲突。
///
/// 规则：凡 `date < today` 的日期，`subject_allocations` 一律清空（不安排任务）。
/// 这是确定性硬保证，不依赖 AI 是否遵循 prompt 中的「从今天开始」约束。
/// 周初（today = 周一）生成时，没有日期早于今天，此函数为空操作。
fn enforce_past_days_empty(plan: &mut WeekPlanFile) {
    crate::core::planning::constraints::enforce_past_days_empty(plan)
}

// ============================================================================
// 复盘后重排相关：辅助函数
// ============================================================================

/// 判断复盘是否需要触发剩余天数重排
///
/// 触发条件（任一命中）：
/// - 存在未完成任务（status 为 incomplete 或 partial）
/// - 有任务掌握不足（mastery == "weak"，需要巩固）
/// - 用户感受困难（daily_review.overall_feeling == "hard"）
/// - 有额外进度记录（overcompletion 非空）
pub fn check_review_needs_regeneration(review: &crate::data::records::ReviewFile) -> bool {
    crate::core::planning::pure::check_review_needs_regeneration(review)
}

/// 汇总重排前后各受影响日期的任务变动明细（标题级 diff，供前端悬停展示）
///
/// - `added`：新计划有而旧计划没有的任务标题
/// - `removed`：旧计划有而新计划没有的任务标题
/// - `adjusted`：标题发生变化的（原标题 → 新标题）
fn compute_regen_changes(
    before: &WeekPlanFile,
    after: &WeekPlanFile,
    dates: &[String],
) -> Vec<RegenDayChange> {
    fn day_titles(plan: &WeekPlanFile, date: &str) -> Vec<String> {
        plan.data
            .days
            .iter()
            .find(|d| d.date == date)
            .map(|d| {
                d.subject_allocations
                    .iter()
                    .flat_map(|a| a.task_templates.iter().map(|t| t.title.clone()))
                    .collect()
            })
            .unwrap_or_default()
    }

    dates
        .iter()
        .map(|date| {
            let before_titles = day_titles(before, date);
            let after_titles = day_titles(after, date);

            // adjusted：同一顺序/同名位置的标题变化简化为「原标题 → 新标题」配对（按位置对齐，尽力而为）
            let mut adjusted: Vec<(String, String)> = Vec::new();
            for (b, a) in before_titles.iter().zip(after_titles.iter()) {
                if b != a {
                    adjusted.push((b.clone(), a.clone()));
                }
            }

            let before_set: std::collections::HashSet<&String> = before_titles.iter().collect();
            let after_set: std::collections::HashSet<&String> = after_titles.iter().collect();
            let added: Vec<String> = after_titles
                .iter()
                .filter(|t| !before_set.contains(t))
                .cloned()
                .collect();
            let removed: Vec<String> = before_titles
                .iter()
                .filter(|t| !after_set.contains(t))
                .cloned()
                .collect();

            RegenDayChange {
                date: date.clone(),
                added,
                removed,
                adjusted,
            }
        })
        .collect()
}
fn save_week_plan_original(data_dir: &Path, plan: &WeekPlanFile) -> DataResult<()> {
    let iso_week = iso_week_string(&plan.meta.week_start)?;
    let path = crate::data::plan::week_plan_path(data_dir, &format!("{}_original", iso_week));

    // 已存在则不覆盖（保留最初版本）
    if path.exists() {
        log::info!("原始周计划副本已存在，不覆盖: {:?}", path);
        return Ok(());
    }

    if let Some(parent) = path.parent() {
        if !parent.exists() {
            std::fs::create_dir_all(parent).map_err(|e| format!("创建 plan 目录失败: {}", e))?;
        }
    }
    let json =
        serde_json::to_string_pretty(plan).map_err(|e| format!("序列化原始周计划失败: {}", e))?;
    crate::data::atomic_write(&path, &json)
        .map_err(|e| format!("写入原始周计划文件失败 {:?}: {}", path, e))?;
    log::info!("原始周计划副本已保存: {:?}", path);
    Ok(())
}

/// 该科目「已完成」章节列表
fn subject_completed_list<'a>(
    state: &'a StudyState,
    subject: &crate::data::state::SubjectKey,
) -> &'a [String] {
    match subject {
        crate::data::state::SubjectKey::Math => &state.subjects.math.completed,
        crate::data::state::SubjectKey::English => &state.subjects.english.completed,
        crate::data::state::SubjectKey::Politics => &state.subjects.politics.completed,
        crate::data::state::SubjectKey::Professional => &state.subjects.professional.completed,
    }
}

/// 判断任务标题是否命中已完成章节（边界匹配，与 scheduler 一致，避免误杀子主题）
fn matches_completed(title: &str, completed: &str) -> bool {
    crate::core::planning::pure::matches_completed(title, completed)
}
fn consistency_check_and_correct(
    week_plan: &mut WeekPlanFile,
    state: &StudyState,
    regen_dates: &[String],
    original_before: &WeekPlanFile,
    declared_subjects: &std::collections::HashSet<String>,
) -> Vec<String> {
    let mut warnings: Vec<String> = Vec::new();

    // 1. 确定性去重：剔除与「已完成」重复的 forward 任务
    let mut removed = false;
    for day in week_plan.data.days.iter_mut() {
        if !regen_dates.contains(&day.date) {
            continue;
        }
        for alloc in day.subject_allocations.iter_mut() {
            let completed = subject_completed_list(state, &alloc.subject);
            if completed.is_empty() {
                continue;
            }
            let before = alloc.task_templates.len();
            alloc
                .task_templates
                .retain(|t| !completed.iter().any(|c| matches_completed(&t.title, c)));
            if alloc.task_templates.len() != before {
                removed = true;
            }
        }
    }
    if removed {
        log::info!("一致性校验：已剔除与「已完成」重复的计划任务");
        // 同步修正 allocation 时长（仅当有剩余模板可求和时）
        for day in week_plan.data.days.iter_mut() {
            for alloc in day.subject_allocations.iter_mut() {
                let sum: f64 = alloc.task_templates.iter().map(|t| t.estimated_hours).sum();
                if sum > 0.0 {
                    alloc.hours = sum;
                }
            }
        }
    }

    // 2. 进度生效检查（delta）：声明了实际进度的科目，若剩余安排与重排前完全一致 → 警告
    let mut warned: std::collections::HashSet<String> = std::collections::HashSet::new();
    for day in week_plan.data.days.iter() {
        if !regen_dates.contains(&day.date) {
            continue;
        }
        let orig_day = original_before
            .data
            .days
            .iter()
            .find(|d| d.date == day.date);
        for alloc in &day.subject_allocations {
            let key = planner_subject_key_str(&alloc.subject);
            if !declared_subjects.contains(key) || warned.contains(key) {
                continue;
            }
            let unchanged = match orig_day.and_then(|od| {
                od.subject_allocations
                    .iter()
                    .find(|a| a.subject == alloc.subject)
            }) {
                // 仅当原日确有该科分配时才比对；否则视为「未改动的新分配」不报警
                Some(oa) => serde_json::to_value(oa).ok() == serde_json::to_value(alloc).ok(),
                None => false,
            };
            if unchanged {
                warnings.push(format!(
                    "{} 的计划外进度（当前重点）未反映到后续计划：重排后该科目剩余安排与重排前一致，请到周计划中手动调整。",
                    planner_subject_cn(&alloc.subject)
                ));
                warned.insert(key.to_string());
            }
        }
    }

    warnings
}

/// 学习活动词库：用于原子性拆分判定（一条任务 = 单一主旨的同动作单元）。
///
/// 标题含并列连词（+/＋/、/与/及）且命中 ≥2 个不同活动词时，
/// 判定为"多个独立学习单元"（如"背 250 词 + 阅读 1 篇 + 长难句 3 个"），
/// 拆为独立任务——不同学习动作即使总时长合理也必须拆。
const ACTIVITY_WORDS: [&str; 24] = [
    "单词",
    "词汇",
    "背诵",
    "默写",
    "听写",
    "阅读",
    "精读",
    "泛读",
    "真题",
    "听力",
    "长难句",
    "翻译",
    "作文",
    "完形",
    "新题型",
    "习题",
    "刷题",
    "做题",
    "错题",
    "例题",
    "讲义",
    "笔记",
    "教材",
    "章节",
];

/// 任务粒度规范化：AI 输出的兜底确定性后处理。
///
/// 1. 时长判据：单条 > TASK_MAX_HOURS 拆分为多条（每条 ≈ 原/ceil(原/1.5)）；
///    单条 < TASK_MIN_HOURS 时合并到同科相邻任务（仅同 allocation 内）。
/// 2. 原子性判据：标题含并列连词且命中多个活动词时拆为独立任务。
/// 3. 拆分/合并后重算 allocation.hours（保持周计划层时长不失真）。
fn normalize_task_granularity(week_plan: &mut WeekPlanFile) {
    for day in week_plan.data.days.iter_mut() {
        normalize_allocations(&mut day.subject_allocations);
    }
}

/// 对单个 allocation 列表做任务粒度规范化。
fn normalize_allocations(allocations: &mut [crate::data::plan::DaySubjectAllocation]) {
    for alloc in allocations.iter_mut() {
        let mut expanded: Vec<crate::data::plan::TaskTemplate> = Vec::new();
        for t in alloc.task_templates.drain(..) {
            expanded.extend(split_task_by_atomicity(&t.title, t.estimated_hours));
        }

        // 时长上限拆分（> TASK_MAX_HOURS → 按 1.5h 粒度切分）
        let mut after_cap: Vec<crate::data::plan::TaskTemplate> = Vec::new();
        for t in expanded {
            if t.estimated_hours > crate::core::planning::pure::TASK_MAX_HOURS {
                let n = (t.estimated_hours / 1.5).ceil() as usize;
                let each = t.estimated_hours / n as f64;
                for i in 0..n {
                    let mut sub = t.clone();
                    sub.title = format!("{}({})", t.title, i + 1);
                    sub.estimated_hours = if i == n - 1 {
                        t.estimated_hours - each * (n as f64 - 1.0)
                    } else {
                        each
                    };
                    after_cap.push(sub);
                }
            } else {
                after_cap.push(t);
            }
        }

        // 时长下限合并（< TASK_MIN_HOURS 且存在前一条时并入，标题用 + 拼接）
        let mut merged: Vec<crate::data::plan::TaskTemplate> = Vec::new();
        for t in after_cap {
            if t.estimated_hours < crate::core::planning::pure::TASK_MIN_HOURS {
                if let Some(last) = merged.last_mut() {
                    last.title = format!("{} + {}", t.title, last.title);
                    last.estimated_hours += t.estimated_hours;
                    continue;
                }
            }
            merged.push(t);
        }
        alloc.task_templates = merged;
        let sum: f64 = alloc.task_templates.iter().map(|t| t.estimated_hours).sum();
        if sum > 0.0 {
            alloc.hours = sum;
        }
    }
}

/// 原子性拆分：标题含并列连词且命中 ≥2 个不同学习活动词时，按连接符拆为多条。
fn split_task_by_atomicity(title: &str, hours: f64) -> Vec<crate::data::plan::TaskTemplate> {
    let single = || {
        vec![crate::data::plan::TaskTemplate {
            title: title.to_string(),
            estimated_hours: hours,
            ..Default::default()
        }]
    };
    if !title.contains(['+', '＋', '、', '与', '及']) {
        return single();
    }
    let matched: Vec<&str> = ACTIVITY_WORDS
        .iter()
        .filter(|w| title.contains(**w))
        .copied()
        .collect();
    let mut unique: Vec<&str> = Vec::new();
    for w in matched {
        if !unique.contains(&w) {
            unique.push(w);
        }
    }
    // 命中不同活动词 < 2 时不拆（如"行列式定义与性质"只有一个活动，是同一单元）
    if unique.len() < 2 {
        return single();
    }
    let parts: Vec<&str> = title
        .split(['+', '＋', '、', '与', '及'])
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .collect();
    if parts.len() < 2 {
        return single();
    }
    let each = hours / parts.len() as f64;
    parts
        .into_iter()
        .map(|p| crate::data::plan::TaskTemplate {
            title: p.to_string(),
            estimated_hours: each,
            ..Default::default()
        })
        .collect()
}

/// 找出声明了实际进度、但重排后剩余安排与重排前完全一致的科目（进度未生效）。
///
/// 返回科目 key 列表（math/english/politics/professional）。
/// 用于驱动「进度未生效 -> 联网/教纲自查 + 按实际进度强制校正重排」。
fn find_declared_subjects_unchanged(
    week_plan: &WeekPlanFile,
    original_before: &WeekPlanFile,
    regen_dates: &[String],
    declared_subjects: &std::collections::HashSet<String>,
) -> Vec<String> {
    let mut unchanged: Vec<String> = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    for day in week_plan.data.days.iter() {
        if !regen_dates.contains(&day.date) {
            continue;
        }
        let orig_day = original_before
            .data
            .days
            .iter()
            .find(|d| d.date == day.date);
        for alloc in &day.subject_allocations {
            let key = planner_subject_key_str(&alloc.subject);
            if !declared_subjects.contains(key) || seen.contains(key) {
                continue;
            }
            let is_unchanged = match orig_day.and_then(|od| {
                od.subject_allocations
                    .iter()
                    .find(|a| a.subject == alloc.subject)
            }) {
                // 仅当原日确有该科分配时才比对；否则视为「未改动的新分配」不算 no-op
                Some(oa) => serde_json::to_value(oa).ok() == serde_json::to_value(alloc).ok(),
                None => false,
            };
            if is_unchanged {
                unchanged.push(key.to_string());
                seen.insert(key.to_string());
            }
        }
    }
    unchanged
}

/// 取科目对应的版本标签（用于章节顺序表定位；政治无版本，空串即可）。
fn subject_version(state: &StudyState, key: &str) -> String {
    match key {
        "math" => state.subjects.math.version.clone().unwrap_or_default(),
        "english" => state.subjects.english.version.clone().unwrap_or_default(),
        _ => String::new(),
    }
}

/// 确定性超前剔除：对声明了实际进度的科目，用内置章节顺序表定位，
/// 把剩余计划中排在实际进度**之前**（用户已学过）的任务模板剔除，并重算对应 allocation 时长。
///
/// 已学内容绝不允许再次安排，这是不依赖 AI、确定能落地的进度锚定兜底。
/// 返回是否剔除了内容。
fn filter_ahead_of_progress(
    week_plan: &mut WeekPlanFile,
    state: &StudyState,
    regen_dates: &[String],
    overcompletion: &[crate::data::records::OvercompletionEntry],
) -> bool {
    let mut changed = false;
    for oc in overcompletion {
        let version = subject_version(state, &oc.subject);
        let Some(prog_pos) =
            crate::core::chapter_seq::position(&oc.subject, &version, &oc.chapter_reached)
        else {
            continue;
        };
        for day in week_plan.data.days.iter_mut() {
            if !regen_dates.contains(&day.date) {
                continue;
            }
            for alloc in day.subject_allocations.iter_mut() {
                if planner_subject_key_str(&alloc.subject) != oc.subject {
                    continue;
                }
                let before = alloc.task_templates.len();
                if before == 0 {
                    continue;
                }
                alloc.task_templates.retain(|t| {
                    match crate::core::chapter_seq::position(&oc.subject, &version, &t.title) {
                        // 能定位且位置不晚于实际进度 → 已学过，剔除；定位不到 → 保留
                        Some(p) => p > prog_pos,
                        None => true,
                    }
                });
                if alloc.task_templates.len() != before {
                    changed = true;
                    // M4：剔除后按剩余模板重算时长（全部被剔除时为 0，避免「0 模板残留时长」）
                    alloc.hours = alloc.task_templates.iter().map(|t| t.estimated_hours).sum();
                }
            }
        }
    }
    changed
}

/// 更新周计划中剩余天数的 subject_allocations
///
/// 用 AI 返回的新安排覆盖对应日期的 subject_allocations。
/// 休息日的 is_rest_day 保持不变，仅更新非休息日的 allocations。
fn update_week_plan_remaining_days(week_plan: &mut WeekPlanFile, updated_days: &[RegenDayPlan]) {
    for updated in updated_days {
        if let Some(day) = week_plan
            .data
            .days
            .iter_mut()
            .find(|d| d.date == updated.date)
        {
            if day.is_rest_day {
                log::info!(
                    "更新周计划: {} 是休息日，跳过 subject_allocations 更新",
                    updated.date
                );
                continue;
            }
            day.subject_allocations = updated.subject_allocations.clone();
            // 任务粒度规范化：兜底 AI 重排输出的不规范粒度
            normalize_allocations(&mut day.subject_allocations);
            log::info!(
                "更新周计划: {} 的 subject_allocations 已更新（{} 个科目分配）",
                updated.date,
                updated.subject_allocations.len()
            );
        } else {
            log::warn!("更新周计划: 未找到日期 {}，跳过", updated.date);
        }
    }
}

// ============================================================================
// AI 重排响应解析
// ============================================================================

/// 未完成任务确定性兜底：当 AI 重排失败或返回空时，把复盘中标记为
/// 「未完成 / 部分完成」的任务以 TaskTemplate 形式插入重排范围的学习日，
/// 逐条分摊到连续的剩余学习日（从 review_date 次日起升序），
/// 保证「未完成任务不丢失」这条硬边界在任何情况下都成立。
fn uncompleted_tasks_fallback(
    week_plan: &mut crate::data::plan::WeekPlanFile,
    review: &crate::data::records::ReviewFile,
    regen_dates: &[String],
) -> bool {
    let unfinished: Vec<(crate::data::state::SubjectKey, String)> = review
        .task_reviews
        .iter()
        .filter(|tr| tr.status == "incomplete" || tr.status == "partial")
        .map(|tr| {
            let subject = match tr.subject.as_str() {
                "math" => crate::data::state::SubjectKey::Math,
                "english" => crate::data::state::SubjectKey::English,
                "politics" => crate::data::state::SubjectKey::Politics,
                "professional" => crate::data::state::SubjectKey::Professional,
                _ => crate::data::state::SubjectKey::Math,
            };
            let title = if tr.title.is_empty() {
                "未完成任务".to_string()
            } else {
                tr.title.clone()
            };
            (subject, title)
        })
        .collect();

    if unfinished.is_empty() {
        return false;
    }

    // 游标：定位重排范围内下一个有效学习日（非休息日/排除日）在 regen_dates 的索引
    let mut cursor: Option<usize> = None;
    let mut placed_any = false;
    for (subject, title) in unfinished {
        let idx = loop {
            let i = cursor.unwrap_or(0);
            if i >= regen_dates.len() {
                break None;
            }
            let date = &regen_dates[i];
            let is_study = week_plan
                .data
                .days
                .iter()
                .any(|d| d.date == *date && !d.is_rest_day);
            if is_study {
                break Some(i);
            }
            cursor = Some(i + 1);
        };

        let Some(i) = idx else { break };
        let date = regen_dates[i].clone();

        if let Some(day) = week_plan.data.days.iter_mut().find(|d| d.date == date) {
            let template = crate::data::plan::TaskTemplate {
                title,
                estimated_hours: 0.5,
                ..Default::default()
            };
            // 放入该日对应科目 allocation 开头；uknown 科目不存在则新建
            let alloc = match day
                .subject_allocations
                .iter_mut()
                .find(|a| a.subject == subject)
            {
                Some(a) => a,
                None => {
                    day.subject_allocations.insert(
                        0,
                        crate::data::plan::DaySubjectAllocation {
                            subject,
                            hours: 0.0,
                            focus: "未完成任务（兜底安排）".to_string(),
                            task_templates: Vec::new(),
                        },
                    );
                    day.subject_allocations.first_mut().unwrap()
                }
            };
            alloc.task_templates.insert(0, template);
            placed_any = true;
            // 下一条未完成任务从下一天开始，避免同一天堆叠过多
            cursor = Some(i + 1);
        }
    }
    placed_any
}

/// 排除日任务量确定性再分摊兜底：当 AI 重排失败或返回空时，
/// 把原本排在排除日的任务逐条轮转分摊到剩余学习日（升序），
/// 保证「排除日的任务量不丢失」这条硬边界在任何情况下都成立。
///
/// 分摊策略：按「已到开始学习日期」过滤任务后，以字母轮转方式逐条放入
/// 剩余学习日对应科目的 allocation；科目 allocation 不存在则新建。
fn exclusion_redistribute_fallback(
    week_plan: &mut crate::data::plan::WeekPlanFile,
    excluded_allocations: &[crate::data::plan::DaySubjectAllocation],
    regen_dates: &[String],
    subject_start_dates: &[(&'static str, String)],
) -> bool {
    if excluded_allocations.is_empty() {
        return false;
    }

    // 收集排除日的所有待分摊任务（含所属科目），过滤未到开始学习日期的科目
    let mut templates: Vec<(
        crate::data::state::SubjectKey,
        crate::data::plan::TaskTemplate,
    )> = Vec::new();
    for alloc in excluded_allocations {
        for t in &alloc.task_templates {
            templates.push((alloc.subject.clone(), t.clone()));
        }
    }
    if templates.is_empty() {
        return false;
    }

    // 剩余有效学习日（非休息日/排除日；排除日已被 enforce 标记为 is_rest_day=true）
    let study_days: Vec<String> = regen_dates
        .iter()
        .filter(|date| regen_dates_contains_study_day(week_plan, date))
        .cloned()
        .collect();
    if study_days.is_empty() {
        log::warn!("排除日再分摊兜底: 剩余学习日为空，无法分摊排除日任务量");
        return false;
    }

    let mut placed_any = false;
    for (i, (subject, template)) in templates.into_iter().enumerate() {
        // 该科目在目标日未到开始学习日期则跳过（由后续其它学习日 / 周计划继续容纳）
        if planner_subject_not_started(
            &subject,
            &study_days[i % study_days.len()],
            subject_start_dates,
        ) {
            log::warn!(
                "排除日再分摊兜底: 科目 {:?} 在 {} 未到开始学习日期，跳过任务「{}」",
                subject,
                study_days[i % study_days.len()],
                template.title
            );
            continue;
        }
        let date = study_days[i % study_days.len()].clone();
        let day = week_plan
            .data
            .days
            .iter_mut()
            .find(|d| d.date == date)
            .expect("study_days 由上一步校验得出，日期必然存在");
        let alloc = match day
            .subject_allocations
            .iter_mut()
            .find(|a| a.subject == subject)
        {
            Some(a) => a,
            None => {
                day.subject_allocations
                    .push(crate::data::plan::DaySubjectAllocation {
                        subject: subject.clone(),
                        hours: 0.0,
                        focus: "排除日任务量（兜底分摊）".to_string(),
                        task_templates: Vec::new(),
                    });
                day.subject_allocations.last_mut().unwrap()
            }
        };
        // 同步累加 hours，确保周计划展示与后续 prompt 引用的任务量不失真
        // （日计划用 estimated_hours，不受影响；此处保持周计划层数据一致）
        alloc.hours += template.estimated_hours;
        alloc.task_templates.push(template);
        placed_any = true;
    }
    placed_any
}

/// 判断某个重排范围日期是否为有效学习日（存在且非休息日）
fn regen_dates_contains_study_day(week_plan: &crate::data::plan::WeekPlanFile, date: &str) -> bool {
    week_plan
        .data
        .days
        .iter()
        .any(|d| d.date == date && !d.is_rest_day)
}

/// 判断某科目在指定日期是否还未到开始学习日期（planner 侧，逻辑与 scheduler 一致）
fn planner_subject_not_started(
    subject: &crate::data::state::SubjectKey,
    date: &str,
    subject_start_dates: &[(&'static str, String)],
) -> bool {
    let key = match subject {
        crate::data::state::SubjectKey::Math => "math",
        crate::data::state::SubjectKey::English => "english",
        crate::data::state::SubjectKey::Politics => "politics",
        crate::data::state::SubjectKey::Professional => "professional",
    };
    for (k, start_date) in subject_start_dates {
        if *k == key && !start_date.is_empty() {
            return start_date.as_str() > date;
        }
    }
    false
}

#[derive(Debug, Clone, serde::Deserialize)]
struct RegenDayPlan {
    date: String,
    #[serde(default)]
    subject_allocations: Vec<crate::data::plan::DaySubjectAllocation>,
}

/// 从 AI 响应中解析重排后的剩余天数安排
///
/// 期望 AI 返回格式：
/// ```json
/// { "days": [ { "date": "...", "subject_allocations": [...] } ] }
/// ```
///
/// 兼容处理：AI 有时会返回完整的周计划结构 `{ version, meta, data: { days: [...] } }`，
/// 此时从 `data.days` 提取。
fn parse_regenerate_response(content: &str, data_dir: &Path) -> DataResult<Vec<RegenDayPlan>> {
    let cleaned = clean_ai_json(content);

    // 方式1：尝试解析为顶层 { days: [...] }
    #[derive(serde::Deserialize)]
    struct RegenResponse {
        #[serde(default)]
        days: Vec<RegenDayPlan>,
    }

    if let Ok(parsed) = serde_json::from_str::<RegenResponse>(&cleaned) {
        if !parsed.days.is_empty() {
            return Ok(parsed.days);
        }
    }

    // 方式2：AI 返回了完整周计划结构，尝试从 data.days 提取
    #[derive(serde::Deserialize)]
    struct FullWeekPlanResponse {
        #[serde(default)]
        data: FullWeekPlanData,
    }

    #[derive(serde::Deserialize, Default)]
    struct FullWeekPlanData {
        #[serde(default)]
        days: Vec<RegenDayPlan>,
    }

    if let Ok(parsed) = serde_json::from_str::<FullWeekPlanResponse>(&cleaned) {
        if !parsed.data.days.is_empty() {
            log::info!(
                "AI 返回了完整周计划结构，已从 data.days 提取 {} 天的重排结果",
                parsed.data.days.len()
            );
            return Ok(parsed.data.days);
        }
    }

    // 两种方式都失败：记录调试日志并返回错误
    let preview: String = cleaned.chars().take(500).collect();
    let raw_preview: String = content.chars().take(1000).collect();
    crate::data::write_ai_debug_log(data_dir, "regenerate_parse_error", &format!(
        "解析 AI 重排响应失败：无法从响应中提取 days 数组。\n\ncleaned 内容前 500 字符:\n{}\n\n原始响应前 1000 字符:\n{}",
        preview,
        raw_preview,
    ));
    Err(
        "解析 AI 重排响应失败：无法从响应中提取 days 数组。AI 可能返回了非预期格式。详细日志已写入 logs/ai-debug.log"
            .to_string(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::state::SubjectKey;

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
    fn test_parse_regenerate_response_top_level_days() {
        // 标准格式：顶层 { days: [...] }
        let raw = r#"{"days":[{"date":"2026-08-04","subject_allocations":[{"subject":"math","hours":2.0,"focus":"测试","task_templates":[]}]}]}"#;
        let tmp = std::env::temp_dir().join("test_regen_top");
        let _ = std::fs::create_dir_all(&tmp);
        let result = parse_regenerate_response(raw, &tmp);
        assert!(result.is_ok());
        let days = result.unwrap();
        assert_eq!(days.len(), 1);
        assert_eq!(days[0].date, "2026-08-04");
        assert_eq!(days[0].subject_allocations.len(), 1);
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_parse_regenerate_response_full_week_plan_format() {
        // AI 返回了完整周计划结构，days 在 data 内部
        let raw = r#"{"version":"1.0.0","meta":{"week_start":"2026-08-03"},"data":{"goals":"测试","subjects":[{"subject":"math","plan":"..."}],"days":[{"date":"2026-08-04","weekday":"周二","is_rest_day":false,"subject_allocations":[{"subject":"math","hours":2.0,"focus":"向量组","task_templates":[{"title":"向量组","priority":"A","estimated_hours":2.0,"goal":"目标","completion_criteria":[]}]}]}]}}"#;
        let tmp = std::env::temp_dir().join("test_regen_full");
        let _ = std::fs::create_dir_all(&tmp);
        let result = parse_regenerate_response(raw, &tmp);
        assert!(result.is_ok(), "应能从 data.days 提取");
        let days = result.unwrap();
        assert_eq!(days.len(), 1);
        assert_eq!(days[0].date, "2026-08-04");
        assert_eq!(days[0].subject_allocations[0].focus, "向量组");
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_parse_regenerate_response_empty_days_returns_error() {
        // 既没有顶层 days 也没有 data.days，应返回错误而非空 Vec
        let raw = r#"{"version":"1.0.0","meta":{"week_start":"2026-08-03"},"data":{"goals":"无 days 字段"}}"#;
        let tmp = std::env::temp_dir().join("test_regen_empty");
        let _ = std::fs::create_dir_all(&tmp);
        let result = parse_regenerate_response(raw, &tmp);
        assert!(result.is_err(), "无 days 数组时应返回错误");
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_parse_week_plan_json() {
        let raw = r#"{"version":"1.0.0","meta":{"week_start":"2026-07-20","week_end":"2026-07-26","week_number":30,"generated_at":"2026-07-20T04:00","based_on":{"state":"state/current.state","user_model":"assets/user_model/_index.md","exam_config":"assets/config/exam-config.md"}},"data":{"goals":[],"subjects":[],"days":[]},"view":""}"#;
        let plan = parse_week_plan_json(raw, "2026-07-20", "2026-07-26").unwrap();
        assert_eq!(plan.meta.week_start, "2026-07-20");
        assert_eq!(plan.meta.week_number, 30);
    }

    #[test]
    fn test_enforce_rest_days_corrects_ai_mistakes() {
        // 模拟 AI 错误地把周六标记为休息日，但用户设置只有周日休息
        let raw = r#"{"version":"1.0.0","meta":{"week_start":"2026-07-20","week_end":"2026-07-26","week_number":30,"generated_at":"2026-07-20T04:00","based_on":{"state":"","user_model":"","exam_config":""}},"data":{"goals":[],"subjects":[],"days":[{"date":"2026-07-25","weekday":"周六","is_rest_day":true,"subject_allocations":[]},{"date":"2026-07-26","weekday":"周日","is_rest_day":false,"subject_allocations":[{"subject":"math","hours":2.0,"focus":"测试","task_templates":[]}]}]},"view":""}"#;
        let mut plan = parse_week_plan_json(raw, "2026-07-20", "2026-07-26").unwrap();

        // 用户设置：仅周日休息
        let rest_days = vec!["周日".to_string()];
        enforce_rest_days(&mut plan, &rest_days, "2026-07-20", "2026-07-26").unwrap();

        // 验证：周六应为学习日（is_rest_day=false）
        let saturday = plan
            .data
            .days
            .iter()
            .find(|d| d.date == "2026-07-25")
            .unwrap();
        assert!(!saturday.is_rest_day, "周六应为学习日");

        // 验证：周日应为休息日（is_rest_day=true），且 allocations 被清空
        let sunday = plan
            .data
            .days
            .iter()
            .find(|d| d.date == "2026-07-26")
            .unwrap();
        assert!(sunday.is_rest_day, "周日应为休息日");
        assert!(
            sunday.subject_allocations.is_empty(),
            "周日的 allocations 应被清空"
        );

        // 验证：补全了缺失的日期（周一到周五）
        assert_eq!(plan.data.days.len(), 7, "应补全为 7 天");
    }

    #[test]
    fn test_enforce_past_days_empty_clears_past_allocations() {
        // 模拟周中生成：昨天/前天被 AI 排了任务，今天为空操作边界
        let today = today_string();
        let yesterday = add_days(&today, -1).unwrap();
        let day_before = add_days(&today, -2).unwrap();

        let raw = format!(
            r#"{{"version":"1.0.0","meta":{{"week_start":"","week_end":"","week_number":1,"generated_at":"","based_on":{{"state":"","user_model":"","exam_config":""}}}},"data":{{"goals":[],"subjects":[],"days":[
                {{"date":"{}","weekday":"","is_rest_day":false,"subject_allocations":[{{"subject":"math","hours":2.0,"focus":"线性方程组","task_templates":[]}}]}},
                {{"date":"{}","weekday":"","is_rest_day":false,"subject_allocations":[{{"subject":"math","hours":2.0,"focus":"行列式","task_templates":[]}}]}},
                {{"date":"{}","weekday":"","is_rest_day":false,"subject_allocations":[{{"subject":"math","hours":2.0,"focus":"向量组","task_templates":[]}}]}}
            ]}},"view":""}}"#,
            day_before, yesterday, today
        );
        let mut plan = parse_week_plan_json(&raw, "", "").unwrap();

        enforce_past_days_empty(&mut plan);

        // 过去的两个日期任务应被清空
        let cleared1 = plan
            .data
            .days
            .iter()
            .find(|d| d.date == day_before)
            .unwrap();
        assert!(cleared1.subject_allocations.is_empty(), "前天任务应被清空");
        let cleared2 = plan.data.days.iter().find(|d| d.date == yesterday).unwrap();
        assert!(cleared2.subject_allocations.is_empty(), "昨天任务应被清空");
        // 今天任务应保留
        let today_plan = plan.data.days.iter().find(|d| d.date == today).unwrap();
        assert_eq!(today_plan.subject_allocations.len(), 1, "今天任务应保留");
    }

    /// 构造一个含 数学(10h)/英语(5h)/专业课(8h) 已开课、政治未开课 的 State
    fn task_budget_state() -> StudyState {
        let mut s = StudyState {
            subjects: Default::default(),
            ..Default::default()
        };
        s.subjects.math.active = true;
        s.subjects.math.weekly_hours = 10.0;
        s.subjects.english.active = true;
        s.subjects.english.weekly_hours = 5.0;
        s.subjects.politics.active = false; // 未开课
        s.subjects.professional.active = true;
        s.subjects.professional.weekly_hours = 8.0;
        s
    }

    #[test]
    fn test_subject_task_budget_at_least_one_when_slots_available() {
        let s = task_budget_state();
        // 3 个已开课科目，每日 3 条：正好每科 1 条
        let budget = subject_task_budget(&s, 3, "2026-08-09", &[]);
        assert_eq!(budget.len(), 3);
        let sum: i64 = budget.iter().map(|(_, n)| n).sum();
        assert_eq!(sum, 3);
        assert!(budget.iter().all(|(_, n)| *n >= 1), "每科至少 1 条");
    }

    #[test]
    fn test_subject_task_budget_spreads_extra_by_weight() {
        let s = task_budget_state();
        // 每日 6 条：先每科 1 条，多余 3 条按 10/5/8 权重分摊，总和严格等于 6
        let budget = subject_task_budget(&s, 6, "2026-08-09", &[]);
        let sum: i64 = budget.iter().map(|(_, n)| n).sum();
        assert_eq!(sum, 6);
        assert!(budget.iter().all(|(_, n)| *n >= 1), "每科至少 1 条");
        // 数学时长最高，应 ≥ 英语
        let m = budget
            .iter()
            .find(|(k, _)| *k == SubjectKey::Math)
            .unwrap()
            .1;
        let e = budget
            .iter()
            .find(|(k, _)| *k == SubjectKey::English)
            .unwrap()
            .1;
        assert!(m >= e, "数学时长高，任务数不应少于英语");
    }

    #[test]
    fn test_subject_task_budget_fewer_slots_than_subjects_weights_only() {
        let s = task_budget_state();
        // 每日 1 条 < 3 个科目：只按权重给 1 条给时长最高的数学
        let budget = subject_task_budget(&s, 1, "2026-08-09", &[]);
        let sum: i64 = budget.iter().map(|(_, n)| n).sum();
        assert_eq!(sum, 1);
        let math = budget
            .iter()
            .find(|(k, _)| *k == SubjectKey::Math)
            .unwrap()
            .1;
        assert_eq!(math, 1);
    }

    #[test]
    fn test_subject_task_budget_respects_start_date() {
        let s = task_budget_state();
        // 数学开始日期晚于本周，应被排除
        let starts = vec![("math", "2026-08-12".to_string())];
        let budget = subject_task_budget(&s, 4, "2026-08-09", &starts);
        assert!(
            !budget.iter().any(|(k, _)| *k == SubjectKey::Math),
            "未开课的数学不应进入分配"
        );
        let sum: i64 = budget.iter().map(|(_, n)| n).sum();
        assert_eq!(sum, 4);
    }

    #[test]
    fn test_memory_curve_review_items_inside_week() {
        // 2026-08-03 学习且 mastery=weak，做 +1/+3/+7 复习
        let mut r = crate::data::records::ReviewFile::default();
        r.meta.date = "2026-08-03".to_string();
        r.task_reviews = vec![crate::data::records::TaskReviewEntry {
            task_id: "t1".to_string(),
            status: "completed".to_string(),
            completion: 1.0,
            mastery: "weak".to_string(),
            blockers: Vec::new(),
            blocker_note: None,
            title: "行列式".to_string(),
            subject: "math".to_string(),
            priority: "A".to_string(),
            estimated_hours: None,
            actual_minutes: None,
        }];
        // 周 08-03..08-09：+1 → 08-04，+3 → 08-06，+7 → 08-10（出周，排除）
        let items = memory_curve_review_items(&[r], "2026-08-03", "2026-08-09");
        assert_eq!(items.len(), 2, "+7 落在周外应排除");
        assert!(items.iter().all(|i| i.subject == SubjectKey::Math));
        assert!(items
            .iter()
            .all(|i| i.due_date.as_str() >= "2026-08-03" && i.due_date.as_str() <= "2026-08-09"));
        assert!(items.iter().all(|i| i.title.contains("回访")));
    }

    #[test]
    fn test_memory_curve_ignores_non_weak() {
        let mut r = crate::data::records::ReviewFile::default();
        r.meta.date = "2026-08-03".to_string();
        r.task_reviews = vec![crate::data::records::TaskReviewEntry {
            task_id: "t1".to_string(),
            status: "completed".to_string(),
            completion: 1.0,
            mastery: "mastered".to_string(),
            blockers: Vec::new(),
            blocker_note: None,
            title: "行列式".to_string(),
            subject: "math".to_string(),
            priority: "A".to_string(),
            estimated_hours: None,
            actual_minutes: None,
        }];
        let items = memory_curve_review_items(&[r], "2026-08-03", "2026-08-09");
        assert!(items.is_empty(), "非 weak 内容不应生成复习点");
    }

    #[test]
    fn test_today_intensity_label_empty_when_no_reviews() {
        assert!(today_intensity_label(&[]).is_empty());
    }

    /// 构造一个仅含 2026-08-21 数学分配（2 条任务）的简化周计划
    fn week_plan_with_math_day() -> crate::data::plan::WeekPlanFile {
        crate::data::plan::WeekPlanFile {
            version: "1.0.0".to_string(),
            meta: crate::data::plan::WeekPlanMeta {
                week_start: "2026-08-17".to_string(),
                week_end: "2026-08-23".to_string(),
                week_number: 34,
                generated_at: "x".to_string(),
                based_on: Default::default(),
            },
            data: crate::data::plan::WeekPlanData {
                days: vec![crate::data::plan::WeekDayPlan {
                    date: "2026-08-21".to_string(),
                    weekday: "周五".to_string(),
                    is_rest_day: false,
                    subject_allocations: vec![crate::data::plan::DaySubjectAllocation {
                        subject: SubjectKey::Math,
                        hours: 4.0,
                        focus: "线性方程组".to_string(),
                        task_templates: vec![
                            crate::data::plan::TaskTemplate {
                                title: "线性方程组解的存在性判定（数二）".to_string(),
                                estimated_hours: 2.0,
                                ..Default::default()
                            },
                            crate::data::plan::TaskTemplate {
                                title: "向量组的线性相关与线性无关（数二）".to_string(),
                                estimated_hours: 1.0,
                                ..Default::default()
                            },
                        ],
                    }],
                }],
                ..Default::default()
            },
            view: None,
        }
    }

    #[test]
    fn test_consistency_dedup_removes_completed_tasks() {
        let mut state = StudyState::default();
        state.subjects.math.completed = vec!["向量组的线性相关与线性无关".to_string()];

        let mut after = week_plan_with_math_day();
        let before = after.clone();
        let regen_dates = vec!["2026-08-21".to_string()];
        let empty = std::collections::HashSet::new();
        let warnings =
            consistency_check_and_correct(&mut after, &state, &regen_dates, &before, &empty);
        assert!(warnings.is_empty(), "无计划外进度声明不应产生告警");

        let day = after
            .data
            .days
            .iter()
            .find(|d| d.date == "2026-08-21")
            .unwrap();
        let math = day
            .subject_allocations
            .iter()
            .find(|a| a.subject == SubjectKey::Math)
            .unwrap();
        assert_eq!(math.task_templates.len(), 1, "重复已学的向量组任务应被剔除");
        assert_eq!(
            math.task_templates[0].title,
            "线性方程组解的存在性判定（数二）"
        );
        assert!(
            (math.hours - 2.0).abs() < 1e-9,
            "hours 应修正为剩余模板预估和"
        );
    }

    #[test]
    fn test_consistency_warns_when_overcompletion_not_applied() {
        let mut state = StudyState::default();
        // 已完成里不含「线性方程组」，故去重不会误删；重排前后完全一致 → 应告警
        state.subjects.math.completed = vec!["向量组的线性相关与线性无关".to_string()];

        let mut after = week_plan_with_math_day();
        // 仅保留「线性方程组」（未命中已完成，去重不会改动它），模拟重排前后一致
        let tv = after.data.days[0].subject_allocations[0]
            .task_templates
            .remove(0);
        after.data.days[0].subject_allocations[0].task_templates = vec![tv];
        after.data.days[0].subject_allocations[0].hours = 2.0;
        let before = after.clone(); // 模拟 AI 重排后该科目剩余安排完全未变
        let regen_dates = vec!["2026-08-21".to_string()];
        let mut declared = std::collections::HashSet::new();
        declared.insert("math".to_string()); // 复盘中声明了数学的计划外进度
        let warnings =
            consistency_check_and_correct(&mut after, &state, &regen_dates, &before, &declared);

        assert!(
            warnings.iter().any(|w| w.contains("数学")),
            "应产生「数学计划外进度未生效」告警, 实际: {:?}",
            warnings
        );
        // 去重不应误删未完成的线性方程组
        let day = after
            .data
            .days
            .iter()
            .find(|d| d.date == "2026-08-21")
            .unwrap();
        let math = day
            .subject_allocations
            .iter()
            .find(|a| a.subject == SubjectKey::Math)
            .unwrap();
        assert_eq!(math.task_templates.len(), 1);
    }
}
