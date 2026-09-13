//! chapter_seq — 内置「科目章节·知识点先后顺序表」（按官方考纲，随包编译分发）
//!
//! 用途：复盘声明「实际进度」后，确定性判断剩余计划任务是否超前于用户实际进度，
//! 并把超前（排在用户实际进度之前 / 已学过的内容）剔除/后置，真正改写周计划文件。
//!
//! 数据粒度：**章节 → 知识点** 两层，尽量接近官方考纲的「考试内容」细分度
//! （如线代「向量」一章细分为：向量组及其线性组合 → 向量组的线性表示 →
//! 向量组等价 → 线性相关与线性无关 → 极大线性无关组 → 向量组的秩）。
//!
//! 各数学版本由共享板块拼装（高数 + 线代 [+ 概率]），用 `OnceLock` 起步时构建一次。
//! 英语一/二无严格前置递进，仅按板块放置，不参与超前剔除的先后判断。

use std::sync::OnceLock;

// ---------------- 高数（数一 8 章） ----------------
const GAODENG_1: &[&str] = &[
    // 一、函数、极限、连续
    "函数的概念及表示法",
    "函数的有界性、单调性、周期性和奇偶性",
    "复合函数、反函数、分段函数和隐函数",
    "基本初等函数的性质及其图形",
    "初等函数",
    "数列极限",
    "函数极限与左、右极限",
    "无穷小与无穷大",
    "无穷小的比较",
    "极限的四则运算法则",
    "两个重要极限",
    "极限存在的准则：单调有界准则和夹逼准则",
    "函数连续的概念",
    "函数间断点的类型",
    "初等函数的连续性",
    "闭区间上连续函数的性质",
    // 二、一元函数微分学
    "导数和微分的概念",
    "导数的几何意义和物理意义",
    "平面曲线的切线和法线",
    "导数与微分的四则运算法则",
    "复合函数、反函数、隐函数以及参数方程所确定的函数的微分法",
    "高阶导数",
    "微分中值定理",
    "洛必达法则",
    "泰勒公式",
    "函数单调性判别",
    "函数的极值",
    "函数图形的凹凸性与拐点",
    "函数图形的渐近线",
    "函数图形的描绘",
    // 三、一元函数积分学
    "原函数与不定积分的概念",
    "不定积分的基本公式",
    "不定积分的换元积分法",
    "不定积分的分部积分法",
    "有理函数与可化为有理函数的积分",
    "定积分的概念与基本性质",
    "微积分基本公式",
    "定积分的换元积分法和分部积分法",
    "反常积分",
    "定积分应用：平面图形的面积、旋转体的体积",
    // 四、向量代数与空间解析几何
    "向量的概念及其线性运算",
    "数量积、向量积、混合积",
    "平面方程",
    "直线方程",
    "平面与直线、直线与直线的相互关系",
    "曲面方程",
    "空间曲线及其方程",
    // 五、多元函数微分学
    "多元函数的概念",
    "多元函数的偏导数与全微分",
    "多元复合函数的求导法",
    "隐函数的求导法",
    "方向导数与梯度",
    "多元函数极值与条件极值",
    "拉格朗日乘数法",
    // 六、多元函数积分学
    "二重积分",
    "三重积分",
    "曲线积分",
    "曲面积分",
    "格林公式",
    "高斯公式",
    "斯托克斯公式",
    // 七、无穷级数
    "常数项级数的概念与性质",
    "正项级数收敛性的判别法",
    "交错级数与绝对收敛、条件收敛",
    "幂级数",
    "幂级数的和函数",
    "函数的幂级数展开",
    "傅里叶级数",
    // 八、常微分方程
    "微分方程的基本概念",
    "可分离变量的微分方程",
    "一阶线性微分方程",
    "可降阶的高阶微分方程",
    "高阶线性微分方程",
    "常系数齐次线性微分方程",
    "常系数非齐次线性微分方程",
];

