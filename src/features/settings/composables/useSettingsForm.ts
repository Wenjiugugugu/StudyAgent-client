/**
 * 设置页 — 主表单状态与保存逻辑（原 SettingsView 本地缓冲表单）
 *
 * 结构与 settingsStore.settings 同构，所有 v-model 绑定到此对象，
 * 点「保存」才提交到 store 并持久化。同时承载教材独立保存、StudyState
 * 加载，以及「未保存修改」离开拦截逻辑。
 */
import { ref, computed } from "vue";
import { onBeforeRouteLeave } from "vue-router";
import { useSettingsStore } from "@/stores/settings";
import { settingsApi } from "../api";
import type { StudyState, SubjectKey } from "@/types/state";

/** 主表单结构（与 settingsStore.settings 对应字段同构） */
export interface SettingsForm {
  user_name: string;
  show_greeting: boolean;
  target_score: number;
  exam_date: string;
  start_time: string;
  end_time: string;
  daily_target_hours: number;
  review_reminder_time: string;
  rest_days: string[];
  daily_task_count: number;
  standard_granularity: number;
  enable_review_tasks: boolean;
  enable_time_tracking: boolean;
  subject_start_dates: {
    math: string;
    english: string;
    politics: string;
    professional: string;
  };
  /** 各科学习时间占比（百分比，活跃科目合计 100；null = 未配置，按各科周学时推导） */
  subject_time_allocation: {
    math: number;
    english: number;
    politics: number;
    professional: number;
  } | null;
}

/** 教材表单结构（独立保存，不走 settingsStore） */
export interface TextbookForm {
  math: string;
  english: string;
  politics: string;
  professional: string;
}

