/** Tauri invoke 的类型化薄封装：提供方模型列表拉取（FR-06「模型拉取」）。 */
import { invoke } from "./tauri";

/** 从提供方 API（OpenAI 兼容 /models）拉取可用模型 id。地址空 = 默认地址。 */
export async function fetchModels(vendor: string, address: string): Promise<string[]> {
  return invoke<string[]>("provider_fetch_models", {
    vendor,
    address: address.trim() === "" ? null : address.trim(),
  });
}