// ---------------- 高数（数二：略空间解析几何、曲线曲面积分、级数） ----------------
const GAODENG_2: &[&str] = &[
    "函数的概念及表示法",
    "函数的有界性、单调性、周期性和奇偶性",
    "复合函数、反函数、分段函数和隐函数",
    "基本初等函数的性质及其图形",
    "数列极限",
    "函数极限与左、右极限",
    "无穷小与无穷大",
    "无穷小的比较",
    "极限的四则运算法则",
    "两个重要极限",
    "极限存在的准则：单调有界准则和夹逼准则",
    "函数连续的概念与间断点类型",
    "闭区间上连续函数的性质",
    "导数和微分的概念",
    "求导法则：四则、复合、反函数、隐函数、参数方程、对数",
    "高阶导数",
    "微分中值定理",
    "洛必达法则",
    "函数单调性判别",
    "函数的极值",
    "函数图形的凹凸性、拐点与渐近线",
    "原函数与不定积分的概念与基本公式",
    "不定积分的换元积分法和分部积分法",
    "定积分的概念与基本性质",
    "微积分基本公式",
    "定积分的换元积分法和分部积分法",
    "反常积分",
    "定积分应用：平面图形的面积、旋转体的体积、弧长",
    "多元函数的概念",
    "多元函数的偏导数与全微分",
    "多元复合函数的求导法",
    "隐函数的求导法",
    "多元函数极值与条件极值",
    "二重积分",
    "二重积分的应用",
    "微分方程的基本概念",
    "可分离变量的微分方程",
    "一阶线性微分方程",
    "可降阶的高阶微分方程",
    "高阶线性微分方程",
    "常系数齐次线性微分方程",
    "常系数非齐次线性微分方程",
];

// ---------------- 高数（数三：含级数，略空间解析几何、三重/曲线曲面积分） ----------------
const GAODENG_3: &[&str] = &[
    "函数的概念及表示法",
    "函数的有界性、单调性、周期性和奇偶性",
    "复合函数、反函数、分段函数和隐函数",
    "基本初等函数的性质及其图形",
    "数列极限",
    "函数极限与左、右极限",
    "无穷小与无穷大",
    "无穷小的比较",
    "极限的四则运算法则",
    "两个重要极限",
    "极限存在的准则：单调有界准则和夹逼准则",
    "函数连续的概念与间断点类型",
    "闭区间上连续函数的性质",
    "导数和微分的概念",
    "求导法则：四则、复合、反函数、隐函数、参数方程、对数",
    "高阶导数",
    "微分中值定理",
    "洛必达法则",
    "函数单调性判别",
    "函数的极值",
    "函数图形的凹凸性、拐点与渐近线",
    "原函数与不定积分的概念与基本公式",
    "不定积分的换元积分法和分部积分法",
    "定积分的概念与基本性质",
    "微积分基本公式",
    "定积分的换元积分法和分部积分法",
    "反常积分",
    "定积分应用：平面图形的面积、旋转体的体积",
    "多元函数的概念",
    "多元函数的偏导数与全微分",
    "多元复合函数的求导法",
    "隐函数的求导法",
    "多元函数极值与条件极值",
    "拉格朗日乘数法",
    "二重积分",
    "无穷级数",
    "正项级数收敛性的判别法",
    "交错级数与绝对收敛、条件收敛",
    "幂级数",
    "幂级数的和函数",
    "函数的幂级数展开",
    "微分方程的基本概念",
    "可分离变量的微分方程",
    "一阶线性微分方程",
    "常系数线性微分方程",
];

// ---------------- 线代（三种版本范围一致；向量章按细分度展开） ----------------
const LINDAI: &[&str] = &[
    // 一、行列式
    "行列式的定义与性质",
    "行列式按行（列）展开",
    "克莱默法则",
    // 二、矩阵
    "矩阵的概念",
    "矩阵的线性运算",
    "矩阵的乘法",
    "矩阵的转置",
    "逆矩阵",
    "伴随矩阵",
    "矩阵的初等变换",
    "初等矩阵",
    "矩阵的等价",
    "矩阵的秩",
    // 三、向量
    "向量组的定义",
    "向量组及其线性组合",
    "向量组的线性表示",
    "向量组等价",
    "线性相关与线性无关",
    "极大线性无关组",
    "向量组的秩",
    "线性方程组解空间的判定与解向量",
    // 四、线性方程组
    "齐次线性方程组",
    "非齐次线性方程组",
    "线性方程组解的存在性判定",
    "基础解系",
    "齐次线性方程组的基础解系",
    "非齐次线性方程组的通解结构",
    "含参数的线性方程组讨论",
    // 五、特征值与特征向量
    "特征值与特征向量",
    "相似矩阵",
    "矩阵的相似对角化",
    "实对称矩阵的特征值与特征向量",
    // 六、二次型
    "二次型及其矩阵表示",
    "二次型的标准形",
    "二次型的规范性",
    "正定二次型与正定矩阵",
    "合同矩阵",
];

