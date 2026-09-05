/**
 * StudyAgent — Mock Data
 * 基于真实 StudyAgent 数据结构，供浏览器开发模式使用
 */

import type {
  StudyState,
  DailyPlan,
  ReviewRecord,
  DashboardSummary,
  AppSettings,
  AIProviderConfig,
  MCPServerConfig,
  MCPServerStatus,
} from "@/types";

/** Mock 学习状态 */
export const mockState: StudyState = {
  meta: {
    last_updated: "2026-07-24",
    exam_date: "2026-12-20",
    target_school: "广东工业大学",
    target_major: "计算机技术/人工智能",
  },
  subjects: {
    politics: {
      active: false,
      phase: "foundation",
      target_score: 70,
      current_score: 0,
      weekly_hours: 0,
      weak_chapters: [],
      strong_chapters: [],
      completed: [],
      current_focus: "",
    },
    english: {
      active: true,
      phase: "foundation",
      target_score: 75,
      current_score: 0,
      weekly_hours: 7,
      weak_chapters: ["阅读真题（已推进至第8篇）"],
      strong_chapters: ["单词基础", "长难句拆解"],
      completed: ["红宝书重点词", "阅读真题第1-8篇"],
      current_focus: "阅读真题第8篇完成，7/24 推进第9篇。单词持续维护。",
    },
    math: {
      active: true,
      phase: "foundation",
      version: "数二",
      textbook: "张宇系列（高等数学18讲 + 题源1000题）",
      target_score: 120,
      current_score: 0,
      weekly_hours: 14,
      weak_chapters: ["线代（全部未开）", "多元微分"],
      strong_chapters: ["极限与连续", "一元微积分"],
      completed: ["高数上册", "二重积分", "微分方程·齐次+一阶线性"],
      current_focus: "线代第一章启动（7/24），高数基础阶段闭环。",
    },
    professional: {
      active: true,
      name: "408计算机综合",
      phase: "foundation",
      target_score: 110,
      current_score: 0,
      weekly_hours: 14,
      weak_chapters: ["OS", "计网"],
      strong_chapters: ["数据结构", "计组（定点数/浮点数/存储系统）"],
      completed: ["数据结构（王道全本）", "计组第2-4章"],
      current_focus: "计组 4.3 指令格式扩展推进中",
      textbook: "王道考研系列",
    },
  },
  current_task: {
    date: "2026-07-24",
    focus: "线代启动日 + 计组4.3推进 + 英语维持节奏",
    total_hours: 4.5,
    tasks: [
      { subject: "math", task: "线代第一章 行列式 启动", priority: "A", status: "in_progress" },
      { subject: "professional", task: "计组4.3 指令格式扩展·推进", priority: "A", status: "pending" },
      { subject: "english", task: "阅读真题第9篇 + 250词 + 长难句1句", priority: "A", status: "pending" },
    ],
    note: "今天是线代启动日，高数基础阶段阻塞解除。",
  },
  risks: {
    items: [
      {
        subject: "math",
        level: "high",
        description: "线代新课首日，2.0h硬约束不可压缩",
        suggested_action: "其他科目让位，正课+1道例题+笔记是底线",
      },
      {
        subject: "overall",
        level: "medium",
        description: "数学连续2日单科高投入疲劳",
        suggested_action: "监控19:00前完成情况，若超时即截断",
      },
    ],
  },
  user_model: {
    preferred_study_time: "下午",
    avg_focus_hours_per_day: 5.5,
    best_subjects: ["数学（计算能力）", "数据结构"],
    worst_subjects: ["操作系统（未启动）", "计算机网络（未启动）"],
    learning_style: "ls-001 例子驱动型 active",
    common_error_types: [
      "计组3.2: 地址线编址概念未真懂",
      "多元微分: 复合函数求导不熟练",
    ],
    review_compliance_rate: 1.0,
  },
  progress: {
    total_study_days: 123,
    last_study_date: "2026-07-23",
    streak_days: 2,
    total_practice_questions: 111,
    note: "7/23 复盘完成，微分方程综合收口闭环，线代启动计划解冻。",
  },
};

