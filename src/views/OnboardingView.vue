<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref } from "vue";
import { useRouter } from "vue-router";
import { useSettingsStore } from "@/stores/settings";
import Card from "@/components/ui/Card.vue";
import Button from "@/components/ui/Button.vue";
import Select from "@/components/ui/Select.vue";
import DatePicker from "@/components/ui/DatePicker.vue";
import TimePicker from "@/components/ui/TimePicker.vue";
import {
  Sparkles,
  ArrowLeft,
  ArrowRight,
  Check,
  Eye,
  EyeOff,
  Bot,
  Clock,
  GraduationCap,
  Calendar,
  Target,
  User,
  MapPin,
  BookOpen,
  TrendingUp,
  Info,
  AlertTriangle,
} from "lucide-vue-next";
import type { AIProviderConfig, ProviderType } from "@/types";
import * as api from "@/api";

const router = useRouter();
const settingsStore = useSettingsStore();

// ── 回车键进入下一步 ──
// 在 onboarding 任意步骤按 Enter（非输入框中 Shift+Enter / 表单内 textarea）即可前进
function handleKeydown(e: KeyboardEvent) {
  if (e.key !== "Enter") return;
  // 在 textarea 中或按 Shift 时不触发，允许换行
  if (e.shiftKey) return;
  const target = e.target as HTMLElement | null;
  if (target) {
    const tag = target.tagName.toLowerCase();
    if (tag === "textarea") return;
    // select / button 默认会通过 Enter 触发 click，避免重复
    if (tag === "select" || tag === "button") return;
  }
  e.preventDefault();
  if (finishing.value) return;
  if (isFirstStep.value) {
    next();
  } else if (isLastStep.value) {
    finish();
  } else {
    next();
  }
}

// ── 步骤定义 ──
interface OnboardingStep {
  key: string;
  title: string;
  description: string;
  skippable: boolean;
}

const steps: OnboardingStep[] = [
  { key: "welcome", title: "欢迎使用 StudyAgent", description: "你的个人考研学习智能体，让每一步都更有方向。", skippable: false },
  { key: "name", title: "该怎么称呼你", description: "我们会用这个名字称呼你，也会出现在工作台问候中。", skippable: false },
  { key: "school", title: "目标院校", description: "告诉我们你的目标院校和专业方向。", skippable: false },
  { key: "subjects", title: "考试科目", description: "选择你的考试科目和版本（如数学二、英语一）。", skippable: false },
  { key: "progress", title: "当前进度", description: "告诉我们各科目的当前学习阶段，帮助 AI 更好地规划。", skippable: false },
  { key: "exam", title: "你的考试目标", description: "设定目标分数，激励自己不断进步。", skippable: false },
  { key: "date", title: "考研年份", description: "考试默认在每年 12 月 20 日，设置年份即可自动计算倒计时。", skippable: false },
  { key: "schedule", title: "学习节奏", description: "设置每周学习天数和休息日，帮助我们安排可持续的学习计划。", skippable: true },
  { key: "ai", title: "配置 AI 助手", description: "智能计划、教学讲解与复盘等核心功能依赖 AI。未配置可跳过，稍后在设置中补全。", skippable: true },
  { key: "done", title: "配置完成", description: "一切就绪，开始你的考研之旅吧。", skippable: false },
];

const currentStep = ref(0);
const direction = ref<"forward" | "backward">("forward");
const showApiKey = ref(false);
const finishing = ref(false);
// H32：initState 失败时的错误信息（保留用户停留在引导页）
const errorMessage = ref("");

const step = computed(() => steps[currentStep.value]);
const isFirstStep = computed(() => currentStep.value === 0);
const isLastStep = computed(() => currentStep.value === steps.length - 1);
const progress = computed(() => Math.round(((currentStep.value + 1) / steps.length) * 100));

// ── 表单数据 ──
const userName = ref("");
// School
const targetSchool = ref("");
const targetMajor = ref("");
// Subjects
const mathVersion = ref<"数一" | "数二" | "数三" | "">("");
const englishVersion = ref<"英一" | "英二" | "英三" | "">("");
const hasPolitics = ref(false);
const hasProfessional = ref(false);
const professionalName = ref("");
// Progress (per subject)
const progressPhase = ref<Record<string, string>>({});
const progressTextbook = ref<Record<string, string>>({});
// Exam
const examYear = ref(new Date().getFullYear());
const targetScore = ref<number | null>(null);
// Schedule
const weekdayOptions = ["周一", "周二", "周三", "周四", "周五", "周六", "周日"];
const restDays = ref<string[]>(["周日"]);
const startTime = ref("09:00");
const endTime = ref("22:00");
const studyDaysPerWeek = computed(() => 7 - restDays.value.length);
// 各科开始学习日期（留空表示立即开始）
const subjectStartDates = ref<{ math: string; english: string; politics: string; professional: string }>({
  math: "",
  english: "",
  politics: "",
  professional: "",
});
// 用户期望每日任务数量（默认 3，每科约一条，未开始的科目不安排）
const dailyTaskCount = ref(3);
// 每日目标学习时长（小时，默认 5，可在引导中调整）
const dailyTargetHours = ref(5);
// 是否允许 AI 安排总结/复习任务（默认 true，关闭时只推进新知识点）
const enableReviewTasks = ref(true);
// 是否启用任务计时（默认 false，开启后任务卡显示开始/暂停按钮，记录专注时长）
const enableTimeTracking = ref(false);
// 开机自启动（首次引导询问，可在设置中更改）
const autostartEnabled = ref(false);
const autostartLoading = ref(false);
// AI
const providerForm = ref<AIProviderConfig>({
  id: "", name: "我的 Provider", type: "openai", base_url: "", api_key: "", model: "",
  temperature: 0.7, max_tokens: 4096, enabled: true, is_default: true,
});

