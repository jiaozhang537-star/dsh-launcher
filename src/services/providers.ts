/** Tauri invoke 的类型化薄封装：模型提供方目录（FR-20）/ 排序 / 自定义。 */
import { invoke } from "./tauri";

export type ApiStyle = "openai" | "responses" | "anthropic" | "gemini" | "ollama";

export interface CatalogEntry {
  id: string;
  label: string;
  /** 目录默认 API 地址（卡片地址留空 = 用它）。 */
  base: string;
  style: ApiStyle;
  /** simple-icons slug；null = 字母块兜底。 */
  icon: string | null;
  /** `curated` = 启动器精选；`dsh` = DSH 本体 llm 层原生实装。 */
  group: string;
}

export interface CustomProvider {
  /** 稳定 id：`custom-<slug>`（Key/模型/地址照旧存凭据库）。 */
  id: string;
  name: string;
  base: string;
  style: ApiStyle;
}

export interface ProvidersView {
  catalog: CatalogEntry[];
  custom: CustomProvider[];
  /** 完整卡片顺序（内置 ∪ 自定义）；空 = 目录缺省顺序。 */
  order: string[];
}



/** 提供方目录视图（内置 11 类 + 自定义 + 排序）。 */
export function fetchProvidersView(): Promise<ProvidersView> {
  return invoke<ProvidersView>("providers_view");
}

/** 保存拖拽排序（必须恰好覆盖全部内置 ∪ 自定义 id）。 */
export function setProvidersOrder(order: string[]): Promise<void> {
  return invoke<void>("providers_set_order", { order });
}

/** 新增自定义提供方（与内置同权；id 自动生成）。 */
export function addCustomProvider(name: string, base: string, style: ApiStyle): Promise<CustomProvider> {
  return invoke<CustomProvider>("providers_add_custom", { name, base, style });
}

/** 删除自定义提供方（内置目录拒绝；连带清除该厂商的凭据库记录）。 */
export function removeCustomProvider(id: string): Promise<void> {
  return invoke<void>("providers_remove_custom", { id });
}

/**
 * 厂商在实例里的**提供方 id**（写进 `agent-default-model.provider` 的值）：
 * `deepseek` → `deepseek-official`，目录厂商走目录 route key（如 `kimi` →
 * `moonshotai-cn`），自描述厂商是自己的 id，`null` = 落不了路由（如 gemini
 * 形态的自定义提供方）。判定由后端单点给出，前端不另立一套。
 */
export function providerRouteKey(vendor: string): Promise<string | null> {
  return invoke<string | null>("provider_route_key", { vendor });
}
