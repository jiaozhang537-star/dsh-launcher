import { getCurrentWindow, type Window } from "@tauri-apps/api/window";
import { el } from "../lib/dom";
import { renderNavIcon, type NavIconName } from "./nav-icons";
import { navigate, pageById, registeredPages, subscribe } from "../router";

/** 页面 id → PCL 导航图标。 */
const NAV_ICONS: Record<string, NavIconName> = {
  launch: "launch",
  downloads: "downloads",
  settings: "settings",
  more: "more",
};

/** 仅在 Tauri WebView 内返回 true；纯浏览器预览时窗口控制为无操作。 */
function isRunningInTauri(): boolean {
  return "__TAURI_INTERNALS__" in (window as unknown as Record<string, unknown>);
}

async function withWindow(action: (win: Window) => Promise<void>): Promise<void> {
  if (!isRunningInTauri()) return;
  try {
    await action(getCurrentWindow());
  } catch (error) {
    console.error("窗口操作失败", error);
  }
}

/**
 * 创建顶栏：品牌区 + 四页面导航（居中）+ 窗口控制（最小化/关闭，与 PCL2 一致）。
 * 整条蓝栏都是拖拽区；交互子元素豁免，导航按钮之间的空白属于栏本身，
 * 按下即触发拖拽——与 PCL 一致。刻意不用 data-tauri-drag-region：它内建
 * 「双击最大化/还原」且无法按元素关闭，需求方判定双击放大为 bug，
 * 故 mousedown 手动 startDragging（只拖拽，无双击行为）。
 */
export function createTopbar(): HTMLElement {
  const navButtons = new Map<string, HTMLButtonElement>();
  const nav = el("nav", { class: "topbar__nav", "aria-label": "主导航" });
  for (const page of registeredPages()) {
    if (page.secondary) continue; // 二级页不进主导航
    const btn = el("button", { type: "button", class: "topbar__nav-btn" });
    // 图标 + 文字（MyRadioButton 结构：ShapeLogo MaxHeight16 左、LabText 右距12）
    const iconName = NAV_ICONS[page.id];
    if (iconName) {
      btn.append(renderNavIcon(iconName));
    }
    btn.append(el("span", { class: "topbar__nav-text" }, [page.navLabel]));
    btn.addEventListener("click", () => navigate(page.id));
    navButtons.set(page.id, btn);
    nav.append(btn);
  }

  // 二级页形态：品牌隐藏（主导航保留），← 返回 + 页面名占据左上角品牌位。
  const backBtn = el(
    "button",
    { type: "button", class: "topbar__back", "aria-label": "返回", title: "返回" },
    ["\u2190"],
  );
  backBtn.addEventListener("click", () => navigate("launch"));
  const secTitle = el("span", { class: "topbar__sec-title" }, []);
  const secWrap = el("div", { class: "topbar__secondary" }, [backBtn, secTitle]);
  secWrap.hidden = true;

  const syncSecondary = (pageId: string): void => {
    const def = pageById(pageId);
    const isSecondary = def?.secondary === true;
    secWrap.hidden = !isSecondary;
    bar.classList.toggle("topbar--secondary", isSecondary);
    if (isSecondary) {
      secTitle.textContent = def?.navLabel ?? "";
    }
  };

  subscribe((pageId) => {
    for (const [id, btn] of navButtons) {
      btn.classList.toggle("is-active", id === pageId);
      if (id === pageId) {
        btn.setAttribute("aria-current", "page");
      } else {
        btn.removeAttribute("aria-current");
      }
    }
    syncSecondary(pageId);
  });

  const minimizeBtn = el(
    "button",
    { type: "button", class: "win-btn", "aria-label": "最小化", title: "最小化" },
    ["\u2500"],
  );
  const closeBtn = el(
    "button",
    { type: "button", class: "win-btn win-btn--close", "aria-label": "关闭", title: "关闭" },
    ["\u2715"],
  );
  // PCL2 FormMain 仅有最小化与关闭两键（无最大化）；双击不改变窗口大小。
  minimizeBtn.addEventListener("click", () => void withWindow((win) => win.minimize()));
  closeBtn.addEventListener("click", () => void withWindow((win) => win.close()));

  const brand = el("span", { class: "topbar__brand" }, ["DSH 启动器"]);

  const bar = el("header", { class: "topbar" });
  // 手动拖拽：mousedown 冒泡自品牌/导航文字/按钮间空隙均可触发；
  // 落在交互控件上不拖拽（避免按住按钮移动误触发窗口拖动）。
  bar.addEventListener("mousedown", (event) => {
    if (event.button !== 0) return;
    const target = event.target as HTMLElement | null;
    if (target?.closest("button, input, select, textarea, a")) return;
    void withWindow((win) => win.startDragging());
  });
  // 二级页：←+标题占最左品牌位、品牌隐藏；普通页隐藏的 secWrap 不占位，brand 居左上。
  bar.append(secWrap, brand, nav, el("div", { class: "topbar__controls" }, [minimizeBtn, closeBtn]));
  return bar;
}