const providerTypeOptions: { value: ProviderType; label: string }[] = [
  { value: "openai", label: "OpenAI" }, { value: "gemini", label: "Gemini" },
  { value: "anthropic", label: "Anthropic" }, { value: "ollama", label: "Ollama (本地)" },
  { value: "openrouter", label: "OpenRouter" }, { value: "siliconflow", label: "硅基流动" },
  { value: "dashscope", label: "通义千问" }, { value: "volcengine", label: "火山引擎" },
  { value: "custom", label: "自定义" },
];

// ── Helpers ──
const phaseOptions = ["foundation", "strengthen", "sprint", "mock"];
const phaseLabels: Record<string, string> = { foundation: "基础阶段", strengthen: "强化阶段", sprint: "冲刺阶段", mock: "模拟阶段" };

/** 各科目教材输入框的占位示例 */
const textbookPlaceholders: Record<string, string> = {
  math: "如：张宇高数18讲 / 李永乐复习全书",
  english: "如：考研英语真题黄皮书 / 唐迟阅读",
  politics: "如：肖秀荣精讲精练 / 徐涛核心考案",
  professional: "如：王道408计算机综合",
};
function textbookPlaceholder(key: string): string {
  return textbookPlaceholders[key] ?? "如：教材名称";
}

/** 当前用户选择的所有活跃科目 */
const activeSubjects = computed(() => {
  const list: { key: string; label: string; version: string }[] = [];
  if (mathVersion.value) list.push({ key: "math", label: "数学", version: mathVersion.value });
  if (englishVersion.value) list.push({ key: "english", label: "英语", version: englishVersion.value });
  if (hasPolitics.value) list.push({ key: "politics", label: "政治", version: "" });
  if (hasProfessional.value) list.push({ key: "professional", label: professionalName.value || "专业课", version: "" });
  return list;
});

// ── 步骤导航 ──
function next() {
  direction.value = "forward";
  persistCurrentStep();
  if (isLastStep.value) { finish(); return; }
  currentStep.value = Math.min(currentStep.value + 1, steps.length - 1);
}

function prev() {
  direction.value = "backward";
  if (isFirstStep.value) return;
  currentStep.value = Math.max(currentStep.value - 1, 0);
}

function skip() {
  direction.value = "forward";
  currentStep.value = Math.min(currentStep.value + 1, steps.length - 1);
}

function persistCurrentStep() {
  const s = settingsStore.settings;
  if (!s) return;
  switch (step.value.key) {
    case "name": s.user_name = userName.value.trim(); break;
    case "school":
      s.target_school = targetSchool.value.trim();
      s.target_major = targetMajor.value.trim();
      break;
    case "exam": s.target_score = targetScore.value ?? 0; break;
    case "date": s.exam_date = `${examYear.value}-12-20`; break;
    case "subjects":
      // H40：科目选择持久化到 exam_type，中断引导后不丢失
      {
        const examTypeParts: string[] = [];
        if (mathVersion.value) examTypeParts.push(mathVersion.value);
        if (englishVersion.value) examTypeParts.push(englishVersion.value);
        if (hasPolitics.value) examTypeParts.push("政治");
        if (hasProfessional.value) examTypeParts.push(professionalName.value || "专业课");
        s.exam_type = examTypeParts.join(" / ");
      }
      break;
    case "progress":
      // H40：进度/教材选择仅在 finish() 时写入 initState（Settings 无对应字段），
      // 此处无需持久化，保留选择至 finish 即可。
      break;
    case "schedule":
      s.study_schedule = {
        ...(s.study_schedule || {}),
        start_time: startTime.value, end_time: endTime.value,
        daily_target_hours: dailyTargetHours.value,
        study_days_per_week: 7 - restDays.value.length,
        rest_days: [...restDays.value],
        review_reminder_time: "23:00",
        subject_start_dates: { ...subjectStartDates.value },
        daily_task_count: dailyTaskCount.value,
        enable_review_tasks: enableReviewTasks.value,
        enable_time_tracking: enableTimeTracking.value,
      };
      break;
    case "ai":
      if (providerForm.value.base_url.trim() || providerForm.value.api_key.trim()) {
        const exists = s.ai_providers.find(p => p.base_url === providerForm.value.base_url && p.name === providerForm.value.name);
        if (!exists) {
          const newProvider: AIProviderConfig = { ...providerForm.value, id: `provider-${Date.now()}` };
          s.ai_providers.forEach(p => (p.is_default = false));
          s.ai_providers.push(newProvider);
          s.default_provider_id = newProvider.id;
        }
      }
      break;
  }
}

