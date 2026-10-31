/** Tauri invoke 的类型化薄封装：多厂商 API Key 保险库（FR-06）。 */
import { invoke } from "./tauri";

export interface VendorKeyStatus {
  vendor: string;
  /** 设置页勾选启用；与是否有 Key 解耦。 */
  enabled: boolean;
  /** 保险库里已有 Key。 */
  hasKey: boolean;
  /** 环境变量里已有 Key（自动配置通道，由后端探测）。 */
  envAuto: boolean;
  /** 已添加的模型 id 列表。 */
  models: string[];
  /** 提供方 API 地址覆盖；null = 默认地址。 */
  address: string | null;
}

export interface KeyvaultStatus {
  vendors: VendorKeyStatus[];
}

/** 内置默认模型（镜像 dsh-llm-deepseek DEFAULT_MODELS id），仅作设置页默认勾选项。 */
export const DEEPSEEK_MODELS = ["deepseek-v4-flash", "deepseek-v4-pro"] as const;

/**
 * key 输入框「已有 Key」态的哑掩码值：password 框把它渲染成圆点串。
 * 保存时值等于哨兵 = 用户未修改，不提交；明文永不回显。
 */
export const KEY_MASK = "dsh-masked-key";



/** 读保险库状态（多厂商视图；无密钥材料）。 */
export function getStatus(): Promise<KeyvaultStatus> {
  return invoke<KeyvaultStatus>("keyvault_status");
}

/** 勾选/取消启用厂商（与 Key 解耦）。 */
export function setEnabled(vendor: string, enabled: boolean): Promise<void> {
  return invoke<void>("keyvault_set_enabled", { vendor, enabled });
}

/** 保存 API Key（DPAPI 加密落盘；明文不回读）。 */
export function setKey(vendor: string, key: string): Promise<void> {
  return invoke<void>("keyvault_set_key", { vendor, key });
}

/** 覆盖厂商的已添加模型列表。 */
export function setModels(vendor: string, models: string[]): Promise<void> {
  return invoke<void>("keyvault_set_models", { vendor, models });
}

/** 保存提供方 API 地址覆盖；空串清除覆盖（回落默认地址）。 */
export function setAddress(vendor: string, address: string): Promise<void> {
  return invoke<void>("keyvault_set_address", { vendor, address });
}

/** 清除厂商已存 Key（保留启用状态与模型列表）。 */
export function clearKey(vendor: string): Promise<void> {
  return invoke<void>("keyvault_clear_key", { vendor });
}

/** 把厂商从模型厂商列表整条移除（Key/模型/启用态一并清除；按需添加式）。 */
export function forgetVendor(vendor: string): Promise<void> {
  return invoke<void>("keyvault_forget_vendor", { vendor });
}