// ---------------- 概率论与数理统计（数一/数三） ----------------
const PROB: &[&str] = &[
    // 一、随机事件与概率
    "随机事件与样本空间",
    "事件的关系与运算",
    "概率的定义与基本性质",
    "古典概型",
    "几何概型",
    "条件概率与事件的独立性",
    "全概率公式与贝叶斯公式",
    // 二、随机变量及其分布
    "随机变量的分布函数",
    "离散型随机变量及其分布律",
    "连续型随机变量及其概率密度",
    "常见离散型随机变量",
    "常见连续型随机变量",
    "随机变量函数的分布",
    // 三、多维随机变量及其分布
    "二维随机变量及其联合分布",
    "边缘分布",
    "条件分布",
    "随机变量的独立性",
    "两个随机变量函数的分布",
    // 四、随机变量的数字特征
    "数学期望",
    "方差",
    "协方差与相关系数",
    "矩、协方差矩阵",
    // 五、大数定律和中心极限定理
    "切比雪夫不等式",
    "大数定律",
    "中心极限定理",
    // 六、数理统计的基本概念
    "总体与样本、统计量",
    "样本均值与样本方差",
    "正态总体的抽样分布",
    // 七、参数估计
    "点估计",
    "矩估计法",
    "最大似然估计法",
    "估计量的无偏性与有效性",
    "区间估计",
    // 八、假设检验
    "假设检验的基本概念",
    "正态总体的假设检验",
];

// ---------------- 政治（六大板块，按复习与考纲顺序；分组结构供内置进度表直接消费） ----------------
/// 内置分组考纲条目：板块（章节）→ 知识点列表。
///
/// 知识点为 `(标题, 建议预估小时)`；小时为 `0.0` 表示按标题特征自动估算
/// （见 `core::estimated_time::estimate_knowledge_hours`）。
///
/// 仅政治/英语提供分组结构：二者都是单版本科目，板块归属在编译期即可确定，
/// 无需运行时按关键词猜测（旧版 `chapter_for_politics` 关键词分组曾把大量
/// 马原/史纲知识点误归入兜底板块，导致章节顺序错乱，已废弃）。
pub struct BuiltinSection {
    /// 板块名（进度表章节节点标题）
    pub phase: &'static str,
    /// 板块下的知识点：(标题, 建议预估小时；0 = 自动估算)
    pub points: &'static [(&'static str, f64)],
}