async function finish() {
  const s = settingsStore.settings;
  if (!s) return;
  finishing.value = true;
  try {
    s.user_name = userName.value.trim();
    s.target_score = targetScore.value ?? 0;
    s.exam_date = `${examYear.value}-12-20`;
    s.study_schedule = {
      ...(s.study_schedule || {}),
      start_time: startTime.value, end_time: endTime.value,
      daily_target_hours: dailyTargetHours.value,
      study_days_per_week: studyDaysPerWeek.value,
      rest_days: [...restDays.value],
      review_reminder_time: s.study_schedule?.review_reminder_time ?? "23:00",
      subject_start_dates: { ...subjectStartDates.value },
      daily_task_count: dailyTaskCount.value,
      enable_review_tasks: enableReviewTasks.value,
      enable_time_tracking: enableTimeTracking.value,
    };
    // 设置 exam_type 用于记录
    const examTypeParts: string[] = [];
    if (mathVersion.value) examTypeParts.push(mathVersion.value);
    if (englishVersion.value) examTypeParts.push(englishVersion.value);
    if (hasPolitics.value) examTypeParts.push("政治");
    if (hasProfessional.value) examTypeParts.push(professionalName.value || "专业课");
    s.exam_type = examTypeParts.join(" / ");

    // 初始化 State 文件
    const subjects: api.InitStatePayload["subjects"] = [];
    if (mathVersion.value) {
      subjects.push({ subject: "math", version: mathVersion.value, active: true, phase: (progressPhase.value.math || "foundation"), weekly_hours: 14.0, target_score: 120, textbook: progressTextbook.value.math || undefined });
    }
    if (englishVersion.value) {
      subjects.push({ subject: "english", version: englishVersion.value, active: true, phase: (progressPhase.value.english || "foundation"), weekly_hours: 7.0, target_score: 75, textbook: progressTextbook.value.english || undefined });
    }
    if (hasPolitics.value) {
      subjects.push({ subject: "politics", active: true, phase: (progressPhase.value.politics || "foundation"), weekly_hours: 5.0, target_score: 70, textbook: progressTextbook.value.politics || undefined });
    }
    if (hasProfessional.value) {
      subjects.push({ subject: "professional", active: true, phase: (progressPhase.value.professional || "foundation"), weekly_hours: 10.0, target_score: 120, textbook: progressTextbook.value.professional || undefined });
    }

    await api.initState({
      target_school: targetSchool.value.trim(),
      target_major: targetMajor.value.trim(),
      exam_date: `${examYear.value}-12-20`,
      subjects,
      professional_name: professionalName.value || undefined,
    });

    await settingsStore.completeOnboarding();
    router.replace("/dashboard");
  } catch (err) {
    console.error("初始化失败:", err);
    // H32：initState 失败时不允许完成引导，用户停留在引导页可重试
    // 显示错误信息让用户了解问题
    errorMessage.value = `初始化失败: ${err instanceof Error ? err.message : String(err)}。请检查配置后重试。`;
    return;
  } finally {
    finishing.value = false;
  }
}

function initFormFromSettings() {
  const s = settingsStore.settings;
  if (!s) return;
  userName.value = s.user_name ?? "";
  targetScore.value = s.target_score || null;
  if (s.exam_date) { const m = s.exam_date.match(/^(\d{4})/); if (m) examYear.value = parseInt(m[1], 10); }
  if (s.target_school) targetSchool.value = s.target_school;
  if (s.target_major) targetMajor.value = s.target_major;
  const ss = s.study_schedule;
  if (ss) {
    restDays.value = ss.rest_days?.length ? [...ss.rest_days] : ["周日"];
    startTime.value = ss.start_time ?? "09:00";
    endTime.value = ss.end_time ?? "22:00";
    if (typeof ss.daily_target_hours === "number" && ss.daily_target_hours > 0) {
      dailyTargetHours.value = ss.daily_target_hours;
    }
  }
  // Exam type parsing
  if (s.exam_type) {
    const parts = s.exam_type.split(" / ");
    for (const p of parts) {
      if (p === "数一" || p === "数二" || p === "数三") mathVersion.value = p as "数一" | "数二" | "数三";
      else if (p === "英一" || p === "英二" || p === "英三") englishVersion.value = p as "英一" | "英二" | "英三";
      else if (p === "政治") hasPolitics.value = true;
      else if (p && p !== "政治") { hasProfessional.value = true; professionalName.value = p; }
    }
  }
  // 读取各科开始学习日期与每日任务数
  const ssSchedule = s.study_schedule;
  if (ssSchedule) {
    if (ssSchedule.subject_start_dates) {
      subjectStartDates.value = {
        math: ssSchedule.subject_start_dates.math ?? "",
        english: ssSchedule.subject_start_dates.english ?? "",
        politics: ssSchedule.subject_start_dates.politics ?? "",
        professional: ssSchedule.subject_start_dates.professional ?? "",
      };
    }
    if (typeof ssSchedule.daily_task_count === "number" && ssSchedule.daily_task_count > 0) {
      dailyTaskCount.value = ssSchedule.daily_task_count;
    }
    if (typeof ssSchedule.enable_review_tasks === "boolean") {
      enableReviewTasks.value = ssSchedule.enable_review_tasks;
    }
    if (typeof ssSchedule.enable_time_tracking === "boolean") {
      enableTimeTracking.value = ssSchedule.enable_time_tracking;
    }
  }
  if (s.ai_providers.length > 0) {
    const def = s.ai_providers.find(p => p.is_default) ?? s.ai_providers[0];
    providerForm.value = { ...def };
  }
}

function toggleRestDay(day: string) {
  if (restDays.value.includes(day)) restDays.value = restDays.value.filter(d => d !== day);
  else restDays.value = [...restDays.value, day];
}

async function setAutostart(enabled: boolean) {
  if (autostartLoading.value) return;
  autostartLoading.value = true;
  try {
    await api.setAutostart(enabled);
    autostartEnabled.value = enabled;
  } catch (e) {
    console.warn("[Onboarding] 设置开机启动失败:", e);
    // 即使失败也不阻塞引导流程，用户可在设置中再开启
  } finally {
    autostartLoading.value = false;
  }
}

onMounted(async () => {
  if (!settingsStore.settings) await settingsStore.load();
  initFormFromSettings();
  // 加载当前开机启动状态（用于引导时回显）
  try {
    autostartEnabled.value = await api.getAutostart().catch(() => false);
  } catch {
    // 非 Tauri 环境忽略
  }
  window.addEventListener("keydown", handleKeydown);
});

onUnmounted(() => {
  window.removeEventListener("keydown", handleKeydown);
});
</script>

