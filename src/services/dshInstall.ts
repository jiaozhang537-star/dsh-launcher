/** Tauri invoke 的类型化薄封装：DSH 版本列表与实例安装（FR-01）。 */

export interface DshVersion {
  version: string;
  latest: boolean;
  publishedAt: string;
}

export type InstallStage =
  | "preparing"
  | { autoNode: { detail: string; received: number; total: number } }
  | { installing: { detail: string } }
  | "verifying"
  | "done";

export interface InstallProgress {
  instanceId: string;
  stage: InstallStage;
}

export interface InstallOutcome {
  instanceId: string;
  version: string;
  binJs: string;
}

async function invoke<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  const { invoke: tauriInvoke } = await import("@tauri-apps/api/core");
  return tauriInvoke<T>(cmd, args);
}

/** 拉取可安装的 DSH 版本列表（semver 降序，latest 标注）。 */
export function listDshVersions(): Promise<DshVersion[]> {
  return invoke<DshVersion[]>("dsh_versions");
}

/**
 * 安装官方 DSH 到指定实例。
 * 进度经 Tauri 事件 instance-install-progress 推送（payload 带 instanceId，
 * 多任务并行时前端按 id 分发）；返回安装结果。
 */
export async function installDsh(instanceId: string, version: string): Promise<InstallOutcome> {
  return invoke<InstallOutcome>("instance_install_dsh", { id: instanceId, version });
}

/** 订阅安装进度事件；返回取消订阅函数。 */
export async function onInstallProgress(
  handler: (progress: InstallProgress) => void,
): Promise<() => void> {
  const { listen } = await import("@tauri-apps/api/event");
  const unlisten = await listen<InstallProgress>("instance-install-progress", (event) => {
    handler(event.payload);
  });
  return unlisten;
}