pub const POLITICS_SECTIONS: &[BuiltinSection] = &[
    BuiltinSection {
        phase: "马克思主义基本原理",
        points: &[
            ("马克思主义的创立与发展", 0.0),
            ("马克思主义的鲜明特征", 0.0),
            ("辩证唯物论", 0.0),
            ("物质与意识的辩证关系", 0.0),
            ("世界的物质统一性", 0.0),
            ("唯物辩证法：对立统一规律", 0.0),
            ("质变与量变规律", 0.0),
            ("否定之否定规律", 0.0),
            ("联系与发展", 0.0),
            ("认识论：实践与认识的辩证关系", 0.0),
            ("真理与谬误", 0.0),
            ("真理的检验标准", 0.0),
            ("唯物史观：社会存在与社会意识", 0.0),
            ("社会基本矛盾运动", 0.0),
            ("人民群众与个人在历史中的作用", 0.0),
            ("商品与货币", 0.0),
            ("剩余价值理论", 0.0),
            ("资本主义经济制度", 0.0),
            ("垄断资本主义与国际经济关系", 0.0),
        ],
    },
    BuiltinSection {
        phase: "毛泽东思想和中国特色社会主义理论体系概论",
        points: &[
            ("新民主主义革命理论", 0.0),
            ("社会主义改造理论", 0.0),
            ("社会主义建设道路初步探索", 0.0),
            ("中国特色社会主义理论体系", 0.0),
            ("经济发展新常态与新发展理念", 0.0),
            ("全面深化改革", 0.0),
            ("社会主义民主政治", 0.0),
            ("文化建设与社会主义核心价值观", 0.0),
            ("民生与社会治理", 0.0),
            ("生态文明建设", 0.0),
            ("党的建设与全面从严治党", 0.0),
        ],
    },
    BuiltinSection {
        phase: "习近平新时代中国特色社会主义思想概论",
        points: &[
            ("新时代坚持和发展中国特色社会主义", 0.0),
            ("中国式现代化", 0.0),
            ("高质量发展与新质生产力", 0.0),
            ("新发展格局与改革开放", 0.0),
        ],
    },
    BuiltinSection {
        phase: "中国近现代史纲要",
        points: &[
            ("近代中国社会性质与主要矛盾", 0.0),
            ("旧民主主义革命", 0.0),
            ("新民主主义革命", 0.0),
            ("中华人民共和国的成立与社会主义制度的确立", 0.0),
            ("社会主义建设道路的探索", 0.0),
            ("改革开放与社会主义现代化建设", 0.0),
            ("中国特色社会主义进入新时代", 0.0),
        ],
    },
    BuiltinSection {
        phase: "思想道德与法治",
        points: &[
            ("人生观与人生价值", 0.0),
            ("理想信念", 0.0),
            ("道德观与职业道德", 0.0),
            ("法律观与法治思维", 0.0),
            ("宪法与全面依法治国", 0.0),
        ],
    },
    BuiltinSection {
        phase: "形势与政策以及当代世界经济与政治",
        points: &[
            ("当代世界经济与政治格局", 0.0),
            ("国际形势与中国外交", 0.0),
            ("时政热点", 0.0),
        ],
    },
];

// ---------------- 英语（七大模块，按学习顺序而非试卷题型顺序；训练单元粒度） ----------------
///
/// 旧版仅为 9 条题型占位（完形/阅读/翻译/作文/写作/词汇/长难句…），粒度是「题型清单」
/// 而非「进度表」：整表隐含总时长 ≈9h、章节顺序=试卷顺序（词汇排最后）、章节与知识点
/// 同名/同义重复。现按「模块 → 训练单元」重写为 50+ 条，模块顺序 = 备考学习顺序
/// （词汇/长难句打地基 → 阅读 → 写作 → 翻译/完形/新题型专项）。
/// 英一/英二共用（差异主要在新题型与作文形式，note 由 AI 生成表时区分）。
pub const ENGLISH_SECTIONS: &[BuiltinSection] = &[
    BuiltinSection {
        phase: "词汇",
        points: &[
            ("核心高频词汇 Unit 1", 2.0),
            ("核心高频词汇 Unit 2", 2.0),
            ("核心高频词汇 Unit 3", 2.0),
            ("核心高频词汇 Unit 4", 2.0),
            ("核心高频词汇 Unit 5", 2.0),
            ("核心高频词汇 Unit 6", 2.0),
            ("中频词汇 Unit 1", 2.0),
            ("中频词汇 Unit 2", 2.0),
            ("中频词汇 Unit 3", 2.0),
            ("低频词汇与熟词僻义", 2.0),
            ("词根词缀系统梳理", 1.5),
        ],
    },
    BuiltinSection {
        phase: "语法与长难句",
        points: &[
            ("五大基本句型与句子成分", 1.5),
            ("三大从句：定语、状语、名词性从句", 2.0),
            ("非谓语动词结构", 1.5),
            ("倒装、强调与分隔结构", 1.5),
            ("长难句拆解训练（一）", 1.5),
            ("长难句拆解训练（二）", 1.5),
            ("长难句拆解训练（三）", 1.5),
            ("真题长难句综合实战", 2.0),
        ],
    },
    BuiltinSection {
        phase: "阅读理解",
        points: &[
            ("阅读命题思路与六大题型概览", 1.5),
            ("细节题与推理题专项", 2.0),
            ("主旨题与态度题专项", 2.0),
            ("例证题与词义猜测题专项", 1.5),
            ("2010–2014 真题精读（一）", 2.5),
            ("2010–2014 真题精读（二）", 2.5),
            ("2010–2014 真题精读（三）", 2.5),
            ("2010–2014 真题精读（四）", 2.5),
            ("2015–2019 真题精读（一）", 2.5),
            ("2015–2019 真题精读（二）", 2.5),
            ("2015–2019 真题精读（三）", 2.5),
            ("2015–2019 真题精读（四）", 2.5),
            ("2020–2024 真题精读（一）", 2.5),
            ("2020–2024 真题精读（二）", 2.5),
            ("2020–2024 真题精读（三）", 2.5),
            ("最新真题限时模考与复盘", 2.5),
        ],
    },
    BuiltinSection {
        phase: "写作",
        points: &[
            ("小作文：书信与通知文体", 2.0),
            ("小作文：备忘录与告示", 1.5),
            ("大作文：图画/图表作文框架", 2.0),
            ("写作主题词汇与高分句型", 1.5),
            ("写作限时训练与批改（一）", 2.0),
            ("写作限时训练与批改（二）", 2.0),
            ("写作限时训练与批改（三）", 2.0),
        ],
    },
    BuiltinSection {
        phase: "翻译",
        points: &[
            ("翻译技巧：长句拆分与语序重组", 1.5),
            ("定语从句与被动语态翻译专项", 1.5),
            ("翻译真题逐句精练（一）", 2.0),
            ("翻译真题逐句精练（二）", 2.0),
        ],
    },
    BuiltinSection {
        phase: "完形填空",
        points: &[
            ("完形填空解题逻辑与高频考点", 1.5),
            ("逻辑衔接与词汇辨析专项", 1.5),
            ("完形真题限时训练（一）", 1.5),
            ("完形真题限时训练（二）", 1.5),
        ],
    },
    BuiltinSection {
        phase: "新题型",
        points: &[
            ("七选五与排序题解题方法", 1.5),
            ("新题型真题训练（一）", 1.5),
            ("新题型真题训练（二）", 1.5),
        ],
    },
];

