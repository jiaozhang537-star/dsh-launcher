import { describe, expect, it } from "vitest";

import { BASELINE_DSH_SHORTCUTS_VERSION, DSH_SHORTCUTS, DSH_SHORTCUT_SECTIONS } from "./dsh-hotkeys";

describe("DSH 0.2.0 快捷键体系快照", () => {
  it("两小节齐备：可编辑 18 + 固定 11，条目小节都有定义", () => {
    expect(DSH_SHORTCUT_SECTIONS.map((s) => s.id)).toEqual(["editable", "fixed"]);
    const editable = DSH_SHORTCUTS.filter((e) => e.section === "editable");
    const fixed = DSH_SHORTCUTS.filter((e) => e.section === "fixed");
    expect(editable, "可编辑命令应 18 条").toHaveLength(18);
    expect(fixed, "固定动作应 11 条").toHaveLength(11);
    for (const s of DSH_SHORTCUT_SECTIONS) {
      expect(
        DSH_SHORTCUTS.some((e) => e.section === s.id),
        `小节「${s.id}」至少应有一条目`,
      ).toBe(true);
    }
  });

  it("官方命令 id 唯一、label 与 keys 非空", () => {
    const ids = new Set<string>();
    for (const entry of DSH_SHORTCUTS) {
      expect(ids.has(entry.id), `重复命令 id：${entry.id}`).toBe(false);
      ids.add(entry.id);
      expect(entry.label.trim().length, `label 为空：${entry.id}`).toBeGreaterThan(0);
      expect(entry.keys.length, `keys 为空：${entry.id}`).toBeGreaterThan(0);
      for (const key of entry.keys) {
        expect(key.trim().length, `空键位：${entry.id}`).toBeGreaterThan(0);
      }
    }
  });

  it("可编辑条目不得混入固定前缀，固定条目以 fixed/response/approval 命名", () => {
    for (const entry of DSH_SHORTCUTS.filter((e) => e.section === "editable")) {
      expect(entry.id.startsWith("fixed."), `可编辑条目不得用 fixed 前缀：${entry.id}`).toBe(false);
    }
    for (const entry of DSH_SHORTCUTS.filter((e) => e.section === "fixed")) {
      expect(
        /^(fixed\.|response\.|approval\.)/.test(entry.id),
        `固定条目命名异常：${entry.id}`,
      ).toBe(true);
    }
  });

  it("基线版本钉住（随官方版本复检后手动更新）", () => {
    expect(BASELINE_DSH_SHORTCUTS_VERSION).toBe("0.2.0-rc.2");
  });
});
