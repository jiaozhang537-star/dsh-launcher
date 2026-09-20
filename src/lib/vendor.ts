/** 启动页厂商/模型视图推导（纯逻辑，vitest 覆盖）。 */

import { KEY_MASK, type KeyvaultStatus } from "../services/keyvault";

export interface VendorEntry {
  id: string;
  /** 胶囊显示文本。 */
  label: string;
  /** 厂商图标资源路径（public/ 下）；缺省时回退字母方块。 */
  icon?: string;
  /** 无图标时的兜底字符。 */
  monogram: string;
  /** 设置页勾选启用（决定它能否被启动页拉出）。 */
  enabled: boolean;
  /** 该厂商 Key 是否可得（保险库或环境变量，任一来源）。 */
  hasKey: boolean;
  /** 该厂商已添加的模型 id 列表。 */
  models: string[];
}

/**
 * 启动页厂商目录：默认仅 deepseek（P2 目录）；FR-20 落位后由启动页/设置页用
 * `setVendorCatalog` 注入完整目录（内置 11 类 + 自定义，来自后端
 * providers_view）。图标用本地 public/vendor-logos/（来源见 素材/LOGOS.md），
 * 缺省回退字母方块。
 */
export interface CatalogItem {
  id: string;
  label: string;
  icon?: string;
  monogram: string;
}

let vendorCatalog: ReadonlyArray<CatalogItem> = [
  { id: "deepseek", label: "deepseek", icon: "/vendor-logos/deepseek.png", monogram: "D" },
];

/** 注入完整厂商目录（providers 目录 → 展示素材映射）。 */
export function setVendorCatalog(catalog: ReadonlyArray<CatalogItem>): void {
  vendorCatalog = catalog;
}

/** 当前厂商目录（只读；测试与启动页共用）。 */
export function getVendorCatalog(): ReadonlyArray<CatalogItem> {
  return vendorCatalog;
}

export interface VendorView {
  entries: VendorEntry[];
  /** 当前生效的厂商 id。 */
  activeId: string;
}

function catalogOf(id: string) {
  return vendorCatalog.find((entry) => entry.id === id);
}

/**
 * 从保险库状态推导启动页厂商视图：**只拉出设置页启用的厂商**（启用与
 * 是否有 Key 解耦），目录提供展示素材，目录外的启用厂商忽略不虚构。
 * 没有任何启用厂商时回落 deepseek 占位（muted 呈现，启动会被校验链拦）。
 * Phase 2 保险库目录只有 deepseek，多厂商扩容后本函数签名不变。
 */
export function deriveVendorView(status: KeyvaultStatus | null): VendorView {
  const enabledVendors = (status?.vendors ?? []).filter((v) => v.enabled);
  const entries: VendorEntry[] = [];
  for (const vendor of enabledVendors) {
    const catalog = catalogOf(vendor.vendor);
    if (!catalog) continue;
    entries.push({
      id: catalog.id,
      label: catalog.label,
      icon: catalog.icon,
      monogram: catalog.monogram,
      enabled: true,
      hasKey: vendor.hasKey || vendor.envAuto,
      models: vendor.models,
    });
  }
  if (entries.length === 0) {
    const fallback = catalogOf("deepseek");
    entries.push({
      id: "deepseek",
      label: fallback?.label ?? "deepseek",
      icon: fallback?.icon,
      monogram: fallback?.monogram ?? "D",
      enabled: false,
      hasKey: false,
      models: [],
    });
  }
  return { entries, activeId: entries[0]!.id };
}

/**
 * 启动页模型选择框的候选项：所选（启用）厂商在设置页添加的模型列表；
 * 厂商未启用或无记录返回空列表（UI 呈现「到设置页添加」引导）。
 */
export function launchModelsFor(status: KeyvaultStatus | null, vendorId: string): string[] {
  return (
    status?.vendors.find((v) => v.vendor === vendorId && v.enabled)?.models ?? []
  );
}

/**
 * Key 输入的「未修改」判定（P3-02，启动页/设置页共用）：
 * dirty 标志位为第一判据——只有用户真实键入/粘贴（input 事件）才算改过；
 * 程序化置值（`.value` 赋值不触发 input）不会误置 dirty。掩码哨兵与空串
 * 恒判未修改：复制掩码回贴不会被当作新 Key 保存。
 */
export function keySaveEligible(value: string, userEdited: boolean): boolean {
  const trimmed = value.trim();
  return userEdited && trimmed !== "" && trimmed !== KEY_MASK;
}

/**
 * 会话掩码标记与保险库对账（P3-01）：权威状态（保险库/环境变量）里已无
 * Key 的厂商标记被撤销——「设置页清除 Key 后启动页仍显掩码」从状态上
 * 杜绝。status 读取失败（null）时不修剪：刚保存的掩码态不被瞬时故障闪掉。
 */
export function pruneSessionKeyMarks(
  marks: Set<string>,
  status: KeyvaultStatus | null,
): void {
  if (!status) return;
  for (const id of marks) {
    const vendor = status.vendors.find((v) => v.vendor === id);
    if (!vendor || (!vendor.hasKey && !vendor.envAuto)) {
      marks.delete(id);
    }
  }
}
