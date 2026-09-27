import { getCurrentWindow, type Window } from "@tauri-apps/api/window";
import { el } from "../lib/dom";
import { icon } from "../lib/icons";
import { renderNavIcon, type NavIconName } from "./nav-icons";
import { goBack, navigate, pageById, registeredPages, subscribe } from "../router";

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
    [icon("back", 16)],
  );
  backBtn.addEventListener("click", () => goBack());
  const secTitle = el("span", { class: "topbar__sec-title" }, []);
  const secWrap = el("div", { class: "topbar__secondary" }, [backBtn, secTitle]);

  // 主↔子切换（R5）：纯 class 驱动过渡（350ms 入 / 150ms 出，spec §5「标题栏主↔子
  // 150/350ms OutBack」），不再用 hidden 硬切；显隐样式全在 CSS（.is-visible）。
  const syncSecondary = (pageId: string): void => {
    const def = pageById(pageId);
    const isSecondary = def?.secondary === true;
    secWrap.classList.toggle("is-visible", isSecondary);
    bar.classList.toggle("topbar--secondary", isSecondary);
    if (isSecondary) {
      secTitle.textContent = def?.titleOf?.() ?? def?.navLabel ?? "";
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
    [icon("win-min", 14)],
  );
  const closeBtn = el(
    "button",
    { type: "button", class: "win-btn win-btn--close", "aria-label": "关闭", title: "关闭" },
    [icon("win-close", 14)],
  );
  // PCL2 FormMain 仅有最小化与关闭两键（无最大化）；双击不改变窗口大小。
  minimizeBtn.addEventListener("click", () => void withWindow((win) => win.minimize()));
  closeBtn.addEventListener("click", () => void withWindow((win) => win.close()));

  // 品牌区：白色鲸鱼标 + DSH 字标（对标 PCL 字标位：白描边矢量、宽约 39px、Margin 19,15.5）
  const brandMark = document.createElement("img");
  brandMark.className = "topbar__brand-mark";
  brandMark.src = "/version-icon/dsh-black.svg";
  brandMark.alt = "";
  brandMark.draggable = false;
  const brand = el("span", { class: "topbar__brand" }, [brandMark, "DSH"]);

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