/// 把分组考纲展开为扁平知识点顺序表（供 position/total_count/syllabus_points 使用）
fn flat_points(sections: &'static [BuiltinSection]) -> Vec<&'static str> {
    sections
        .iter()
        .flat_map(|s| s.points.iter().map(|(t, _)| *t))
        .collect()
}

/// 返回某科目的内置分组考纲（板块 → 知识点 + 建议时长），供内置进度表生成使用。
/// 仅政治/英语提供分组结构（数学为跨板块拼装，仍走 `chapter_for_math` 关键词分组）；
/// 未命中（未知科目/版本）返回 None。
pub fn builtin_sections(subject: &str, version: &str) -> Option<&'static [BuiltinSection]> {
    let v = version.trim();
    match subject {
        // 政治只有一套；空版本匹配，但禁止把非政治版本匹配进来
        "politics" if v.is_empty() || v.contains("政治") => Some(POLITICS_SECTIONS),
        // 英语一/二共用同一套分组；空版本匹配，但禁止把非英语版本匹配进来
        "english" if v.is_empty() || v.contains("英") => Some(ENGLISH_SECTIONS),
        _ => None,
    }
}

fn concat3(
    a: &'static [&'static str],
    b: &'static [&'static str],
    c: &'static [&'static str],
) -> Vec<&'static str> {
    let mut out = Vec::new();
    out.extend_from_slice(a);
    out.extend_from_slice(b);
    out.extend_from_slice(c);
    out
}

/// 起步构建一次：<subject_key, version_key, 有序条目>
static CHAPTER_SEQ: OnceLock<Vec<(String, String, Vec<&'static str>)>> = OnceLock::new();

fn tables() -> &'static Vec<(String, String, Vec<&'static str>)> {
    CHAPTER_SEQ.get_or_init(|| {
        vec![
            (
                "math".to_string(),
                "数一".to_string(),
                concat3(GAODENG_1, LINDAI, PROB),
            ),
            (
                "math".to_string(),
                "数二".to_string(),
                concat3(GAODENG_2, LINDAI, &[]),
            ),
            (
                "math".to_string(),
                "数三".to_string(),
                concat3(GAODENG_3, LINDAI, PROB),
            ),
            (
                "politics".to_string(),
                "".to_string(),
                flat_points(POLITICS_SECTIONS),
            ),
            (
                "english".to_string(),
                "".to_string(),
                flat_points(ENGLISH_SECTIONS),
            ),
        ]
    })
}

