import { beforeEach, describe, expect, it } from "vitest";

import {
  clearFinishedTasks,
  clearTasks,
  controlKeyOf,
  getTask,
  getTasks,
  isTaskActive,
  progressMatchesSpec,
  recordProgress,
  subscribeTasks,
  terminalFailure,
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

describe("FR-09 W1：暂停/取消状态与控制柄键", () => {
  beforeEach(() => clearTasks());

  it("isTaskActive：running 与 paused 算活动，终态不算", () => {
    upsertTask(baseTask("a"));
    expect(isTaskActive(getTask("a")!)).toBe(true);
    updateTask("a", { status: "paused", detail: "已暂停" });
    expect(isTaskActive(getTask("a")!)).toBe(true);
    updateTask("a", { status: "cancelled", phase: "cancelled", detail: "已取消" });
    expect(isTaskActive(getTask("a")!)).toBe(false);
  });

  it("controlKeyOf 按任务类别拼出与后端 task_queue 一致的键", () => {
    const base = baseTask("i-1");
    expect(controlKeyOf({ ...base, kind: "install" })).toBe("install-i-1");
    expect(controlKeyOf({ ...base, kind: "plugin" })).toBe("plugin-i-1");
    expect(controlKeyOf({ ...base, kind: "node", instanceId: "node-24.12.0" })).toBe("node-24.12.0");
    expect(controlKeyOf({ ...base, kind: "source", instanceId: "source-global-0.1.1-rc.2" })).toBe(
      "source-global-0.1.1-rc.2",
    );
    // 旧调用方未标 kind：按 install 兜底。
    expect(controlKeyOf(base)).toBe("install-i-1");
  });

  it("terminalFailure：后端取消错误归类为 cancelled，其余为 failed", () => {
    const cancelled = terminalFailure("任务已取消（已接收 1.2 MB）");
    expect(cancelled.status).toBe("cancelled");
    expect(cancelled.phase).toBe("cancelled");
    expect(cancelled.detail).toBe("已取消");

    const failed = terminalFailure("npm install 失败（exit 1）");
    expect(failed.status).toBe("failed");
    expect(failed.phase).toBe("failed");
    expect(failed.detail).toContain("npm install 失败");
  });

  it("clearFinishedTasks 只清终态，活动任务保留", () => {
    upsertTask({ ...baseTask("done-1"), kind: "install" });
    updateTask("done-1", { status: "done", phase: "done", detail: "" });
    upsertTask({ ...baseTask("run-1"), kind: "install" });
    upsertTask({ ...baseTask("paused-1"), kind: "source" });
    updateTask("paused-1", { status: "paused", detail: "已暂停" });
    clearFinishedTasks();
    expect(getTask("done-1")).toBeUndefined();
    expect(getTask("run-1")).toBeDefined();
    expect(getTask("paused-1")).toBeDefined();
  });

  it("paused 任务的字节进度照常更新（恢复后续传显示）", () => {
    upsertTask({ ...baseTask("p"), kind: "source", canPause: true });
    updateTask("p", { status: "paused", detail: "已暂停" });
    recordProgress("p", 500, 1000);
    expect(getTask("p")?.received).toBe(500);
    expect(getTask("p")?.status).toBe("paused");
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
