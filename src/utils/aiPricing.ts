/**
 * AI 模型定价表与费用估算工具
 *
 * 价格数据核对于 2026 年 8 月 31 日，单位为「元 / 百万 Token」（USD 条目按 7.2 汇率折算展示）。
 * 厂商可能随时调整定价，使用 `fetchLatestPricingNote()` 获取更新提示。
 *
 * 数据来源：
 *  - DeepSeek: api-docs.deepseek.com
 *  - 通义千问（阿里百炼）: help.aliyun.com/百炼
 *  - 智谱 GLM: open.bigmodel.cn
 *  - OpenAI: openai.com/api/pricing/
 *  - Anthropic: www.anthropic.com/pricing
 *  - Google Gemini: ai.google.dev/pricing
 */

/**
 * 单个模型的定价信息
 *
 * 价格单位：元（人民币）/ 百万 Token
 * 1 元 = 1_000_000 / 1_000_000 = 1 单位价格对应 1M token
 */
export interface ModelPricing {
  /** 模型 ID 关键字（用于在 `model` 字符串中模糊匹配，小写） */
  pattern: string;
  /** 显示名称 */
  label: string;
  /** 输入价格（元 / 百万 Token） */
  inputPrice: number;
  /** 输出价格（元 / 百万 Token） */
  outputPrice: number;
  /** 缓存命中时的输入价格（元 / 百万 Token），未注明则与 inputPrice 相同 */
  cachedInputPrice?: number;
  /** 币种：默认 CNY */
  currency: "CNY" | "USD";
  /** 备注（如限时折扣、免费额度等） */
  note?: string;
}

/**
 * 已知模型定价表
 *
 * 匹配规则：按数组顺序遍历，使用第一个匹配的 `pattern`（小写包含匹配）。
 * 因此通用关键字应放在具体关键字之后（如 "deepseek-chat" 应在 "deepseek" 之前）。
 */
