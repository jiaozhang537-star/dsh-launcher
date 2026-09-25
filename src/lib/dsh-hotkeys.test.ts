import { describe, expect, it } from "vitest";

import { BASELINE_DSH_VERSION, BASELINE_VERIFIED_RANGE, DSH_HOTKEYS, DSH_HOTKEY_GROUPS } from "./dsh-hotkeys";

describe("DSH 网页端快捷键目录", () => {
  it("分组定义完备：每组都有条目，条目分组都有定义", () => {
    const used = new Set(DSH_HOTKEYS.map((e) => e.group));
    for (const g of DSH_HOTKEY_GROUPS) {
      expect(used.has(g.id), `分组「${g.id}」至少应有一条目`).toBe(true);
    }
    for (const id of used) {
      expect(DSH_HOTKEY_GROUPS.some((g) => g.id === id), `条目分组「${id}」应有分组定义`).toBe(true);
    }
  });

  it("条目 id 唯一、label 与 keys 非空", () => {
    const ids = new Set<string>();
    for (const entry of DSH_HOTKEYS) {
      expect(ids.has(entry.id), `重复条目 id：${entry.id}`).toBe(false);
      ids.add(entry.id);
      expect(entry.label.trim().length, `label 为空：${entry.id}`).toBeGreaterThan(0);
      expect(entry.keys.length, `keys 为空：${entry.id}`).toBeGreaterThan(0);
      for (const key of entry.keys) {
        expect(key.trim().length, `空键位：${entry.id}`).toBeGreaterThan(0);
      }
    }
  });

  it("基线版本钉住（DSH 升级复检后手动更新）", () => {
    expect(BASELINE_DSH_VERSION).toBe("0.1.5-rc.1");
    expect(BASELINE_VERIFIED_RANGE).toContain(BASELINE_DSH_VERSION.slice(0, 3));
  });
});
