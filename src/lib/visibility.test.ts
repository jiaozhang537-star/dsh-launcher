import { describe, expect, it } from "vitest";
import { exitHookDecision, visibilityOp } from "./visibility";

describe("visibilityOp（FR-28：网页就绪时的窗口动作，PCL LaunchArgumentVisible 值系）", () => {
  it("档 0 = 立即关闭，档 2/3 = 隐藏，档 4 = 最小化", () => {
    expect(visibilityOp(0)).toBe("exit");
    expect(visibilityOp(2)).toBe("hide");
    expect(visibilityOp(3)).toBe("hide");
    expect(visibilityOp(4)).toBe("minimize");
  });

  it("档 5 与未配置（null/undefined/越界值）一律保持不变（含 PCL 弃用档位 1）", () => {
    expect(visibilityOp(5)).toBe("keep");
    expect(visibilityOp(null)).toBe("keep");
    expect(visibilityOp(undefined)).toBe("keep");
    // PCL 值系里 1 从未使用；收到时按默认档处理。
    expect(visibilityOp(1)).toBe("keep");
    expect(visibilityOp(9)).toBe("keep");
  });
});

describe("exitHookDecision（FR-28：最后实例退出后的关/开裁决）", () => {
  it("档 2 = 最后实例退出后关闭启动器", () => {
    expect(exitHookDecision(2, 0)).toBe("exit");
  });

  it("档 3 = 最后实例退出后重新打开启动器", () => {
    expect(exitHookDecision(3, 0)).toBe("show");
  });

  it("仍有实例在跑时不动作（多实例泛化：全部退出才算「游戏退出」）", () => {
    expect(exitHookDecision(2, 1)).toBe("none");
    expect(exitHookDecision(3, 2)).toBe("none");
  });

  it("档 0/4/5 与未配置在退出时一律不动", () => {
    expect(exitHookDecision(0, 0)).toBe("none");
    expect(exitHookDecision(4, 0)).toBe("none");
    expect(exitHookDecision(5, 0)).toBe("none");
    expect(exitHookDecision(null, 0)).toBe("none");
    expect(exitHookDecision(undefined, 0)).toBe("none");
  });
});
