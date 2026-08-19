//! Focus（番茄钟）数据层 — 读取/写入 `focus/YYYY-MM-DD_focus.json`
//!
//! 记录一次专注会话（学习/休息/长休息），供专注页展示今日统计与历史记录。
//! 沿用结构化 JSON 契约：{ version, meta, sessions: [...] }。

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use super::{atomic_write, read_file_content, DataResult, today_string};

// ============================================================================
// 数据结构
// ============================================================================

/// 会话类型
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FocusSessionType {
    /// 学习番茄
    Focus,
    /// 短休息
    ShortBreak,
    /// 长休息
    LongBreak,
    /// 正计时（预留）
    Stopwatch,
}

/// 会话状态
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FocusSessionStatus {
    /// 自然完成
    Completed,
    /// 中途打断（如重置）
    Interrupted,
}

/// 一次专注会话
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FocusSession {
    pub id: String,
    #[serde(rename = "type")]
    pub r#type: FocusSessionType,
    /// ISO 8601 开始时间（UTC）
    pub started_at: String,
    /// ISO 8601 结束时间（UTC）
    pub ended_at: String,
    pub duration_minutes: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,
    pub status: FocusSessionStatus,
}

/// 单日专注记录文件
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FocusDayFile {
    pub version: String,
    pub meta: FocusDayMeta,
    #[serde(default)]
    pub sessions: Vec<FocusSession>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FocusDayMeta {
    pub date: String,
}

/// 单日统计（供前端展示）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FocusDayStats {
    pub date: String,
    /// 完成的学习番茄数
    pub pomodoros: i64,
    /// 完成的专注总分钟（学习番茄）
    pub focus_minutes: i64,
    /// 完成休息次数（短休 + 长休）
    pub breaks: i64,
}

// ============================================================================
// 读写
// ============================================================================

fn focus_day_path(data_dir: &Path, date: &str) -> PathBuf {
    data_dir.join("focus").join(format!("{}_focus.json", date))
}

fn default_day_file(date: &str) -> FocusDayFile {
    FocusDayFile {
        version: "1".to_string(),
        meta: FocusDayMeta { date: date.to_string() },
        sessions: Vec::new(),
    }
}

/// 读取某天专注记录（文件不存在返回空文件）
pub fn read_focus_day(data_dir: &Path, date: &str) -> DataResult<FocusDayFile> {
    let path = focus_day_path(data_dir, date);
    if !path.exists() {
        return Ok(default_day_file(date));
    }
    let content = read_file_content(&path)?;
    serde_json::from_str(&content).map_err(|e| format!("解析专注记录失败 {:?}: {}", path, e))
}

/// 追加一条专注会话（按 ended_at 日期落盘）
pub fn append_focus_session(data_dir: &Path, session: FocusSession) -> DataResult<()> {
    // H3：校验 ended_at 前缀为合法 YYYY-MM-DD，防止路径穿越与非字符边界字节切片 panic
    let date_prefix = session
        .ended_at
        .get(..10)
        .ok_or_else(|| "专注会话 ended_at 格式不合法".to_string())?;
    chrono::NaiveDate::parse_from_str(date_prefix, "%Y-%m-%d")
        .map_err(|_| format!("专注会话 ended_at 日期格式不合法: {}", date_prefix))?;
    let date = date_prefix.to_string();
    let mut day = read_focus_day(data_dir, &date)?;
    day.sessions.push(session);
    let path = focus_day_path(data_dir, &date);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("创建目录失败 {:?}: {}", parent, e))?;
    }
    let json = serde_json::to_string_pretty(&day)
        .map_err(|e| format!("序列化专注记录失败: {}", e))?;
    atomic_write(&path, &json)
}

/// 读取某天的会话列表
pub fn list_focus_sessions(data_dir: &Path, date: &str) -> DataResult<Vec<FocusSession>> {
    Ok(read_focus_day(data_dir, date)?.sessions)
}

/// 读取 [start, end]（含）区间内所有会话
///
/// L14：区间跨度上限 366 天，防止恶意大范围查询拖慢性能 / 耗尽可能的 IO。
pub fn list_focus_sessions_in_range(
    data_dir: &Path,
    start: &str,
    end: &str,
) -> DataResult<Vec<FocusSession>> {
    use chrono::Datelike;
    const MAX_DAYS: i64 = 366;
    let start_date = chrono::NaiveDate::parse_from_str(start, "%Y-%m-%d")
        .map_err(|e| format!("开始日期格式不合法 {}: {}", start, e))?;
    let end_date = chrono::NaiveDate::parse_from_str(end, "%Y-%m-%d")
        .map_err(|e| format!("结束日期格式不合法 {}: {}", end, e))?;
    if end_date < start_date {
        return Err(format!("结束日期 {} 早于开始日期 {}", end, start));
    }
    let span = (end_date - start_date).num_days();
    if span > MAX_DAYS {
        return Err(format!(
            "区间跨度 {} 天超过上限 {} 天",
            span, MAX_DAYS
        ));
    }
    let mut sessions = Vec::new();
    let mut d = start_date;
    while d <= end_date {
        let date_str = format!(
            "{}-{:02}-{:02}",
            d.year(),
            d.month(),
            d.day()
        );
        sessions.extend(list_focus_sessions(data_dir, &date_str)?);
        d += chrono::Duration::days(1);
    }
    Ok(sessions)
}