<template>
  <div class="onboarding-view">
    <div class="onboarding-shell">
      <!-- Brand -->
      <div class="brand">
        <div class="brand-icon">
          <GraduationCap :size="22" />
        </div>
        <span class="brand-name">StudyAgent</span>
      </div>

      <!-- Step indicator -->
      <div class="step-indicator" v-if="!isFirstStep && !isLastStep">
        <div class="step-track">
          <div class="step-progress" :style="{ width: `${progress}%` }" />
        </div>
        <div class="step-dots">
          <button
            v-for="(s, i) in steps"
            :key="s.key"
            class="step-dot"
            :class="{
              active: i === currentStep,
              done: i < currentStep,
            }"
            :disabled="i > currentStep"
            :title="s.title"
            @click="i < currentStep ? (currentStep = i) : null"
          />
        </div>
        <span class="step-counter">{{ currentStep + 1 }} / {{ steps.length }}</span>
      </div>

      <!-- Step content -->
      <Card padding="lg" class="step-card">
        <transition :name="direction === 'forward' ? 'step-forward' : 'step-backward'" mode="out-in">
          <div :key="step.key" class="step-content">
            <!-- Welcome -->
            <template v-if="step.key === 'welcome'">
              <div class="hero-step">
                <div class="hero-badge">
                  <Sparkles :size="20" />
                </div>
                <h1 class="hero-title">{{ step.title }}</h1>
                <p class="hero-desc">{{ step.description }}</p>
                <ul class="hero-features">
                  <li><Check :size="15" /> 个性化每日学习计划</li>
                  <li><Check :size="15" /> 知识图谱与教材管理</li>
                  <li><Check :size="15" /> AI 助手随时答疑</li>
                  <li><Check :size="15" /> 每日复盘持续改进</li>
                </ul>
              </div>
            </template>

            <!-- Done -->
            <template v-else-if="step.key === 'done'">
              <div class="hero-step">
                <div class="hero-badge done">
                  <Check :size="24" />
                </div>
                <h1 class="hero-title">{{ step.title }}</h1>
                <p class="hero-desc">{{ step.description }}</p>
                <div class="done-summary">
                  <div class="summary-item">
                    <User :size="15" />
                    <span>{{ userName || "你好" }}</span>
                  </div>
                  <div class="summary-item" v-if="targetScore">
                    <Target :size="15" />
                    <span>目标 {{ targetScore }} 分</span>
                  </div>
                  <div class="summary-item" v-if="examYear">
                    <Calendar :size="15" />
                    <span>{{ examYear }} 年考研 · {{ examYear }}-12-20</span>
                  </div>
                </div>
              </div>
            </template>

            <!-- Name -->
            <template v-else-if="step.key === 'name'">
              <div class="form-step">
                <h2 class="form-title">{{ step.title }}</h2>
                <p class="form-desc">{{ step.description }}</p>
                <div class="field">
                  <label class="field-label">用户称呼</label>
                  <input
                    v-model="userName"
                    type="text"
                    class="field-input"
                    placeholder="如：小明"
                    autofocus
                  />
                </div>
              </div>
            </template>

            <!-- School -->
            <template v-else-if="step.key === 'school'">
              <div class="form-step">
                <div class="step-head-row">
                  <div class="step-head-icon"><MapPin :size="18" /></div>
                  <div>
                    <h2 class="form-title">{{ step.title }}</h2>
                    <p class="form-desc">{{ step.description }}</p>
                  </div>
                </div>
                <div class="field">
                  <label class="field-label">目标院校</label>
                  <input v-model="targetSchool" type="text" class="field-input" placeholder="如：广东工业大学" autofocus />
                </div>
                <div class="field">
                  <label class="field-label">目标专业</label>
                  <input v-model="targetMajor" type="text" class="field-input" placeholder="如：计算机技术 / 人工智能" />
                </div>
              </div>
            </template>

            <!-- Subjects -->
            <template v-else-if="step.key === 'subjects'">
              <div class="form-step">
                <div class="step-head-row">
                  <div class="step-head-icon"><BookOpen :size="18" /></div>
                  <div>
                    <h2 class="form-title">{{ step.title }}</h2>
                    <p class="form-desc">{{ step.description }}</p>
                  </div>
                </div>
                <!-- 数学 -->
                <div class="subject-block">
                  <label class="field-label">数学</label>
                  <div class="option-grid cols-4">
                    <button class="option-chip" :class="{ active: mathVersion === '' }" @click="mathVersion = ''">不考</button>
                    <button class="option-chip" :class="{ active: mathVersion === '数一' }" @click="mathVersion = '数一'">数一</button>
                    <button class="option-chip" :class="{ active: mathVersion === '数二' }" @click="mathVersion = '数二'">数二</button>
                    <button class="option-chip" :class="{ active: mathVersion === '数三' }" @click="mathVersion = '数三'">数三</button>
                  </div>
                </div>
                <!-- 英语 -->
                <div class="subject-block">
                  <label class="field-label">英语</label>
                  <div class="option-grid cols-4">
                    <button class="option-chip" :class="{ active: englishVersion === '' }" @click="englishVersion = ''">不考</button>
                    <button class="option-chip" :class="{ active: englishVersion === '英一' }" @click="englishVersion = '英一'">英一</button>
                    <button class="option-chip" :class="{ active: englishVersion === '英二' }" @click="englishVersion = '英二'">英二</button>
                    <button class="option-chip" :class="{ active: englishVersion === '英三' }" @click="englishVersion = '英三'">英三</button>
                  </div>
                </div>
                <!-- 政治 -->
                <div class="subject-block">
                  <label class="field-label">政治</label>
                  <div class="option-grid cols-2">
                    <button class="option-chip" :class="{ active: hasPolitics }" @click="hasPolitics = !hasPolitics">{{ hasPolitics ? '已选择' : '不考' }}</button>
                  </div>
                </div>
                <!-- 专业课 -->
                <div class="subject-block">
                  <label class="field-label">专业课</label>
                  <div class="option-grid cols-2">
                    <button class="option-chip" :class="{ active: hasProfessional }" @click="hasProfessional = !hasProfessional">{{ hasProfessional ? '已选择' : '不考' }}</button>
                  </div>
                  <input v-if="hasProfessional" v-model="professionalName" type="text" class="field-input" placeholder="如：408计算机综合" style="margin-top: var(--space-2)" />
                </div>
              </div>
            </template>

            <!-- Progress -->
            <template v-else-if="step.key === 'progress'">
              <div class="form-step">
                <div class="step-head-row">
                  <div class="step-head-icon"><TrendingUp :size="18" /></div>
                  <div>
                    <h2 class="form-title">{{ step.title }}</h2>
                    <p class="form-desc">{{ step.description }}</p>
                  </div>
                </div>
                <div v-if="activeSubjects.length === 0" class="empty-hint">
                  请先在上一页选择至少一个考试科目。
                </div>
                <div v-for="subj in activeSubjects" :key="subj.key" class="subject-progress-block">
                  <h3 class="subj-progress-title">{{ subj.label }}<span v-if="subj.version"> · {{ subj.version }}</span></h3>
                  <div class="field">
                    <label class="field-label">当前阶段</label>
                    <div class="option-grid cols-4">
                      <button v-for="ph in phaseOptions" :key="ph" class="option-chip"
                        :class="{ active: (progressPhase[subj.key] || 'foundation') === ph }"
                        @click="progressPhase[subj.key] = ph">
                        {{ phaseLabels[ph] }}
                      </button>
                    </div>
                  </div>
                  <div class="field">
                    <label class="field-label">教材（选填）</label>
                    <input v-model="progressTextbook[subj.key]" type="text" class="field-input" :placeholder="textbookPlaceholder(subj.key)" />
                  </div>
                </div>
              </div>
            </template>

            <!-- Exam -->
            <template v-else-if="step.key === 'exam'">
              <div class="form-step">
                <h2 class="form-title">{{ step.title }}</h2>
                <p class="form-desc">{{ step.description }}</p>
                <div class="field">
                  <label class="field-label">目标分数</label>
                  <input
                    v-model.number="targetScore"
                    type="number"
                    min="0"
                    max="500"
                    class="field-input"
                    placeholder="如：360"
                  />
                </div>
              </div>
            </template>

            <!-- Date -->
            <template v-else-if="step.key === 'date'">
              <div class="form-step">
                <h2 class="form-title">{{ step.title }}</h2>
                <p class="form-desc">{{ step.description }}</p>
                <div class="field">
                  <label class="field-label">考研年份</label>
                  <input
                    v-model.number="examYear"
                    type="number"
                    :min="new Date().getFullYear()"
                    :max="new Date().getFullYear() + 3"
                    class="field-input"
                  />
                </div>
                <div class="field">
                  <label class="field-label">考试日期（自动计算）</label>
                  <div class="computed-date">
                    <Calendar :size="16" />
                    <span>{{ examYear }}-12-20</span>
                  </div>
                </div>
              </div>
            </template>

            <!-- Schedule -->
            <template v-else-if="step.key === 'schedule'">
              <div class="form-step">
                <div class="step-head-row">
                  <div class="step-head-icon">
                    <Clock :size="18" />
                  </div>
                  <div>
                    <h2 class="form-title">{{ step.title }}</h2>
                    <p class="form-desc">{{ step.description }}</p>
                  </div>
                </div>
                <div class="field">
                  <label class="field-label">
                    每周学习几天
                    <span class="field-hint">（自动计算：7 - 休息天数）</span>
                  </label>
                  <input
                    :value="studyDaysPerWeek"
                    type="number"
                    min="1"
                    max="7"
                    class="field-input"
                    readonly
                    tabindex="-1"
                  />
                </div>
                <div class="field">
                  <label class="field-label">
                    每日目标学习时长（小时）
                    <span class="field-hint">（AI 排计划时会参考此目标安排任务量）</span>
                  </label>
                  <input
                    v-model.number="dailyTargetHours"
                    type="number"
                    min="1"
                    max="16"
                    step="0.5"
                    class="field-input"
                  />
                  <p class="field-hint">默认 5 小时，可根据实际情况调整，例如在职备考可设为 3 小时。</p>
                </div>
                <div class="field">
                  <label class="field-label">每周休息日（可多选）</label>
                  <div class="option-grid">
                    <button
                      v-for="day in weekdayOptions"
                      :key="day"
                      type="button"
                      class="option-chip"
                      :class="{ active: restDays.includes(day) }"
                      @click="toggleRestDay(day)"
                    >
                      {{ day }}
                    </button>
                  </div>
                  <p class="field-hint">选择休息日后，学习天数会自动调整为剩余天数。</p>
                </div>
                <div class="field-row">
                  <div class="field">
                    <label class="field-label">每日开始时间</label>
                    <TimePicker v-model="startTime" :minute-step="15" />
                  </div>
                  <div class="field">
                    <label class="field-label">每日结束时间</label>
                    <TimePicker v-model="endTime" :minute-step="15" />
                  </div>
                </div>
                <div class="field">
                  <label class="field-label">
                    每天安排多少任务
                    <span class="field-hint">（每科约一条；未开始的科目不安排，相应减少当日任务数）</span>
                  </label>
                  <input
                    v-model.number="dailyTaskCount"
                    type="number"
                    min="1"
                    max="8"
                    class="field-input"
                  />
                  <p class="field-hint">默认 3-4 个。例如政治未到开始日期时，当天不会排政治任务。</p>
                </div>
                <div class="field">
                  <label class="field-label">是否安排总结/复习任务</label>
                  <div class="option-grid cols-2">
                    <button type="button" class="option-chip" :class="{ active: enableReviewTasks }" @click="enableReviewTasks = true">安排（推荐）</button>
                    <button type="button" class="option-chip" :class="{ active: !enableReviewTasks }" @click="enableReviewTasks = false">只推进新知识点</button>
                  </div>
                  <p class="field-hint">关闭后 AI 不会安排"回顾"/"总结"/"复习"类任务，适合希望持续向前推进的用户。</p>
                </div>
                <div class="field">
                  <label class="field-label">记录学习时长</label>
                  <div class="option-grid cols-2">
                    <button type="button" class="option-chip" :class="{ active: enableTimeTracking }" @click="enableTimeTracking = true">开启</button>
                    <button type="button" class="option-chip" :class="{ active: !enableTimeTracking }" @click="enableTimeTracking = false">不开启（默认）</button>
                  </div>
                  <p class="field-hint">开启后任务卡显示开始/暂停按钮，记录每项任务的专注时长；关闭时只关注完成内容。</p>
                </div>
                <div class="field">
                  <label class="field-label">
                    各科开始学习日期
                    <span class="field-hint">（留空表示立即开始；未到日期前不为该科安排任务）</span>
                  </label>
                  <div class="subject-start-grid">
                    <div v-if="mathVersion" class="subject-start-item">
                      <span class="subject-start-label">数学</span>
                      <DatePicker v-model="subjectStartDates.math" placeholder="立即开始" />
                    </div>
                    <div v-if="englishVersion" class="subject-start-item">
                      <span class="subject-start-label">英语</span>
                      <DatePicker v-model="subjectStartDates.english" placeholder="立即开始" />
                    </div>
                    <div v-if="hasPolitics" class="subject-start-item">
                      <span class="subject-start-label">政治</span>
                      <DatePicker v-model="subjectStartDates.politics" placeholder="立即开始" />
                    </div>
                    <div v-if="hasProfessional" class="subject-start-item">
                      <span class="subject-start-label">{{ professionalName || '专业课' }}</span>
                      <DatePicker v-model="subjectStartDates.professional" placeholder="立即开始" />
                    </div>
                  </div>
                  <p class="field-hint">例如政治计划 8 月中旬开始，可将政治开始日期设为 2026-08-15，此前不会安排政治任务。</p>
                </div>
                <div class="field">
                  <label class="field-label">开机启动</label>
                  <div class="option-grid cols-2">
                    <button type="button" class="option-chip" :class="{ active: autostartEnabled }" @click="setAutostart(true)">开机自启</button>
                    <button type="button" class="option-chip" :class="{ active: !autostartEnabled }" @click="setAutostart(false)">不自启</button>
                  </div>
                  <p class="field-hint">开启后开机时自动启动 StudyAgent，可在「设置 → 通用」中修改。</p>
                </div>
              </div>
            </template>

            <!-- AI Provider -->
            <template v-else-if="step.key === 'ai'">
              <div class="form-step">
                <div class="step-head-row">
                  <div class="step-head-icon">
                    <Bot :size="18" />
                  </div>
                  <div>
                    <h2 class="form-title">{{ step.title }}</h2>
                    <p class="form-desc">{{ step.description }}</p>
                  </div>
                </div>

                <!-- AI 依赖说明 -->
                <div class="ai-notice">
                  <div class="ai-notice-head">
                    <Info :size="14" />
                    <span class="ai-notice-title">哪些功能依赖 AI？</span>
                  </div>
                  <ul class="ai-notice-list">
                    <li>智能生成日计划 / 周计划（自动按科目、进度与剩余时间编排）</li>
                    <li>教学讲解与答疑（知识点讲解、错题分析）</li>
                    <li>每日复盘生成与改进建议</li>
                    <li>基于教材的智能问答与检索</li>
                  </ul>
                  <div class="ai-notice-warn">
                    <AlertTriangle :size="13" />
                    <span>未配置 AI Provider 时，以上功能将无法使用；其他如查看计划、勾选任务、统计等本地功能不受影响。</span>
                  </div>
                  <p class="ai-notice-foot">可在此填写任一兼容 OpenAI 接口的服务（OpenAI、火山引擎、硅基流动、Ollama 等）；稍后可在「设置 → AI Provider」中添加、修改或切换。</p>
                </div>

                <div class="field">
                  <label class="field-label">Provider 类型</label>
                  <Select v-model="providerForm.type" :max-width="'100%'">
                    <option v-for="opt in providerTypeOptions" :key="opt.value" :value="opt.value">
                      {{ opt.label }}
                    </option>
                  </Select>
                </div>
                <div class="field">
                  <label class="field-label">Base URL</label>
                  <input
                    v-model="providerForm.base_url"
                    type="text"
                    class="field-input"
                    placeholder="https://api.openai.com/v1"
                  />
                </div>
                <div class="field">
                  <label class="field-label">API Key</label>
                  <div class="input-with-action">
                    <input
                      v-model="providerForm.api_key"
                      :type="showApiKey ? 'text' : 'password'"
                      class="field-input"
                      placeholder="sk-..."
                    />
                    <button class="input-suffix-btn" type="button" @click="showApiKey = !showApiKey">
                      <component :is="showApiKey ? EyeOff : Eye" :size="15" />
                    </button>
                  </div>
                </div>
                <div class="field">
                  <label class="field-label">Model</label>
                  <input
                    v-model="providerForm.model"
                    type="text"
                    class="field-input"
                    placeholder="gpt-4o"
                  />
                </div>
              </div>
            </template>

          </div>
        </transition>
      </Card>

      <!-- Footer actions -->
      <div class="step-actions">
        <Button
          v-if="!isFirstStep && !isLastStep"
          variant="ghost"
          size="md"
          @click="prev"
        >
          <ArrowLeft :size="16" />
          上一步
        </Button>

        <div class="actions-right">
          <Button
            v-if="step.skippable"
            variant="ghost"
            size="md"
            @click="skip"
          >
            跳过
          </Button>

          <Button
            v-if="isFirstStep"
            variant="primary"
            size="lg"
            @click="next"
          >
            <Sparkles :size="16" />
            开始配置
          </Button>

          <Button
            v-else-if="isLastStep"
            variant="primary"
            size="lg"
            :loading="finishing"
            @click="finish"
          >
            <Check :size="16" />
            完成，进入工作台
          </Button>

          <Button v-else variant="primary" size="md" @click="next">
            下一步
            <ArrowRight :size="16" />
          </Button>
        </div>

        <!-- H32：初始化失败错误提示 -->
        <p v-if="errorMessage" class="init-error">{{ errorMessage }}</p>
      </div>
    </div>
  </div>
