//! 日期 / 时间工具 —— 日期运算的单一实现。
//!
//! 背景：日期运算此前散落在 `data/mod.rs`（`add_days` / `days_between` / 周界计算）
//! 与 `core/goal_planner.rs`（`day_diff`，同一个「天数差」的第二套实现与错误口径）
//! 中。现统一收口到本模块：
//! - `data` 层通过 `pub use` 再导出（见 `data/mod.rs`），既有
//!   `crate::data::add_days` 等调用点不受影响；
//! - `core` 层统一从这里 `use crate::core::date_utils::...`。
//!
//! 两个语义函数（比裸字符串比较更难写错，是跨模块口径分歧的根源）：
//! - [`today_string`]：**本地日期**（UTC+8）——「今天是哪天」只看这一处，
//!   避免个别模块用 UTC 日期导致 00:00-08:00 之间差一天；
//! - [`deadline_active_on`]：**截止日当天仍生效**——目标生命周期、生效目标过滤、
//!   自动顺延触发三处必须同口径，曾因三处不一致导致截止日当天目标被判过期。

use chrono::Datelike;

use crate::data::DataResult;

/// 获取当前日期字符串 (YYYY-MM-DD)
///
/// 使用 UTC+8 时区（中国标准时间，M13：改用 chrono 实现），避免 UTC+8 用户在
/// 00:00-08:00 之间因 UTC 时间偏移而得到前一天的日期。
pub fn today_string() -> String {
    let tz = chrono::FixedOffset::east_opt(8 * 3600).expect("UTC+8 偏移有效");
    chrono::Utc::now()
        .with_timezone(&tz)
        .format("%Y-%m-%d")
        .to_string()
}

/// 获取当前时间字符串 (YYYY-MM-DDTHH:mm)
///
/// 同样使用 UTC+8 时区。
pub fn now_string() -> String {
    let tz = chrono::FixedOffset::east_opt(8 * 3600).expect("UTC+8 偏移有效");
    chrono::Utc::now()
        .with_timezone(&tz)
        .format("%Y-%m-%dT%H:%M")
        .to_string()
}

/// 计算两个日期之间的天数差（date1 - date2）
pub fn days_between(date1: &str, date2: &str) -> DataResult<i64> {
    let d1 = parse_naive_date(date1)?;
    let d2 = parse_naive_date(date2)?;
    Ok((d1 - d2).num_days())
}

/// `to - from` 的天数差（from 晚于 to 时返回负数）；任一日期非法时返回 0。
///
/// 与 [`days_between`] 是同一实现的两种错误口径：需要向上报错时用
/// `days_between`，纯展示/兜底场景用本函数（不引入错误分支）。
pub fn day_diff(from: &str, to: &str) -> i64 {
    days_between(to, from).unwrap_or(0)
}

/// 解析 `YYYY-MM-DD` 为 chrono::NaiveDate
fn parse_naive_date(date: &str) -> DataResult<chrono::NaiveDate> {
    chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d")
        .map_err(|e| format!("无效日期格式: {} ({})", date, e))
}

/// 校验日期字符串是否为合法的 `YYYY-MM-DD` 格式**且日期真实存在**（C4-c）
///
/// 用于所有接收 `date` 参数的命令入口，防止恶意输入（如 `../../config/settings`）
/// 通过路径拼接穿越数据目录。
///
/// 除格式外还做语义校验（复用 `parse_naive_date`）：`2026-13-45`、`2026-02-30`
/// 这类字符串格式合法但日期不存在，若放行会被下游的字符串日期比较误判为
/// 「永不过期」（`"2026-99-99" >= today` 恒真），并在 `add_days`/解析链路静默失败。
pub fn validate_date(date: &str) -> DataResult<()> {
    if !is_valid_date_format(date) {
        return Err(format!("无效日期格式: {}", date));
    }
    parse_naive_date(date)?;
    Ok(())
}

/// 判断字符串是否为合法的 `YYYY-MM-DD` 格式
fn is_valid_date_format(date: &str) -> bool {
    let bytes = date.as_bytes();
    if bytes.len() != 10 {
        return false;
    }
    for (i, b) in bytes.iter().enumerate() {
        match i {
            4 | 7 => {
                if *b != b'-' {
                    return false;
                }
            }
            _ => {
                if !b.is_ascii_digit() {
                    return false;
                }
            }
        }
    }
    true
}

/// 获取日期对应的星期几（0=周一, 6=周日）
pub fn get_weekday(date: &str) -> DataResult<u32> {
    let d = parse_naive_date(date)?;
    Ok(d.weekday().num_days_from_monday() as u32)
}

