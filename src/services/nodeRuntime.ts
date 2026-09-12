/** Tauri invoke 的类型化薄封装：Node 运行时代管相关命令。 */

export interface SystemNodeInfo {
  version: string;
  path: string;
  meetsRange: boolean;
}

export interface DownloadProgress {
  received: number;
  total: number;
}

async function invoke<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  const { invoke: tauriInvoke } = await import("@tauri-apps/api/core");
  return tauriInvoke<T>(cmd, args);
}

/** 检测 PATH 上的系统 Node；未安装返回 null。 */
export function detectSystemNode(): Promise<SystemNodeInfo | null> {
  return invoke<SystemNodeInfo | null>("node_system_info");
}

/** 已托管的便携版 Node 版本列表（升序）。 */
export function listManagedVersions(): Promise<string[]> {
  return invoke<string[]>("node_managed_versions");
}

/**
 * 下载并安装指定版本的便携版 Node。
 * 进度经 onProgress 回调（Tauri 事件 node-download-progress）；返回安装目录。
 */
export async function downloadNode(
  version: string,
  onProgress: (progress: DownloadProgress) => void,
): Promise<string> {
  const { listen } = await import("@tauri-apps/api/event");
  const unlisten = await listen<DownloadProgress>("node-download-progress", (event) => {
    onProgress(event.payload);
  });
  try {
    return await invoke<string>("node_download", { version });
  } finally {
    unlisten();
  }
}

/** 请求中止进行中的 Node 下载（后端停滞看门狗至多 60s 后生效）。 */
export function cancelDownload(): Promise<void> {
  return invoke<void>("node_download_cancel");
}

/** 读全局默认托管 Node 版本。 */
export function getDefaultVersion(): Promise<string | null> {
  return invoke<string | null>("node_default_get");
}

/** 设全局默认托管 Node 版本（必须已托管）。 */
export function setDefaultVersion(version: string): Promise<void> {
  return invoke<void>("node_default_set", { version });
}