</template>

<style scoped>
.onboarding-view {
  width: 100vw;
  height: 100vh;
  overflow-y: auto;
  background: var(--bg-primary);
  display: flex;
  align-items: center;
  justify-content: center;
  padding: var(--space-8) var(--space-4);
}

.onboarding-shell {
  width: 100%;
  max-width: 560px;
  display: flex;
  flex-direction: column;
  gap: var(--space-6);
}

/* Brand */
.brand {
  display: flex;
  align-items: center;
  justify-content: center;
  gap: var(--space-3);
}

.brand-icon {
  width: 40px;
  height: 40px;
  display: flex;
  align-items: center;
  justify-content: center;
  background: var(--accent);
  color: var(--text-on-accent);
  border-radius: var(--radius-md);
  flex-shrink: 0;
}

.brand-name {
  font-size: var(--text-xl);
  font-weight: var(--font-bold);
  color: var(--text-primary);
  letter-spacing: -0.02em;
}

/* Step indicator */
.step-indicator {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: var(--space-2);
}

.step-track {
  width: 100%;
  height: 4px;
  background: var(--bg-tertiary);
  border-radius: var(--radius-full);
  overflow: hidden;
}

.step-progress {
  height: 100%;
  background: var(--accent);
  border-radius: var(--radius-full);
  transition: width var(--transition-normal);
}

