// @vitest-environment happy-dom
import { beforeEach, describe, expect, it } from "vitest";

import type { MarketCard } from "../services/market";
import { marketDetailPage, openMarketDetail } from "./market-detail";

const baseCard = {
  name: "demo",
  subName: "owner/demo",
  description: "测试描述",
  iconUrl: "",
  tags: ["效率"],
  monthlyDownloads: null,
  latestVersion: null,
  stars: null,
  updatedAt: "2026-09-01T00:00:00Z",
  source: "github",
  npmName: null,
  repo: "owner/demo",
  category: "其他",
  verified: null,
} as unknown as MarketCard;

describe("market-detail 二级页", () => {
  beforeEach(() => {
    location.hash = "";
  });

  it("无选中项（刷新直达）时渲染空态指路", () => {
    // 直接渲染（未 open 过）→ 空态
    const root = marketDetailPage.render();
    expect(root.textContent).toContain("没有选中的资源");
  });

  it("信息卡含名称/描述/元信息，操作行含打开仓库与复制名称", () => {
    openMarketDetail({
      kind: "plugin",
      name: "demo",
      subName: "owner/demo",
      iconUrl: "",
      tags: ["效率"],
      description: "测试描述",
      meta: document.createElement("div"),
      displayName: "demo (owner/demo)",
      repo: "owner/demo",
      marketCard: baseCard,
    });
    const root = marketDetailPage.render();
    expect(root.querySelector(".mk-detail__head")).not.toBeNull();
    expect(root.textContent).toContain("demo");
    expect(root.textContent).toContain("测试描述");
    const actions = root.querySelector(".settings-actions");
    expect(actions?.textContent).toContain("打开仓库");
    expect(actions?.textContent).toContain("复制名称");
  });

  it("npm 卡渲染「全部版本」版本卡（加载态先出现）", () => {
    openMarketDetail({
      kind: "plugin",
      name: "npm-demo",
      subName: "@scope/demo",
      iconUrl: "",
      tags: [],
      description: "",
      meta: document.createElement("div"),
      displayName: "npm-demo",
      repo: null,
      marketCard: { ...baseCard, npmName: "@scope/demo" } as unknown as MarketCard,
    });
    const root = marketDetailPage.render();
    expect(root.textContent).toContain("全部版本");
  });

  it("GitHub 直装卡（无 npmName）渲染 Releases 版本卡、操作行带安装；标题随 displayName", () => {
    openMarketDetail({
      kind: "modpack",
      name: "pack",
      subName: "owner/pack",
      iconUrl: "",
      tags: [],
      description: "",
      meta: document.createElement("div"),
      displayName: "pack (owner/pack)",
      repo: "owner/pack",
      marketCard: baseCard,
      installDirect: () => {},
    });
    expect(marketDetailPage.titleOf?.()).toBe("pack (owner/pack)");
    const root = marketDetailPage.render();
    expect(root.textContent).toContain("全部版本");
    const actions = root.querySelector(".settings-actions");
    expect(actions?.textContent).toContain("安装");
  });
});
