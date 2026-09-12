import { describe, expect, it } from "vitest";

import { getSelectedInstance, getSelectedInstanceName, setSelectedInstance } from "./selected-instance";

describe("selected-instance（P3-06：跨页单例可测化）", () => {
  it("写入后可读回 id 与名称", () => {
    setSelectedInstance("i-abc", "我的实例");
    expect(getSelectedInstance()).toBe("i-abc");
    expect(getSelectedInstanceName()).toBe("我的实例");
  });

  it("清除（null）时名称一并清空", () => {
    setSelectedInstance("i-abc", "我的实例");
    setSelectedInstance(null);
    expect(getSelectedInstance()).toBeNull();
    expect(getSelectedInstanceName()).toBeNull();
  });

  it("更新 id 不带名称参数时保留旧名称（顶栏标题不闪「未选择」）", () => {
    setSelectedInstance("i-abc", "旧名");
    setSelectedInstance("i-def");
    expect(getSelectedInstance()).toBe("i-def");
    expect(getSelectedInstanceName()).toBe("旧名");
  });

  it("显式传 null 名称即清除名称", () => {
    setSelectedInstance("i-abc", "旧名");
    setSelectedInstance("i-def", null);
    expect(getSelectedInstanceName()).toBeNull();
  });
});
