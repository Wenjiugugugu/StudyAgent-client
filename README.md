# StudyAgent Desktop

考研 AI 学习工作台 (Learning Workspace) — 本地运行的 Windows 桌面客户端。

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
cd desktop
npm install
```

### 开发模式 (浏览器 + Mock 数据)

无需 Rust，直接在浏览器中开发，所有 API 返回 Mock 数据：

```bash
npm run dev
```

访问 `http://localhost:1420`

### 类型检查

```bash
npm run type-check
```

### Tauri 开发模式

需要安装 Rust toolchain：

```bash
npm run tauri:dev
```

### 构建 EXE

```bash
npm run tauri:build
```

生成的安装包位于 `src-tauri/target/release/bundle/`。

## 目录结构

```
desktop/
├── src/                         # 前端源码
│   ├── api/                     # API 服务层 (统一入口)
│   │   ├── index.ts             #   所有 API 函数导出
│   │   ├── tauri.ts             #   Tauri invoke 封装 + 浏览器回退
│   │   └── mock-data.ts         #   Mock 数据 (开发用)
│   ├── components/              # 可复用组件
│   │   ├── ui/                  #   基础 UI 组件
│   │   │   ├── Card.vue
│   │   │   ├── Button.vue
│   │   │   ├── Badge.vue
│   │   │   ├── ProgressBar.vue
│   │   │   ├── LoadingSpinner.vue
│   │   │   └── EmptyState.vue
│   │   └── assistant/           #   AI 助手组件
│   │       └── AssistantPanel.vue
│   ├── composables/             # 组合式函数
│   │   └── useTheme.ts          #   主题管理
│   ├── layouts/                 # 布局组件
│   │   ├── AppLayout.vue        #   主布局
│   │   └── SideBar.vue          #   侧边导航
│   ├── router/                  # 路由配置
│   │   └── index.ts
│   ├── stores/                  # Pinia 状态管理
│   │   ├── settings.ts          #   设置
│   │   ├── dashboard.ts         #   仪表盘
│   │   ├── today.ts             #   今日计划
│   │   ├── assistant.ts         #   AI 助手
│   │   ├── knowledge.ts         #   知识库
│   │   └── review.ts            #   复盘
│   ├── styles/                  # 全局样式
│   │   ├── variables.css        #   CSS 设计系统变量
│   │   └── global.css           #   全局样式重置
│   ├── types/                   # TypeScript 类型定义
│   │   ├── index.ts             #   统一导出
│   │   ├── state.ts             #   StudyState
│   │   ├── plan.ts              #   DailyPlan / WeekPlan
│   │   ├── review.ts            #   ReviewRecord
│   │   ├── knowledge.ts         #   KnowledgeObject
│   │   ├── ai.ts                #   AI Provider / Chat
│   │   ├── mcp.ts               #   MCP Server
│   │   └── settings.ts          #   AppSettings
│   ├── views/                   # 页面视图
│   │   ├── DashboardView.vue    #   首页仪表盘
│   │   ├── TodayView.vue        #   今日学习
│   │   ├── WeekPlanView.vue     #   周计划
│   │   ├── KnowledgeView.vue    #   知识库
│   │   ├── ReviewView.vue       #   复盘
│   │   ├── SettingsView.vue     #   设置
│   │   ├── TimelineView.vue     #   时间线 (预留)
│   │   └── AnalyticsView.vue    #   分析 (预留)
│   ├── App.vue                  # 根组件
│   └── main.ts                  # 入口
├── src-tauri/                   # Tauri 后端 (Rust)
│   ├── src/                     #   Rust 源码
│   ├── Cargo.toml               #   Rust 依赖
│   ├── tauri.conf.json          #   Tauri 配置
│   ├── build.rs                 #   构建脚本
│   └── capabilities/            #   权限配置
├── index.html
├── vite.config.ts
├── tsconfig.json
├── package.json
└── postcss.config.cjs
```

## 架构设计

### 分层架构

