/** 应用内快捷键：动作目录、组合键规范化、匹配与覆盖缓存（纯逻辑，vitest 覆盖）。
 * 持久化落 `hotkeys.json`（随数据目录走，FILE-LAYOUT；2026-09-12 起取代
 * localStorage）——本模块只管缓存与目录默认键的合并，文件读写由
 * services/hotkeys.ts 注册的通道完成，测试不注册通道 = 纯内存。 */

export interface HotkeyAction {
  id: string;
  label: string;
  defaultCombo: string;
  /** 触发后导航到的页面 id；非导航动作为 null。 */
  target: string | null;
}

/** 动作目录：新增快捷键在此登记（后续阶段的动作自然扩展此表）。 */
export const HOTKEY_ACTIONS: readonly HotkeyAction[] = [
  { id: "nav-launch", label: "打开启动页", defaultCombo: "Alt+1", target: "launch" },
  { id: "nav-downloads", label: "打开下载页", defaultCombo: "Alt+2", target: "downloads" },
  { id: "nav-settings", label: "打开设置页", defaultCombo: "Alt+3", target: "settings" },
  { id: "nav-more", label: "打开更多页", defaultCombo: "Alt+4", target: "more" },
] as const;

/** 旧 localStorage 键名（2026-09-12 前的存储位，仅迁移时读取/清除）。 */
export const STORE_KEY = "launcher.hotkeys.v1";
const MODIFIERS = ["Ctrl", "Alt", "Shift", "Meta"] as const;

export interface HotkeyBinding {
  action: string;
  combo: string;
}

let cachedOverrides: Record<string, string> | null = null;
let persistFn: ((overrides: Record<string, string>) => void) | null = null;

/** 注册持久化通道（services/hotkeys 水合后调用；未注册 = 纯内存，测试安全）。 */
export function setHotkeyPersistence(persist: (overrides: Record<string, string>) => void): void {
  persistFn = persist;
}

/** 覆盖缓存整体替换（水合与测试用）。null = 无覆盖（全部目录默认键）。 */
export function applyOverrides(overrides: Record<string, string> | null): void {
  cachedOverrides = overrides;
}

/** 读生效绑定：缓存覆盖 + 目录默认（目录新增动作自动获得默认键）。 */
export function loadBindings(): HotkeyBinding[] {
  const overrides = cachedOverrides ?? {};
  return HOTKEY_ACTIONS.map((action) => ({
    action: action.id,
    combo: typeof overrides[action.id] === "string" ? overrides[action.id] : action.defaultCombo,
  }));
}

/** 保存单条覆盖（combo = 规范化后的组合串）；已注册通道时异步落盘。 */
export function saveBinding(action: string, combo: string): void {
  const overrides = { ...(cachedOverrides ?? {}) };
  overrides[action] = combo;
  cachedOverrides = overrides;
  persistFn?.(overrides);
}

/** 清除全部覆盖，回到目录默认键；已注册通道时异步落盘空表。 */
export function resetBindings(): void {
  cachedOverrides = {};
  persistFn?.({});
  try {
    localStorage.removeItem(STORE_KEY);
  } catch {
    // 无 localStorage 的环境忽略（缓存与文件才是权威）。
  }
}

/** 裸修饰键名（e.key 形态）：录制时按住单个修饰键不产生主键。 */
const MODIFIER_KEYS = new Set<string>(["Control", "Alt", "Shift", "Meta"]);

/** 尾段是否为修饰键：规范名（Ctrl/Control/Alt/Shift/Meta）+ 平台 code 形态
 * （ControlLeft/AltRight 等）。keyName 把裸修饰键映射为规范名后，合成键盘
 * 与真实键盘都会产出 "Ctrl+Control" 形态——规范名必须一并判为修饰键
 * （2026-09-20 实机走查：只认 *Left/Right 时裸 Ctrl 录制即被存成主键）。 */
function isModifierName(name: string): boolean {
  return (
    (MODIFIERS as readonly string[]).includes(name) ||
    MODIFIER_KEYS.has(name) ||
    /^(Ctrl|Control|Alt|Shift|Meta)(Left|Right)$/.test(name)
  );
}

/** 键名规范化：可打印字符/功能键原样（字母大写），其余用 code 去掉前缀。
 * 修饰键返回规范名（Ctrl/Alt/Shift/Meta）——裸按修饰键时 e.code 是
 * AltLeft/ControlRight 之类平台形态，直用会把修饰键录成主键（审查 P2-06）。 */
function keyName(e: KeyboardEvent): string {
  if (MODIFIER_KEYS.has(e.key)) return e.key;
  if (/^F\d{1,2}$/.test(e.key)) return e.key;
  if (e.key.length === 1) return e.key.toUpperCase();
  return e.code.replace(/^Key|^Digit/, "") || e.key;
}

/**
 * 把键盘事件规范化为组合串：修饰键按 Ctrl+Alt+Shift+Meta 固定顺序，
 * 主键居末（如 "Ctrl+Alt+1"、"Alt+F4" 形态）。
 */
export function formatCombo(e: KeyboardEvent): string {
  const parts: string[] = [];
  if (e.ctrlKey) parts.push("Ctrl");
  if (e.altKey) parts.push("Alt");
  if (e.shiftKey) parts.push("Shift");
  if (e.metaKey) parts.push("Meta");
  parts.push(keyName(e));
  return parts.join("+");
}

/** 组合是否含非修饰主键（如 "Ctrl+Alt"、"Alt+AltLeft" 这类无主键串不完整，
 * 不可保存/匹配）。 */
export function hasPrimaryKey(combo: string): boolean {
  const parts = combo.split("+").filter((p) => p !== "");
  const last = parts[parts.length - 1];
  return !!last && !isModifierName(last);
}

/** 事件是否命中组合串（修饰键精确匹配 + 主键键名一致）。 */
export function comboMatches(e: KeyboardEvent, combo: string): boolean {
  if (!hasPrimaryKey(combo)) return false;
  const parts = combo.split("+");
  const key = parts[parts.length - 1];
  const mods = new Set(parts.slice(0, -1));
  return (
    e.ctrlKey === mods.has("Ctrl") &&
    e.altKey === mods.has("Alt") &&
    e.shiftKey === mods.has("Shift") &&
    e.metaKey === mods.has("Meta") &&
    keyName(e) === key
  );
}

/** 动作是否需要捕获输入框让位（输入类控件聚焦时不触发全局键）。 */
export function isTypingTarget(target: EventTarget | null): boolean {
  const el = target as HTMLElement | null;
  return !!el && /^(INPUT|TEXTAREA|SELECT)$/.test(el.tagName);
}
