import { describe, expect, it } from "vitest";

import { fmtCount, relTime } from "./market-shared";

describe("fmtCount（市场/Skill 共享格式化）", () => {
  it("小数字原样显示", () => {
    expect(fmtCount(0)).toBe("0");
    expect(fmtCount(876)).toBe("876");
    expect(fmtCount(9999)).toBe("9999");
  });

  it("万级一位小数", () => {
    expect(fmtCount(31000)).toBe("3.1 万");
    expect(fmtCount(99999)).toBe("10.0 万");
  });

  it("亿级两位小数", () => {
    expect(fmtCount(218_000_000)).toBe("2.18 亿");
  });
});

describe("relTime（相对时间）", () => {
  it("空串与无法解析的输入不产出误导文案", () => {
    expect(relTime("")).toBe("未知");
    expect(relTime("not-a-date")).toBe("not-a-date");
  });

  it("今天 / 近 30 天 / 30 天外三段语义", () => {
    const now = Date.now();
    expect(relTime(new Date(now - 3600_000).toISOString())).toBe("今天");
    expect(relTime(new Date(now - 3 * 86400_000).toISOString())).toBe("3 天前");
    // 超 30 天回退日期显示（取 RFC3339 前 10 位）。
    const old = "2025-01-02T03:04:05Z";
    expect(relTime(old)).toBe("2025-01-02");
  });
});
