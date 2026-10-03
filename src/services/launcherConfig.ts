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