/** Mock 今日计划 */
export const mockTodayPlan: DailyPlan = {
  version: "1.0.0",
  meta: {
    date: "2026-07-24",
    generated_at: "2026-07-24T08:30",
    type: "daily",
    based_on: {
      state: "state/current.state",
      user_model: "assets/user_model/_index.md",
      exam_config: "assets/config/exam-config.md",
      review_ref: "records/2026-07-23_review.json",
      week_plan: "plan/2026-W30_week.json",
    },
  },
  data: {
    remaining_days: 149,
    target: "广东工业大学 计算机技术/人工智能 | 总分 375 / 500",
    strategy: "上午数学，下午专业课，晚上英语",
    tasks: [
      {
        id: "2026-07-24-01",
        subject: "math",
        title: "线代第一章 行列式 启动（张宇线代9讲 第1讲）",
        priority: "A",
        estimated_hours: 2.0,
        goal: "正式启动线代，第1讲行列式是基础。首次启动聚焦概念建立+1道例题。",
        completion_criteria: [
          "张宇线代9讲第1讲视频/正课1节",
          "沉淀笔记：行列式5大性质对照表+展开定理公式",
          "例题1道独立做",
          "课后选择题3-5道",
        ],
        textbook: "张宇线代9讲第1讲",
        style_tips: "ls-001 例子驱动型 — 视频看完先做例题再总结",
        fallback_plan: "若2.0h内未完成，先保正课+笔记闭环（1.5h必杀），例题拆为明天A级",
        status: "in_progress",
      },
      {
        id: "2026-07-24-02",
        subject: "professional",
        title: "计组 4.3 指令格式扩展·推进",
        priority: "A",
        estimated_hours: 1.5,
        goal: "4.3新课框架已建立，今日推进剩余内容：扩展操作码设计+指令操作类型。",
        completion_criteria: [
          "王道计组4.3剩余视频/正课1节",
          "课后选择题完成5-6道",
          "沉淀笔记：扩展操作码设计2-3种方案对比+指令操作类型分类表",
        ],
        textbook: "王道计组 4.3 节",
        style_tips: "obs-004 以题代学偏好 — 做题比看书更易掌握",
        fallback_plan: "1.5h总投入，今日先把4.3剩余内容学完，4.4留到明日",
        status: "pending",
      },
      {
        id: "2026-07-24-03",
        subject: "english",
        title: "英语阅读真题第9篇 + 250词 + 长难句1句",
        priority: "A",
        estimated_hours: 1.0,
        goal: "维持每日1篇+250词+1句长难句节奏，不增量。",
        completion_criteria: [
          "精读第9篇文章（查词+理解+标记生词）",
          "答题",
          "250词背诵",
          "1句长难句拆解",
        ],
        textbook: "考研英语阅读真题第9篇、红宝书单词",
        status: "pending",
      },
    ],
    risks: [
      {
        subject: "math",
        item: "线代新课首日 vs 数学时长不足",
        level: "high",
        suggestion: "2.0h硬约束，不压缩；其他科目让位；正课+1道例题+笔记是底线",
      },
      {
        subject: "overall",
        item: "数学连续2日单科高投入疲劳",
        level: "medium",
        suggestion: "监控19:00前完成情况，若超时即截断",
      },
      {
        subject: "professional",
        item: "4.3推进 vs 线代冲突",
        level: "medium",
        suggestion: "4.3仅1.5h，不应挤压数学时间",
      },
    ],
    style_tips: [
      "ls-001 例子驱动型 active：线代第1讲看完正课先做1道例题再总结",
      "obs-005 韧性重启：7/23微分方程综合收口成功验证scope裁剪+单科聚焦模式有效",
    ],
    after_today: "线代第一章启动成功，明日可推进第2讲行列式综合计算。4.3收口后7/25可开4.4。",
    reminders: [
      "按重要性排序：线代第一章启动 > 计组4.3推进 > 英语阅读",
      "数学A级2.0h是硬约束",
      "今日19:00前A级3项全完成，触发复盘技能",
    ],
    total_hours: 4.5,
    total_tasks: 3,
  },
  view: "# 2026-07-24 学习计划\n\n## 目标\n广东工业大学 计算机技术/人工智能 | 总分 375 / 500\n\n## 策略\n上午数学，下午专业课，晚上英语\n\n## 核心任务\n1. **数学** 线代第一章行列式启动（2.0h）\n2. **专业课** 计组4.3指令格式扩展推进（1.5h）\n3. **英语** 阅读真题第9篇 + 250词 + 长难句（1.0h）\n",
};

