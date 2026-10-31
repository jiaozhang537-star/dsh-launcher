/**
 * UI 偏好（FR-27 个性化）：背景不透明度 / 标题栏形态 / 功能隐藏清单。
 *
 * 存 localStorage（theme.ts 同款先例——纯本机 UI 态，无跨机迁移诉求）。
 * 任何变更都经 window 事件 `dsh-uiprefs-changed` 广播，各生效点（顶栏导航、
 * 左栏分类、启动页版本按钮）自行重渲染，不集中调度。
 *
 * 功能隐藏的「临时显示」= body 类 `ui-show-all`（PCL：任意界面按 F12 暂时
 * 显示被隐藏的功能，PageSetupUI.xaml:297）。判定函数统一走本模块——
 * 生效点不得自行读 localStorage。
 */

export type SettingsCatId = "providers" | "config" | "hotkeys" | "personalize";
export type MoreCatId = "about" | "help" | "update";
export type TitlebarMode = "default" | "none" | "text" | "image";

export interface UiPrefs {
  /** 背景图不透明度 0~100（PCL UiBackgroundOpacity，默认 10 = 淡淡一层）。 */
  backgroundOpacity: number;
  titlebar: { mode: TitlebarMode; text: string };
  hidden: {
    /** 主页面（download/settings/more；启动页不可隐藏）。 */
    pages: string[];
    /** 设置子页面分类 id。 */
    settingsCats: SettingsCatId[];
    /** 更多子页面分类 id。 */
    moreCats: MoreCatId[];
    /** 启动页「版本选择 / 版本设置」两按钮（PCL「版本管理」）。 */
    versionButtons: boolean;
    /** 仅隐藏功能隐藏卡自身（PCL UiHiddenFunctionHidden：设置依然有效）。 */
    hideSwitchCard: boolean;
  };
}

const KEY = "dsh.ui.prefs.v1";

/** 偏好变更广播事件（含 F12 临时显示切换）。 */
export const UI_PREFS_EVENT = "dsh-uiprefs-changed";

function defaults(): UiPrefs {
  return {
    backgroundOpacity: 10,
    titlebar: { mode: "default", text: "" },
    hidden: { pages: [], settingsCats: [], moreCats: [], versionButtons: false, hideSwitchCard: false },
  };
}

/** 读偏好（缺失 / 损坏字段回落默认值，绝不抛）。 */
export function loadUiPrefs(): UiPrefs {
  const base = defaults();
  try {
    const raw = localStorage.getItem(KEY);
    if (!raw) return base;
    const parsed = JSON.parse(raw) as Partial<UiPrefs>;
    const op = Number(parsed.backgroundOpacity);
    if (Number.isFinite(op)) base.backgroundOpacity = Math.max(0, Math.min(100, op));
    if (parsed.titlebar) {
      if (["default", "none", "text", "image"].includes(parsed.titlebar.mode ?? "")) {
        base.titlebar.mode = parsed.titlebar.mode;
      }
      if (typeof parsed.titlebar.text === "string") {
        base.titlebar.text = parsed.titlebar.text.slice(0, 100);
      }
    }
    if (parsed.hidden) {
      const h = parsed.hidden;
      if (Array.isArray(h.pages)) {
        base.hidden.pages = h.pages.filter((p): p is string =>
          ["downloads", "settings", "more"].includes(p),
        );
      }
      if (Array.isArray(h.settingsCats)) {
        base.hidden.settingsCats = h.settingsCats.filter((c): c is SettingsCatId =>
          ["providers", "config", "hotkeys", "personalize"].includes(c),
        );
      }
      if (Array.isArray(h.moreCats)) {
        base.hidden.moreCats = h.moreCats.filter((c): c is MoreCatId =>
          ["about", "help", "update"].includes(c),
        );
      }
      base.hidden.versionButtons = Boolean(h.versionButtons);
      base.hidden.hideSwitchCard = Boolean(h.hideSwitchCard);
    }
  } catch {
    // 存储不可用 / 内容损坏：用默认值。
  }
  return base;
}

/** 保存并广播变更。 */
export function saveUiPrefs(prefs: UiPrefs): void {
  try {
    localStorage.setItem(KEY, JSON.stringify(prefs));
  } catch {
    // 同 theme.ts：存储失败仅本次会话生效。
  }
  window.dispatchEvent(new CustomEvent(UI_PREFS_EVENT));
}

/** 读—改—写便捷口（改动即时广播）。 */
export function updateUiPrefs(mutate: (prefs: UiPrefs) => void): UiPrefs {
  const prefs = loadUiPrefs();
  mutate(prefs);
  saveUiPrefs(prefs);
  return prefs;
}

// ── F12 临时显示（不落盘：会话级开关） ──────────────────────────────────────

const SHOW_ALL_CLASS = "ui-show-all";

export function isTempShowAll(): boolean {
  return document.body.classList.contains(SHOW_ALL_CLASS);
}

/** 切换「暂时显示被隐藏的功能」（PCL F12），并广播让各生效点重渲染。 */
export function setTempShowAll(on: boolean): void {
  document.body.classList.toggle(SHOW_ALL_CLASS, on);
  window.dispatchEvent(new CustomEvent(UI_PREFS_EVENT));
}

// ── 隐藏判定（生效点唯一入口） ─────────────────────────────────────────────

/** 主页面是否被隐藏（启动页永 false；F12 临时显示期间全部 false）。 */
export function isPageHidden(pageId: string): boolean {
  if (isTempShowAll()) return false;
  return pageId !== "launch" && loadUiPrefs().hidden.pages.includes(pageId);
}

export function isSettingsCatHidden(cat: SettingsCatId): boolean {
  if (isTempShowAll()) return false;
  return loadUiPrefs().hidden.settingsCats.includes(cat);
}

export function isMoreCatHidden(cat: MoreCatId): boolean {
  if (isTempShowAll()) return false;
  return loadUiPrefs().hidden.moreCats.includes(cat);
}

export function versionButtonsHidden(): boolean {
  if (isTempShowAll()) return false;
  return loadUiPrefs().hidden.versionButtons;
}
