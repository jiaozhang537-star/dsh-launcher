/** Tauri invoke 的类型化薄封装：启动器自更新（FR-13，self_update.rs）。 */
import { invoke } from "./tauri";

export interface UpdateStatus {
  /** 是否已配置更新源（未配置 = 尚未发布，如实呈现）。 */
  configured: boolean;
  currentVersion: string;
  latestVersion: string | null;
  releaseNotes: string | null;
  assetName: string | null;
  hasUpdate: boolean;
}

/** 检查更新（GitHub Releases；更新源 = config.json 的 update_repo）。 */
export function checkUpdate(currentVersion: string): Promise<UpdateStatus> {
  return invoke<UpdateStatus>("update_check", { currentVersion });
}

/** 下载最新安装包（统一任务队列，任务键 `update`）。返回本地路径。 */
export function downloadUpdate(currentVersion: string): Promise<string> {
  return invoke<string>("update_download", { currentVersion });
}

/** 打开已下载的安装包（交给系统安装器）。 */
export function installUpdate(path: string): Promise<void> {
  return invoke<void>("update_install", { path });
}