/** Mock 复盘记录 */
export const mockReview: ReviewRecord = {
  version: "1.0.0",
  meta: {
    date: "2026-07-23",
    type: "review",
    plan_ref: "plan/2026-07-23_day.json",
    generated_at: "2026-07-23T23:00",
  },
  data: {
    completed_tasks: [
      {
        task_id: "2026-07-23-01",
        subject: "math",
        title: "微分方程·综合收口（数二范围：可分离+齐次+一阶线性三类各2题共6题+判别流程表）",
        priority: "A",
        completed: true,
        completion_time: "17:42",
        note: "该任务从7/20→7/21→7/22连续三次延期后，今日终于闭环。",
      },
      {
        task_id: "2026-07-23-02",
        subject: "math",
        title: "多元微分·链式法则回顾（防遗忘）",
        priority: "B",
        completed: true,
        completion_time: "17:42",
        note: "连续6日未启动后今日重启成功",
      },
      {
        task_id: "2026-07-23-03",
        subject: "professional",
        title: "计组4.3指令格式扩展·新课启动",
        priority: "A",
        completed: true,
        completion_time: "14:20",
        note: "新课第一天框架建立",
      },
      {
        task_id: "2026-07-23-04",
        subject: "english",
        title: "阅读真题第8篇精读+250词+长难句1句",
        priority: "A",
        completed: true,
        completion_time: "14:24",
        note: "每日一篇节奏维持",
      },
    ],
    unplanned_tasks: [],
    difficulties: [
      {
        description: "微分方程综合收口此前连续3次延期，根因是任务scope过大",
        root_cause: "原含伯努利/全微分四类8题，scope过大",
        resolution: "通过scope裁剪至数二范围3类6题后成功闭环",
      },
      {
        description: "数学执行时间偏晚（17:42才完成）",
        resolution: "学习时段分布存在优化空间",
      },
    ],
    time_spent: [
      { subject: "math", hours: 2.0, planned_hours: 1.5 },
      { subject: "professional", hours: 1.0, planned_hours: 1.0 },
      { subject: "english", hours: 1.0, planned_hours: 1.0 },
      { subject: "math", hours: 0.5, planned_hours: 0.5 },
    ],
    total_hours: 4.5,
    completion: {
      priority_a_total: 3,
      priority_a_done: 3,
      priority_b_total: 1,
      priority_b_done: 1,
      completion_rate: 100,
    },
    energy_level: 4,
    external_interference: "无",
    key_achievements: [
      "微分方程综合收口正式闭环——高数基础阶段最后阻塞项解除",
      "计组4.3指令格式扩展新课成功启动",
      "英语阅读第8篇完成，每日节奏维持",
      "多元微分连续6日中断后成功重启",
    ],
    next_steps: [
      "7/24启动线代第一章",
      "7/24推进计组4.3剩余内容或4.4",
      "英语继续每日1篇阅读+250词节奏",
    ],
  },
  view: "# 2026-07-23 学习复盘\n\n## 完成情况\n- A级: 3/3 (100%)\n- B级: 1/1 (100%)\n\n## 关键成果\n- 微分方程综合收口正式闭环\n- 计组4.3新课启动\n- 英语阅读第8篇完成\n",
};

