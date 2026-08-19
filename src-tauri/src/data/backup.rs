//! 数据备份 / 导出 / 导入
//!
//! - `export_backup`：把数据目录下允许的子目录（state/plan/records/config/assets/focus，
//!   可选 logs/）压缩为 zip 备份文件。
//! - `import_backup`：校验并解压备份 zip，覆盖前先把现有数据目录备份为 `.bak-{ts}`，
//!   解压时逐项做路径穿越防护（zip-slip），导入后可重启应用加载。
//!
//! zip 内所有条目使用相对路径（不含 data_dir 前缀），恢复时按相对路径写回 data_dir。

use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use super::DataResult;

/// 允许导出/导入的子目录（相对于 data_dir）
/// M10：包含 focus（专注记录），避免导入后番茄钟历史丢失
const BACKUP_SUBDIRS: [&str; 6] = ["state", "plan", "records", "config", "assets", "focus"];

/// 从 data_dir 递归收集待备份的相对路径文件列表（不含目录项）
///
/// 相对路径以 data_dir 为基准（如 `plan/2026-08-18_day.json`），
/// 确保恢复时可正确写回 data_dir 下对应子目录。
fn collect_files(data_dir: &Path) -> DataResult<Vec<(PathBuf, PathBuf)>> {
    let mut files = Vec::new();
    for sub in BACKUP_SUBDIRS {
        let root = data_dir.join(sub);
        if !root.exists() {
            continue;
        }
        collect_dir(&root, data_dir, &mut files)?;
    }
    Ok(files)
}

/// 递归收集 dir 下的文件，rel 为 dir 相对 root 的路径前缀
fn collect_dir(
    dir: &Path,
    root: &Path,
    out: &mut Vec<(PathBuf, PathBuf)>,
) -> DataResult<()> {
    let entries = std::fs::read_dir(dir)
        .map_err(|e| format!("读取目录失败 {:?}: {}", dir, e))?;
    for entry in entries {
        let entry = entry.map_err(|e| format!("读取目录条目失败: {}", e))?;
        let path = entry.path();
        let rel = path.strip_prefix(root).map_err(|e| e.to_string())?;
        if path.is_dir() {
            collect_dir(&path, root, out)?;
        } else if path.is_file() {
            out.push((rel.to_path_buf(), path));
        }
    }
    Ok(())
}

/// 导出备份：把 data_dir 下的允许子目录压缩为 dest_zip_path
///
/// `include_logs`：是否把 `logs/` 一并导出（体积较大，默认可选）。
pub fn export_backup(
    data_dir: &Path,
    dest_zip_path: &Path,
    include_logs: bool,
) -> DataResult<usize> {
    let canon_dest = std::fs::canonicalize(dest_zip_path.parent().unwrap_or(dest_zip_path))
        .unwrap_or_else(|_| dest_zip_path.to_path_buf());
    let canon_data = std::fs::canonicalize(data_dir)
        .unwrap_or_else(|_| data_dir.to_path_buf());
    if canon_dest.starts_with(&canon_data) {
        return Err(format!(
            "导出目标路径不能位于数据目录内: {:?} (数据目录 {:?})",
            dest_zip_path, data_dir
        ));
    }

    if let Some(parent) = dest_zip_path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("创建导出目录失败 {:?}: {}", parent, e))?;
    }

    let mut files = collect_files(data_dir)?;
    if include_logs {
        let logs_dir = data_dir.join("logs");
        if logs_dir.exists() {
            collect_dir(&logs_dir, data_dir, &mut files)?;
        }
    }
    // 按相对路径排序，保证导出内容稳定
    files.sort_by(|a, b| a.0.cmp(&b.0));

    let file = std::fs::File::create(dest_zip_path)
        .map_err(|e| format!("创建备份文件失败 {:?}: {}", dest_zip_path, e))?;
    let mut zip_writer = zip::ZipWriter::new(std::io::BufWriter::new(file));

    let options: zip::write::SimpleFileOptions = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);

    for (rel, abs) in &files {
        let rel_str = rel.to_string_lossy().replace('\\', "/");
        zip_writer
            .start_file(rel_str.clone(), options)
            .map_err(|e| format!("写入 zip 条目失败 {}: {}", rel_str, e))?;
        let mut content = Vec::new();
        std::fs::File::open(abs)
            .map_err(|e| format!("读取文件失败 {:?}: {}", abs, e))?
            .read_to_end(&mut content)
            .map_err(|e| format!("读取文件失败 {:?}: {}", abs, e))?;
        zip_writer
            .write_all(&content)
            .map_err(|e| format!("写入 zip 内容失败 {}: {}", rel_str, e))?;
    }

    let count = files.len();
    let mut zf = zip_writer
        .finish()
        .map_err(|e| format!("完成 zip 写入失败: {}", e))?;
    let _ = zf.flush();

    Ok(count)
}

