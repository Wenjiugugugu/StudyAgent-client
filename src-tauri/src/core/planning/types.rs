//! 规划层的结果类型，不依赖命令层。
/// 单个受影响日期的任务变动摘要（重排前后对比）
#[derive(Debug, Clone, serde::Serialize)]
pub struct RegenDayChange {
    /// 受影响日期 (YYYY-MM-DD)
    pub date: String,
    /// 该日新增的任务标题
    #[serde(default)]
    pub added: Vec<String>,
    /// 该日被移除的任务标题
    #[serde(default)]
    pub removed: Vec<String>,
    /// 该日标题变化的（原标题 → 新标题）
    #[serde(default)]
    pub adjusted: Vec<(String, String)>,
}

#[derive(Debug, Clone, Default)]
pub struct RegenerationResult {
    pub regenerated: bool,
    pub dates: Vec<String>,
    pub used_fallback: bool,
    pub warnings: Vec<String>,
    pub changes: Vec<RegenDayChange>,
}
