/** Tauri invoke 的类型化薄封装：DSH 源码归档下载（FR-22 修订落点见 FILE-LAYOUT §5）。 */
import { invoke } from "./tauri";

export interface SourceDownloadProgress {
  version: string;
  /** 任务归属：实例 id = 落版本内 sources/；null = 落全局 sources/<tag>/。 */
  instanceId: string | null;
  received: number;
  total: number;
}



/**
 * 下载并解压指定版本的官方源码归档（GitHub tag dsh-v<版本>）。
 * `instanceId` 非空 → 落 `versions/<id>/sources/`（已下载过即报错）；
 * 空 → 落全局 `sources/dsh-v<版本>/`（重名自动加副本后缀）。
 * 启动器不安装不管理；进度经 onProgress 回调（source-download-progress 事件）。
 */
export async function downloadSourceArchive(
  version: string,
  instanceId: string | null,
  onProgress: (progress: SourceDownloadProgress) => void,
): Promise<string> {
  const { listen } = await import("@tauri-apps/api/event");
  const unlisten = await listen<SourceDownloadProgress>("source-download-progress", (event) => {
    onProgress(event.payload);
  });
  try {
    return await invoke<string>("source_download", { version, instanceId });
  } finally {
    unlisten();
  }
}

/** 请求中止进行中的源码归档下载（与 Node 下载共用取消标志）。 */
export function cancelSourceDownload(): Promise<void> {
  return invoke<void>("source_download_cancel");
}

/** 在系统文件管理器中打开已下载的源码目录（instanceId 语义同上）。 */
export function openSourceDir(version: string, instanceId: string | null = null): Promise<void> {
  return invoke<void>("source_open_dir", { version, instanceId });
}

/** 删除版本内的源码归档目录（不存在时幂等成功）。 */
export function deleteInstanceSources(instanceId: string): Promise<void> {
  return invoke<void>("source_delete", { instanceId });
}

/** 版本内源码归档是否已下载（卡片状态回显）。 */
export function instanceSourcesDownloaded(instanceId: string): Promise<boolean> {
  return invoke<boolean>("source_status", { instanceId });
}