.step-dots {
  display: flex;
  align-items: center;
  gap: var(--space-2);
}

.step-dot {
  width: 8px;
  height: 8px;
  border-radius: var(--radius-full);
  border: none;
  background: var(--bg-tertiary);
  cursor: pointer;
  padding: 0;
  transition: all var(--transition-fast);
}

.step-dot.done {
  background: var(--accent);
}

.step-dot.active {
  background: var(--accent);
  width: 20px;
}

.step-dot:disabled {
  cursor: default;
}

.step-counter {
  font-size: var(--text-xs);
  color: var(--text-tertiary);
  font-weight: var(--font-medium);
}

/* Step card */
.step-card {
  min-height: 320px;
  display: flex;
  align-items: stretch;
}

.step-content {
  width: 100%;
  display: flex;
  flex-direction: column;
  justify-content: center;
}

/* Hero step (welcome / done) */
.hero-step {
  display: flex;
  flex-direction: column;
  align-items: center;
  text-align: center;
  gap: var(--space-4);
  padding: var(--space-6) var(--space-2);
}

.hero-badge {
  width: 64px;
  height: 64px;
  display: flex;
  align-items: center;
  justify-content: center;
  background: var(--accent-subtle);
  color: var(--accent);
  border-radius: var(--radius-lg);
}

