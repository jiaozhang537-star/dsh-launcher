import { describe, expect, it } from "vitest";

import {
  deriveVendorView,
  getVendorCatalog,
  keySaveEligible,
  launchModelsFor,
  pruneSessionKeyMarks,
} from "./vendor";
import { KEY_MASK } from "../services/keyvault";

describe("deriveVendorView", () => {
  it("无保险库状态时回落 deepseek 占位（未启用、无 key）", () => {
    const view = deriveVendorView(null);
    expect(view.activeId).toBe("deepseek");
    expect(view.entries).toHaveLength(1);
    expect(view.entries[0]?.enabled).toBe(false);
    expect(view.entries[0]?.hasKey).toBe(false);
    expect(view.entries[0]?.models).toEqual([]);
  });

  it("空状态同 null：仅占位展示 deepseek", () => {
    const view = deriveVendorView({ vendors: [] });
    expect(view.activeId).toBe("deepseek");
    expect(view.entries[0]?.enabled).toBe(false);
  });

  it("只拉出启用的厂商，并携带 key 状态、地址与模型列表", () => {
    const view = deriveVendorView({
      vendors: [
        {
          vendor: "deepseek",
          enabled: true,
          hasKey: true,
          envAuto: false,
          address: null,
          models: ["deepseek-v4-flash"],
        },
      ],
    });
    expect(view.activeId).toBe("deepseek");
    expect(view.entries[0]?.enabled).toBe(true);
    expect(view.entries[0]?.hasKey).toBe(true);
    expect(view.entries[0]?.models).toEqual(["deepseek-v4-flash"]);
  });

  it("环境变量自动配置同样计为有 key", () => {
    const view = deriveVendorView({
      vendors: [
        { vendor: "deepseek", enabled: true, hasKey: false, envAuto: true, address: null, models: [] },
      ],
    });
    expect(view.entries[0]?.hasKey).toBe(true);
  });

  it("全部停用回落占位；目录外的启用厂商被忽略", () => {
    const view = deriveVendorView({
      vendors: [
        { vendor: "deepseek", enabled: false, hasKey: true, envAuto: false, address: null, models: [] },
        { vendor: "future-vendor", enabled: true, hasKey: true, envAuto: false, address: null, models: [] },
      ],
    });
    expect(view.entries).toHaveLength(1);
    expect(view.entries[0]?.id).toBe("deepseek");
    expect(view.entries[0]?.enabled).toBe(false);
    expect(view.entries[0]?.hasKey).toBe(false);
  });

  it("目录条目带官方图标资源路径", () => {
    expect(getVendorCatalog()[0]?.icon).toBe("/vendor-logos/deepseek.png");
    expect(getVendorCatalog()[0]?.label).toBe("deepseek");
  });
});

describe("launchModelsFor", () => {
  it("返回启用厂商的模型列表", () => {
    const status = {
      vendors: [
        {
          vendor: "deepseek",
          enabled: true,
          hasKey: true,
          envAuto: false,
          address: null,
          models: ["m1", "m2"],
        },
      ],
    };
    expect(launchModelsFor(status, "deepseek")).toEqual(["m1", "m2"]);
  });

  it("厂商停用或未知时返回空列表", () => {
    const status = {
      vendors: [
        { vendor: "deepseek", enabled: false, hasKey: true, envAuto: false, address: null, models: ["m1"] },
      ],
    };
    expect(launchModelsFor(status, "deepseek")).toEqual([]);
    expect(launchModelsFor(null, "deepseek")).toEqual([]);
  });
});

describe("keySaveEligible（P3-02）", () => {
  it("用户真实输入且非哨兵/空值才允许保存", () => {
    expect(keySaveEligible("sk-real-key", true)).toBe(true);
    expect(keySaveEligible("  sk-with-space  ", true)).toBe(true);
  });

  it("未编辑（dirty=false）恒判未修改", () => {
    expect(keySaveEligible("sk-real-key", false)).toBe(false);
    expect(keySaveEligible("", false)).toBe(false);
  });

  it("掩码哨兵与空串恒判未修改——复制掩码回贴不误存", () => {
    expect(keySaveEligible(KEY_MASK, true)).toBe(false);
    expect(keySaveEligible(`  ${KEY_MASK}  `, true)).toBe(false);
    expect(keySaveEligible("", true)).toBe(false);
    expect(keySaveEligible("   ", true)).toBe(false);
  });
});

describe("pruneSessionKeyMarks（P3-01）", () => {
  it("权威状态已无 Key 的会话标记被撤销（保险库清除后不残留掩码态）", () => {
    const marks = new Set(["deepseek", "kimi"]);
    pruneSessionKeyMarks(marks, {
      vendors: [
        { vendor: "deepseek", enabled: true, hasKey: false, envAuto: false, address: null, models: [] },
        { vendor: "kimi", enabled: true, hasKey: true, envAuto: false, address: null, models: [] },
      ],
    });
    // 保险库已无 Key → 标记撤销；仍有 Key → 标记保留。
    expect(marks.has("deepseek")).toBe(false);
    expect(marks.has("kimi")).toBe(true);
  });

  it("环境变量 Key 同样算权威来源；厂商被删则标记无意义一并撤销", () => {
    const marks = new Set(["ollama", "custom-x"]);
    pruneSessionKeyMarks(marks, {
      vendors: [
        { vendor: "ollama", enabled: true, hasKey: false, envAuto: true, address: null, models: [] },
      ],
    });
    expect(marks.has("ollama")).toBe(true);
    // 厂商已不存在 → 标记撤销。
    expect(marks.has("custom-x")).toBe(false);
  });

  it("状态读取失败（null）不修剪——刚保存的掩码态不被瞬时故障闪掉", () => {
    const marks = new Set(["deepseek"]);
    pruneSessionKeyMarks(marks, null);
    expect(marks.has("deepseek")).toBe(true);
  });
});
