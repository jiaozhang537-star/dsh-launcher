/**
 * 左下轻提示（Phase 5 · spec G6）。
 * 对照 PCL2 FormMain PanHint（左下 bottom 20、高 26、右圆角 6、字 13）+
 * ModMain.vb:44-135 的调度（入场 Y+30 OutElastic 400ms；
 * 停留 800 + len.Clamp(5,23)×180 ms；消失 = MyHint AniDispose 三段）。
 * 用于全局轻提示（导入成功、复制成功等）；就地状态行不受影响。
 */

import { outElastic } from "./anim";

const HOST_CLASS = "hint-host";
const STRIP_CLASS = "hint-strip";
const LEAVING_CLASS = "hint-strip--leaving";

let host: HTMLElement | null = null;

/** 挂载提示宿主（main.ts 调用一次；挂在 app-shell 内、弹窗层之下）。 */
export function initHintHost(parent: HTMLElement): void {
  if (host) return;
  host = document.createElement("div");
  host.className = HOST_CLASS;
  parent.append(host);
}

/** 停留时长：800 + clamp(len,5,23)×180 ms（ModMain.vb 原式）。 */
function dwellMs(text: string): number {
  const len = Math.min(23, Math.max(5, text.length));
  return 800 + len * 180;
}

/** 创建并展示一条提示；自动消失。同屏多条纵向堆叠。 */
export function showHint(text: string): void {
  if (!host || typeof requestAnimationFrame !== "function") return;
  const strip = document.createElement("div");
  strip.className = STRIP_CLASS;
  strip.setAttribute("role", "status");
  strip.textContent = text;
  host.append(strip);

  // 入场：Y+30 弹性（OutElastic 400ms）+ 淡入
  const enter = outElastic(3);
  const t0 = performance.now();
  const ENTER_MS = 400;
  const tick = (now: number): void => {
    const k = Math.min(1, (now - t0) / ENTER_MS);
    const y = 30 * (1 - enter(k));
    strip.style.transform = `translateY(${y.toFixed(2)}px)`;
    strip.style.opacity = String(Math.min(1, k * 3));
    if (k < 1) requestAnimationFrame(tick);
  };
  requestAnimationFrame(tick);

  // 停留后三段消失（MyHint AniDispose：scale-0.08 200ms / fade 200ms / 高度收起 150ms 延 100ms）
  setTimeout(() => {
    strip.classList.add(LEAVING_CLASS);
    setTimeout(() => strip.remove(), 350);
  }, dwellMs(text));
}
