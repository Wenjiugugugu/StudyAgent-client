//! User Model — 用户画像读取
//!
//! 提供用户能力（Capability）和观察（Observation）的读取功能。
//! 底层调用 data::assets 层读取 Markdown 文件。

use std::path::Path;

use crate::data::assets::{UserCapability, UserModelIndex, UserObservation};

/// User Model Service — 用户画像服务
pub struct UserModelService;

impl UserModelService {
    /// 读取用户画像索引
    ///
    /// 读取 `assets/user_model/_index.md` 并解析
    pub fn read_user_model(data_dir: &Path) -> Result<UserModelIndex, String> {
        crate::data::assets::read_user_model_index(data_dir)
    }

    /// 获取所有用户能力
    pub fn get_capabilities(data_dir: &Path) -> Result<Vec<UserCapability>, String> {
        let model = crate::data::assets::read_user_model_index(data_dir)?;
        Ok(model.capabilities)
    }

    /// 获取活跃的用户能力
    pub fn get_active_capabilities(data_dir: &Path) -> Result<Vec<UserCapability>, String> {
        let model = crate::data::assets::read_user_model_index(data_dir)?;
        Ok(model
            .capabilities
            .into_iter()
            .filter(|c| c.activity == "active")
            .collect())
    }

    /// 获取指定类别的用户能力
    pub fn get_capabilities_by_category(
        data_dir: &Path,
        category: &str,
    ) -> Result<Vec<UserCapability>, String> {
        let model = crate::data::assets::read_user_model_index(data_dir)?;
        Ok(model
            .capabilities
            .into_iter()
            .filter(|c| c.category == category)
            .collect())
    }

    /// 获取单个用户能力详情
    ///
    /// 读取 `assets/user_model/capabilities/{id}.md`
    pub fn get_capability(data_dir: &Path, cap_id: &str) -> Result<UserCapability, String> {
        crate::data::assets::read_capability(data_dir, cap_id)
    }

    /// 获取所有用户观察
    pub fn get_observations(data_dir: &Path) -> Result<Vec<UserObservation>, String> {
        let model = crate::data::assets::read_user_model_index(data_dir)?;
        Ok(model.observations)
    }

    /// 获取待处理的用户观察
    pub fn get_pending_observations(data_dir: &Path) -> Result<Vec<UserObservation>, String> {
        let model = crate::data::assets::read_user_model_index(data_dir)?;
        Ok(model
            .observations
            .into_iter()
            .filter(|o| o.status == "pending")
            .collect())
    }

    /// 获取学习风格摘要
    ///
    /// 汇总所有 learning_style 类别的能力
    pub fn get_learning_style_summary(data_dir: &Path) -> Result<String, String> {
        let capabilities = Self::get_capabilities_by_category(data_dir, "learning_style")?;

        if capabilities.is_empty() {
            return Ok("暂无学习风格画像数据".to_string());
        }

        let mut summary = Vec::new();
        for cap in &capabilities {
            let confidence_pct = (cap.confidence * 100.0) as u32;
            summary.push(format!(
                "{} ({}, 置信度 {}%)",
                cap.title, cap.activity, confidence_pct
            ));
        }

        Ok(summary.join("; "))
    }

    /// 获取用户画像摘要文本
    ///
    /// 用于 AI prompt 注入
    pub fn get_user_model_summary(data_dir: &Path) -> Result<String, String> {
        let model = crate::data::assets::read_user_model_index(data_dir)?;
        let state = crate::data::state::read_state_or_default(data_dir);

        let mut summary = String::new();

        // 基础信息
        summary.push_str(&format!(
            "## 用户学习画像\n\n- 平均每日专注时长: {:.1}h\n- 擅长科目: {}\n- 薄弱科目: {}\n- 复盘完成率: {:.0}%\n\n",
            state.user_model.avg_focus_hours_per_day,
            state.user_model.best_subjects.join(", "),
            state.user_model.worst_subjects.join(", "),
            state.user_model.review_compliance_rate * 100.0
        ));

        // 能力特征
        if !model.capabilities.is_empty() {
            summary.push_str("### 学习能力特征\n");
            for cap in &model.capabilities {
                let confidence_pct = (cap.confidence * 100.0) as u32;
                summary.push_str(&format!(
                    "- **{}** ({}, 置信度 {}%): {} — {}\n",
                    cap.title,
                    cap.category,
                    confidence_pct,
                    cap.activity,
                    cap.description.lines().next().map(str::trim).unwrap_or("")
                ));
            }
            summary.push_str("\n");
        }

        // 观察记录
        if !model.observations.is_empty() {
            summary.push_str("### 近期观察\n");
            for obs in model.observations.iter().take(5) {
                let confidence_pct = (obs.confidence * 100.0) as u32;
                summary.push_str(&format!(
                    "- {} (置信度 {}%, 状态: {})\n",
                    obs.summary, confidence_pct, obs.status
                ));
            }
        }

        Ok(summary)
    }
}