.hero-badge.done {
  background: var(--color-success-subtle);
  color: var(--color-success);
}

.hero-title {
  font-size: var(--text-2xl);
  font-weight: var(--font-bold);
  color: var(--text-primary);
  letter-spacing: -0.02em;
}

.hero-desc {
  font-size: var(--text-base);
  color: var(--text-secondary);
  max-width: 420px;
  line-height: var(--leading-relaxed);
  margin: 0;
}

.hero-features {
  list-style: none;
  margin: var(--space-2) 0 0;
  padding: 0;
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: var(--space-2) var(--space-6);
  text-align: left;
}

.hero-features li {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  font-size: var(--text-sm);
  color: var(--text-secondary);
}

.hero-features svg {
  color: var(--color-success);
  flex-shrink: 0;
}

.done-summary {
  display: flex;
  flex-direction: column;
  gap: var(--space-2);
  margin-top: var(--space-2);
  width: 100%;
  max-width: 320px;
}

.summary-item {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  padding: var(--space-2) var(--space-4);
  background: var(--bg-tertiary);
  border-radius: var(--radius-md);
  font-size: var(--text-sm);
  color: var(--text-primary);
  font-weight: var(--font-medium);
}

.summary-item svg {
  color: var(--accent);
  flex-shrink: 0;
}

/* Form step */
.form-step {
  display: flex;
  flex-direction: column;
  gap: var(--space-5);
}

.step-head-row {
  display: flex;
  align-items: flex-start;
  gap: var(--space-3);
}

.step-head-icon {
  width: 36px;
  height: 36px;
  display: flex;
  align-items: center;
  justify-content: center;
  background: var(--accent-subtle);
  color: var(--accent);
  border-radius: var(--radius-md);
  flex-shrink: 0;
}

.form-title {
  font-size: var(--text-xl);
  font-weight: var(--font-semibold);
  color: var(--text-primary);
  letter-spacing: -0.01em;
  line-height: var(--leading-tight);
}

.form-desc {
  font-size: var(--text-sm);
  color: var(--text-secondary);
  margin-top: 2px;
  line-height: var(--leading-normal);
}

/* ── AI 依赖说明卡片 ── */
.ai-notice {
  display: flex;
  flex-direction: column;
  gap: var(--space-2);
  padding: var(--space-3) var(--space-4);
  background: var(--accent-subtle);
  border: 1px solid color-mix(in srgb, var(--accent) 30%, transparent);
  border-radius: var(--radius-md);
  margin: var(--space-2) 0 var(--space-3);
}

.ai-notice-head {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  color: var(--accent);
  font-weight: var(--font-semibold);
  font-size: var(--text-sm);
}

.ai-notice-title {
  font-size: var(--text-sm);
}

.ai-notice-list {
  margin: 0;
  padding-left: var(--space-5);
  font-size: var(--text-sm);
  color: var(--text-secondary);
  line-height: var(--leading-relaxed);
}

.ai-notice-list li {
  list-style: disc;
  margin-bottom: 2px;
}

