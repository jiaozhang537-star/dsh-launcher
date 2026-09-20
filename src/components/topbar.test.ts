// @vitest-environment happy-dom
import { beforeEach, describe, expect, it, vi } from "vitest";

import type { PageDefinition } from "../router";

/** 顶栏在创建时订阅 router 单例，用例之间必须把模块整体换新。 */
async function freshTopbar() {
  vi.resetModules();
  const router = await import("../router");
  const { createTopbar } = await import("./topbar");
  return { router, createTopbar };
}

const settle = async (): Promise<void> => {
  for (let i = 0; i < 4; i++) await new Promise((resolve) => setTimeout(resolve, 0));
};

const LAUNCH: PageDefinition = {
  id: "launch",
  navLabel: "启动",
  render: () => document.createElement("section"),
};

const SETTINGS: PageDefinition = {
  id: "instance-settings",
  navLabel: "版本设置",
  secondary: true,
  render: () => document.createElement("section"),
};

async function mounted() {
  const { router, createTopbar } = await freshTopbar();
  router.registerPages([LAUNCH, SETTINGS]);
  const container = document.createElement("main");
  document.body.append(container, createTopbar());
  router.mountRouter(container);
  await settle();
  return {
    router,
    secondary: document.querySelector(".topbar__secondary") as HTMLElement,
    title: document.querySelector(".topbar__sec-title") as HTMLElement,
  };
}

describe("topbar 二级页形态", () => {
  beforeEach(() => {
    location.hash = "";
    document.body.replaceChildren();
  });

  it("导航页隐藏二级栏（← + 页面名）", async () => {
    const { secondary } = await mounted();
    expect(secondary.hidden).toBe(true);
  });

  it("二级页带返回栏，标题只到页面名——不带实例名（需求方 2026-09-20 裁决）", async () => {
    const { router, secondary, title } = await mounted();
    router.navigate("instance-settings");
    await settle();

    expect(secondary.hidden).toBe(false);
    expect(title.textContent).toBe("版本设置");
  });
});
