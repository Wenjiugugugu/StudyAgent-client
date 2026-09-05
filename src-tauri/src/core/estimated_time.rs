//! estimated_time — 内置考纲表「预估学习时长」（隐藏数据，不展示给用户）
//!
//! 为所有官方内置考纲表（数学 / 英语 / 政治 / 专业课）的每个章节与知识点生成
//! 基准预估学习时长（小时）。该值仅写入进度表节点（`ProgressNode.estimated_hours`），
//! 界面不展示；周计划生成时作为任务时长参考，并按自适应周计划学到的用户效率系数
//! （`AdaptiveState.subjects[*].estimation_factor`）缩放：
//!
//! - 用户长期反馈任务量偏少 / 实际用时低于预计 → `estimation_factor < 1` → 预估时长缩短；
//! - 用户长期反馈任务量偏多 / 实际用时高于预计 → `estimation_factor > 1` → 预估时长延长。

/// 单条知识点预估时长的下限（小时）
pub const MIN_KNOWLEDGE_HOURS: f64 = 0.5;
/// 单条知识点预估时长的上限（小时）
pub const MAX_KNOWLEDGE_HOURS: f64 = 4.0;

/// 各科知识点的基准时长（小时）：数学/专业课需要较多理解+练习，英语/政治偏背诵略低。
fn subject_base(subject: &str) -> f64 {
    match subject {
        "math" => 1.5,
        "professional" => 1.5,
        "english" => 1.0,
        "politics" => 1.0,
        _ => 1.0,
    }
}

fn clamp(value: f64, min: f64, max: f64) -> f64 {
    value.max(min).min(max)
}

/// 按标题特征估算一个知识点的基准学习时长（小时）。
///
/// 规则（确定性）：
/// - 证明/推导/算法/综合类动手推理内容耗时最长（×1.4）；
/// - 计算/求解/方法/应用/练习类次之（×1.2）；
/// - 概念/定义/性质/概述等偏识记内容最短（×0.8）；
/// - 标题较长（含并列多个子考点）时略增（×1.1）。
pub fn estimate_knowledge_hours(subject: &str, title: &str) -> f64 {
    let t = title.trim();
    let factor = if t.is_empty() {
        1.0
    } else if t.contains("证明") || t.contains("推导") || t.contains("算法") || t.contains("综合")
    {
        1.4
    } else if t.contains("计算")
        || t.contains("求解")
        || t.contains("方法")
        || t.contains("应用")
        || t.contains("练习")
        || t.contains("习题")
        || t.contains("套路")
    {
        1.2
    } else if t.contains("概念")
        || t.contains("定义")
        || t.contains("性质")
        || t.contains("概述")
        || t.contains("简介")
        || t.contains("特点")
        || t.contains("意义")
        || t.contains("作用")
    {
        0.8
    } else {
        1.0
    };
    let length_factor = if t.chars().count() >= 18 { 1.1 } else { 1.0 };
    let hours = subject_base(subject) * factor * length_factor;
    round1(clamp(hours, MIN_KNOWLEDGE_HOURS, MAX_KNOWLEDGE_HOURS))
}

/// 估算一个章节的基准学习时长（小时）。
///
/// 章节时长 = 其下知识点预估值之和（体现「学完这一章所需总时长」）；
/// 无子知识点（异常数据）时按科目基准 × 3 兜底。
pub fn estimate_chapter_hours(subject: &str, _chapter_title: &str, children: &[f64]) -> f64 {
    if children.is_empty() {
        return round1(clamp(subject_base(subject) * 3.0, 1.0, 8.0));
    }
    round1(children.iter().sum())
}

