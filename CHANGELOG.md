# Changelog

所有 StudyAgent Desktop 的显著变更都记录在此文件。

格式基于 [Keep a Changelog](https://keepachangelog.com/zh-CN/1.1.0/)，版本号遵循 [Semantic Versioning](https://semver.org/lang/zh-CN/)。

## [0.3.1] - 2026-08-02

本次版本整合了学习时长体验打磨、休息日提示、AI 用量追踪以及安装包品牌化升级。

### Added

- 休息日提示：TodayView 和 DashboardView 在休息日不再显示"今日无计划"和生成计划入口，改为展示"今日是休息日"提示，并显示休息日名称。
- AI 用量日志持久化（后端）：每次 AI 调用后自动记录 token 消耗（输入/输出 token、总 token）、耗时、模型名、Agent 类型到 `state/ai_usage_log.json`，重启后不丢失，最多保留 500 条。
  - 新增 `data::ai_usage` 模块，提供 `append`、`read_all`、`clear` 接口。
  - 新增 `get_ai_usage_log`、`clear_ai_usage_log` Tauri 命令。
- AI 调用记录持久化（前端）：`aiDebug` store 通过 localStorage 持久化已完成记录，最多保留 30 条，重启后不丢失。
- AI 用量与费用估算：调试页面新增「AI 用量日志」区块，展示历史调用明细、汇总统计（调用次数、输入/输出 token、总耗时），并根据各厂商官方定价（DeepSeek、通义千问、智谱 GLM、Kimi、OpenAI、Claude、Gemini）估算人民币费用。
  - 新增 `utils/aiPricing.ts` 定价表模块，按模型名模糊匹配，支持 USD 按汇率折算人民币。
  - 支持按时间范围筛选（全部/24h/7天/30天），按模型和 Agent 类型分组汇总。
- 自定义应用背景图：在「设置 → 外观」中可上传本机图片作为应用背景，支持调整模糊度（0-20px）与不透明度（10%-100%）；图片保存在应用数据目录，重启后自动加载。
- 安装包品牌化升级：NSIS 安装器侧栏与页眉替换为 StudyAgent 品牌视觉（品牌蓝渐变 + App 图标 + "AI 学习工作台" 标语），安装界面文案中文化。

### Changed

- 移除定价表中已停用的 DeepSeek R1（deepseek-reasoner）和 DeepSeek Chat（deepseek-chat）条目。
- 学习时长展示与「记录学习时长」设置联动：关闭该设置时，每日计划、周计划、历史计划和分析页均隐藏学习时长相关信息；开启时每日计划展示 AI 估时，复盘展示估时与实际用时。
- 首页工作台「今日焦点」：当今日计划全部完成后，展示「今日计划已全部完成」提示，不再显示已完成的任务卡。
- 历史计划与周计划中，未到达的日期展示「未开始」而非「未复盘」，只有当天及已过去的日期才显示「未复盘」。

### Fixed

- 修复复盘提交时 `data.total_hours` 被 `Default::default()` 覆盖为 0 的问题，学习时长统计现在会正确聚合各任务的实际学习分钟数。
- 为旧复盘文件增加 `review_actual_hours()` 兼容读取逻辑，历史学习时长无需手动修正。
- 修复历史计划与未来计划视图中未来日期状态显示错误的问题。

### Engineering

- 前端 `vue-tsc --noEmit` 类型检查通过。
- 后端 `cargo check` 编译通过。
- 构建并生成 Windows 安装包 `StudyAgent_0.3.1_x64-setup.exe`。
