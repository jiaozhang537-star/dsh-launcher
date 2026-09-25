/** Tauri invoke 的类型化薄封装：整合包导出/导入/预览（P4，FR-04）。 */

export type ModpackType = "plugins" | "settings" | "mixed";

/** manifest v1（Rust modpack::Manifest 的 camelCase 视图）。 */
export interface ModpackManifest {
  formatVersion: number;
  type: ModpackType;
  name: string;
  generator: string;
  createdAt: string;
  plugins: { name: string; version: string; source: string; file: string }[];
  settings: {
    vendors: {
      vendor: string;
      enabled: boolean;
      models: string[];
      address: string | null;
      name: string | null;
      style: string | null;
    }[];
    injectSettings: Record<string, unknown>;
    agentsMd: { global: string | null; instance: string | null } | null;
  } | null;
}

/** 导出请求（Rust modpack::ExportOptions）。 */
export interface ModpackExportOptions {
  instanceId: string;
  name: string;
  plugins: string[];
  includeVendors: boolean;
  includeInject: boolean;
  includeAgentsMd: boolean;
}

/** 导入报告（Rust modpack::ImportReport）。 */
export interface ModpackImportReport {
  manifestName: string;
  installed: {
    instanceId: string;
    packageName: string;
    version: string;
    bundleActivated: boolean;
  }[];
  applied: string[];
  warnings: string[];
}

async function invoke<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  const { invoke: tauriInvoke } = await import("@tauri-apps/api/core");
  return tauriInvoke<T>(cmd, args);
}

/**
 * 导出整合包到 dest（保存对话框给的完整路径）；阻塞至完成，
 * 进度经 onDetail 回调（modpack-progress 事件）。
 */
export async function modpackExportWithProgress(
  opts: ModpackExportOptions,
  dest: string,
  onDetail: (detail: string) => void,
): Promise<ModpackManifest> {
  const { listen } = await import("@tauri-apps/api/event");
  const unlisten = await listen<{ detail: string }>("modpack-progress", (event) => {
    onDetail(event.payload.detail);
  });
  try {
    return await invoke<ModpackManifest>("modpack_export", { opts, dest });
  } finally {
    unlisten();
  }
}

/** 读取整合包清单（不解包；导入确认弹窗的预览数据源）。 */
export function modpackPeek(path: string): Promise<ModpackManifest> {
  return invoke<ModpackManifest>("modpack_peek", { path });
}

/**
 * 导入整合包：插件装进 instanceId，设置按 settingsTarget 落点写入
 * （包内无设置节时传 null）。失败整体回滚（Err 文案逐条可读）。
 */
export function modpackImport(
  path: string,
  instanceId: string,
  settingsTarget: "global" | "instance" | null,
): Promise<ModpackImportReport> {
  return invoke<ModpackImportReport>("modpack_import", { path, instanceId, settingsTarget });
}

/** 保存对话框：导出整合包的落点（取消 = null）。 */
export async function pickSaveTgz(defaultName: string): Promise<string | null> {
  const { save } = await import("@tauri-apps/plugin-dialog");
  return (await save({
    defaultPath: `${defaultName}.tgz`,
    filters: [{ name: "DSH 整合包", extensions: ["tgz"] }],
  })) as string | null;
}

/** 打开对话框：选择要导入的整合包文件（取消 = null）。 */
export async function pickOpenTgz(): Promise<string | null> {
  const { open } = await import("@tauri-apps/plugin-dialog");
  const picked = (await open({
    multiple: false,
    filters: [{ name: "DSH 整合包", extensions: ["tgz"] }],
  })) as string | string[] | null;
  if (Array.isArray(picked)) return picked[0] ?? null;
  return picked;
}
