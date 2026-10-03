// @vitest-environment happy-dom
import { beforeEach, describe, expect, it } from "vitest";

import {
  DEFAULT_THEME,
  PRESET_HUES,
  applyTheme,
  clearThemeOverride,
  deriveThemeTokens,
  hexToHsl,
  hslToHex,
  loadTheme,
  saveTheme,
} from "./theme";

describe("hslToHex / hexToHsl（PCL HSL 派生的数学基座）", () => {
  it("标准值换算正确", () => {
    expect(hslToHex(0, 100, 50)).toBe("#ff0000");
    expect(hslToHex(120, 100, 25)).toBe("#008000");
    expect(hslToHex(240, 100, 50)).toBe("#0000ff");
    expect(hslToHex(0, 0, 100)).toBe("#ffffff");
  });

  it("色相取模：370° 与 10° 同色", () => {
    expect(hslToHex(370, 85, 55)).toBe(hslToHex(10, 85, 55));
  });

  it("hexToHsl 与 hslToHex 往返一致（蓝主色实测值 spec §1）", () => {
    const hsl = hexToHsl("#1370f3");
    expect(hsl).not.toBeNull();
    expect(hsl!.s).toBeGreaterThanOrEqual(88);
    expect(Math.abs(hsl!.l - 51)).toBeLessThanOrEqual(1);
    const back = hexToHsl(hslToHex(hsl!.h, hsl!.s, hsl!.l));
    expect(back).not.toBeNull();
    // 8bit 量化允许 hue ±1°、s/l ±2 的往返误差。
    expect(Math.abs(back!.h - hsl!.h)).toBeLessThanOrEqual(1);
    expect(Math.abs(back!.s - hsl!.s)).toBeLessThanOrEqual(2);
    expect(Math.abs(back!.l - hsl!.l)).toBeLessThanOrEqual(2);
  });

  it("非法输入返回 null", () => {
    expect(hexToHsl("banana")).toBeNull();
    expect(hexToHsl("#12345")).toBeNull();
  });
});

describe("deriveThemeTokens（PCL ModSecret ThemeRefresh 同式）", () => {
  it("产出全部 12 个 token，且明度阶梯单调", () => {
    const tokens = deriveThemeTokens({ hue: 216, sat: 85 });
    for (const key of ["--cb1", "--cb2", "--cb3", "--cb4", "--cb5", "--cb6", "--cb7", "--cb8", "--cb-bg0", "--cb-bg1", "--tint-primary-8", "--titlebar-gradient"]) {
      expect(tokens[key], key).toBeTruthy();
    }
    // 主色 8% 叠层 = cb3 加 0x14 alpha（原 --tab-active-bg 更名后随主题联动）。
    expect(tokens["--tint-primary-8"]).toBe(`${tokens["--cb3"]}14`);
    const lightness = ["--cb2", "--cb3", "--cb4", "--cb5", "--cb6", "--cb7", "--cb8"].map((k) => hexToHsl(tokens[k])!.l);
    for (let i = 1; i < lightness.length; i += 1) {
      expect(lightness[i]).toBeGreaterThanOrEqual(lightness[i - 1]);
    }
  });

  it("饱和度乘子：CB1 = S*0.2、CB5 = S*0.4、CB6 = S*0.1（低明度档降饱和）", () => {
    const tokens = deriveThemeTokens({ hue: 216, sat: 100 });
    expect(hexToHsl(tokens["--cb1"])!.s).toBeLessThanOrEqual(21);
    expect(hexToHsl(tokens["--cb5"])!.s).toBeLessThanOrEqual(41);
    expect(hexToHsl(tokens["--cb6"])!.s).toBeLessThanOrEqual(11);
  });

  it("cb-bg1 = cb7 加 0xbe alpha；titlebar 为三段 90deg 渐变", () => {
    const tokens = deriveThemeTokens(DEFAULT_THEME);
    expect(tokens["--cb-bg1"]).toBe(`${tokens["--cb7"]}be`);
    expect(tokens["--titlebar-gradient"]).toMatch(/^linear-gradient\(90deg, #[0-9a-f]{6} 0%, #[0-9a-f]{6} 55%, #[0-9a-f]{6} 100%\)$/);
  });

  it("色相位移换主题色会改变主色（216 蓝 ≠ 0 红）", () => {
    const blue = deriveThemeTokens({ hue: 216, sat: 85 })["--cb3"];
    const red = deriveThemeTokens({ hue: 0, sat: 85 })["--cb3"];
    expect(blue).not.toBe(red);
  });

  it("预设色板非空且各色相主色互异", () => {
    expect(PRESET_HUES.length).toBeGreaterThanOrEqual(8);
    const mains = new Set(PRESET_HUES.map((hue) => deriveThemeTokens({ hue, sat: 85 })["--cb3"]));
    expect(mains.size).toBe(PRESET_HUES.length);
  });
});

describe("主题持久化与应用", () => {
  beforeEach(() => {
    localStorage.clear();
    clearThemeOverride();
  });

  it("默认无覆盖：loadTheme 为 null", () => {
    expect(loadTheme()).toBeNull();
  });

  it("saveTheme 持久化 + applyTheme 写入 :root 内联变量", () => {
    saveTheme({ hue: 120, sat: 85 });
    expect(loadTheme()).toEqual({ hue: 120, sat: 85 });
    expect(document.documentElement.style.getPropertyValue("--cb3")).toBe(deriveThemeTokens({ hue: 120, sat: 85 })["--cb3"]);
  });

  it("clearThemeOverride 移除内联变量与存储（恢复静态默认蓝）", () => {
    applyTheme({ hue: 0, sat: 85 });
    expect(document.documentElement.style.getPropertyValue("--cb3")).not.toBe("");
    clearThemeOverride();
    expect(document.documentElement.style.getPropertyValue("--cb3")).toBe("");
    expect(loadTheme()).toBeNull();
  });

  it("损坏的存储内容安全回落 null", () => {
    localStorage.setItem("dsh-launcher:theme", "{oops");
    expect(loadTheme()).toBeNull();
  });
});
