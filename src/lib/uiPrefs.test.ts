// @vitest-environment happy-dom
import { afterEach, describe, expect, it } from "vitest";
import {
  isPageHidden,
  isSettingsCatHidden,
  isTempShowAll,
  loadUiPrefs,
  saveUiPrefs,
  setTempShowAll,
  updateUiPrefs,
  versionButtonsHidden,
} from "./uiPrefs";

/**
 * UI 偏好存取与功能隐藏判定（FR-27）：损坏输入回落默认、数值钳制、
 * 枚举白名单过滤、F12 临时显示覆盖一切隐藏判定。
 */
describe("uiPrefs", () => {
  afterEach(() => {
    localStorage.clear();
    document.body.classList.remove("ui-show-all");
  });

  it("空存储给默认值（背景 10%、标题栏默认、全不隐藏）", () => {
    const prefs = loadUiPrefs();
    expect(prefs.backgroundOpacity).toBe(10);
    expect(prefs.titlebar.mode).toBe("default");
    expect(prefs.hidden.pages).toEqual([]);
    expect(prefs.hidden.versionButtons).toBe(false);
  });

  it("保存往返：不透明度钳制 0~100、文本截断 100、非法枚举被滤除", () => {
    // 非法枚举是**故意**塞入的（类型上不允许、存储可能被外部改），验证读取侧过滤。
    const poisoned = {
      backgroundOpacity: 150,
      titlebar: { mode: "image", text: "x".repeat(200) },
      hidden: {
        pages: ["downloads", "bogus"],
        settingsCats: ["hotkeys", "nope"],
        moreCats: ["help", "update"],
        versionButtons: true,
        hideSwitchCard: false,
      },
    } as unknown as Parameters<typeof saveUiPrefs>[0];
    saveUiPrefs(poisoned);
    const prefs = loadUiPrefs();
    expect(prefs.backgroundOpacity).toBe(100);
    expect(prefs.titlebar.text.length).toBe(100);
    expect(prefs.hidden.pages).toEqual(["downloads"]);
    expect(prefs.hidden.settingsCats).toEqual(["hotkeys"]);
    expect(prefs.hidden.moreCats).toEqual(["help", "update"]);
    expect(prefs.hidden.versionButtons).toBe(true);
  });

  it("损坏 JSON 回落默认值而不抛", () => {
    localStorage.setItem("dsh.ui.prefs.v1", "{not json");
    expect(loadUiPrefs().titlebar.mode).toBe("default");
  });

  it("隐藏判定：启动页永可见；F12 临时显示期间全部可见", () => {
    updateUiPrefs((p) => {
      p.hidden.pages = ["downloads", "settings", "more"];
      p.hidden.settingsCats = ["hotkeys"];
    });
    expect(isPageHidden("launch")).toBe(false);
    expect(isPageHidden("downloads")).toBe(true);
    expect(isSettingsCatHidden("hotkeys")).toBe(true);

    setTempShowAll(true);
    expect(isTempShowAll()).toBe(true);
    expect(isPageHidden("downloads")).toBe(false);
    expect(isSettingsCatHidden("hotkeys")).toBe(false);
    setTempShowAll(false);
    expect(isPageHidden("downloads")).toBe(true);
  });

  it("版本管理开关往返", () => {
    expect(versionButtonsHidden()).toBe(false);
    updateUiPrefs((p) => {
      p.hidden.versionButtons = true;
    });
    expect(versionButtonsHidden()).toBe(true);
  });
});