/// 获取日期对应的中文星期几名称（周一 至 周日）
pub fn weekday_name(date: &str) -> DataResult<String> {
    let weekday = get_weekday(date)?;
    let names = ["周一", "周二", "周三", "周四", "周五", "周六", "周日"];
    Ok(names[weekday as usize].to_string())
}

/// 获取指定日期所在周的周一日期 (YYYY-MM-DD)
pub fn get_week_start(date: &str) -> DataResult<String> {
    let d = parse_naive_date(date)?;
    let weekday = d.weekday().num_days_from_monday() as i64;
    Ok(days_from_epoch_to_date_string(d, -weekday))
}

/// 获取指定日期所在周的周日日期 (YYYY-MM-DD)
pub fn get_week_end(date: &str) -> DataResult<String> {
    let d = parse_naive_date(date)?;
    let weekday = d.weekday().num_days_from_monday() as i64;
    Ok(days_from_epoch_to_date_string(d, 6 - weekday))
}

/// 获取从 start_date 开始的 n 天后的日期
pub fn add_days(date: &str, n: i64) -> DataResult<String> {
    let d = parse_naive_date(date)?;
    Ok(days_from_epoch_to_date_string(d, n))
}

/// 以基准日期 d 为原点，偏移 offset_days 天后的日期字符串
fn days_from_epoch_to_date_string(d: chrono::NaiveDate, offset_days: i64) -> String {
    d.checked_add_signed(chrono::Duration::days(offset_days))
        .map(|nd| nd.format("%Y-%m-%d").to_string())
        .unwrap_or_default()
}

/// 截止日是否仍然生效（**截止日当天仍算生效**）。
///
/// 口径：`deadline >= today` 生效，严格早于今天才失效；空 deadline 视为无效。
/// 目标生命周期的三处判定必须共用本函数：
/// - `data::goal::refresh_goal_lifecycle`（编辑保存后重算状态）；
/// - `data::goal::active_goals_for_subject`（取当日生效目标）；
/// - `core::goal_planner::extend_goal_deadline_on_miss`（未达标才顺延）。
///
/// 反例（历史缺陷）：三处各写各的 `< today` / `>= today`，且在截止日当天
/// 复盘时把目标判为过期，当天任务再没机会把它带达标。
pub fn deadline_active_on(deadline: &str, today: &str) -> bool {
    !deadline.is_empty() && deadline >= today
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_date_accepts_real_dates() {
        assert!(validate_date("2026-09-13").is_ok());
        assert!(validate_date("2024-02-29").is_ok()); // 闰年 2/29 存在
    }

    #[test]
    fn validate_date_rejects_bad_format() {
        assert!(validate_date("2026-9-13").is_err());
        assert!(validate_date("2026/09/13").is_err());
        assert!(validate_date("").is_err());
        // 路径穿越防护（原职责）仍生效
        assert!(validate_date("../../config/settings").is_err());
    }

    #[test]
    fn validate_date_rejects_nonexistent_dates() {
        // 格式合法但日期不存在：必须拒绝，否则字符串比较会把它当成「永不过期」
        assert!(validate_date("2026-13-45").is_err());
        assert!(validate_date("2026-02-30").is_err());
        assert!(validate_date("2025-02-29").is_err()); // 非闰年
        assert!(validate_date("2026-00-10").is_err());
    }

    #[test]
    fn day_diff_matches_days_between_with_opposite_sign() {
        // day_diff(to, from) = 正数；days_between(from, to) = 正数，二者符号相反、绝对值相同
        assert_eq!(day_diff("2026-09-20", "2026-09-25"), 5);
        assert_eq!(day_diff("2026-09-25", "2026-09-20"), -5);
        assert_eq!(days_between("2026-09-25", "2026-09-20").unwrap(), 5);
        // 任一日期非法 → 0（不引入错误分支）
        assert_eq!(day_diff("", "2026-09-20"), 0);
        assert_eq!(day_diff("2026-09-20", "2026-13-45"), 0);
    }

    #[test]
    fn deadline_active_on_includes_deadline_day() {
        // 截止日当天仍生效：这是与「自动顺延」「生命周期」共用的口径
        assert!(deadline_active_on("2026-09-20", "2026-09-20"));
        assert!(deadline_active_on("2026-09-21", "2026-09-20"));
        assert!(!deadline_active_on("2026-09-19", "2026-09-20"));
        // 空截止日视为无效，任何时候都不生效
        assert!(!deadline_active_on("", "2026-09-20"));
    }
}
