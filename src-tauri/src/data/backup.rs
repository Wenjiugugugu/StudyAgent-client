//! 数据备份 / 导出 / 导入
//!
//! - `export_backup`：把统一注册的持久化数据目录（可选 logs/）压缩为 zip 备份文件，
//!   并写入带逐文件 SHA-256 的 manifest。
//! - `import_backup`：校验并解压备份 zip，覆盖前先把现有数据目录备份为 `.bak-{ts}`，
//!   解压时逐项做路径穿越防护（zip-slip），导入后可重启应用加载。
//!
//! zip 内所有条目使用相对路径（不含 data_dir 前缀），恢复时按相对路径写回 data_dir。

use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::{DataResult, PERSISTED_DATA_SUBDIRS};

const BACKUP_MANIFEST_FILE: &str = "backup-manifest.json";
const BACKUP_SCHEMA_VERSION: u32 = 1;
const MAX_ENTRY_SIZE: u64 = 64 * 1024 * 1024;
const MAX_TOTAL_SIZE: u64 = 512 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct BackupManifest {
    schema_version: u32,
    app_version: String,
    created_at: String,
    files: Vec<BackupManifestEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct BackupManifestEntry {
    path: String,
    size: u64,
    sha256: String,
}

fn sha256_bytes(content: &[u8]) -> String {
    Sha256::digest(content)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn is_allowed_backup_path(normalized: &str, include_logs: bool) -> bool {
    let top = normalized.split('/').next().unwrap_or("");
    PERSISTED_DATA_SUBDIRS.contains(&top) || include_logs && top == "logs"
}

/// 从 data_dir 递归收集待备份的相对路径文件列表（不含目录项）
///
/// 相对路径以 data_dir 为基准（如 `plan/2026-08-18_day.json`），
/// 确保恢复时可正确写回 data_dir 下对应子目录。
fn collect_files(data_dir: &Path) -> DataResult<Vec<(PathBuf, PathBuf)>> {
    let mut files = Vec::new();
    for sub in PERSISTED_DATA_SUBDIRS {
        let root = data_dir.join(sub);
        if !root.exists() {
            continue;
        }
        collect_dir(&root, data_dir, &mut files)?;
    }
    Ok(files)
}

/// 递归收集 dir 下的文件，rel 为 dir 相对 root 的路径前缀
fn collect_dir(dir: &Path, root: &Path, out: &mut Vec<(PathBuf, PathBuf)>) -> DataResult<()> {
    let entries = std::fs::read_dir(dir).map_err(|e| format!("读取目录失败 {:?}: {}", dir, e))?;
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
    let canon_data = std::fs::canonicalize(data_dir).unwrap_or_else(|_| data_dir.to_path_buf());
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

    let mut manifest_entries = Vec::with_capacity(files.len());
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
        manifest_entries.push(BackupManifestEntry {
            path: rel_str.clone(),
            size: content.len() as u64,
            sha256: sha256_bytes(&content),
        });
        zip_writer
            .write_all(&content)
            .map_err(|e| format!("写入 zip 内容失败 {}: {}", rel_str, e))?;
    }

    let manifest = BackupManifest {
        schema_version: BACKUP_SCHEMA_VERSION,
        app_version: env!("CARGO_PKG_VERSION").to_string(),
        created_at: crate::data::now_string(),
        files: manifest_entries,
    };
    let manifest_json =
        serde_json::to_vec_pretty(&manifest).map_err(|e| format!("序列化备份清单失败: {}", e))?;
    zip_writer
        .start_file(BACKUP_MANIFEST_FILE, options)
        .map_err(|e| format!("写入备份清单失败: {}", e))?;
    zip_writer
        .write_all(&manifest_json)
        .map_err(|e| format!("写入备份清单内容失败: {}", e))?;

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
    let mut seen_names = std::collections::HashSet::new();
    let mut manifest: Option<BackupManifest> = None;
    for i in 0..archive.len() {
        let mut entry = archive
            .by_index(i)
            .map_err(|e| format!("读取 zip 条目失败: {}", e))?;
        let name = entry.name().to_string();
        if !seen_names.insert(name.clone()) {
            return Err(format!("备份文件包含重复路径，已拒绝导入: {}", name));
        }
        if name == BACKUP_MANIFEST_FILE {
            let mut raw = String::new();
            entry
                .read_to_string(&mut raw)
                .map_err(|e| format!("读取备份清单失败: {}", e))?;
            let parsed: BackupManifest =
                serde_json::from_str(&raw).map_err(|e| format!("解析备份清单失败: {}", e))?;
            if parsed.schema_version > BACKUP_SCHEMA_VERSION {
                return Err(format!(
                    "备份格式版本 {} 高于当前支持版本 {}，请先升级应用",
                    parsed.schema_version, BACKUP_SCHEMA_VERSION
                ));
            }
            manifest = Some(parsed);
            continue;
        }
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
        if !is_allowed_backup_path(&normalized, true) {
            return Err(format!(
                "备份文件包含不在允许范围内的路径，已拒绝导入: {}",
                name
            ));
        }
        entries.push((name, false));
    }

    // 新格式备份在破坏现有数据前完成逐文件完整性验证；旧格式仍保持向后兼容。
    if let Some(manifest) = &manifest {
        let manifest_by_path: std::collections::HashMap<&str, &BackupManifestEntry> = manifest
            .files
            .iter()
            .map(|item| (item.path.as_str(), item))
            .collect();
        let archive_files: Vec<&str> = entries
            .iter()
            .filter_map(|(name, is_dir)| (!is_dir).then_some(name.as_str()))
            .collect();
        if manifest_by_path.len() != manifest.files.len()
            || archive_files.len() != manifest.files.len()
            || archive_files
                .iter()
                .any(|name| !manifest_by_path.contains_key(*name))
        {
            return Err("备份清单与压缩包文件列表不一致，已拒绝导入".to_string());
        }
        for name in archive_files {
            let expected = manifest_by_path[name];
            let mut entry = archive
                .by_name(name)
                .map_err(|e| format!("读取 zip 条目失败 {}: {}", name, e))?;
            if entry.size() > MAX_ENTRY_SIZE || entry.size() != expected.size {
                return Err(format!("备份文件大小校验失败，已拒绝导入: {}", name));
            }
            let mut content = Vec::new();
            entry
                .read_to_end(&mut content)
                .map_err(|e| format!("读取 zip 内容失败 {}: {}", name, e))?;
            if sha256_bytes(&content) != expected.sha256 {
                return Err(format!("备份文件完整性校验失败，已拒绝导入: {}", name));
            }
        }
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
        std::fs::rename(data_dir, &backup_dir).map_err(|e| {
            format!(
                "备份现有数据目录失败 {:?} -> {:?}: {}",
                data_dir, backup_dir, e
            )
        })?;
        log::warn!("导入前已备份原数据目录到 {:?}", backup_dir);
    }
    std::fs::create_dir_all(data_dir)
        .map_err(|e| format!("重建数据目录失败 {:?}: {}", data_dir, e))?;

    // 第二遍：解压写入（带大小上限防 zip 炸弹，M11）；中途失败时回滚恢复原数据目录（M12）
    let extraction = (|| -> DataResult<usize> {
        let mut total_size: u64 = 0;
        let mut restored = 0usize;
        for (name, is_dir) in entries {
            if is_dir {
                continue;
            }
            let normalized = name.replace('\\', "/");
            // 二次防御：相对路径必须仍落在允许的子目录内（第一遍已过滤 ../ 与绝对路径）
            if !is_allowed_backup_path(&normalized, true) {
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
                std::fs::create_dir_all(p).map_err(|e| format!("创建目录失败 {:?}: {}", p, e))?;
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
        std::fs::write(
            data_dir.join("plan").join("2026-08-18_day.json"),
            r#"{"a":1}"#,
        )
        .unwrap();
        std::fs::write(data_dir.join("state").join("current.state"), "[meta]\n").unwrap();

        // 导出
        let zip_path = root.join("backup.zip");
        let count = export_backup(&data_dir, &zip_path, false).unwrap();
        assert_eq!(count, 2, "应导出 2 个文件");
        assert!(zip_path.exists());

        let file = std::fs::File::open(&zip_path).unwrap();
        let mut archive = zip::ZipArchive::new(file).unwrap();
        let mut raw_manifest = String::new();
        archive
            .by_name(BACKUP_MANIFEST_FILE)
            .unwrap()
            .read_to_string(&mut raw_manifest)
            .unwrap();
        let manifest: BackupManifest = serde_json::from_str(&raw_manifest).unwrap();
        assert_eq!(manifest.schema_version, BACKUP_SCHEMA_VERSION);
        assert_eq!(manifest.files.len(), 2);
        drop(archive);

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
    fn export_covers_every_registered_data_directory() {
        let root = tmpdir("all-dirs");
        let data_dir = root.join("data");
        for dir in PERSISTED_DATA_SUBDIRS {
            let path = data_dir.join(dir).join("sentinel.txt");
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, dir).unwrap();
        }

        let zip_path = root.join("backup.zip");
        let count = export_backup(&data_dir, &zip_path, false).unwrap();
        assert_eq!(count, PERSISTED_DATA_SUBDIRS.len());

        let restore_dir = root.join("restore");
        let summary = import_backup(&restore_dir, &zip_path).unwrap();
        assert_eq!(summary.files_restored, PERSISTED_DATA_SUBDIRS.len());
        for dir in PERSISTED_DATA_SUBDIRS {
            assert!(
                restore_dir.join(dir).join("sentinel.txt").is_file(),
                "备份遗漏目录 {dir}"
            );
        }

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn import_rejects_manifest_hash_mismatch_before_replacing_data() {
        let root = tmpdir("hash-mismatch");
        let data_dir = root.join("data");
        std::fs::create_dir_all(data_dir.join("state")).unwrap();
        std::fs::write(data_dir.join("state").join("keep.txt"), "original").unwrap();

        let zip_path = root.join("tampered.zip");
        let file = std::fs::File::create(&zip_path).unwrap();
        let mut writer = zip::ZipWriter::new(std::io::BufWriter::new(file));
        let options = zip::write::SimpleFileOptions::default();
        writer.start_file("state/current.state", options).unwrap();
        writer.write_all(b"tampered").unwrap();
        let manifest = BackupManifest {
            schema_version: BACKUP_SCHEMA_VERSION,
            app_version: "test".to_string(),
            created_at: "test".to_string(),
            files: vec![BackupManifestEntry {
                path: "state/current.state".to_string(),
                size: 8,
                sha256: sha256_bytes(b"expected"),
            }],
        };
        writer.start_file(BACKUP_MANIFEST_FILE, options).unwrap();
        writer
            .write_all(&serde_json::to_vec(&manifest).unwrap())
            .unwrap();
        writer.finish().unwrap();

        let error = import_backup(&data_dir, &zip_path).unwrap_err();
        assert!(error.contains("完整性校验失败"));
        assert_eq!(
            std::fs::read_to_string(data_dir.join("state").join("keep.txt")).unwrap(),
            "original"
        );

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