fn seq_for(subject: &str, version: &str) -> Option<&'static [&'static str]> {
    let v = version.trim();
    for (subj, ver, seq) in tables().iter() {
        if subj != subject {
            continue;
        }
        let maybe = match subj.as_str() {
            "math" => {
                ver == v
                    || (!v.is_empty() && v.contains(ver))
                    || (!ver.is_empty() && ver.contains(v))
            }
            // 英语一/二共用同一套顺序表；空版本也视为匹配，但禁止把非英语版本匹配进来
            "english" => ver.is_empty() || v.is_empty() || v.contains("英") || v.contains("英语"),
            // 政治只有一套顺序表；空版本匹配，但禁止把非政治版本匹配进来
            "politics" => ver.is_empty() || v.is_empty() || v.contains("政治"),
            _ => false,
        };
        if maybe {
            return Some(seq);
        }
    }
    None
}

/// 在当前版本顺序表中定位某文本（章节/知识点/任务标题）的位置。
/// 匹配策略：在与文本存在包含关系的条目里取**最长者**（最具体），避免短文本
/// 命中首个泛化条目导致定位偏早。未命中返回 None（视为无法确认顺序的内容，不做超前判定）。
pub fn position(subject: &str, version: &str, text: &str) -> Option<usize> {
    let seq = seq_for(subject, version)?;
    let norm = normalize(text);
    let mut contained_in_entry: Option<(usize, usize)> = None; // 条目 ⊆ 文本（任务标题包含知识条目），取最长条目
    let mut entry_contains_text: Option<(usize, usize)> = None; // 文本 ⊆ 条目（进度填简写「线性表示」→ 命中「向量组的线性表示」），取最长条目
    for (idx, entry) in seq.iter().enumerate() {
        let ent = normalize(entry);
        if ent.is_empty() {
            continue;
        }
        if norm.contains(&ent) && contained_in_entry.is_none_or(|(_, blen)| ent.len() > blen) {
            contained_in_entry = Some((idx, ent.len()));
        }
        if !norm.is_empty()
            && norm.len() >= 2
            && ent.contains(&norm)
            && entry_contains_text.is_none_or(|(_, blen)| ent.len() > blen)
        {
            entry_contains_text = Some((idx, ent.len()));
        }
    }
    // 文本是条目子串的匹配更具体，优先于条目是文本子串的匹配
    entry_contains_text.or(contained_in_entry).map(|(i, _)| i)
}

/// 返回某科目在当前版本顺序表中的条目总数（未命中返回 0）。
///
/// 用于按已完成内容在顺序表中的位置计算学习进度百分比，
/// 替代硬编码的「每科 50 章」估算。
pub fn total_count(subject: &str, version: &str) -> usize {
    seq_for(subject, version).map_or(0, |seq| seq.len())
}

/// 公开访问：返回某科目在指定版本下的内置考纲知识点顺序表（官方考研考纲）。
///
/// 供「进度表」的 AI 生成使用，作为不联网时的可靠考纲来源。
/// 未命中（未知科目/版本）返回 None。
pub fn syllabus_points(subject: &str, version: &str) -> Option<&'static [&'static str]> {
    seq_for(subject, version)
}

/// 判断某任务标题是否**早于**用户实际进度（即已学过、不应再排）。
pub fn is_ahead_of_progress(
    subject: &str,
    version: &str,
    task_title: &str,
    progress_chapter: &str,
) -> Option<bool> {
    let task_pos = position(subject, version, task_title)?;
    let prog_pos = position(subject, version, progress_chapter)?;
    Some(task_pos <= prog_pos)
}

