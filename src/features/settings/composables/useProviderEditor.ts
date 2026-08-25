/**
 * 设置页 — AI Provider 编辑逻辑
 *
 * 承载 Provider 表单状态、模型列表动态加载、连接测试、保存/删除/设默认，
 * 以及「上下文超限自动修正 Max Tokens」逻辑（原 SettingsView 中 Provider 相关逻辑）。
 */
import { ref, computed } from "vue";
import { useSettingsStore } from "@/stores/settings";
import { settingsApi } from "../api";
import type { AIProviderConfig, ProviderType, ModelInfo } from "@/types";

export function useProviderEditor() {
  const settingsStore = useSettingsStore();

  // ── Provider 表单状态 ──
  const showProviderForm = ref(false);
  const editingProviderId = ref<string | null>(null);
  // C5：编辑已存在 Provider 时，在内存保留原 api_key（不回显到输入框/表单）。
  // 用户未重新输入 key 时，保存/测试沿用该原 key。
  const editingOriginalKey = ref("");
  const showApiKey = ref(false);
  const testing = ref(false);
  const testResult = ref<string | null>(null);

  function emptyProvider(): AIProviderConfig {
    return {
      id: "",
      name: "",
      type: "openai",
      base_url: "",
      api_key: "",
      model: "",
      temperature: 0.7,
      max_tokens: 4096,
      enabled: true,
      is_default: false,
    };
  }

  const providerForm = ref<AIProviderConfig>(emptyProvider());

  // ── 模型列表加载（基于当前 base_url + api_key 动态获取）──
  const modelList = ref<ModelInfo[]>([]);
  const modelListLoading = ref(false);
  const modelListError = ref<string | null>(null);
  const showModelDropdown = ref(false);
  const modelSearchKeyword = ref("");

  /** 过滤后的模型列表 */
  const filteredModels = computed(() => {
    const kw = modelSearchKeyword.value.trim().toLowerCase();
    if (!kw) return modelList.value;
    return modelList.value.filter((m) => m.id.toLowerCase().includes(kw));
  });

  /** 从 ModelInfo.extra 中尝试提取上下文长度 */
  function modelContextLength(m: ModelInfo): number | null {
    const extra = m.extra as Record<string, unknown>;
    // 常见字段名：context_length / context_window / max_context_length / max_input_tokens；
    // _studyagent_ctx_len 为后端在服务商未返回字段时按模型名查表注入的兜底值
    const candidates = ["context_length", "context_window", "max_context_length", "max_input_tokens", "context", "_studyagent_ctx_len"];
    for (const key of candidates) {
      const v = extra[key];
      if (typeof v === "number" && v > 0) return v;
      if (typeof v === "string") {
        const n = parseInt(v, 10);
        if (!isNaN(n) && n > 0) return n;
      }
    }
    return null;
  }

  function formatContextLength(n: number | null): string {
    if (!n) return "";
    if (n >= 1000) return `${(n / 1000).toFixed(0)}K`;
    return String(n);
  }

  /** 当前表单的有效 API Key：优先用户新输入，否则沿用编辑时保留的原 key（C5） */
  function effectiveApiKey(): string {
    return providerForm.value.api_key || editingOriginalKey.value;
  }

  /** 加载模型列表（使用当前表单中的 base_url + api_key） */
  async function loadModelList() {
    const cfg = { ...providerForm.value, api_key: effectiveApiKey() };
    if (!cfg.base_url.trim()) {
      modelListError.value = "请先填写 Base URL";
      return;
    }
    if (!cfg.api_key.trim() && cfg.type !== "ollama") {
      modelListError.value = "请先填写 API Key";
      return;
    }
    modelListLoading.value = true;
    modelListError.value = null;
    try {
      const list = await settingsApi.listAIModels(cfg);
      modelList.value = list;
      if (list.length === 0) {
        modelListError.value = "未获取到模型，请检查配置或手动输入";
      }
      showModelDropdown.value = true;
    } catch (e) {
      modelListError.value = e instanceof Error ? e.message : String(e);
      modelList.value = [];
    } finally {
      modelListLoading.value = false;
    }
  }

  function selectModel(modelId: string) {
    providerForm.value.model = modelId;
    showModelDropdown.value = false;
    modelSearchKeyword.value = "";
  }

  const providerTypeOptions: { value: ProviderType; label: string }[] = [
    { value: "openai", label: "OpenAI" },
    { value: "gemini", label: "Gemini" },
    { value: "anthropic", label: "Anthropic" },
    { value: "ollama", label: "Ollama (本地)" },
    { value: "openrouter", label: "OpenRouter" },
    { value: "siliconflow", label: "硅基流动" },
    { value: "dashscope", label: "通义千问" },
    { value: "volcengine", label: "火山引擎" },
    { value: "custom", label: "自定义" },
  ];

  function startAddProvider() {
    editingProviderId.value = null;
    editingOriginalKey.value = "";
    providerForm.value = emptyProvider();
    testResult.value = null;
    modelList.value = [];
    modelListError.value = null;
    showModelDropdown.value = false;
    modelSearchKeyword.value = "";
    showProviderForm.value = true;
  }

  function editProvider(p: AIProviderConfig) {
    editingProviderId.value = p.id;
    // C5：不回显原 api_key 到表单；仅在内存保留，供未重输时测试/保存沿用。
    editingOriginalKey.value = p.api_key || "";
    providerForm.value = { ...p, api_key: "" };
    testResult.value = null;
    modelList.value = [];
    modelListError.value = null;
    showModelDropdown.value = false;
    modelSearchKeyword.value = "";
    showProviderForm.value = true;
  }

  function cancelProviderForm() {
    showProviderForm.value = false;
    editingProviderId.value = null;
    editingOriginalKey.value = "";
    testResult.value = null;
  }

  async function saveProvider() {
    if (!providerForm.value.name.trim()) return;
    // 需求：修改 Provider 后必须先测试连接，成功才保存
    testing.value = true;
    testResult.value = null;
    try {
      const result = await settingsApi.testAIProvider({ ...providerForm.value, api_key: effectiveApiKey() });
      // aiInvoke 已把 success:false 转为抛错；能走到这说明连接成功
      if (!result.success) {
        testResult.value = `测试失败：${result.message}`;
        return;
      }
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e);
      testResult.value = `测试失败：${msg}`;
      // 识别上下文超限报错并自动修正 max_tokens
      applyContextLimitFix(msg);
      return;
    } finally {
      testing.value = false;
    }

    // C5：保存时若用户未重新输入 key，则沿用原 key（表单留空 → 保留，而非清空）
    const keyToSave = providerForm.value.api_key || editingOriginalKey.value;
    const payload = { ...providerForm.value, api_key: keyToSave };

    if (editingProviderId.value) {
      settingsStore.updateProvider(editingProviderId.value, payload);
    } else {
      // 第一个 provider 自动设为默认，避免 default_provider_id 为空导致其他页面显示「未配置」
      const isFirst = (settingsStore.settings?.ai_providers.length ?? 0) === 0;
      settingsStore.addProvider({
        ...providerForm.value,
        id: `provider-${Date.now()}`,
        is_default: providerForm.value.is_default || isFirst,
      });
    }
    showProviderForm.value = false;
    await settingsStore.save();
    editingProviderId.value = null;
    testResult.value = null;
  }

  async function removeProvider(id: string) {
    const provider = settingsStore.aiProviders.find((item) => item.id === id);
    if (!provider) return;
    const defaultWarning = provider.is_default ? " 这是当前默认 Provider，删除后将自动切换到列表中的下一项。" : "";
    if (!window.confirm(`确定删除 AI Provider「${provider.name}」吗？${defaultWarning}`)) return;
    settingsStore.removeProvider(id);
    // H33：修改后立即持久化，避免刷新丢失
    await settingsStore.save();
  }

  async function setDefaultProvider(id: string) {
    const s = settingsStore.settings;
    if (!s) return;
    s.ai_providers.forEach((p) => {
      p.is_default = p.id === id;
    });
    s.default_provider_id = id;
    // H33：修改后立即持久化，避免刷新丢失
    await settingsStore.save();
  }

  async function handleTestProvider() {
    testing.value = true;
    testResult.value = null;
    try {
      const result = await settingsApi.testAIProvider({ ...providerForm.value, api_key: effectiveApiKey() });
      testResult.value = result.success ? (result.message || "连接成功") : `测试失败：${result.message}`;
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e);
      testResult.value = msg;
      // 识别上下文超限报错并自动修正 max_tokens
      applyContextLimitFix(msg);
    } finally {
      testing.value = false;
    }
  }

  /** 从错误信息中解析服务商返回的上下文最大 token 数（识别超限报错） */
  function parseContextLimit(message: string): number | null {
    const patterns = [
      /maximum context length is (\d[\d,]*)/i,
      /maximum context length of (\d[\d,]*)/i,
      /context length of (\d[\d,]*)/i,
      /maximum input token count[^\d]*(\d[\d,]*)/i,
      /max(?:imum)? input tokens?[^\d]*(\d[\d,]*)/i,
      /context window[^\d]*(\d[\d,]*)/i,
    ];
    for (const re of patterns) {
      const m = message.match(re);
      if (m) {
        const n = parseInt(m[1].replace(/,/g, ""), 10);
        if (!isNaN(n) && n > 0) return n;
      }
    }
    return null;
  }

  /** 识别上下文超限报错：自动把 Max Tokens 调整到合理范围，返回是否修正 */
  function applyContextLimitFix(message: string): boolean {
    const limit = parseContextLimit(message);
    if (!limit) return false;
    // 自适应输出预算：窗口越大输出占比越高（大窗口下输入通常占不满）
    const ratio = limit <= 16_384 ? 0.25 : limit <= 131_072 ? 0.5 : 0.75;
    const safe = Math.max(1, Math.floor(limit * ratio));
    const current = providerForm.value.max_tokens ?? 0;
    if (current === 0 || current > limit) {
      providerForm.value.max_tokens = safe;
      testResult.value = `检测到上下文超限（最大 ${limit.toLocaleString()} tokens），已将 Max Tokens 自动调整为 ${safe.toLocaleString()}，请重新测试连接。`;
      return true;
    }
    return false;
  }

  return {
    showProviderForm,
    editingProviderId,
    editingOriginalKey,
    showApiKey,
    testing,
    testResult,
    providerForm,
    modelList,
    modelListLoading,
    modelListError,
    showModelDropdown,
    modelSearchKeyword,
    filteredModels,
    modelContextLength,
    formatContextLength,
    providerTypeOptions,
    startAddProvider,
    editProvider,
    cancelProviderForm,
    saveProvider,
    removeProvider,
    setDefaultProvider,
    handleTestProvider,
    loadModelList,
    selectModel,
  };
}
