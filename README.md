# StudyAgent

考研 AI 学习助手 — 基于 AI 的智能学习规划与复盘桌面客户端。

> 应用正处于快速迭代阶段，可能存在一些问题，但每一次更新都在持续打磨与修复。建议保持应用更新到最新版本，以便第一时间体验新功能与各类修复。可通过应用内「设置 → 检查更新」或在 [Releases](https://github.com/Wenjiugugugu/StudyAgent-client/releases) 页面下载最新安装包。

## 功能概览

### 计划
- **今日计划** — 围绕「今天具体学什么」的左右两栏视图：左侧按学科分组的紧凑任务列表，右侧今日概览、学科时间分配与任务状态；任务只保留「完成」与「计时」两个操作
- **目标计划** — 按学科 + 书/板块设置「截止日期 + 目标章节」，后端按考纲顺序确定性倒排每日任务（AI 仅参与估时）；同一学科的多本书可并行推进，复盘后按实际进度与目标差距重排
- **历史计划** — 日历视图按月/周回顾过往计划

### 进度管理
- **进度表** — 以「章节 → 知识点」两级树维护长期学习进度，知识点五级状态（待学 → 学习中 → 基础 → 强化中 → 掌握）点击即可推进
- **内置考纲一键生成** — 覆盖数学（数一/二/三）、英语（英一/二）、政治与 11 类全国统考专业课（408、法硕、311、312、313、333、306、307、396、199、农学门类），无需联网或配置 AI；专业课按教材拆分独立进度表
- **AI 生成 / 多表管理 / 导入导出** — AI 按章节顺序生成进度节点（失败自动回退内置考纲），多表并存自由切换，支持 JSON 导入导出与分享
- **智能联动** — 任务完成、复盘反馈与次日计划自动推进知识点状态；计划外学习以「进度指针」补记整段进度

### 自适应计划引擎
- **长期自适应** — 持续记录实际学习容量、任务量反馈与各科估时误差，任务量与预估时长随使用逐渐贴近实际情况
- **完成率信号 v2** — 以最近 5 个有效学习日的加权窗口（越近权重越高）结合回升趋势、连续达标天数与精力闸门判定，单日失手不会拖低整周任务量，连续达标逐级加量
- **每科保底** — 任务量下调时每个学科每天至少保留一条任务；计划被裁剪或目标生成失败时，今日计划页会说明原因
- **学科时间分配** — 设置每日总学习时长后用滑块调整各科占比，实时等比联动始终保持 100%，直接驱动周计划生成与复盘重排

### 复盘总结
- **日复盘** — 记录完成内容与困难反馈，提交后 AI 自动调整剩余计划；计划外学习可直接关联进度表
- **学习分析** — 完成率 / 学习量 / 复盘质量 / 周期对比四维图表

### 学习执行
- **专注（番茄钟）** — 倒计时 / 正计时、休息 / 长休息循环、结束后系统通知，专注记录持久化

### 个性化设置
- **多 AI Provider** — 支持 OpenAI / Gemini / Anthropic / Ollama / OpenRouter / 硅基流动 / 通义千问 / 火山引擎 / 自定义；还可**按功能指定服务商**——进度表生成、每日简报等时效敏感功能可单独交给更合适的模型
- **学习时间配置** — 每日学习起止时间、休息日、任务粒度；每日任务数由「每日目标学时 ÷ 粒度」自动派生
- **科目管理** — 设置各科开始日期，未开始科目不排任务
- **外观个性化** — 深浅主题、液态玻璃视觉模式、可收起侧边栏、自定义背景图与主色调
- **数据安全** — API Key 存储于系统凭据库，支持数据备份 / 导入 / 导出
- **检查更新** — 一键检查新版本

## 技术栈

| 层 | 技术 |
|---|---|
| 桌面框架 | Tauri 2 |
| 前端框架 | Vue 3 + TypeScript |
| 构建工具 | Vite 6 |
| 状态管理 | Pinia |
| 路由 | Vue Router 4 |
| UI 图标 | Lucide Icons |
| 图表 | ECharts |
| 公式渲染 | KaTeX |
| 日期处理 | date-fns |

## 快速开始

### 环境要求

- Node.js 18+
- Rust toolchain (仅 Tauri 构建时需要)
- Inno Setup 6 (仅打包 Windows 安装程序时需要)
- Windows 10/11

### 安装依赖

```bash
pnpm install
```

### 开发模式 (浏览器 + Mock 数据)

无需 Rust，直接在浏览器中开发，所有 API 返回 Mock 数据：

```bash
pnpm dev
```

访问 `http://localhost:1420`

### 类型检查

```bash
pnpm type-check
```

### Tauri 开发模式

需要安装 Rust toolchain：

```bash
pnpm tauri:dev
```

### 构建安装包

Windows 安装程序由 [Inno Setup 6](https://jrsoftware.org/isdl.php) 生成（Tauri 内置的 NSIS / MSI 打包已关闭）：

```powershell
pnpm install
.\installer\build.ps1          # 已构建过 Rust 时加 -SkipBuild 只重新打包
```

- `build.ps1` 先执行 `pnpm tauri build` 产出 `src-tauri/target/release/studyagent-desktop.exe`，
  再调用 `ISCC.exe` 编译 `installer/StudyAgent.iss`。
- 产物：`src-tauri/target/release/bundle/inno/StudyAgent_<version>_x64-setup.exe`

安装程序源码：

```
installer/                    # Windows 安装程序（Inno Setup）
├── StudyAgent.iss            #   安装脚本：安装模式 / 快捷方式 / 卸载与数据清理
├── build.ps1                 #   一键构建：tauri build → 暂存 → ISCC 编译
└── assets/                   #   向导位图与生成脚本（generate_assets.py）
```

## 项目结构

```
src/                          # 前端源码
├── api/                      #   API 服务层 (统一入口，兼容层)
├── features/                 #   领域目录（按业务领域组织）
│   ├── dashboard/            #     工作台（首页）
│   ├── settings/             #     设置页
│   ├── tour/                 #     首次引导
│   └── debug/                #     调试页
├── components/               #   可复用组件
│   ├── ui/                   #     基础 UI 组件
│   └── progress/             #     进度表组件
├── composables/              #   组合式函数
├── layouts/                  #   布局组件
├── router/                   #   路由配置
├── stores/                   #   Pinia 状态管理
├── styles/                   #   全局样式
├── types/                    #   跨领域共享类型定义
├── changelogs/               #   内置版本更新日志
├── views/                    #   页面视图（薄壳：仅路由入口 + 页面组合）
├── App.vue                   #   根组件
└── main.ts                   #   入口

src-tauri/                    # Tauri 后端 (Rust)
├── src/
│   ├── ai/                   #   AI Provider 服务（支持按功能路由）
│   ├── api/                  #   Tauri 命令
│   ├── core/                 #   业务逻辑 (规划/调度/自适应/复盘等)
│   ├── data/                 #   数据层（本地 JSON 读写）
│   └── tools/                #   MCP Tool 调度
├── tests/                    #   集成测试（基于仓库外 demo-data，缺失时自动跳过）
├── Cargo.toml
├── tauri.conf.json
└── capabilities/             #   权限配置
```

### 领域目录约定

- **views/** 只负责路由入口和页面布局，不承载业务逻辑；实际页面在 `features/<domain>/` 下。
- 每个 feature 以 `api.ts` 作为公开入口，页面/组件/composable 通过它调用后端，避免直接耦合全局 `@/api`。
- 调用链：`组件 → composable → store → feature api → api/index.ts → Tauri`。
- 跨领域调用走公开 API 或 Store selector，不直接访问另一个 feature 的内部文件。

## 设计风格

遵循 Apple HIG，强调简洁克制。浅色/深色双主题，CSS 变量驱动，10-16px 圆角，柔和阴影，Lucide 图标统一风格。
