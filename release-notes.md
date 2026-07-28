### 修复
- 修复今日计划一打开就出现任务已完成的问题
- DailyScheduler 生成日计划时无条件重置 current_task，所有任务状态初始为 Pending
- 防止旧版本遗留的错位 task_id / done 状态被带到新一天计划

### 安装包
- NSIS: StudyAgent_0.2.4_x64-setup.exe
- MSI: StudyAgent_0.2.4_x64_en-US.msi
