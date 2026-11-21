//! 编辑版本只随响应返回，不写进学习记录。
use super::DataResult;
use serde::Serialize;
use sha2::{Digest, Sha256};

pub fn content_revision<T: Serialize>(value: &T) -> DataResult<String> {
    let bytes = serde_json::to_vec(value).map_err(|e| e.to_string())?;
    Ok(Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect())
}

pub fn with_revision<T: Serialize>(value: &T) -> DataResult<serde_json::Value> {
    let mut dto = serde_json::to_value(value).map_err(|e| e.to_string())?;
    dto.as_object_mut()
        .ok_or("编辑对象必须为 JSON 对象")?
        .insert("revision".into(), content_revision(value)?.into());
    Ok(dto)
}

pub fn check_revision<T: Serialize>(value: &T, expected: Option<&str>) -> DataResult<()> {
    if expected != Some(content_revision(value)?.as_str()) {
        return Err("数据已发生变化，请重新加载后再保存；本次编辑尚未写入。".into());
    }
    Ok(())
}
