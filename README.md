# StudyAgent

考研 AI 学习助手 — 基于 AI 的智能学习规划与复盘桌面客户端。

> 应用正处于快速迭代阶段，可能存在一些问题，但每一次更新都在持续打磨与修复。建议保持应用更新到最新版本，以便第一时间体验新功能与各类修复。可通过应用内「设置 → 检查更新」或在 [Releases](https://github.com/Wenjiugugugu/StudyAgent-client/releases) 页面下载最新安装包。

## 功能概览

### 智能规划
- **日计划生成** — 根据科目进度、知识图谱和剩余时间，AI 自动生成每日学习计划
- **周计划生成** — 参考前一周的计划与复盘，动态调整任务量
- **实时调整** — 支持当天随时调整计划任务

### 学习执行
- **今日学习** — 任务开始/暂停/完成，每项任务含目标、完成标准、教材、风格提示、备选方案
- **科目进度追踪** — 数学(紫)、英语(橙)、政治(粉)、专业课(蓝) 分科展示进度
- **知识库** — 搜索知识点、依赖关系、教材、真题，支持 LaTeX 渲染
- **AI 助手** — 侧边栏 AI 面板，基于当前页面上下文提供答疑

### 复盘总结
- **日复盘** — 记录完成内容、遇到的困难
- **历史复盘** — 查看过去每日的复盘记录
- **历史计划** — 日历视图按月/周浏览过往计划

### 个性化设置
- **多 AI Provider** — 支持 OpenAI / Gemini / Anthropic / Ollama / OpenRouter / 硅基流动 / 通义千问 / 火山引擎 / 自定义
- **学习时间配置** — 设置每日学习起止时间、每日任务数(1-8)、休息日
- **科目管理** — 设置各科开始日期，未开始科目不排任务
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
| 日期处理 | date-fns |

## 快速开始

### 环境要求

- Node.js 18+
- Rust toolchain (仅 Tauri 构建时需要)
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

```bash
pnpm tauri:build
```

生成的安装包位于 `src-tauri/target/release/bundle/`。

## 项目结构

```
src/                          # 前端源码
├── api/                      #   API 服务层 (统一入口)
├── components/               #   可复用组件
│   ├── ui/                   #     基础 UI 组件
│   └── assistant/            #     AI 助手面板
├── composables/              #   组合式函数
├── layouts/                  #   布局组件
├── router/                   #   路由配置
├── stores/                   #   Pinia 状态管理
├── styles/                   #   全局样式
├── types/                    #   TypeScript 类型定义
├── views/                    #   页面视图
├── App.vue                   #   根组件
└── main.ts                   #   入口

src-tauri/                    # Tauri 后端 (Rust)
├── src/
│   ├── ai/                   #   AI Provider 服务
│   ├── api/                  #   Tauri 命令
│   ├── core/                 #   业务逻辑 (规划/复盘/调度等)
│   └── tools/                #   MCP Tool 调度
├── Cargo.toml
├── tauri.conf.json
└── capabilities/             #   权限配置
```

## 设计风格

遵循 Apple HIG，强调简洁克制。浅色/深色双主题，CSS 变量驱动，10-16px 圆角，柔和阴影，Lucide 图标统一风格。
