/** 个性化（FR-27）：背景图片与标题栏图片的 invoke 薄封装。 */
import { invoke } from "./tauri";

export interface PersonalizeImage {
  name: string;
  dataUrl: string;
}

/** 打开背景图片文件夹（托管根 Pictures/，不存在则创建）。 */
export function openBackgroundFolder(): Promise<void> {
  return invoke<void>("ui_background_open");
}

/** 随机取一张背景图（PCL：每次启动随机加载一张）；空文件夹 = null。 */
export function randomBackground(): Promise<PersonalizeImage | null> {
  return invoke<PersonalizeImage | null>("ui_background_random");
}

/** 清空背景文件夹里的图片。 */
export function clearBackground(): Promise<number> {
  return invoke<number>("ui_background_clear");
}

/** 当前标题栏图片；未设置 = null。 */
export function getLogo(): Promise<PersonalizeImage | null> {
  return invoke<PersonalizeImage | null>("ui_logo_get");
}

/** 从文件设置标题栏图片（复制进托管根）。 */
export function setLogoFromFile(src: string): Promise<PersonalizeImage> {
  return invoke<PersonalizeImage>("ui_logo_set", { src });
}

/** 清除标题栏图片。 */
export function clearLogo(): Promise<boolean> {
  return invoke<boolean>("ui_logo_clear");
}