/** Mock Dashboard 汇总 */
export const mockDashboardSummary: DashboardSummary = {
  date: "2026-07-24",
  remaining_days: 149,
  today_tasks: {
    total: 3,
    done: 0,
    in_progress: 1,
    pending: 2,
  },
  week_progress: {
    week_start: "2026-07-21",
    week_end: "2026-07-27",
    completed_hours: 12.5,
    target_hours: 24.5,
    daily_breakdown: [
      { date: "2026-07-21", hours: 3.5, tasks_done: 3 },
      { date: "2026-07-22", hours: 2.5, tasks_done: 2 },
      { date: "2026-07-23", hours: 4.5, tasks_done: 4 },
      { date: "2026-07-24", hours: 2.0, tasks_done: 1 },
      { date: "2026-07-25", hours: 0, tasks_done: 0 },
      { date: "2026-07-26", hours: 0, tasks_done: 0 },
      { date: "2026-07-27", hours: 0, tasks_done: 0 },
    ],
  },
  current_phase: "基础阶段",
  streak_days: 2,
  total_study_days: 123,
  upcoming_deadlines: [
    { date: "2026-07-24", title: "线代第一章启动", subject: "math", priority: "A" },
    { date: "2026-07-25", title: "计组4.3收口", subject: "professional", priority: "A" },
    { date: "2026-07-26", title: "线代第2讲", subject: "math", priority: "B" },
  ],
  review_reminder: {
    last_review_date: "2026-07-23",
    pending_review: false,
  },
  subject_progress: [
    { subject: "math", name: "数学（数二）", phase: "基础阶段", weekly_hours: 14, target_score: 120, completion_percentage: 65, current_topic: "常微分方程" },
    { subject: "english", name: "英语", phase: "基础阶段", weekly_hours: 7, target_score: 75, completion_percentage: 45, current_topic: "阅读理解·长难句" },
    { subject: "professional", name: "408计算机综合", phase: "基础阶段", weekly_hours: 14, target_score: 110, completion_percentage: 55, current_topic: "数据结构·图" },
    { subject: "politics", name: "政治", phase: "未启动", weekly_hours: 0, target_score: 70, completion_percentage: 0, current_topic: "" },
  ],
};

/** Mock 应用设置 */
export const mockSettings: AppSettings = {
  data_directory: ".",
  theme: "light",
  language: "zh-CN",
  user_name: "",
  show_greeting: true,
  exam_type: "数学二",
  exam_date: "2026-12-26",
  target_score: 360,
  onboarding_completed: true,
  study_schedule: {
    start_time: "09:00",
    end_time: "22:00",
    daily_target_hours: 5.5,
    study_days_per_week: 6,
    rest_days: ["周日"],
    review_reminder_time: "23:00",
  },
  ai_providers: [
    {
      id: "provider-1",
      name: "默认 Provider",
      type: "openai",
      base_url: "https://api.openai.com/v1",
      api_key: "",
      model: "",
      temperature: 0.7,
      max_tokens: 8192,
      enabled: true,
      is_default: true,
    },
  ],
  default_provider_id: "provider-1",
  mcp_servers: [
    {
      id: "mcp-ticktick",
      name: "滴答清单",
      type: "ticktick",
      enabled: true,
      transport: "stdio",
      command: "npx",
      args: ["-y", "@modelcontextprotocol/server-ticktick"],
    },
    {
      id: "mcp-filesystem",
      name: "文件系统",
      type: "filesystem",
      enabled: true,
      transport: "stdio",
      command: "npx",
      args: ["-y", "@modelcontextprotocol/server-filesystem", "."],
    },
  ],
  enabled_mcp_ids: ["mcp-ticktick", "mcp-filesystem"],
  ticktick: {
    enabled: true,
    tag_prefix: "计划",
    default_project_id: "study-list",
  },
  window: {
    width: 1280,
    height: 820,
    maximized: false,
  },
};

/** Mock MCP Server 状态 */
export const mockMCPServerStatus: MCPServerStatus[] = [
  { id: "mcp-ticktick", name: "滴答清单", connected: true, tools_count: 12 },
  { id: "mcp-filesystem", name: "文件系统", connected: true, tools_count: 8 },
];

/** Mock AI Provider 配置列表 */
export const mockAIProviders: AIProviderConfig[] = mockSettings.ai_providers;

/** Mock MCP Server 配置列表 */
export const mockMCPServers: MCPServerConfig[] = mockSettings.mcp_servers;
