//! Knowledge — 知识对象读取与搜索
//!
//! 提供知识对象的列表、详情、搜索和图谱构建功能。
//! 底层调用 data::assets 层读取 Markdown 文件。

use std::path::Path;

use crate::data::assets::{
    KnowledgeGraph, KnowledgeObject, KnowledgeSubjectIndex,
};

/// Knowledge Service — 知识对象服务
pub struct KnowledgeService;

impl KnowledgeService {
    /// 列出指定学科的知识对象索引
    ///
    /// 读取 `assets/knowledge/objects/{subject}/_index.md` 并解析
    pub fn list_knowledge(data_dir: &Path, subject: &str) -> Result<Vec<KnowledgeSubjectIndex>, String> {
        if subject.is_empty() || subject == "all" {
            // 列出所有学科
            return Self::list_all_knowledge(data_dir);
        }

        let index = crate::data::assets::read_knowledge_index(data_dir, subject)?;
        Ok(vec![index])
    }

    /// 列出所有学科的知识对象索引
    fn list_all_knowledge(data_dir: &Path) -> Result<Vec<KnowledgeSubjectIndex>, String> {
        let objects_dir = data_dir
            .join(crate::data::assets::ASSETS_DIR)
            .join(crate::data::assets::KNOWLEDGE_DIR)
            .join(crate::data::assets::KNOWLEDGE_OBJECTS_DIR);

        if !objects_dir.exists() {
            return Ok(Vec::new());
        }

        let mut result = Vec::new();

        // 遍历 objects 目录下的子目录（每个子目录是一个学科）
        let entries = std::fs::read_dir(&objects_dir)
            .map_err(|e| format!("读取知识对象目录失败: {}", e))?;

        for entry in entries {
            let entry = entry.map_err(|e| format!("读取目录条目失败: {}", e))?;
            let path = entry.path();

            if path.is_dir() {
                if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                    // 检查是否有 _index.md
                    let index_path = path.join("_index.md");
                    if index_path.exists() {
                        let index =
                            crate::data::assets::read_knowledge_index(data_dir, name)?;
                        result.push(index);
                    }
                }
            }
        }

        Ok(result)
    }

    /// 获取单个知识对象详情
    ///
    /// 根据 ID 在 `assets/knowledge/objects/**/*.md` 中搜索对应文件
    pub fn get_knowledge(data_dir: &Path, id: &str) -> Result<KnowledgeObject, String> {
        crate::data::assets::read_knowledge_object(data_dir, id)
    }

    /// 搜索知识对象
    ///
    /// 在所有知识对象的标题、内容和标签中搜索匹配的关键词
    pub fn search_knowledge(
        data_dir: &Path,
        query: &str,
    ) -> Result<Vec<KnowledgeObject>, String> {
        crate::data::assets::search_knowledge_objects(data_dir, query)
    }

    /// 构建知识图谱
    ///
    /// 遍历指定学科的所有知识对象，
    /// 根据 prerequisites 字段构建有向无环图 (DAG)
    pub fn get_knowledge_graph(
        data_dir: &Path,
        subject: &str,
    ) -> Result<KnowledgeGraph, String> {
        crate::data::assets::build_knowledge_graph(data_dir, subject)
    }

    /// 获取知识对象的前置知识链
    ///
    /// 递归查找所有前置知识点
    pub fn get_prerequisite_chain(
        data_dir: &Path,
        id: &str,
    ) -> Result<Vec<KnowledgeObject>, String> {
        let mut chain = Vec::new();
        let mut visited = std::collections::HashSet::new();
        Self::collect_prerequisites(data_dir, id, &mut chain, &mut visited)?;
        Ok(chain)
    }

    /// 递归收集前置知识
    fn collect_prerequisites(
        data_dir: &Path,
        id: &str,
        chain: &mut Vec<KnowledgeObject>,
        visited: &mut std::collections::HashSet<String>,
    ) -> Result<(), String> {
        if visited.contains(id) {
            return Ok(());
        }
        visited.insert(id.to_string());

        let obj = crate::data::assets::read_knowledge_object(data_dir, id)?;

        for prereq in &obj.prerequisites {
            Self::collect_prerequisites(data_dir, prereq, chain, visited)?;
        }

        chain.push(obj);
        Ok(())
    }

    /// 获取知识对象的后继知识
    ///
    /// 查找所有 prerequisites 中引用了指定 ID 的知识对象
    pub fn get_dependents(
        data_dir: &Path,
        id: &str,
    ) -> Result<Vec<KnowledgeObject>, String> {
        let all_ids = crate::data::assets::list_knowledge_object_ids(data_dir)?;
        let mut dependents = Vec::new();

        for other_id in &all_ids {
            if other_id == id {
                continue;
            }

            if let Ok(obj) = crate::data::assets::read_knowledge_object(data_dir, other_id) {
                if obj.prerequisites.iter().any(|p| p == id) {
                    dependents.push(obj);
                }
            }
        }

        Ok(dependents)
    }

    /// 获取薄弱知识点
    ///
    /// 从 State 中读取 weak_chapters，匹配对应的知识对象
    pub fn get_weak_knowledge(
        data_dir: &Path,
        subject: &str,
    ) -> Result<Vec<KnowledgeObject>, String> {
        let state = crate::data::state::read_state(data_dir).unwrap_or_default();

        let subject_state = match subject {
            "math" => &state.subjects.math,
            "english" => &state.subjects.english,
            "politics" => &state.subjects.politics,
            "professional" => &state.subjects.professional,
            _ => return Ok(Vec::new()),
        };

        let all_ids = crate::data::assets::list_knowledge_object_ids(data_dir)?;
        let mut weak_objects = Vec::new();

        for id in &all_ids {
            if let Ok(obj) = crate::data::assets::read_knowledge_object(data_dir, id) {
                // 检查标题或内容是否匹配薄弱章节
                for weak in &subject_state.weak_chapters {
                    if obj.title.contains(weak.as_str())
                        || obj.tags.iter().any(|t| t.contains(weak.as_str()))
                        || weak.contains(&obj.title)
                    {
                        weak_objects.push(obj);
                        break;
                    }
                }
            }
        }

        Ok(weak_objects)
    }

    /// 统计知识对象数量
    pub fn count_knowledge(data_dir: &Path) -> Result<usize, String> {
        crate::data::assets::list_knowledge_object_ids(data_dir).map(|ids| ids.len())
    }

    /// 列出所有学科
    pub fn list_subjects(data_dir: &Path) -> Result<Vec<String>, String> {
        let objects_dir = data_dir
            .join(crate::data::assets::ASSETS_DIR)
            .join(crate::data::assets::KNOWLEDGE_DIR)
            .join(crate::data::assets::KNOWLEDGE_OBJECTS_DIR);

        if !objects_dir.exists() {
            return Ok(Vec::new());
        }

        let mut subjects = Vec::new();

        let entries = std::fs::read_dir(&objects_dir)
            .map_err(|e| format!("读取知识对象目录失败: {}", e))?;

        for entry in entries {
            let entry = entry.map_err(|e| format!("读取目录条目失败: {}", e))?;
            let path = entry.path();

            if path.is_dir() {
                if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                    subjects.push(name.to_string());
                }
            }
        }

        subjects.sort();
        Ok(subjects)
    }
}
