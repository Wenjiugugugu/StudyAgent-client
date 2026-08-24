# 开发流程规范

本仓库采用 **基于功能分支 + 受保护 main** 的协作模型。核心原则：

- **main 永远保持可用**：main 上的任何提交都必须是可直接发布、可运行的稳定状态。
- **每任务一分支**：每个任务/需求在独立分支上开发，通过 Pull Request（PR）合入 main。
- **禁止直接向 main 推送**：main 通过分支保护规则强制 PR + 通过 CI 检查才能合并。

---

## 1. 分支模型

```
main                         ← 唯一长期分支，始终可用、只收 PR
 └─ feature/人名-任务         ← 功能/需求分支
 └─ fix/人名-bug描述          ← 缺陷修复分支
 └─ docs/人名-文档任务         ← 文档改动分支
 └─ refactor/人名-重构任务     ← 重构分支
 └─ chore/人名-杂项            ← 依赖升级、配置、CI 等
```

## 2. 分支命名

分支一律使用 `类型/人名-任务` 格式：

| 类型 | 用途 | 示例 |
|---|---|---|
| `feature/` | 新功能 / 需求 | `feature/zhangsan-周计划调整` |
| `fix/` | Bug 修复 | `fix/lisi-手机号校验` |
| `docs/` | 文档 | `docs/wangwu-接口文档` |
| `refactor/` | 重构、无行为变化 | `refactor/zhaoliu-planner拆分` |
| `chore/` | 依赖 / 配置 / CI | `chore/tianqi-升级tauri` |

要求：
- `人名` 使用自己姓名（可用中文或拼音）。
- `任务` 用简短、描述性的词，不要过长的句子。
- 同一人并行多任务时，用 `feature/人名-任务A`、`feature/人名-任务B` 区分。

## 3. 开发流程

### 3.1 开始一个任务

```bash
# 确保本地 main 是最新且干净的
git checkout main
git pull origin main

# 从最新的 main 开出任务分支
git checkout -b feature/zhangsan-周计划调整
```

> 分支只从最新的 `origin/main` 创建，避免基于过期代码开发。

### 3.2 提交规范

- 提交消息建议遵循 `类型: 简述`（可参考 [Conventional Commits](https://www.conventionalcommits.org/zh-hans/)），例如 `feat: 调整周计划任务量`、`fix: 修复日期越界`、`docs: 补充贡献指南`。
- 提交保持小而聚焦，一个提交对应一个逻辑改动，便于 review 与回滚。
- 不要提交构建产物、本地缓存、密钥等（相关路径已由 `.gitignore` 排除）。

### 3.3 同步与冲突

分支开发期间 main 可能有更新，及时合并避免冲突越积越大：

```bash
git fetch origin
git rebase origin/main   # 或 git merge origin/main
```

### 3.4 提交 Pull Request

1. 推送分支：`git push origin feature/zhangsan-周计划调整`
2. 在 GitHub 新建 PR → base 为 `main`，写好标题与描述（说明改动、测试情况、相关 issue）。
3. PR 的 CI 检查（前端 type-check/test/build + Rust fmt/clippy/test）必须全部通过。
4. 至少一名协作者 review 通过后方可合并。
5. 合并后**删除分支**，再立即 `git pull origin main` 保持本地同步。

### 3.5 合并策略

- 使用 **Squash and merge** 或保持清晰历史的 **Merge commit**，在 PR review 讨论中统一一种并在项目设置中固定。
- 合并前再次确认分支已包含最新的 `origin/main`，避免合并后 main 变红。

---

## 4. main 分支保护

`main` 已在 GitHub 侧开启分支保护，规则如下：

- 禁止直接推送（force push 一律禁止）。
- 要求 PR status check 通过后才能合并（CI 工作流）。
- 合并前必须解决冲突。

如需调整保护规则，需仓库管理员权限在 **Settings → Branches** 修改。

## 5. 注意事项

- **不要**在 main 上直接开发或提交任何临时改动；临时想法统一放分支。
- 发现 main 被意外推送了不可用改动时，第一时间通知维护者回滚。
- PR 保持范围单一：别把多个无关任务塞进一个 PR。