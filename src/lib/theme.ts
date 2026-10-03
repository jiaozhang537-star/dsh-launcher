/**
 * 主题色引擎（FR-11 · PCL ModSecret.vb:67-108 同式推导）。
 *
 * PCL 的八档 ColorBrush 全部由主题色 HSL 派生：CB1 = HSL(H, S*0.2, 25)、
 * CB2/3/4 = L45/55/65、CB5 = HSL(H, S*0.4, 80)、CB6 = HSL(H, S*0.1, 91)、
 * CB7/8 = L95/97；标题栏为三段水平渐变 L=48/54/48、两端 hue ±10
 * （ThemeRefreshMain）。本文件按同一公式从 {hue, sat} 派生 token，写进
 * :root 的 CSS 变量；**未选择自定义主题时不写任何变量**——默认蓝保持
 * phase-05 逐属性对齐的静态值（theme.css），不因重算引入舍入漂移。
 */

export interface ThemeColor {
  /** 色相 0~360。 */
  hue: number;
  /** 饱和度 0~100（PCL ColorSat，缺省 85）。 */
  sat: number;
}

const KEY = "dsh-launcher:theme";

/** PCL 缺省主题（ModSecret.vb:64：ColorHue=210、ColorSat=85）；蓝的实测
 * 色相 ≈216（spec §1），预设色板按此口径取「蓝」档。 */
export const DEFAULT_THEME: ThemeColor = { hue: 216, sat: 85 };

/** 预设色板（PCL 个性化同款思路：固定 sat、横扫色相）。 */
export const PRESET_HUES: number[] = [216, 180, 120, 60, 30, 0, 320, 270];

/** 标题栏渐变两端的色相偏移（ThemeRefreshMain 的 ColorHueTopbarDelta）。 */
const TITLEBAR_HUE_DELTA = 10;

/** 派生时写进 :root 的全部 token 名（清除覆盖用）。 */
const TOKEN_KEYS = [
  "--cb1",
  "--cb2",
  "--cb3",
  "--cb4",
  "--cb5",
  "--cb6",
  "--cb7",
  "--cb8",
  "--cb-bg0",
  "--cb-bg1",
  "--tint-primary-8",
  "--titlebar-gradient",
];

function clamp(v: number, min: number, max: number): number {
  return Math.min(max, Math.max(min, v));
}

/** CSS 归一化色相到 0~360。 */
function normHue(h: number): number {
  return ((h % 360) + 360) % 360;
}

/** HSL → #rrggbb（h 无界取模，s/l 为 0~100 百分数）。 */
export function hslToHex(h: number, sPercent: number, lPercent: number): string {
  const hN = normHue(h);
  const s = clamp(sPercent, 0, 100) / 100;
  const l = clamp(lPercent, 0, 100) / 100;
  const c = (1 - Math.abs(2 * l - 1)) * s;
  const x = c * (1 - Math.abs(((hN / 60) % 2) - 1));
  const m = l - c / 2;
  let [r, g, b] = [0, 0, 0];
  if (hN < 60) [r, g, b] = [c, x, 0];
  else if (hN < 120) [r, g, b] = [x, c, 0];
  else if (hN < 180) [r, g, b] = [0, c, x];
  else if (hN < 240) [r, g, b] = [0, x, c];
  else if (hN < 300) [r, g, b] = [x, 0, c];
  else [r, g, b] = [c, 0, x];
  const to255 = (v: number): number => Math.round((v + m) * 255);
  return `#${[to255(r), to255(g), to255(b)].map((v) => v.toString(16).padStart(2, "0")).join("")}`;
}

/** #rrggbb → HSL（h 0~360、s/l 0~100）；非法输入返回 null。 */
export function hexToHsl(hex: string): { h: number; s: number; l: number } | null {
  const m = /^#?([0-9a-f]{6})$/i.exec(hex.trim());
  if (!m) return null;
  const int = parseInt(m[1], 16);
  const r = ((int >> 16) & 0xff) / 255;
  const g = ((int >> 8) & 0xff) / 255;
  const b = (int & 0xff) / 255;
  const max = Math.max(r, g, b);
  const min = Math.min(r, g, b);
  const l = (max + min) / 2;
  if (max === min) return { h: 0, s: 0, l: Math.round(l * 100) };
  const d = max - min;
  const s = l > 0.5 ? d / (2 - max - min) : d / (max + min);
  let h: number;
  if (max === r) h = 60 * (((g - b) / d) % 6);
  else if (max === g) h = 60 * ((b - r) / d + 2);
  else h = 60 * ((r - g) / d + 4);
  return { h: normHue(h), s: Math.round(s * 100), l: Math.round(l * 100) };
}

