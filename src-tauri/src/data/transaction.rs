//! 多文件提交先持久化日志；异常退出后只补写仍匹配旧内容的文件。
use super::{atomic_write, DataResult};
use serde::{Deserialize, Serialize};
use std::path::Path;
const JOURNAL: &str = "config/pending-file-transaction.json";
#[derive(Serialize, Deserialize)]
struct Change {
    path: String,
    before: Option<String>,
    after: String,
}
fn read_optional(path: &Path) -> DataResult<Option<String>> {
    match std::fs::read_to_string(path) {
        Ok(s) => Ok(Some(s)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("读取事务文件失败 {:?}: {}", path, e)),
    }
}
pub fn recover(dir: &Path) -> DataResult<()> {
    let Some(raw) = read_optional(&dir.join(JOURNAL))? else {
        return Ok(());
    };
    let changes: Vec<Change> = serde_json::from_str(&raw).map_err(|e| e.to_string())?;
    // 先检查全部，遇到后来编辑的文件绝不覆盖。
    for change in &changes {
        let relative = Path::new(&change.path);
        if relative.is_absolute()
            || relative
                .components()
                .any(|c| !matches!(c, std::path::Component::Normal(_)))
        {
            return Err("事务日志包含无效路径".into());
        }
        let current = read_optional(&dir.join(relative))?;
        if current != change.before && current.as_deref() != Some(&change.after) {
            return Err("保存恢复遇到后来修改的数据，请检查事务日志；原文件未覆盖。".into());
        }
    }
    for change in changes {
        atomic_write(&dir.join(change.path), &change.after)?;
    }
    std::fs::remove_file(dir.join(JOURNAL)).map_err(|e| e.to_string())
}
pub fn commit(dir: &Path, files: Vec<(String, String)>) -> DataResult<()> {
    recover(dir)?;
    let changes = files
        .into_iter()
        .map(|(path, after)| {
            Ok(Change {
                before: read_optional(&dir.join(&path))?,
                path,
                after,
            })
        })
        .collect::<DataResult<Vec<_>>>()?;
    atomic_write(
        &dir.join(JOURNAL),
        &serde_json::to_string(&changes).map_err(|e| e.to_string())?,
    )?;
    recover(dir).map_err(|e| format!("保存尚未完成，已保留恢复日志: {}", e))
}