.ai-notice-warn {
  display: flex;
  align-items: flex-start;
  gap: var(--space-2);
  padding: var(--space-2) var(--space-3);
  background: var(--color-warning-subtle, color-mix(in srgb, #f59e0b 12%, transparent));
  border-radius: var(--radius-sm);
  color: var(--color-warning, #b45309);
  font-size: var(--text-xs);
  line-height: var(--leading-relaxed);
}

.ai-notice-warn svg {
  flex-shrink: 0;
  margin-top: 2px;
}

.ai-notice-foot {
  margin: 0;
  font-size: var(--text-xs);
  color: var(--text-tertiary);
  line-height: var(--leading-relaxed);
}

.field {
  display: flex;
  flex-direction: column;
  gap: var(--space-2);
}

.field-row {
  display: grid;
  grid-template-columns: repeat(2, 1fr);
  gap: var(--space-3);
}

.field-label {
  font-size: var(--text-sm);
  font-weight: var(--font-medium);
  color: var(--text-secondary);
}

.field-input {
  background: var(--bg-elevated);
  border: 1px solid var(--border-color);
  border-radius: var(--radius-md);
  padding: var(--space-3) var(--space-4);
  font-size: var(--text-base);
  font-family: inherit;
  color: var(--text-primary);
  outline: none;
  transition: border-color var(--transition-fast);
  width: 100%;
}

.field-input:focus {
  border-color: var(--accent);
}

.field-select {
  cursor: pointer;
  appearance: none;
  background-image: url("data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' width='12' height='12' viewBox='0 0 24 24' fill='none' stroke='%23a1a1a6' stroke-width='1.5' stroke-linecap='round' stroke-linejoin='round'%3E%3Cpolyline points='6 9 12 15 18 9'/%3E%3C/svg%3E");
  background-repeat: no-repeat;
  background-position: right var(--space-4) center;
  padding-right: var(--space-8);
}

.field-hint {
  font-size: var(--text-xs);
  color: var(--text-tertiary);
  margin: 0;
}

/* Subject block in subjects step */
.subject-block {
  padding: var(--space-3);
  background: var(--bg-tertiary);
  border-radius: var(--radius-md);
  display: flex;
  flex-direction: column;
  gap: var(--space-2);
}

.empty-hint {
  text-align: center;
  color: var(--text-tertiary);
  font-size: var(--text-sm);
  padding: var(--space-6) 0;
}

.subject-progress-block {
  padding: var(--space-4);
  background: var(--bg-tertiary);
  border-radius: var(--radius-md);
  display: flex;
  flex-direction: column;
  gap: var(--space-4);
}

.subj-progress-title {
  font-size: var(--text-base);
  font-weight: var(--font-semibold);
  color: var(--text-primary);
  margin: 0;
}

/* Computed date display */
.computed-date {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  padding: var(--space-3) var(--space-4);
  background: var(--bg-elevated);
  border: 1px solid var(--border-color);
  border-radius: var(--radius-md);
  font-size: var(--text-base);
  color: var(--text-primary);
  font-weight: var(--font-medium);
}

.computed-date svg {
  color: var(--accent);
  flex-shrink: 0;
}

/* Option grid (exam type chips) */
.option-grid {
  display: grid;
  grid-template-columns: repeat(4, 1fr);
  gap: var(--space-2);
}

.option-grid.cols-4 {
  grid-template-columns: repeat(4, 1fr);
}

.option-grid.cols-2 {
  grid-template-columns: repeat(2, 1fr);
}

/* Subject start dates grid */
.subject-start-grid {
  display: grid;
  grid-template-columns: repeat(2, 1fr);
  gap: var(--space-3);
  margin-top: var(--space-2);
}

.subject-start-item {
  display: flex;
  flex-direction: column;
  gap: var(--space-1);
}

.subject-start-label {
  font-size: var(--text-xs);
  font-weight: var(--font-medium);
  color: var(--text-tertiary);
}

.option-chip {
  padding: var(--space-2) var(--space-3);
  background: var(--bg-tertiary);
  border: 1.5px solid transparent;
  border-radius: var(--radius-md);
  font-size: var(--text-sm);
  font-weight: var(--font-medium);
  color: var(--text-secondary);
  cursor: pointer;
  transition: all var(--transition-fast);
  font-family: inherit;
  text-align: center;
}

.option-chip:hover {
  background: var(--sidebar-item-hover);
  color: var(--text-primary);
}

.option-chip.active {
  border-color: var(--accent);
  background: var(--accent-subtle);
  color: var(--accent);
}

/* Input with action (api key) */
.input-with-action {
  position: relative;
  display: flex;
  align-items: center;
}

.input-with-action .field-input {
  padding-right: var(--space-10);
}

.input-suffix-btn {
  position: absolute;
  right: var(--space-2);
  border: none;
  background: transparent;
  color: var(--text-tertiary);
  cursor: pointer;
  padding: var(--space-1);
  display: flex;
  align-items: center;
  justify-content: center;
  border-radius: var(--radius-xs);
  transition: color var(--transition-fast);
}

.input-suffix-btn:hover {
  color: var(--text-primary);
}

/* Footer actions */
.step-actions {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--space-3);
}

.actions-right {
  display: flex;
  align-items: center;
  gap: var(--space-2);
  margin-left: auto;
}

.init-error {
  margin-top: var(--space-3);
  color: var(--color-danger);
  font-size: var(--font-size-sm);
  text-align: center;
}

/* Step transitions */
.step-forward-enter-active,
.step-forward-leave-active,
.step-backward-enter-active,
.step-backward-leave-active {
  transition: opacity var(--transition-normal), transform var(--transition-normal);
}

.step-forward-enter-from {
  opacity: 0;
  transform: translateX(24px);
}

.step-forward-leave-to {
  opacity: 0;
  transform: translateX(-24px);
}

.step-backward-enter-from {
  opacity: 0;
  transform: translateX(-24px);
}

.step-backward-leave-to {
  opacity: 0;
  transform: translateX(24px);
}

@media (max-width: 600px) {
  .option-grid {
    grid-template-columns: repeat(4, 1fr);
  }
  .hero-features {
    grid-template-columns: 1fr;
  }
  .step-actions {
    flex-wrap: wrap;
  }
  .field-row {
    grid-template-columns: 1fr;
  }
  .subject-start-grid {
    grid-template-columns: 1fr;
  }
}
</style>
