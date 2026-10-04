/** 内置提供商 id → 本地图标路径（public/vendor-logos/，素材源自各提供商官方渠道，见 素材/LOGOS.md）。 */

/**
 * 目录 id → 品牌图文件名（不含扩展名）。同品牌变体共用一张图：
 * kimi/kimi-coding→kimi，qwen-token-plan(-cn)→qwen，
 * minimax-cn→minimax，zai-coding-cn→glm（智谱现以 Z.ai 品牌统一）。
 * zai/moonshotai 已随撞名合并裁撤（并入 glm/kimi 卡，2026-10-03）。
 */
const PROVIDER_ICON_BRANDS: Readonly<Record<string, string>> = {
  deepseek: "deepseek",
  glm: "glm",
  "zai-coding-cn": "glm",
  kimi: "kimi",
  "kimi-coding": "kimi",
  qwen: "qwen",
  "qwen-token-plan": "qwen",
  "qwen-token-plan-cn": "qwen",
  openai: "openai",
  anthropic: "anthropic",
  xai: "xai",
  gemini: "gemini",
  siliconflow: "siliconflow",
  openrouter: "openrouter",
  ollama: "ollama",
  groq: "groq",
  cerebras: "cerebras",
  together: "together",
  nvidia: "nvidia",
  huggingface: "huggingface",
  "ant-ling": "ant-ling",
  xiaomi: "xiaomi",
  minimax: "minimax",
  "minimax-cn": "minimax",
  fireworks: "fireworks",
  "vercel-ai-gateway": "vercel",
};

/** 提供商本地图标路径；自定义提供商（custom-*）不在表内，返回 undefined 走字母块兜底。 */
export function providerIconPath(id: string): string | undefined {
  const brand = PROVIDER_ICON_BRANDS[id];
  return brand ? `/vendor-logos/${brand}.png` : undefined;
}