const PRICING_TABLE: ModelPricing[] = [
  // ── DeepSeek（2026-08-17 起峰谷计费：9:00-12:00、14:00-18:00 为高峰，其余及周末全天为低谷半价）──
  {
    pattern: "deepseek-v4-flash",
    label: "DeepSeek V4 Flash",
    inputPrice: 3,
    outputPrice: 9,
    cachedInputPrice: 0.1,
    currency: "CNY",
    note: "高峰时段价；低谷时段半价（¥1.5/¥4.5，缓存命中 ¥0.05）",
  },
  {
    pattern: "deepseek-v4-pro",
    label: "DeepSeek V4 Pro",
    inputPrice: 9,
    outputPrice: 27,
    cachedInputPrice: 0.3,
    currency: "CNY",
    note: "高峰时段价；低谷时段半价（¥4.5/¥13.5，缓存命中 ¥0.15）",
  },
  {
    pattern: "deepseek",
    label: "DeepSeek (通用)",
    inputPrice: 3,
    outputPrice: 9,
    cachedInputPrice: 0.1,
    currency: "CNY",
    note: "默认按 V4 Flash 高峰价估算",
  },

  // ── 通义千问（阿里百炼）──
  {
    pattern: "qwen3.7-max",
    label: "Qwen3.7 Max",
    inputPrice: 2.5,
    outputPrice: 7.5,
    currency: "CNY",
  },
  {
    pattern: "qwen3.7-plus",
    label: "Qwen3.7 Plus",
    inputPrice: 1.5,
    outputPrice: 4.5,
    currency: "CNY",
  },
  {
    pattern: "qwen3.7-flash",
    label: "Qwen3.7 Flash",
    inputPrice: 0.8,
    outputPrice: 2.4,
    currency: "CNY",
  },
  {
    pattern: "qwen3-max",
    label: "Qwen3 Max",
    inputPrice: 2.5,
    outputPrice: 10,
    currency: "CNY",
  },
  {
    pattern: "qwen-plus",
    label: "Qwen Plus",
    inputPrice: 0.8,
    outputPrice: 2,
    currency: "CNY",
  },
  {
    pattern: "qwen-turbo",
    label: "Qwen Turbo",
    inputPrice: 0.3,
    outputPrice: 0.6,
    currency: "CNY",
  },
  {
    pattern: "qwen-long",
    label: "Qwen Long",
    inputPrice: 0.5,
    outputPrice: 0.5,
    currency: "CNY",
  },
  {
    pattern: "qwen",
    label: "Qwen (通用)",
    inputPrice: 0.8,
    outputPrice: 2,
    currency: "CNY",
  },

  // ── 智谱 GLM ──
  {
    pattern: "glm-4.7-flash",
    label: "GLM-4.7 Flash",
    inputPrice: 0,
    outputPrice: 0,
    currency: "CNY",
    note: "永久免费",
  },
  {
    pattern: "glm-5.3-flash",
    label: "GLM-5.3 Flash",
    inputPrice: 1.1,
    outputPrice: 3.6,
    currency: "CNY",
    note: "官方约 $0.15/$0.50 每百万 token，限时活动价更低",
  },
  {
    pattern: "glm-5.3",
    label: "GLM-5.3",
    inputPrice: 8,
    outputPrice: 28,
    currency: "CNY",
    note: "定价与 GLM-5.2 持平",
  },
  {
    pattern: "glm-5.2",
    label: "GLM-5.2",
    inputPrice: 8,
    outputPrice: 28,
    currency: "CNY",
  },
  {
    pattern: "glm-5.1",
    label: "GLM-5.1",
    inputPrice: 6,
    outputPrice: 24,
    currency: "CNY",
  },
  {
    pattern: "glm-4-plus",
    label: "GLM-4 Plus",
    inputPrice: 5,
    outputPrice: 5,
    currency: "CNY",
  },
  {
    pattern: "glm-4-flash",
    label: "GLM-4 Flash",
    inputPrice: 0,
    outputPrice: 0,
    currency: "CNY",
    note: "免费",
  },
  {
    pattern: "glm",
    label: "GLM (通用)",
    inputPrice: 5,
    outputPrice: 5,
    currency: "CNY",
  },

  // ── Kimi ──
  {
    pattern: "kimi-k3",
    label: "Kimi K3",
    inputPrice: 21.6,
    outputPrice: 108,
    currency: "CNY",
    note: "官方 $3/$15 每百万 token 折算；缓存命中输入 $0.30",
  },
  {
    pattern: "kimi-k2.6",
    label: "Kimi K2.6",
    inputPrice: 6.5,
    outputPrice: 27,
    currency: "CNY",
  },
  {
    pattern: "moonshot",
    label: "Moonshot (通用)",
    inputPrice: 12,
    outputPrice: 12,
    currency: "CNY",
  },

  // ── MiniMax ──
  {
    pattern: "minimax-m3",
    label: "MiniMax M3",
    inputPrice: 4.2,
    outputPrice: 16.8,
    currency: "CNY",
    note: "512K 以内上下文档原价；512K-1M 档为 ¥8.4/¥33.6，限时活动另有五折",
  },

  // ── LongCat（美团） ──
  {
    pattern: "longcat-2.0",
    label: "LongCat-2.0",
    inputPrice: 5,
    outputPrice: 20,
    currency: "CNY",
    note: "原价；平台初上线限时折扣价 ¥2/¥8",
  },

  // ── OpenAI ──
  {
    pattern: "gpt-5.6-sol",
    label: "GPT-5.6 Sol",
    inputPrice: 4,
    outputPrice: 20,
    cachedInputPrice: 0.4,
    currency: "USD",
    note: "2026-08 降价后（原 $5/$30）；272K 以上长上下文档 $8/$30",
  },
  {
    pattern: "gpt-5.6-terra",
    label: "GPT-5.6 Terra",
    inputPrice: 2.5,
    outputPrice: 15,
    currency: "USD",
  },
  {
    pattern: "gpt-5.6-luna",
    label: "GPT-5.6 Luna",
    inputPrice: 1,
    outputPrice: 6,
    currency: "USD",
  },
  {
    pattern: "gpt-5.6",
    label: "GPT-5.6",
    inputPrice: 4,
    outputPrice: 20,
    currency: "USD",
    note: "未识别子型号时按 Sol 降价后价格估算",
  },
  {
    pattern: "gpt-5.5-pro",
    label: "GPT-5.5 Pro",
    inputPrice: 30,
    outputPrice: 180,
    currency: "USD",
  },
  {
    pattern: "gpt-5.5",
    label: "GPT-5.5",
    inputPrice: 5,
    outputPrice: 30,
    cachedInputPrice: 2.5,
    currency: "USD",
  },
  {
    pattern: "gpt-5.2",
    label: "GPT-5.2",
    inputPrice: 1.75,
    outputPrice: 14,
    currency: "USD",
  },
  {
    pattern: "gpt-5",
    label: "GPT-5",
    inputPrice: 1.25,
    outputPrice: 10,
    currency: "USD",
  },
  {
    pattern: "gpt-4o-mini",
    label: "GPT-4o mini",
    inputPrice: 0.15,
    outputPrice: 0.6,
    currency: "USD",
  },
  {
    pattern: "gpt-4o",
    label: "GPT-4o",
    inputPrice: 2.5,
    outputPrice: 10,
    currency: "USD",
  },
  {
    pattern: "gpt-4-turbo",
    label: "GPT-4 Turbo",
    inputPrice: 10,
    outputPrice: 30,
    currency: "USD",
  },
  {
    pattern: "gpt-3.5",
    label: "GPT-3.5 Turbo",
    inputPrice: 0.5,
    outputPrice: 1.5,
    currency: "USD",
  },

  // ── Anthropic Claude ──
  {
    pattern: "claude-opus-4",
    label: "Claude Opus 4.x",
    inputPrice: 5,
    outputPrice: 25,
    currency: "USD",
    note: "Opus 4.x 起大幅降价；Opus 3 为 $15/$75",
  },
  {
    pattern: "claude-opus",
    label: "Claude Opus (3.x)",
    inputPrice: 15,
    outputPrice: 75,
    currency: "USD",
  },
  {
    pattern: "claude-sonnet",
    label: "Claude Sonnet",
    inputPrice: 3,
    outputPrice: 15,
    currency: "USD",
  },
  {
    pattern: "claude-haiku-4",
    label: "Claude Haiku 4.5",
    inputPrice: 1,
    outputPrice: 5,
    currency: "USD",
  },
  {
    pattern: "claude-haiku",
    label: "Claude Haiku",
    inputPrice: 0.25,
    outputPrice: 1.25,
    currency: "USD",
  },

  // ── Google Gemini ──
  {
    pattern: "gemini-3",
    label: "Gemini 3 Pro / 3.1 Pro",
    inputPrice: 2,
    outputPrice: 12,
    currency: "USD",
    note: "200K 以上长文档档为 $4/$18",
  },
  {
    pattern: "gemini-2.5-pro",
    label: "Gemini 2.5 Pro",
    inputPrice: 1.25,
    outputPrice: 10,
    currency: "USD",
  },
  {
    pattern: "gemini-2.5-flash",
    label: "Gemini 2.5 Flash",
    inputPrice: 0.3,
    outputPrice: 2.5,
    currency: "USD",
  },
  {
    pattern: "gemini-1.5-pro",
    label: "Gemini 1.5 Pro",
    inputPrice: 1.25,
    outputPrice: 5,
    currency: "USD",
  },
  {
    pattern: "gemini-1.5-flash",
    label: "Gemini 1.5 Flash",
    inputPrice: 0.075,
    outputPrice: 0.3,
    currency: "USD",
  },
  {
    pattern: "gemini",
    label: "Gemini (通用)",
    inputPrice: 0.075,
    outputPrice: 0.3,
    currency: "USD",
  },
];

