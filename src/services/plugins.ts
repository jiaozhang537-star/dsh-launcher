/** Tauri invoke 的类型化薄封装：插件安装/移除/清单/更新检查（FR-03 配套）。 */

export type PluginGroup = "builtin" | "thirdparty" | "selfmade";

export interface PluginRow {
  name: string;
  version: string;
  /** 清单里的依赖声明原文（内置组为空串）。 */
  spec: string;
  /** 已装 manifest 是否声明 dsh.bundle.patch——false = 纯依赖包（非插件层）。 */
  declaresPatch: boolean;
  group: PluginGroup;
  inBundles: boolean;
  removable: boolean;
}

export interface PluginList {
  instanceId: string;
  profile: string;
  plugins: PluginRow[];
}

export interface PluginUpdate {
  name: string;
  current: string;
  latest: string;
  updatable: boolean;
}

export interface PluginInstallOutcome {
  instanceId: string;
  packageName: string;
  version: string;
  bundleActivated: boolean;
}

export interface PluginProgress {
  instanceId: string;
  /** 发起安装的 spec：插件与整合包共用本通道，前端按它分流状态行（审查 R1）。 */
  spec?: string;
  detail: string;
}

async function invoke<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  const { invoke: tauriInvoke } = await import("@tauri-apps/api/core");
  return tauriInvoke<T>(cmd, args);
}

/** 已装插件三分组清单（dsh 内置 / 第三方 / 自建，内置组不可删）。 */
export function pluginList(instanceId: string): Promise<PluginList> {
  return invoke<PluginList>("plugin_list", { id: instanceId });
}

/**
 * 安装插件到实例（官方 `dsh plugin add` 链；spec 支持 npm `name@ver` /
 * `github:owner/repo` / `file:` 本地路径）。进度经 plugin-install-progress
 * 事件推送。注意：插件在实例下次启动时才加载，安装完成提示须写明这一点。
 */
export function pluginInstall(instanceId: string, spec: string): Promise<PluginInstallOutcome> {
  return invoke<PluginInstallOutcome>("plugin_install", { id: instanceId, spec });
}

/** 从实例 profile 移除插件（内置组拒绝；官方 remove 链，bundles 同步）。 */
export function pluginRemove(instanceId: string, name: string): Promise<void> {
  return invoke<void>("plugin_remove", { id: instanceId, name });
}

/**
 * 切换插件启用状态（勾选框）：写入 profile 清单的 `dsh.profile.bundles`
 * 层（停用 = 依赖保留、移出层；下次启动实例生效）。注意官方
 * `dsh plugin add/remove` 的 reconcile 会复活停用项——由前端告知。
 */
export function pluginSetEnabled(instanceId: string, name: string, enabled: boolean): Promise<void> {
  return invoke<void>("plugin_set_enabled", { id: instanceId, name, enabled });
}

/** 检查第三方插件更新（对比 npm dist-tags.latest；显式触发）。 */
export function pluginCheckUpdates(instanceId: string): Promise<PluginUpdate[]> {
  return invoke<PluginUpdate[]>("plugin_check_updates", { id: instanceId });
}

/** 订阅插件安装/移除进度事件；返回取消订阅函数。 */
export async function onPluginProgress(
  handler: (progress: PluginProgress) => void,
): Promise<() => void> {
  const { listen } = await import("@tauri-apps/api/event");
  const unlisten = await listen<PluginProgress>("plugin-install-progress", (event) => {
    handler(event.payload);
  });
  return unlisten;
}