/// 用户效率校准的复合调整参数（由自适应周计划学习得出，不展示给用户）。
///
/// 调整不再只是「基准 × 单一效率系数」，而是融合三个可解释信号：
/// 1. `efficiency_factor`：学科估时系数，由「实际用时 / 预估用时」的指数平滑（EMA）学习，
///    <1 表示用户完成同类内容偏快 → 预估应缩短，>1 反之；
/// 2. `feedback_signal`：任务量反馈信号（-1..1），源自复盘「任务量偏少/合适/偏多」的 EMA，
///    与自适应计划 `feedback_score` 符号一致：**>0 = 反馈任务量偏少 → 时间预估偏长 → 缩短预估**；
///    <0 = 反馈任务量偏多 → 延长预估；
/// 3. `completion_rate`：学科计划完成率（0..1），完成率偏低说明计划偏紧/预估偏乐观 →
///    略延长预估，完成率高则略缩短。
///
/// 三者先相乘，再按 `confidence` 向中性值 1 收缩（低置信度时不轻易大幅调整），
/// 最后 clamp 到 [0.7, 1.4] 防止单周剧烈跳变。
#[derive(Debug, Clone, Copy)]
pub struct EstimateAdjustment {
    /// 学科估时系数（通常 0.8~1.25）
    pub efficiency_factor: f64,
    /// 任务量反馈信号（-1..1；>0 = 偏少 → 缩短预估）
    pub feedback_signal: f64,
    /// 学科计划完成率（0..1）
    pub completion_rate: f64,
    /// 置信度（0..1；越低越向中性收缩）
    pub confidence: f64,
}

impl EstimateAdjustment {
    /// 融合后的综合调整系数（不含知识点难度敏感性）。
    pub fn combined_factor(&self) -> f64 {
        let eff = if self.efficiency_factor.is_finite() && self.efficiency_factor > 0.0 {
            self.efficiency_factor
        } else {
            1.0
        };
        // 反馈：信号 > 0（偏少）→ 系数 < 1 → 缩短预估；< 0（偏多）→ 延长
        let feedback_adj = 1.0 - 0.12 * self.feedback_signal.clamp(-1.0, 1.0);
        // 完成率：高于 0.85 说明计划偏松（用户快）→ 略缩短；低于 0.85 → 略延长
        let completion_adj = 1.0 - 0.15 * (self.completion_rate.clamp(0.0, 1.0) - 0.85);
        let conf = self.confidence.clamp(0.0, 1.0);
        let raw = eff * feedback_adj * completion_adj;
        // 置信度收缩 + 护栏：低置信度向 1 靠拢，整体限制在 [0.7, 1.4]
        (1.0 + (raw - 1.0) * conf).clamp(0.7, 1.4)
    }
}

/// 知识点难度对效率调整的「敏感性」：难度越高，调整越保守（向 1 收缩），
/// 避免「用户整体效率高 → 难知识点也被大幅压缩」；识记/概念类允许更激进。
pub fn difficulty_sensitivity(title: &str) -> f64 {
    let t = title.trim();
    if t.contains("证明") || t.contains("推导") || t.contains("算法") || t.contains("综合")
    {
        0.7
    } else if t.contains("计算")
        || t.contains("求解")
        || t.contains("方法")
        || t.contains("应用")
        || t.contains("练习")
        || t.contains("习题")
    {
        0.9
    } else if t.contains("概念")
        || t.contains("定义")
        || t.contains("性质")
        || t.contains("概述")
        || t.contains("简介")
        || t.contains("识记")
        || t.contains("背诵")
        || t.contains("记忆")
    {
        1.2
    } else {
        1.0
    }
}

/// 按复合调整参数 + 知识点难度敏感性缩放预估时长（小时）。
///
/// `adjustment` 非法（efficiency_factor ≤ 0 / NaN）时视为无调整（系数 1.0）。
pub fn adjust_hours(base: f64, adjustment: &EstimateAdjustment, title: &str) -> f64 {
    let combined = adjustment.combined_factor();
    let sensitivity = difficulty_sensitivity(title).clamp(0.5, 1.5);
    let effective = 1.0 + (combined - 1.0) * sensitivity;
    round1(clamp(
        base * effective,
        MIN_KNOWLEDGE_HOURS,
        MAX_KNOWLEDGE_HOURS,
    ))
}

