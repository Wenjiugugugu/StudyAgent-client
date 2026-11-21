//! 未提交复盘单独存放在 config 中，随备份迁移，不影响正式记录或进度。
use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{atomic_write, validate_date, DataResult};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewDraft {
    pub version: u32,
    pub date: String,
    pub form: Value,
}

fn draft_path(data_dir: &Path, date: &str) -> std::path::PathBuf {
    data_dir
        .join("config")
        .join("review-drafts")
        .join(format!("{date}.json"))
}

pub fn read_review_draft(data_dir: &Path, date: &str) -> DataResult<Option<ReviewDraft>> {
    validate_date(date)?;
    let content = match std::fs::read_to_string(draft_path(data_dir, date)) {
        Ok(content) => content,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("读取复盘草稿失败: {error}")),
    };
    let draft: ReviewDraft =
        serde_json::from_str(&content).map_err(|error| format!("复盘草稿损坏: {error}"))?;
    if draft.version != 1 || draft.date != date || !draft.form.is_object() {
        return Err("复盘草稿格式不兼容，请保留文件后检查".to_string());
    }
    Ok(Some(draft))
}

pub fn save_review_draft(data_dir: &Path, draft: &ReviewDraft) -> DataResult<()> {
    validate_date(&draft.date)?;
    if draft.version != 1 || !draft.form.is_object() {
        return Err("无效的复盘草稿格式".to_string());
    }
    let json = serde_json::to_string_pretty(draft).map_err(|error| error.to_string())?;
    atomic_write(&draft_path(data_dir, &draft.date), &json)
}

pub fn delete_review_draft(data_dir: &Path, date: &str) -> DataResult<()> {
    validate_date(date)?;
    match std::fs::remove_file(draft_path(data_dir, date)) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!("删除复盘草稿失败: {error}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drafts_survive_restart_without_writing_reviews_and_keep_corrupt_files() {
        let dir = std::env::temp_dir().join(format!(
            "studyagent-review-draft-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let draft = ReviewDraft {
            version: 1,
            date: "2026-10-12".to_string(),
            form: serde_json::json!({"mainDifficulty": "未提交的困难", "actualStudy": [{"content": "第一章"}]}),
        };
        save_review_draft(&dir, &draft).unwrap();
        assert_eq!(
            read_review_draft(&dir, &draft.date).unwrap().unwrap().form,
            draft.form
        );
        assert!(!dir.join("records").exists());
        assert!(!dir.join("state").exists());
        assert!(read_review_draft(&dir, "2026-10-11").unwrap().is_none());
        assert!(read_review_draft(&dir.join("other-data"), &draft.date)
            .unwrap()
            .is_none());
        std::fs::write(draft_path(&dir, &draft.date), "{broken").unwrap();
        assert!(read_review_draft(&dir, &draft.date).is_err());
        assert_eq!(
            std::fs::read_to_string(draft_path(&dir, &draft.date)).unwrap(),
            "{broken"
        );
        assert!(delete_review_draft(&dir, "../../other").is_err());
        delete_review_draft(&dir, &draft.date).unwrap();
        delete_review_draft(&dir, &draft.date).unwrap();
        std::fs::remove_dir_all(dir).unwrap();
    }
}