fn normalize(s: &str) -> String {
    s.chars()
        .filter(|c| !c.is_whitespace())
        .map(|c| {
            if c.is_ascii() {
                c.to_ascii_lowercase()
            } else {
                c
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vector_fine_granularity() {
        // 向量章细分度：线性表示 < 等价 < 线性相关与线性无关 < 极大线性无关组 < 向量组的秩
        let a = position("math", "数二", "向量组的线性表示").unwrap();
        let b = position("math", "数二", "向量组等价").unwrap();
        let c = position("math", "数二", "线性相关与线性无关").unwrap();
        let d = position("math", "数二", "极大线性无关组").unwrap();
        let e = position("math", "数二", "向量组的秩").unwrap();
        assert!(a < b && b < c && c < d && d < e, "{a} {b} {c} {d} {e}");
    }

    #[test]
    fn linear_equations_after_vector() {
        // 「线性方程组」位于「向量」之后：任务不早于实际进度 → 不判超前
        let r = is_ahead_of_progress(
            "math",
            "数二",
            "线性方程组解的存在性判定",
            "向量组的线性表示",
        );
        assert_eq!(r, Some(false));
        // 行列式远早于向量进度 → 判超前（应剔除）
        let r2 = is_ahead_of_progress("math", "数二", "行列式", "极大线性无关组");
        assert_eq!(r2, Some(true));
    }

    #[test]
    fn num3_and_politics_resolve() {
        assert!(position("math", "数三", "正项级数收敛性的判别法").is_some());
        assert!(position("politics", "", "剩余价值理论").is_some());
    }

    #[test]
    fn politics_sections_grouping_is_correct() {
        let secs = builtin_sections("politics", "").unwrap();
        // 板块顺序 = 考纲顺序，形势与政策在最后（旧 bug：被关键词兜底顶到第 2 位）
        assert_eq!(secs.first().unwrap().phase, "马克思主义基本原理");
        assert_eq!(
            secs.last().unwrap().phase,
            "形势与政策以及当代世界经济与政治"
        );
        // 旧 bug 回归：以下马原知识点必须归属马原，不得落入「形势与政策」兜底
        let mayuan = &secs[0];
        for title in [
            "联系与发展",
            "质变与量变规律",
            "物质与意识的辩证关系",
            "真理与谬误",
            "社会基本矛盾运动",
        ] {
            assert!(
                mayuan.points.iter().any(|(t, _)| *t == title),
                "「{title}」应归属马原"
            );
        }
        // 旧 bug 回归：史纲知识点不得误入毛中特/习概
        let shigang = secs
            .iter()
            .find(|s| s.phase == "中国近现代史纲要")
            .expect("应有史纲板块");
        for title in [
            "新民主主义革命",
            "旧民主主义革命",
            "社会主义建设道路的探索",
            "中国特色社会主义进入新时代",
        ] {
            assert!(
                shigang.points.iter().any(|(t, _)| *t == title),
                "「{title}」应归属史纲"
            );
        }
        // 扁平顺序表与分组结构一致（position/total_count 消费）
        let flat_total: usize = secs.iter().map(|s| s.points.len()).sum();
        assert_eq!(total_count("politics", ""), flat_total);
    }

    #[test]
    fn english_sections_learning_order_and_granularity() {
        let secs = builtin_sections("english", "英一").unwrap();
        // 模块顺序 = 学习顺序：词汇打地基在最前，专项（完形/新题型）在最后
        assert_eq!(secs.first().unwrap().phase, "词汇");
        assert_eq!(secs.last().unwrap().phase, "新题型");
        // 粒度：训练单元 ≥ 40 条（旧版仅 9 条题型占位，无法支撑长期打卡）
        let total: usize = secs.iter().map(|s| s.points.len()).sum();
        assert!(total >= 40, "英语内置表训练单元应 ≥40，实际 {total}");
        // 无重复知识点标题
        let mut seen = std::collections::HashSet::new();
        for s in secs {
            for (t, _) in s.points {
                assert!(seen.insert(*t), "英语知识点标题重复: {t}");
            }
        }
        // 建议时长落在隐藏预估时长的合法区间
        for s in secs {
            for (_, h) in s.points {
                assert!(
                    (0.5..=4.0).contains(h),
                    "「{}」建议时长 {h} 超出 [0.5, 4.0]",
                    s.phase
                );
            }
        }
        // 扁平顺序表与分组结构一致
        assert_eq!(total_count("english", "英二"), total);
    }

    #[test]
    fn builtin_sections_rejects_cross_subject_versions() {
        assert!(builtin_sections("politics", "英一").is_none());
        assert!(builtin_sections("english", "政治").is_none());
        assert!(builtin_sections("math", "").is_none());
        assert!(builtin_sections("politics", "").is_some());
        assert!(builtin_sections("english", "英二").is_some());
    }
}
