import { describe, expect, it } from "vitest";
import { sortInstancesByRecency, uniqueInstanceName } from "./instanceOrder";
import type { Instance } from "../services/instances";

function inst(partial: Partial<Instance> & { id: string }): Instance {
  return {
    name: partial.id,
    profile: "web",
    nodeVersion: null,
    dshVersion: null,
    port: 3080,
    lastLaunchedAt: null,
    createdAt: "2026-01-01T00:00:00Z",
    dir: null,
    ...partial,
  };
}

describe("sortInstancesByRecency", () => {
  it("lastLaunchedAt 新的排最前，未启动的按 createdAt 参与 同轴比较", () => {
    const a = inst({ id: "a", lastLaunchedAt: "2026-08-01T10:00:00Z", createdAt: "2026-01-01T00:00:00Z" });
    const b = inst({ id: "b", lastLaunchedAt: "2026-08-28T09:00:00Z", createdAt: "2026-01-02T00:00:00Z" });
    const c = inst({ id: "c", lastLaunchedAt: null, createdAt: "2026-08-27T00:00:00Z" });
    expect(sortInstancesByRecency([a, c, b]).map((i) => i.id)).toEqual(["b", "c", "a"]);
  });

  it("从未启动的按 createdAt 降序", () => {
    const old = inst({ id: "old", createdAt: "2026-01-01T00:00:00Z" });
    const mid = inst({ id: "mid", createdAt: "2026-05-01T00:00:00Z" });
    const recent = inst({ id: "recent", createdAt: "2026-08-28T00:00:00Z" });
    expect(sortInstancesByRecency([old, recent, mid]).map((i) => i.id)).toEqual([
      "recent",
      "mid",
      "old",
    ]);
  });

  it("字段缺失不抛错（回落空串，防御 IPC 异常）", () => {
    const broken = { id: "x" } as unknown as Instance;
    const normal = inst({ id: "ok" });
    expect(() => sortInstancesByRecency([broken, normal])).not.toThrow();
    expect(sortInstancesByRecency([broken, normal])[0].id).toBe("ok");
  });

  it("空列表返回空列表且不改原数组", () => {
    const list: Instance[] = [];
    expect(sortInstancesByRecency(list)).toEqual([]);
    const single = [inst({ id: "only" })];
    const sorted = sortInstancesByRecency(single);
    expect(sorted).not.toBe(single);
  });
});

describe("uniqueInstanceName", () => {
  it("无冲突原样返回", () => {
    expect(uniqueInstanceName("dsh-0.1.1", ["dsh-0.2.0"])).toBe("dsh-0.1.1");
  });

  it("撞名顺延 -2/-3", () => {
    const taken = ["dsh-0.1.1", "dsh-0.1.1-2", "dsh-0.1.1-3"];
    expect(uniqueInstanceName("dsh-0.1.1", taken)).toBe("dsh-0.1.1-4");
  });

  it("中间空洞会被填上（与后端 unique_name 同规则）", () => {
    const taken = ["dsh-0.1.1", "dsh-0.1.1-2", "dsh-0.1.1-4"];
    expect(uniqueInstanceName("dsh-0.1.1", taken)).toBe("dsh-0.1.1-3");
  });
});
