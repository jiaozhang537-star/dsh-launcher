import { describe, expect, it } from "vitest";

import {
  applyOverrides,
  comboMatches,
  formatCombo,
  hasPrimaryKey,
  HOTKEY_ACTIONS,
  loadBindings,
  resetBindings,
  saveBinding,
} from "./hotkeys";

function keyEvent(init: Partial<KeyboardEvent> & { key: string }): KeyboardEvent {
  return {
    key: init.key,
    code: init.code ?? init.key,
    ctrlKey: init.ctrlKey ?? false,
    altKey: init.altKey ?? false,
    shiftKey: init.shiftKey ?? false,
    metaKey: init.metaKey ?? false,
  } as KeyboardEvent;
}

describe("formatCombo / hasPrimaryKey", () => {
  it("修饰键按固定顺序输出，主键居末", () => {
    expect(formatCombo(keyEvent({ key: "1", altKey: true }))).toBe("Alt+1");
    expect(formatCombo(keyEvent({ key: "k", ctrlKey: true, shiftKey: true }))).toBe("Ctrl+Shift+K");
    expect(formatCombo(keyEvent({ key: "F5" }))).toBe("F5");
    expect(formatCombo(keyEvent({ key: "a", metaKey: true }))).toBe("Meta+A");
  });

  it("无主键的组合串不完整", () => {
    expect(hasPrimaryKey("Alt+1")).toBe(true);
    expect(hasPrimaryKey("Ctrl+Alt")).toBe(false);
    expect(hasPrimaryKey("")).toBe(false);
  });
});

describe("comboMatches", () => {
  it("修饰键精确匹配 + 主键键名一致", () => {
    const combo = "Ctrl+Alt+1";
    expect(comboMatches(keyEvent({ key: "1", ctrlKey: true, altKey: true }), combo)).toBe(true);
    expect(comboMatches(keyEvent({ key: "1", ctrlKey: true }), combo)).toBe(false);
    expect(comboMatches(keyEvent({ key: "2", ctrlKey: true, altKey: true }), combo)).toBe(false);
  });
});

describe("绑定缓存（未注册持久化通道 = 纯内存）", () => {
  it("默认绑定来自动作目录；覆盖生效；保存与重置语义正确", () => {
    applyOverrides(null);
    let bindings = loadBindings();
    expect(bindings).toHaveLength(HOTKEY_ACTIONS.length);
    expect(bindings[0]).toEqual({ action: "nav-launch", combo: "Alt+1" });

    applyOverrides({ "nav-launch": "Ctrl+Alt+L" });
    bindings = loadBindings();
    expect(bindings.find((b) => b.action === "nav-launch")?.combo).toBe("Ctrl+Alt+L");
    expect(bindings.find((b) => b.action === "nav-downloads")?.combo).toBe("Alt+2");

    saveBinding("nav-more", "Meta+M");
    expect(loadBindings().find((b) => b.action === "nav-more")?.combo).toBe("Meta+M");
    // 保存是增量覆盖，不动其他键。
    expect(loadBindings().find((b) => b.action === "nav-launch")?.combo).toBe("Ctrl+Alt+L");

    resetBindings();
    expect(loadBindings().find((b) => b.action === "nav-launch")?.combo).toBe("Alt+1");
    expect(loadBindings().find((b) => b.action === "nav-more")?.combo).toBe("Alt+4");
  });

  it("损坏的覆盖值不影响目录默认键（仅 string 类型生效）", () => {
    applyOverrides({ "nav-launch": 123 } as unknown as Record<string, string>);
    expect(loadBindings().find((b) => b.action === "nav-launch")?.combo).toBe("Alt+1");
    applyOverrides(null);
  });
});
