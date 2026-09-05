//! professional — 专业课「全国统考科目」内置考纲进度表数据与装配引擎
//!
//! 用途：将随包内置的官方统考专业课考纲（总表 + 指定教材表）直接转换为进度表草稿，
//! 不依赖 AI。前端调用 `builtin_progress_table(subject="professional", exam_type=...)`
//! 一次拿到多份草稿：第 1 份为「总专业课进度表」（考纲板块→章节），其后为每本指定教材一张表。
//!
//! 覆盖范围（以教育部《多年度研究生招生工作管理规定》确定的全国统一命题专业课为准）：
//!
//! 1. 408 计算机学科专业基础综合
//! 2. 法律硕士联考（专业基础课 + 专业综合课，法学/非法学共用内容体系）
//! 3. 311 教育学专业基础综合
//! 4. 312 心理学专业基础综合
//! 5. 313 历史学专业基础
//! 6. 333 教育综合（教育硕士）
//! 7. 306 临床医学综合能力（西医）
//! 8. 307 临床医学综合能力（中医）
//! 9. 396 经济类综合能力
//! 10. 199 管理类综合能力
//! 11. 农学门类（314 数学(农) / 315 化学(农) / 414 植物生理学与生物化学 / 415 动物生理学与生物化学）
//!
//! 数据依据：最新一届官方考试大纲（随大纲发布逐年迭代核对），教材为各科公认/官方指定版本。

use std::sync::OnceLock;

use crate::data::now_string;
use crate::data::progress_tables::{
    new_progress_id, NodeLevel, NodeStatus, ProgressNode, ProgressTable, TableOrigin,
};

mod data_408_law;
mod data_edu;
mod data_med;
mod data_other;

/// 板块：phase（阶段/章节分组标题）+ items（该板块下的章节/知识点列表）
#[derive(Clone, Copy)]
pub struct ProfSection {
    pub phase: &'static str,
    pub items: &'static [&'static str],
}

/// 教材进度表：sections 为教材内部的篇/章分组（多数教材单组「全书章节」，长篇教材按篇分节）
#[derive(Clone, Copy)]
pub struct ProfBook {
    pub name: &'static str,
    pub sections: &'static [ProfSection],
}

/// 一门统考专业课的内置数据
#[derive(Clone)]
pub struct ProfExam {
    /// 长名（完整考试科目名称）
    pub name: &'static str,
    /// 短名（表中展示，如「408 计算机」）
    pub short: &'static str,
    /// 匹配关键词（用于按用户考试类型文本识别）
    pub keys: &'static [&'static str],
    /// 总专业课进度表：考纲板块 → 章节
    pub master: &'static [ProfSection],
    /// 指定/公认教材进度表（含【考纲变化备注】在 name 中说明）——运行时构建
    pub books: Vec<ProfBook>,
}

/// 构造「全书章节」单一分组的教材：展开为直接内联数组字面量（编译期可提升）
#[macro_export]
macro_rules! book {
    ($name:expr, [$($ch:expr),* $(,)?]) => {
        $crate::core::professional::ProfBook {
            name: $name,
            sections: &[
                $crate::core::professional::ProfSection {
                    phase: "全书章节",
                    items: &[$($ch),*],
                }
            ],
        }
    };
}
#[allow(unused_imports)]
pub(crate) use book;

/// 全部内置统考专业课（惰性构建一次）
fn all() -> &'static [ProfExam] {
    static ALL: OnceLock<Vec<ProfExam>> = OnceLock::new();
    ALL.get_or_init(|| {
        [
            data_408_law::group(),
            data_edu::group(),
            data_med::group(),
            data_other::group(),
        ]
        .into_iter()
        .flatten()
        .collect()
    })
}

/// 按用户「考试类型」文本匹配一门统考专业课（关键词子串匹配）。
pub fn find(exam_type: &str) -> Option<ProfExam> {
    let et = exam_type.trim();
    all()
        .iter()
        .find(|e| e.keys.iter().any(|k| !k.is_empty() && et.contains(k)))
        .cloned()
}

/// 支持的全部统考专业课短名清单（用于报错提示）
pub fn all_names() -> String {
    all()
        .iter()
        .map(|e| e.short.to_string())
        .collect::<Vec<_>>()
        .join("、")
}

/// 把一门统考专业课装配为多份进度表草稿（不落盘，前端预览确认后再各自保存）
///
/// 第 1 份为「总专业课进度表」，其后每本指定教材一张进度表。表 id 留空（保存时分配）。
/// 节点为两级结构：每个板块/篇章输出一个「章节」节点，其下 items 输出「知识点」子节点。
pub fn build_tables(exam: &ProfExam) -> Vec<ProgressTable> {
    // 1) 总专业课进度表：考纲板块(章节) → 知识点
    let mut tables = Vec::with_capacity(1 + exam.books.len());
    tables.push(build_master(exam));

    // 2) 每本指定教材一张进度表：教材篇/章分组(章节) → 章节目录(知识点)
    for book in &exam.books {
        tables.push(build_book(exam, book));
    }
    tables
}

