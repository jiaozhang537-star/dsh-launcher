import { describe, expect, it } from "vitest";

import { getSelectedInstance, setSelectedInstance } from "./selected-instance";

describe("selected-instance（P3-06：跨页单例可测化）", () => {
  it("写入后可读回 id", () => {
    setSelectedInstance("i-abc");
    expect(getSelectedInstance()).toBe("i-abc");
  });

  it("清除（null）后读回 null", () => {
    setSelectedInstance("i-abc");
    setSelectedInstance(null);
    expect(getSelectedInstance()).toBeNull();
  });
});