/// 汇总某天统计（从当天记录计算）
pub fn focus_day_stats(data_dir: &Path, date: &str) -> DataResult<FocusDayStats> {
    let sessions = list_focus_sessions(data_dir, date)?;
    let mut stats = FocusDayStats {
        date: date.to_string(),
        pomodoros: 0,
        focus_minutes: 0,
        breaks: 0,
    };
    for s in &sessions {
        if s.status != FocusSessionStatus::Completed {
            continue;
        }
        match s.r#type {
            FocusSessionType::Focus => {
                stats.pomodoros += 1;
                stats.focus_minutes += s.duration_minutes.max(0);
            }
            FocusSessionType::ShortBreak | FocusSessionType::LongBreak => {
                stats.breaks += 1;
            }
            FocusSessionType::Stopwatch => {}
        }
    }
    Ok(stats)
}

/// 今日统计
pub fn focus_today_stats(data_dir: &Path) -> DataResult<FocusDayStats> {
    focus_day_stats(data_dir, &today_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "studyagent_focus_test_{}_{}",
            tag,
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn sample_session(id: &str, ended_at: &str) -> FocusSession {
        FocusSession {
            id: id.to_string(),
            r#type: FocusSessionType::Focus,
            started_at: "2026-08-19T01:00:00Z".to_string(),
            ended_at: ended_at.to_string(),
            duration_minutes: 25,
            task_id: Some("task-1".to_string()),
            status: FocusSessionStatus::Completed,
        }
    }

    #[test]
    fn test_focus_session_roundtrip() {
        let dir = tmp_dir("roundtrip");
        let s1 = sample_session("f1", "2026-08-19T01:25:00Z");
        let s2 = sample_session("f2", "2026-08-19T02:25:00Z");
        append_focus_session(&dir, s1.clone()).unwrap();
        append_focus_session(&dir, s2.clone()).unwrap();

        let sessions = list_focus_sessions(&dir, "2026-08-19").unwrap();
        assert_eq!(sessions.len(), 2);
        assert_eq!(sessions[0].id, "f1");
        assert_eq!(sessions[1].id, "f2");

        // 跨日期区间读取
        let range = list_focus_sessions_in_range(&dir, "2026-08-18", "2026-08-20").unwrap();
        assert_eq!(range.len(), 2);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_focus_day_stats() {
        let dir = tmp_dir("stats");
        // 2 个完成番茄 + 1 个打断番茄 + 1 个休息
        append_focus_session(&dir, sample_session("f1", "2026-08-19T01:25:00Z")).unwrap();
        append_focus_session(&dir, sample_session("f2", "2026-08-19T02:25:00Z")).unwrap();
        append_focus_session(
            &dir,
            FocusSession {
                id: "f3".to_string(),
                r#type: FocusSessionType::Focus,
                started_at: "2026-08-19T03:00:00Z".to_string(),
                ended_at: "2026-08-19T03:10:00Z".to_string(),
                duration_minutes: 10,
                task_id: None,
                status: FocusSessionStatus::Interrupted,
            },
        )
        .unwrap();
        append_focus_session(
            &dir,
            FocusSession {
                id: "b1".to_string(),
                r#type: FocusSessionType::ShortBreak,
                started_at: "2026-08-19T01:25:00Z".to_string(),
                ended_at: "2026-08-19T01:30:00Z".to_string(),
                duration_minutes: 5,
                task_id: None,
                status: FocusSessionStatus::Completed,
            },
        )
        .unwrap();

        let stats = focus_day_stats(&dir, "2026-08-19").unwrap();
        assert_eq!(stats.pomodoros, 2);
        assert_eq!(stats.focus_minutes, 50); // 打断的不计入
        assert_eq!(stats.breaks, 1);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_append_requires_valid_date() {
        let dir = tmp_dir("bad_date");
        let mut s = sample_session("f1", "2026-08-19T01:25:00Z");
        s.ended_at = "bad".to_string();
        assert!(append_focus_session(&dir, s).is_err());
        std::fs::remove_dir_all(&dir).ok();
    }
}
