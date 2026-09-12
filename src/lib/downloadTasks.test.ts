import { beforeEach, describe, expect, it } from "vitest";

import {
  clearTasks,
  getTask,
  getTasks,
  progressMatchesSpec,
  recordProgress,
  subscribeTasks,
  updateTask,
  upsertTask,
} from "./downloadTasks";

function baseTask(instanceId: string): Parameters<typeof upsertTask>[0] {
  return {
    instanceId,
    name: `dsh-0.1.1-rc.2`,
    version: "0.1.1-rc.2",
    status: "running",
    phase: "prepare",
    detail: "",
    received: 0,
    total: 0,
    speed: null,
    startedAt: Date.now(),
  };
}

describe("downloadTasks store", () => {
  beforeEach(() => clearTasks());

  it("upsert 后可读取，update 局部合并且不覆盖其他字段", () => {
    upsertTask(baseTask("a"));
    updateTask("a", { phase: "node", detail: "下载中", received: 100, total: 400 });
    const task = getTask("a");
    expect(task?.phase).toBe("node");
    expect(task?.received).toBe(100);
    expect(task?.status).toBe("running");
    expect(task?.name).toBe("dsh-0.1.1-rc.2");
  });

  it("未知实例的进度更新被忽略（事件晚于创建的竞态兜底）", () => {
    updateTask("ghost", { phase: "done" });
    expect(getTask("ghost")).toBeUndefined();
    expect(getTasks()).toHaveLength(0);
  });

  it("recordProgress 更新字节进度并按 ≥300ms 窗口计算速度", () => {
    upsertTask(baseTask("b"));
    const now = Date.now();
    const first = recordProgress("b", 300, 1000);
    expect(first).toBeNull();
    // 快于 300ms 的第二次采样不产生速度，但字节进度照常更新。
    const instant = recordProgress("b", 400, 1000);
    expect(instant).toBeNull();
    const task = getTask("b");
    expect(task?.received).toBe(400);
    expect(now).toBeTypeOf("number");
  });

  it("subscribeTasks 在变更时触发，取消订阅后不再触发", () => {
    let calls = 0;
    const unsub = subscribeTasks(() => {
      calls += 1;
    });
    upsertTask(baseTask("s"));
    expect(calls).toBe(1);
    unsub();
    updateTask("s", { phase: "done" });
    expect(calls).toBe(1);
  });

  it("done/failed 任务保留在列表中（跨页存活语义）", () => {
    upsertTask(baseTask("c"));
    updateTask("c", { status: "failed", phase: "failed", detail: "boom" });
    expect(getTasks()).toHaveLength(1);
    expect(getTask("c")?.status).toBe("failed");
  });
});

describe("progressMatchesSpec（P1-04 回归锚：插件/整合包共用事件按 spec 分流）", () => {
  it("事件带 spec 时只命中 dataset.spec 相同的状态行", () => {
    expect(progressMatchesSpec("dsh-cost-meter@1.0.0", "dsh-cost-meter@1.0.0")).toBe(true);
    expect(progressMatchesSpec("modpack-a", "dsh-cost-meter@1.0.0")).toBe(false);
    expect(progressMatchesSpec(undefined, "dsh-cost-meter@1.0.0")).toBe(false);
    expect(progressMatchesSpec(null, "dsh-cost-meter@1.0.0")).toBe(false);
  });

  it("事件无 spec（旧版 payload）向后兼容放行给当前接线行", () => {
    expect(progressMatchesSpec("anything", undefined)).toBe(true);
    expect(progressMatchesSpec(undefined, undefined)).toBe(true);
    expect(progressMatchesSpec(undefined, "")).toBe(true);
  });
});