```
┌──────────────────────────────────────┐
│           Vue Views (UI)             │  ← 仅负责展示和交互
├──────────────────────────────────────┤
│         Pinia Stores                 │  ← UI 状态 + API 调用
├──────────────────────────────────────┤
│         API Service Layer            │  ← 统一 API 入口
├────────────────────┬─────────────────┤
│   Tauri Commands   │   Mock Data     │  ← 双模式: Tauri / 浏览器
├────────────────────┴─────────────────┤
│       StudyAgent Core (Rust)         │  ← 业务逻辑 (待实现)
├──────────────────────────────────────┤
│     AI Service  │  Tool Dispatcher   │  ← Core 能力层
├─────────────────┴────────────────────┤
│  AI Providers   │  MCP Servers       │  ← 外部服务
└─────────────────┴────────────────────┘
```

### 核心原则

- **UI 不保存业务状态** — 所有数据来自 Core
- **前端不直接调用模型 API** — 通过 Core 的 AI Service
- **前端不直接调用 MCP** — 通过 Core 的 Tool Dispatcher
- **API 层统一入口** — 所有数据请求经过 `src/api/index.ts`

### API 双模式

API 层支持两种运行模式，通过 `isTauri()` 自动切换：

- **Tauri 模式**: 调用 Rust 后端 `invoke()` 命令
- **浏览器模式**: 返回 Mock 数据，便于前端独立开发

```typescript
// src/api/index.ts 示例
export async function getTodayPlan(): Promise<DailyPlan> {
  return invokeWithFallback("get_today_plan", undefined, async () => mockTodayPlan);
}
```

### AI Provider 架构

采用 OpenAI Compatible 接口设计，支持以下 Provider：

| Provider | 类型标识 |
|---|---|
| OpenAI | `openai` |
| Gemini | `gemini` |
| Anthropic | `anthropic` |
| Ollama (本地) | `ollama` |
| OpenRouter | `openrouter` |
| 硅基流动 | `siliconflow` |
| 通义千问 | `dashscope` |
| 火山引擎 | `volcengine` |
| 自定义 | `custom` |

在 Settings 页面配置 Base URL / API Key / Model。

### MCP 架构

通过统一 Tool Layer 调度，新增 MCP 不需要修改业务逻辑：

```
Tool Dispatcher
├── TickTick MCP (滴答清单)
├── Filesystem MCP (文件系统)
├── Browser MCP (浏览器)
├── Obsidian MCP
└── ...
```

## 页面说明

| 页面 | 路由 | 说明 |
|---|---|---|
| Dashboard | `/dashboard` | 首页：今日计划、本周进度、学习阶段、统计 |
| Today | `/today` | 今日学习任务：开始/暂停/完成、DoD、关联知识 |
| Week Plan | `/week-plan` | 周计划：生成、目标、工作量、每日分配 |
| Knowledge | `/knowledge` | 知识库：搜索、依赖关系、教材、真题、笔记 |
| Review | `/review` | 复盘：总结、困难记录、原因分析、建议 |
| Timeline | `/timeline` | 时间线 (预留) |
| Analytics | `/analytics` | 分析 (预留) |
| Settings | `/settings` | 设置：AI Provider、MCP、TickTick、学习时间、主题 |

侧边栏还提供 AI 助手面板 (Assistant Panel)，基于当前页面上下文提供答疑。

## 设计系统

遵循 Apple Human Interface Guidelines，强调简洁、克制、高品质。

- 浅色/深色双主题
- CSS 变量驱动 (`src/styles/variables.css`)
- 10-16px 圆角
- 柔和阴影
- Lucide 图标统一风格
- 科目配色：数学(蓝)、英语(绿)、政治(橙)、专业课(紫)

## 后续开发指南

### 新增页面

1. 在 `src/views/` 创建 `XxxView.vue`
2. 在 `src/router/index.ts` 添加路由
3. 在 `src/stores/` 创建对应 Store (如需)
4. 在 `src/api/index.ts` 添加 API 函数

### 新增 AI Provider

1. 在 `src/types/ai.ts` 的 `ProviderType` 添加类型
2. 在 SettingsView 的 `providerTypeOptions` 添加选项
3. 在 Rust 后端实现对应的 API 调用逻辑

### 新增 MCP Server

1. 在 `src/types/mcp.ts` 的 `MCPServerType` 添加类型
2. 在 SettingsView 的 `mcpTypeOptions` 添加选项
3. 在 Rust 后端的 Tool Dispatcher 注册新 MCP

### 新增 UI 组件

在 `src/components/ui/` 创建组件，使用 CSS 变量保持主题一致。