/// 导入恢复结果摘要
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct ImportSummary {
    /// 从 zip 中恢复的文件数
    pub files_restored: usize,
    /// 被备份的旧数据目录备份名（相对 data_dir 所在目录）
    pub backup_dir: String,
}

/// 导入备份：校验并解压 zip 到 data_dir，覆盖前备份现有数据目录
///
/// 流程：
/// 1. 校验 zip 内所有路径都位于允许的子目录内（防 zip-slip 路径穿越）
/// 2. 把现有数据目录备份为 `{data_dir}-bak-{timestamp}`（重命名）
/// 3. 重新创建数据目录，解压 zip 内容写入
pub fn import_backup(data_dir: &Path, zip_path: &Path) -> DataResult<ImportSummary> {
    if !zip_path.is_file() {
        return Err(format!("备份文件不存在: {:?}", zip_path));
    }

    let file = std::fs::File::open(zip_path)
        .map_err(|e| format!("打开备份文件失败 {:?}: {}", zip_path, e))?;
    let mut archive = zip::ZipArchive::new(std::io::BufReader::new(file))
        .map_err(|e| format!("备份文件不是有效的 zip: {}", e))?;

    // 第一遍：校验所有条目路径，拒绝越界 / 绝对路径 / 盘符
    let mut entries: Vec<(String, bool)> = Vec::new();
    for i in 0..archive.len() {
        let entry = archive
            .by_index(i)
            .map_err(|e| format!("读取 zip 条目失败: {}", e))?;
        let name = entry.name().to_string();
        if entry.is_dir() {
            entries.push((name, true));
            continue;
        }
        let normalized = name.replace('\\', "/");
        if normalized.starts_with('/')
            || normalized
                .split('/')
                .any(|seg| seg == ".." || seg.is_empty() && !normalized.is_empty())
        {
            return Err(format!("备份文件包含非法路径，已拒绝导入: {}", name));
        }
        let top = normalized.split('/').next().unwrap_or("");
        if !BACKUP_SUBDIRS.contains(&top) {
            return Err(format!(
                "备份文件包含不在允许范围内的路径，已拒绝导入: {}",
                name
            ));
        }
        entries.push((name, false));
    }

    // 备份现有数据目录（重命名），失败则不继续，避免破坏现有数据
    let ts = crate::data::now_string().replace(':', "-");
    let parent = data_dir
        .parent()
        .ok_or_else(|| "无法定位数据目录父目录".to_string())?;
    let backup_dir = parent.join(format!(
        "{}-bak-{}",
        data_dir
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "data".to_string()),
        ts
    ));
    let had_original = data_dir.exists();
    if had_original {
        std::fs::rename(data_dir, &backup_dir)
            .map_err(|e| format!("备份现有数据目录失败 {:?} -> {:?}: {}", data_dir, backup_dir, e))?;
        log::warn!("导入前已备份原数据目录到 {:?}", backup_dir);
    }
    std::fs::create_dir_all(data_dir)
        .map_err(|e| format!("重建数据目录失败 {:?}: {}", data_dir, e))?;

    // 第二遍：解压写入（带大小上限防 zip 炸弹，M11）；中途失败时回滚恢复原数据目录（M12）
    const MAX_ENTRY_SIZE: u64 = 64 * 1024 * 1024; // 单条目上限 64MB
    const MAX_TOTAL_SIZE: u64 = 512 * 1024 * 1024; // 总解压上限 512MB

    let extraction = (|| -> DataResult<usize> {
        let mut total_size: u64 = 0;
        let mut restored = 0usize;
        for (name, is_dir) in entries {
            if is_dir {
                continue;
            }
            let normalized = name.replace('\\', "/");
            // 二次防御：相对路径必须仍落在允许的子目录内（第一遍已过滤 ../ 与绝对路径）
            let top = normalized.split('/').next().unwrap_or("");
            if !BACKUP_SUBDIRS.contains(&top) {
                return Err(format!("解压路径越界，已中止导入: {}", name));
            }
            let mut entry = archive
                .by_name(&name)
                .map_err(|e| format!("读取 zip 条目失败 {}: {}", name, e))?;
            let entry_size = entry.size();
            if entry_size > MAX_ENTRY_SIZE {
                return Err(format!(
                    "备份条目过大（{:.1}MB，上限 64MB），已中止导入: {}",
                    entry_size as f64 / 1024.0 / 1024.0,
                    name
                ));
            }
            total_size += entry_size;
            if total_size > MAX_TOTAL_SIZE {
                return Err(format!("备份总大小超过上限（512MB），已中止导入: {}", name));
            }
            let target = data_dir.join(&normalized);
            if let Some(p) = target.parent() {
                std::fs::create_dir_all(p)
                    .map_err(|e| format!("创建目录失败 {:?}: {}", p, e))?;
            }
            let mut content = Vec::new();
            entry
                .read_to_end(&mut content)
                .map_err(|e| format!("读取 zip 内容失败 {}: {}", name, e))?;
            std::fs::write(&target, &content)
                .map_err(|e| format!("写入恢复文件失败 {:?}: {}", target, e))?;
            restored += 1;
        }
        Ok(restored)
    })();

    let backup_dir_str = backup_dir.to_string_lossy().to_string();
    match extraction {
        Ok(restored) => Ok(ImportSummary {
            files_restored: restored,
            backup_dir: backup_dir_str,
        }),
        Err(e) => {
            // M12：解压中途失败时回滚——删除残缺的新目录，把备份目录改回原位置
            let _ = std::fs::remove_dir_all(data_dir);
            if had_original && std::fs::rename(&backup_dir, data_dir).is_ok() {
                Err(format!("{e}；已自动回滚，原数据已恢复"))
            } else {
                Err(format!("{e}；原数据已备份至 {:?}，可手动恢复", backup_dir))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn tmpdir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!(
            "studyagent-backup-test-{}-{}",
            tag,
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn export_then_import_roundtrip() {
        let root = tmpdir("roundtrip");
        let data_dir = root.join("data");
        // 构造数据目录
        std::fs::create_dir_all(data_dir.join("plan")).unwrap();
        std::fs::create_dir_all(data_dir.join("state")).unwrap();
        std::fs::write(data_dir.join("plan").join("2026-08-18_day.json"), r#"{"a":1}"#).unwrap();
        std::fs::write(data_dir.join("state").join("current.state"), "[meta]\n").unwrap();

        // 导出
        let zip_path = root.join("backup.zip");
        let count = export_backup(&data_dir, &zip_path, false).unwrap();
        assert_eq!(count, 2, "应导出 2 个文件");
        assert!(zip_path.exists());

        // 修改原目录（制造差异），再导入恢复
        std::fs::write(data_dir.join("plan").join("extra.txt"), "x").unwrap();

        // 导入前数据目录会被重命名备份
        let summary = match import_backup(&data_dir, &zip_path) {
            Ok(s) => s,
            Err(e) => panic!("import_backup 失败: {}", e),
        };
        assert_eq!(summary.files_restored, 2, "应恢复 2 个文件");
        assert!(Path::new(&summary.backup_dir).exists(), "原数据应已备份");
        // 恢复后的 plan 内容应与导出时一致，且不再有 extra.txt
        assert!(data_dir.join("plan").join("2026-08-18_day.json").exists());
        assert!(!data_dir.join("plan").join("extra.txt").exists());

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn import_rejects_path_traversal() {
        let root = tmpdir("traversal");
        let data_dir = root.join("data");
        std::fs::create_dir_all(&data_dir).unwrap();

        // 构造含 ../ 条目的恶意 zip
        let zip_path = root.join("evil.zip");
        let file = std::fs::File::create(&zip_path).unwrap();
        let mut zw = zip::ZipWriter::new(std::io::BufWriter::new(file));
        let options = zip::write::SimpleFileOptions::default();
        zw.start_file("../evil.txt", options).unwrap();
        std::io::Write::write_all(&mut zw, b"pwn").unwrap();
        zw.finish().unwrap();

        let err = import_backup(&data_dir, &zip_path).unwrap_err();
        assert!(err.contains("非法路径"), "应拒绝路径穿越，实际: {}", err);

        // 数据目录不应被破坏
        assert!(data_dir.exists());
        let _ = std::fs::remove_dir_all(&root);
    }
}