/// 保留一位小数的时长（小时），避免浮点噪音。
pub fn round1(value: f64) -> f64 {
    (value * 10.0).round() / 10.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn knowledge_estimate_bounds_and_ordering() {
        // 长标题的「证明/综合」类最长，纯「概念」类最短
        let proof = estimate_knowledge_hours("math", "中值定理的证明与应用综合");
        let concept = estimate_knowledge_hours("math", "函数的概念");
        let calc = estimate_knowledge_hours("math", "极限的计算方法");
        assert!(concept < calc && calc <= proof, "{concept} {calc} {proof}");
        // 全部落在合理区间
        for title in ["函数的概念", "矩阵乘法", "剩余价值理论的证明与应用"] {
            let h = estimate_knowledge_hours("professional", title);
            assert!(
                (MIN_KNOWLEDGE_HOURS..=MAX_KNOWLEDGE_HOURS).contains(&h),
                "{title}: {h}"
            );
        }
    }

    #[test]
    fn chapter_estimate_is_sum_of_children() {
        let kids = vec![1.0, 1.5, 0.5];
        let ch = estimate_chapter_hours("math", "第一章", &kids);
        assert!((ch - 3.0).abs() < 1e-9);
        // 无子节点时兜底
        assert!(estimate_chapter_hours("math", "第一章", &[]) > 0.0);
    }

    #[test]
    fn adjust_combines_efficiency_feedback_completion() {
        let neutral = EstimateAdjustment {
            efficiency_factor: 1.0,
            feedback_signal: 0.0,
            completion_rate: 0.85,
            confidence: 1.0,
        };
        // 中性 → 不调整
        assert!((adjust_hours(2.0, &neutral, "矩阵乘法") - 2.0).abs() < 1e-9);

        // 用户完成偏快 + 反馈任务量偏少 + 完成率偏高 → 综合缩短
        let fast = EstimateAdjustment {
            efficiency_factor: 0.85,
            feedback_signal: 0.6,  // 偏少 → 缩短
            completion_rate: 0.95, // 偏松 → 缩短
            confidence: 1.0,
        };
        let shorter = adjust_hours(2.0, &fast, "矩阵乘法");
        assert!(shorter < 2.0, "应缩短，实际 {shorter}");

        // 用户偏慢 + 反馈偏多 + 完成率低 → 综合延长
        let slow = EstimateAdjustment {
            efficiency_factor: 1.2,
            feedback_signal: -0.6, // 偏多 → 延长
            completion_rate: 0.6,  // 偏紧 → 延长
            confidence: 1.0,
        };
        let longer = adjust_hours(2.0, &slow, "矩阵乘法");
        assert!(longer > 2.0, "应延长，实际 {longer}");

        // 低置信度 → 向中性收缩：同样的信号在低置信度下调整幅度更小
        let low_conf = EstimateAdjustment {
            efficiency_factor: 0.8,
            feedback_signal: 1.0,
            completion_rate: 0.9,
            confidence: 0.2,
        };
        let high_conf = EstimateAdjustment {
            efficiency_factor: 0.8,
            feedback_signal: 1.0,
            completion_rate: 0.9,
            confidence: 1.0,
        };
        assert!(
            (adjust_hours(2.0, &low_conf, "矩阵乘法") - 2.0).abs()
                < (adjust_hours(2.0, &high_conf, "矩阵乘法") - 2.0).abs()
        );

        // 非法效率系数按 1.0 处理
        let bad = EstimateAdjustment {
            efficiency_factor: f64::NAN,
            feedback_signal: 0.0,
            completion_rate: 0.85,
            confidence: 1.0,
        };
        assert!((adjust_hours(2.0, &bad, "矩阵乘法") - 2.0).abs() < 1e-9);
    }

    #[test]
    fn difficulty_sensitivity_bounds_scaling() {
        // 难度高（证明/综合）：同样信号下缩放更保守（接近 1）
        let adj = EstimateAdjustment {
            efficiency_factor: 0.8,
            feedback_signal: 1.0, // 偏少 → 缩短
            completion_rate: 1.0,
            confidence: 1.0,
        };
        let hard = adjust_hours(2.0, &adj, "中值定理的证明综合");
        let easy = adjust_hours(2.0, &adj, "基本概念与定义");
        assert!(hard >= easy, "难内容缩放应更保守: hard={hard} easy={easy}");
        // 无论如何都落在合理区间
        for title in ["中值定理的证明综合", "基本概念与定义", "极限的计算方法"]
        {
            let h = adjust_hours(2.0, &adj, title);
            assert!(
                (MIN_KNOWLEDGE_HOURS..=MAX_KNOWLEDGE_HOURS).contains(&h),
                "{title}: {h}"
            );
        }
    }
}
