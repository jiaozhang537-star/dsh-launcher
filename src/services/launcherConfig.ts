/** Tauri invoke 的类型化薄封装：启动器自身设置（FR-11 并发数 / 代理）。 */
import { invoke } from "./tauri";

/** 下载并发上限（1~10；默认 3）。 */
export function getMaxDownloads(): Promise<number> {
  return invoke<number>("max_downloads_get");
}

export function setMaxDownloads(value: number): Promise<void> {
  return invoke<void>("max_downloads_set", { value });
}

/** HTTP(S) 代理（空串 = 直连）。 */
export function getProxy(): Promise<string> {
  return invoke<string>("proxy_get");
}

export function setProxy(proxy: string): Promise<void> {
  return invoke<void>("proxy_set", { proxy });
}

/** 默认版本隔离（Phase 8.5，FR-17 扩展；PCL LaunchArgumentIndieV2 同语义）：
 * 新装实例的隔离默认；true = 完全隔离。已有实例各走版本设置页开关。 */
export function getDefaultIsolation(): Promise<boolean> {
  return invoke<boolean>("default_isolation_get");
}

export function setDefaultIsolation(value: boolean): Promise<void> {
  return invoke<void>("default_isolation_set", { value });
}

/** 启动器可见性（Phase 8.5，FR-28；PCL LaunchArgumentVisible 原值系，1 弃用）：
 * 0 = 网页就绪后立即关闭；2 = 隐藏，最后实例退出后自动关闭；
 * 3 = 隐藏，最后实例退出后重新打开；4 = 最小化；5 = 保持不变。 */
export function getLauncherVisibility(): Promise<number> {
  return invoke<number>("launcher_visibility_get");
}

export function setLauncherVisibility(value: number): Promise<void> {
  return invoke<void>("launcher_visibility_set", { value });
}
