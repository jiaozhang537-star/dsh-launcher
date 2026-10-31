/**
 * 背景图层（FR-27 个性化）：#bg-layer 是 .app-shell 的第一个子层（绝对定位
 * 铺满壳内、不渗透明边带），图片与不透明度都走内联样式；各内容面在
 * `body.has-bg` 下改半透明（main.css）。PCL 口径：每次启动随机加载
 * Pictures/ 里的一张（PageSetupUI.xaml:152 ToolTip）。
 */
import { loadUiPrefs } from "./uiPrefs";
import { randomBackground } from "../services/personalize";

function layer(): HTMLElement {
  const found = document.querySelector<HTMLElement>("#bg-layer");
  if (found) return found;
  const el = document.createElement("div");
  el.id = "bg-layer";
  el.setAttribute("aria-hidden", "true");
  return el;
}

/** 应用背景：随机取一张；有图铺层并标 has-bg，无图清干净。 */
export async function applyBackground(): Promise<void> {
  const host = document.querySelector<HTMLElement>("#app");
  if (!host) return;
  const bg = layer();
  if (bg.parentElement !== host) {
    host.insertBefore(bg, host.firstChild);
  }
  try {
    const image = await randomBackground();
    if (!image) {
      clearBackgroundLayer();
      return;
    }
    bg.style.backgroundImage = `url("${image.dataUrl}")`;
    bg.style.opacity = String(loadUiPrefs().backgroundOpacity / 100);
    document.body.classList.add("has-bg");
  } catch {
    // 读不到（后端不可用等）：保持无背景，不报错打扰。
    clearBackgroundLayer();
  }
}

/** 清除背景层（清空背景 / 失败回落）。 */
export function clearBackgroundLayer(): void {
  const bg = document.querySelector<HTMLElement>("#bg-layer");
  if (bg) {
    bg.style.backgroundImage = "";
    bg.style.opacity = "";
  }
  document.body.classList.remove("has-bg");
}

/** 仅调不透明度（滑杆实时预览，不重新随机）。 */
export function setBackgroundOpacity(percent: number): void {
  const bg = document.querySelector<HTMLElement>("#bg-layer");
  if (bg && bg.style.backgroundImage) {
    bg.style.opacity = String(Math.max(0, Math.min(100, percent)) / 100);
  }
}