export function useSettingsForm() {
  const settingsStore = useSettingsStore();

  const weekdayOptions = ["周一", "周二", "周三", "周四", "周五", "周六", "周日"];

  // ── 本地缓冲表单（避免即时生效，点保存才提交） ──
  const form = ref<SettingsForm | null>(null);

  // 教材表单（独立保存，不走 settingsStore）
  const textbookForm = ref<TextbookForm>({ math: "", english: "", politics: "", professional: "" });

  const studyState = ref<StudyState | null>(null);
  const stateLoading = ref(false);
  const stateError = ref<string | null>(null);

  // 各科是否活跃（用于决定教材输入框是否显示）
  const subjectActive = computed(() => {
    const s = studyState.value?.subjects;
    return {
      math: s?.math?.active ?? false,
      english: s?.english?.active ?? false,
      politics: s?.politics?.active ?? false,
      professional: s?.professional?.active ?? false,
    };
  });

  const professionalName = computed(() => studyState.value?.subjects?.professional?.name || "专业课");

  // ── rest_days 切换 ──
  function toggleRestDay(day: string) {
    if (!form.value) return;
    const days = form.value.rest_days ?? [];
    if (days.includes(day)) {
      form.value.rest_days = days.filter((d) => d !== day);
    } else {
      form.value.rest_days = [...days, day];
    }
  }

  const computedStudyDays = computed(() => {
    if (!form.value) return 6;
    return 7 - (form.value.rest_days ?? []).length;
  });

  const isStudyDaysValid = computed(() => computedStudyDays.value >= 1);

  // ── 加载状态 ──
  const saving = ref(false);
  const savedFlash = ref(false);

  // ── 未保存修改检测 ──
  const savedSnapshot = ref("");
  const showUnsavedModal = ref(false);
  let pendingLeaveResolve: ((ok: boolean) => void) | null = null;

  /** 记录当前已保存的缓冲快照（form + 教材），作为「未保存」基准 */
  function syncSavedSnapshot() {
    savedSnapshot.value = JSON.stringify({
      form: form.value,
      textbook: textbookForm.value,
    });
  }

  /** 是否有未保存的修改 */
  const hasUnsavedChanges = computed(() => {
    if (!form.value) return false;
    return JSON.stringify({ form: form.value, textbook: textbookForm.value }) !== savedSnapshot.value;
  });

  /** 确认离开设置页（有未保存修改时返回弹窗 Promise 拦截） */
  function confirmLeave(): boolean | Promise<boolean> {
    if (!hasUnsavedChanges.value) return true;
    showUnsavedModal.value = true;
    return new Promise<boolean>((resolve) => {
      pendingLeaveResolve = resolve;
    });
  }

  /** 放弃修改并离开 */
  function discardAndLeave() {
    showUnsavedModal.value = false;
    pendingLeaveResolve?.(true);
    pendingLeaveResolve = null;
  }

  /** 取消离开，留在页面继续编辑 */
  function cancelLeave() {
    showUnsavedModal.value = false;
    pendingLeaveResolve?.(false);
    pendingLeaveResolve = null;
  }

  // 离开路由守卫：存在未保存修改时弹出确认，避免误触侧边栏等导致改动丢失
  onBeforeRouteLeave(() => confirmLeave());

  /**
   * 弹窗关闭（遮罩/ESC/关闭按钮）视为取消离开，留在页面。
   * 此时路由守卫的 Promise 需被拒绝，否则导航挂起。
   */
  function onUnsavedModalClose() {
    cancelLeave();
  }

  // ── 教材保存（独立保存，立即生效） ──
  const textbookSaving = ref<Record<SubjectKey, boolean>>({
    math: false,
    english: false,
    politics: false,
    professional: false,
  });
  const textbookSavedFlash = ref<Record<SubjectKey, boolean>>({
    math: false,
    english: false,
    politics: false,
    professional: false,
  });

  async function saveTextbook(subject: SubjectKey) {
    textbookSaving.value[subject] = true;
    try {
      const value = textbookForm.value[subject].trim();
      await settingsApi.updateSubjectTextbook(subject, value || null);
      if (studyState.value) {
        studyState.value.subjects[subject].textbook = value || undefined;
      }
      syncSavedSnapshot();
      textbookSavedFlash.value[subject] = true;
      setTimeout(() => {
        textbookSavedFlash.value[subject] = false;
      }, 1800);
    } catch (e) {
      stateError.value = e instanceof Error ? e.message : String(e);
    } finally {
      textbookSaving.value[subject] = false;
    }
  }

  // ── 同步 form 从 store ──
  function syncFormFromStore() {
    const s = settingsStore.settings;
    if (!s) return;
    form.value = {
      user_name: s.user_name ?? "",
      show_greeting: s.show_greeting ?? true,
      target_score: s.target_score ?? 0,
      exam_date: s.exam_date ?? "",
      start_time: s.study_schedule?.start_time ?? "09:00",
      end_time: s.study_schedule?.end_time ?? "22:00",
      daily_target_hours: s.study_schedule?.daily_target_hours ?? 5,
      review_reminder_time: s.study_schedule?.review_reminder_time ?? "23:00",
      rest_days: s.study_schedule?.rest_days?.length ? [...s.study_schedule.rest_days] : ["周日"],
      daily_task_count: s.study_schedule?.daily_task_count ?? 3,
      standard_granularity: s.study_schedule?.standard_granularity ?? 1.5,
      enable_review_tasks: s.study_schedule?.enable_review_tasks ?? true,
      enable_time_tracking: s.study_schedule?.enable_time_tracking ?? false,
      subject_start_dates: {
        math: s.study_schedule?.subject_start_dates?.math ?? "",
        english: s.study_schedule?.subject_start_dates?.english ?? "",
        politics: s.study_schedule?.subject_start_dates?.politics ?? "",
        professional: s.study_schedule?.subject_start_dates?.professional ?? "",
      },
      subject_time_allocation: s.study_schedule?.subject_time_allocation
        ? { ...s.study_schedule.subject_time_allocation }
        : null,
    };
  }

  // ── 同步 form 从 state（教材） ──
  function syncTextbookFormFromState() {
    const s = studyState.value?.subjects;
    if (!s) return;
    textbookForm.value = {
      math: s.math?.textbook ?? "",
      english: s.english?.textbook ?? "",
      politics: s.politics?.textbook ?? "",
      professional: s.professional?.textbook ?? "",
    };
  }

  // ── 加载 StudyState ──
  async function loadStudyState() {
    stateLoading.value = true;
    stateError.value = null;
    try {
      studyState.value = await settingsApi.getState();
      syncTextbookFormFromState();
    } catch (e) {
      stateError.value = e instanceof Error ? e.message : String(e);
    } finally {
      stateLoading.value = false;
    }
  }

  // ── 保存 ──
  async function handleSave() {
    if (!isStudyDaysValid.value || !form.value || !settingsStore.settings) {
      return;
    }
    saving.value = true;
    try {
      const s = settingsStore.settings;
      s.user_name = form.value.user_name;
      s.show_greeting = form.value.show_greeting;
      s.target_score = form.value.target_score;
      s.exam_date = form.value.exam_date;
      s.study_schedule = {
        ...s.study_schedule,
        start_time: form.value.start_time,
        end_time: form.value.end_time,
        daily_target_hours: form.value.daily_target_hours,
        review_reminder_time: form.value.review_reminder_time,
        rest_days: [...form.value.rest_days],
        study_days_per_week: 7 - form.value.rest_days.length,
        daily_task_count: form.value.daily_task_count,
        standard_granularity: form.value.standard_granularity,
        enable_review_tasks: form.value.enable_review_tasks,
        enable_time_tracking: form.value.enable_time_tracking,
        subject_start_dates: { ...form.value.subject_start_dates },
        subject_time_allocation: form.value.subject_time_allocation
          ? { ...form.value.subject_time_allocation }
          : null,
      };
      await settingsStore.save();
      await settingsStore.load();
      syncFormFromStore();
      syncSavedSnapshot();
      savedFlash.value = true;
      setTimeout(() => {
        savedFlash.value = false;
      }, 1800);
    } finally {
      saving.value = false;
    }
  }

  return {
    weekdayOptions,
    form,
    textbookForm,
    studyState,
    stateLoading,
    stateError,
    subjectActive,
    professionalName,
    toggleRestDay,
    computedStudyDays,
    isStudyDaysValid,
    saving,
    savedFlash,
    savedSnapshot,
    showUnsavedModal,
    hasUnsavedChanges,
    confirmLeave,
    discardAndLeave,
    cancelLeave,
    onUnsavedModalClose,
    textbookSaving,
    textbookSavedFlash,
    saveTextbook,
    syncFormFromStore,
    syncTextbookFormFromState,
    loadStudyState,
    handleSave,
    syncSavedSnapshot,
  };
}