/** 美元兑人民币估算汇率（用于将 USD 定价换算为人民币展示） */
const USD_TO_CNY_ESTIMATE = 7.2;

/** 估算结果 */
export interface CostEstimate {
  /** 估算费用（人民币元） */
  costCny: number;
  /** 匹配到的定价条目（null 表示未匹配到，按 0 元计算） */
  pricing: ModelPricing | null;
  /** 是否为估算值（汇率换算/未匹配都会导致估算） */
  isEstimate: boolean;
  /** 说明文本 */
  note: string;
}

/**
 * 根据模型名与 token 用量估算单次调用费用
 *
 * @param model 模型名（来自 ChatResponse.model）
 * @param promptTokens 输入 token 数
 * @param completionTokens 输出 token 数
 * @returns 估算结果（人民币元）
 */
export function estimateCost(
  model: string,
  promptTokens: number,
  completionTokens: number,
): CostEstimate {
  if (!model) {
    return {
      costCny: 0,
      pricing: null,
      isEstimate: true,
      note: "无模型信息，按 0 元计算",
    };
  }

  const pricing = matchPricing(model);
  if (!pricing) {
    return {
      costCny: 0,
      pricing: null,
      isEstimate: true,
      note: `未找到「${model}」的定价信息，按 0 元计算`,
    };
  }

  // 输入费用 = (promptTokens / 1_000_000) * inputPrice
  const inputCost = (promptTokens / 1_000_000) * pricing.inputPrice;
  // 输出费用 = (completionTokens / 1_000_000) * outputPrice
  const outputCost = (completionTokens / 1_000_000) * pricing.outputPrice;
  // 折算为人民币
  const costCny =
    pricing.currency === "USD"
      ? (inputCost + outputCost) * USD_TO_CNY_ESTIMATE
      : inputCost + outputCost;

  const isEstimate = pricing.currency === "USD";
  const note = isEstimate
    ? `${pricing.label}（$${pricing.inputPrice}/$${pricing.outputPrice} per 1M，按 1$≈${USD_TO_CNY_ESTIMATE}¥ 估算）`
    : `${pricing.label}（¥${pricing.inputPrice}/¥${pricing.outputPrice} per 1M Token）${pricing.note ? "；" + pricing.note : ""}`;

  return {
    costCny,
    pricing,
    isEstimate,
    note,
  };
}

/** 根据模型名匹配定价表（按顺序匹配第一个 pattern） */
function matchPricing(model: string): ModelPricing | null {
  const lower = model.toLowerCase();
  for (const p of PRICING_TABLE) {
    if (lower.includes(p.pattern)) {
      return p;
    }
  }
  return null;
}

/** 获取定价表更新提示 */
export function fetchLatestPricingNote(): string {
  return "价格数据核对于 2026 年 8 月 31 日，仅供参考。请以各家厂商官方最新价格为准。";
}

/** 格式化费用为可读字符串 */
export function formatCost(costCny: number): string {
  if (costCny === 0) return "¥0.00";
  if (costCny < 0.01) return `¥${costCny.toFixed(6)}`;
  if (costCny < 1) return `¥${costCny.toFixed(4)}`;
  return `¥${costCny.toFixed(2)}`; // M40：1 元以上保留两位小数
}

/** 格式化 token 数为可读字符串（如 1.2k / 12.5k / 1.2M） */
export function formatTokens(tokens: number): string {
  if (tokens < 1000) return `${tokens}`;
  if (tokens < 1_000_000) return `${(tokens / 1000).toFixed(1)}k`;
  return `${(tokens / 1_000_000).toFixed(2)}M`;
}
