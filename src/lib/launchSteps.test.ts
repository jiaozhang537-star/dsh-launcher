import { describe, expect, it } from "vitest";

import { settledStageText, stepLevelOf } from "./launchSteps";

describe("stepLevelOf", () => {
  it("后端 Error 级 → note（未通过：拦截/失败那一路）", () => {
    expect(stepLevelOf("error")).toBe("note");
  });

  it("后端 Warning 级 → info（级别原样，不升格成通过也不降格成问题）", () => {
    expect(stepLevelOf("warning")).toBe("info");
  });
});

describe("settledStageText", () => {
  it("认识的阶段换成完成态措辞（进行时留在一行已完成项里是错的）", () => {
    expect(settledStageText("正在校验运行环境…")).toBe("运行环境校验通过");
    expect(settledStageText("正在对账实例配置…")).toBe("实例配置已对账");
    expect(settledStageText("正在同步模型配置…")).toBe("模型配置已同步");
    expect(settledStageText("实例正在运行，正在重启实例…")).toBe("旧进程已停止，按原端口重启");
    expect(settledStageText("正在拉起实例进程…")).toBe("实例进程已拉起");
    expect(settledStageText("等待 DSH 装载插件层并开放网页…")).toBe("DSH 已装载插件层并开放网页");
  });

  it("不认识的阶段原样返回——不替后端编完成态", () => {
    const unknown = "正在做一件新阶段才会做的事…";
    expect(settledStageText(unknown)).toBe(unknown);
  });

  it("结论文案（不是阶段）原样返回", () => {
    expect(settledStageText("插件体检通过：3 个插件无静态冲突")).toBe("插件体检通过：3 个插件无静态冲突");
  });
});