/** #rrggbb 附加 alpha 字节 → #rrggbbaa（对齐 PCL 的 (alpha, Color) 形态）。 */
function hexWithAlpha(hex: string, alpha: number): string {
  return `${hex}${alpha.toString(16).padStart(2, "0")}`;
}

/** 由 {hue, sat} 派生全套色彩 token（PCL ThemeRefresh 公式）。 */
export function deriveThemeTokens(theme: ThemeColor): Record<string, string> {
  const hue = normHue(theme.hue);
  const sat = clamp(theme.sat, 0, 100);
  const cb = (l: number, satMul = 1): string => hslToHex(hue, sat * satMul, l);
  const tokens: Record<string, string> = {
    "--cb1": cb(25, 0.2),
    "--cb2": cb(45),
    "--cb3": cb(55),
    "--cb4": cb(65),
    "--cb5": cb(80, 0.4),
    "--cb6": cb(91, 0.1),
    "--cb7": cb(95),
    "--cb8": cb(97),
  };
  tokens["--cb-bg0"] = tokens["--cb5"];
  tokens["--cb-bg1"] = hexWithAlpha(tokens["--cb7"], 0xbe);
  // 主色 8% 叠层（tab 选中底、「去处理」钮）——随主题联动（原 --tab-active-bg）。
  tokens["--tint-primary-8"] = hexWithAlpha(tokens["--cb3"], 0x14);
  const stop = (delta: number, l: number): string => hslToHex(hue + delta, sat, l);
  tokens["--titlebar-gradient"] = `linear-gradient(90deg, ${stop(-TITLEBAR_HUE_DELTA, 48)} 0%, ${stop(0, 54)} 55%, ${stop(TITLEBAR_HUE_DELTA, 48)} 100%)`;
  return tokens;
}

/** 把派生 token 写进 :root（仅自定义主题时调用）。 */
export function applyTheme(theme: ThemeColor): void {
  const root = document.documentElement;
  for (const [key, value] of Object.entries(deriveThemeTokens(theme))) {
    root.style.setProperty(key, value);
  }
}

/** 清除自定义覆盖：移除内联变量，恢复 theme.css 静态默认蓝。 */
export function clearThemeOverride(): void {
  const root = document.documentElement;
  for (const key of TOKEN_KEYS) {
    root.style.removeProperty(key);
  }
  try {
    localStorage.removeItem(KEY);
  } catch {
    // 存储不可用（隐私模式等）：仅本次会话生效，忽略。
  }
}

/** 读已保存的自定义主题；无 / 损坏返回 null（= 用默认蓝）。 */
export function loadTheme(): ThemeColor | null {
  try {
    const raw = localStorage.getItem(KEY);
    if (!raw) return null;
    const parsed: unknown = JSON.parse(raw);
    if (typeof parsed !== "object" || parsed === null) return null;
    const { hue, sat } = parsed as Record<string, unknown>;
    if (typeof hue !== "number" || typeof sat !== "number") return null;
    return { hue: normHue(hue), sat: clamp(sat, 0, 100) };
  } catch {
    return null;
  }
}

/** 保存并立即应用自定义主题。 */
export function saveTheme(theme: ThemeColor): void {
  const normalized: ThemeColor = { hue: normHue(theme.hue), sat: clamp(theme.sat, 0, 100) };
  try {
    localStorage.setItem(KEY, JSON.stringify(normalized));
  } catch {
    // 同上：存储失败不阻断应用。
  }
  applyTheme(normalized);
}

/** 应用启动时调用（首帧前）：有自定义主题则套用，否则保持默认蓝。 */
export function initTheme(): void {
  const theme = loadTheme();
  if (theme) applyTheme(theme);
}