/// 装配一张教材进度表：每个分组(篇/章)作为章节节点，其下章节条目作为知识点子节点
fn build_book(exam: &ProfExam, book: &ProfBook) -> ProgressTable {
    let mut nodes = Vec::new();
    for sec in book.sections {
        let chapter_id = new_progress_id("c", sec.phase);
        nodes.push(ProgressNode {
            id: chapter_id.clone(),
            title: sec.phase.to_string(),
            level: NodeLevel::Chapter,
            parent_id: None,
            phase: sec.phase.to_string(),
            status: NodeStatus::Pending,
            planned_date: None,
            note: String::new(),
        });
        for item in sec.items {
            nodes.push(ProgressNode {
                id: new_progress_id("n", item),
                title: item.to_string(),
                level: NodeLevel::Knowledge,
                parent_id: Some(chapter_id.clone()),
                phase: sec.phase.to_string(),
                status: NodeStatus::Pending,
                planned_date: None,
                note: String::new(),
            });
        }
    }
    ProgressTable {
        id: String::new(),
        subject: "professional".to_string(),
        variant: exam.short.to_string(),
        name: format!("{} · 教材：{}", exam.name, book.name),
        origin: TableOrigin::Builtin,
        created_at: now_string(),
        updated_at: now_string(),
        nodes,
    }
}

/// 总专业课进度表：每个考纲板块作为章节节点，其下知识点作为知识点子节点
fn build_master(exam: &ProfExam) -> ProgressTable {
    let mut nodes = Vec::new();
    for sec in exam.master {
        let chapter_id = new_progress_id("c", sec.phase);
        nodes.push(ProgressNode {
            id: chapter_id.clone(),
            title: sec.phase.to_string(),
            level: NodeLevel::Chapter,
            parent_id: None,
            phase: sec.phase.to_string(),
            status: NodeStatus::Pending,
            planned_date: None,
            note: String::new(),
        });
        for item in sec.items {
            nodes.push(ProgressNode {
                id: new_progress_id("n", item),
                title: item.to_string(),
                level: NodeLevel::Knowledge,
                parent_id: Some(chapter_id.clone()),
                phase: sec.phase.to_string(),
                status: NodeStatus::Pending,
                planned_date: None,
                note: String::new(),
            });
        }
    }
    ProgressTable {
        id: String::new(),
        subject: "professional".to_string(),
        variant: exam.short.to_string(),
        name: format!("{} · 总专业课进度表", exam.name),
        origin: TableOrigin::Builtin,
        created_at: now_string(),
        updated_at: now_string(),
        nodes,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn find_matches_by_keywords() {
        assert!(find("408计算机").is_some());
        assert!(find("法硕（非法学）").is_some());
        assert!(find("法硕").is_some());
        assert!(find("311教育学").is_some());
        assert!(find("312心理学").is_some());
        assert!(find("313历史学").is_some());
        assert!(find("333教育综合").is_some());
        assert!(find("西医综合").is_some());
        assert!(find("306").is_some());
        assert!(find("中医综合").is_some());
        assert!(find("396经济类").is_some());
        assert!(find("199管理类").is_some());
        assert!(find("农学").is_some());
        assert!(find("314数学(农)").is_some());
        assert!(find("不存在科目").is_none());
    }

    #[test]
    fn build_tables_returns_master_plus_books() {
        let exam = find("408计算机").expect("408 应可识别");
        let tables = build_tables(&exam);
        assert!(!tables.is_empty());
        // 第 1 份为总表
        assert!(tables[0].name.contains("总专业课进度表"));
        assert!(!tables[0].nodes.is_empty());
        // 总节点 phase 与 title 非空
        let first = &tables[0].nodes[0];
        assert!(!first.title.is_empty());
        assert!(!first.phase.is_empty());
        // 每份教材表至少一个节点
        for t in &tables[1..] {
            assert!(!t.nodes.is_empty());
            assert!(t.name.contains("教材"));
        }
    }

    #[test]
    fn every_exam_has_master_and_unique_short() {
        let exams = all();
        assert!(!exams.is_empty());
        assert!(
            exams.len() >= 5,
            "至少 5 门统考专业课，当前 {}",
            exams.len()
        );
        let mut shorts = vec![];
        for e in exams {
            assert!(!e.master.is_empty(), "「{}」缺少总表板块", e.name);
            assert!(!shorts.contains(&e.short), "短名重复: {}", e.short);
            shorts.push(e.short);
        }
    }
}
