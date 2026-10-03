// @vitest-environment happy-dom
import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  DEFAULT_PAGE_ID,
  registeredPages,
  type PageDefinition,
} from "./router";

/** 每个用例独立模块实例：router 内部持有模块级注册表与监听器集合。 */
async function freshRouter() {
  vi.resetModules();
  return await import("./router");
}

function makePage(id: string): PageDefinition {
  return {
    id,
    navLabel: id,
    render: () => {
      const root = document.createElement("section");
      root.dataset.page = id;
      return root;
    },
  };
}

/** 等 hash 变化传播稳定：连续两个宏任务无变化即视为已落定。 */
async function waitForHashSettled(expected: string): Promise<void> {
  for (let i = 0; i < 50; i++) {
    if (location.hash === expected) {
      await new Promise((resolve) => setTimeout(resolve, 150));
      await new Promise((resolve) => setTimeout(resolve, 150));
      if (location.hash === expected) return;
    }
    await new Promise((resolve) => setTimeout(resolve, 150));
  }
}

describe("router", () => {
  beforeEach(() => {
    location.hash = "";
    document.body.replaceChildren();
  });

  it("未挂载时 navigate 仅改 hash，不抛错；挂载后渲染对应页面", async () => {
    const r = await freshRouter();
    r.registerPages([makePage("launch"), makePage("downloads")]);
    const container = document.createElement("main");
    document.body.append(container);
    r.mountRouter(container);

    r.navigate("downloads");
    // hashchange 由 happy-dom 异步派发，等待微任务后断言
    await new Promise((resolve) => setTimeout(resolve, 150));
    expect(location.hash).toBe("#/downloads");
    expect(container.querySelector("section")?.dataset.page).toBe("downloads");
  });

  it("未知 hash 回落到默认页", async () => {
    const r = await freshRouter();
    r.registerPages([makePage(DEFAULT_PAGE_ID)]);
    location.hash = "#/no-such-page";
    const container = document.createElement("main");
    document.body.append(container);
    r.mountRouter(container);
    expect(container.querySelector("section")?.dataset.page).toBe(DEFAULT_PAGE_ID);
  });

  it("空 hash 挂载时补写默认页 hash 并渲染默认页", async () => {
    const r = await freshRouter();
    r.registerPages([makePage(DEFAULT_PAGE_ID), makePage("more")]);
    const container = document.createElement("main");
    document.body.append(container);
    r.mountRouter(container);
    expect(location.hash).toBe(`#/${DEFAULT_PAGE_ID}`);
    expect(container.querySelector("section")?.dataset.page).toBe(DEFAULT_PAGE_ID);
  });

  it("subscribe 在挂载后的每次切换收到当前页 id", async () => {
    const r = await freshRouter();
    r.registerPages([makePage("launch"), makePage("settings")]);
    const seen: string[] = [];
    const container = document.createElement("main");
    document.body.append(container);
    r.mountRouter(container);
    r.subscribe((id) => seen.push(id));

    r.navigate("settings");
    await waitForHashSettled("#/settings");
    // happy-dom 可能对一次赋值派发多个 hashchange（真实浏览器恰好一个），
    // 断言只锁定语义结果：回调收到的是目标页且渲染层同步。
    expect(seen.length).toBeGreaterThan(0);
    expect(new Set(seen)).toEqual(new Set(["settings"]));
    expect(container.querySelector("section")?.dataset.page).toBe("settings");
  });

  it("registeredPages 按注册顺序返回页面定义", async () => {
    const r = await freshRouter();
    r.registerPages([makePage("b"), makePage("a")]);
    expect(r.registeredPages().map((p) => p.id)).toEqual(["b", "a"]);
    expect(registeredPages().map((p) => p.id)).toEqual([]); // 主实例不受 resetModules 影响
  });

  it("切页时调用上一页的 unmount（审查 R1：事件/定时器清理钩子）", async () => {
    const r = await freshRouter();
    const unmount = vi.fn();
    r.registerPages([{ ...makePage("launch"), unmount }, makePage("downloads")]);
    const container = document.createElement("main");
    document.body.append(container);
    r.mountRouter(container);
    expect(unmount).not.toHaveBeenCalled();

    r.navigate("downloads");
    await waitForHashSettled("#/downloads");
    expect(unmount).toHaveBeenCalledTimes(1);
    expect(container.querySelector("section")?.dataset.page).toBe("downloads");
  });

  it("unmount 抛错不阻断新页渲染", async () => {
    const r = await freshRouter();
    r.registerPages([
      {
        ...makePage("launch"),
        unmount: () => {
          throw new Error("cleanup boom");
        },
      },
      makePage("downloads"),
    ]);
    const container = document.createElement("main");
    document.body.append(container);
    r.mountRouter(container);
    r.navigate("downloads");
    await waitForHashSettled("#/downloads");
    expect(container.querySelector("section")?.dataset.page).toBe("downloads");
  });

  it("goBack 按来路逐级回退，不与上一页互相弹跳", async () => {
    const r = await freshRouter();
    r.registerPages([makePage("launch"), makePage("select"), makePage("settings")]);
    const container = document.createElement("main");
    document.body.append(container);
    r.mountRouter(container);

    r.navigate("select");
    await waitForHashSettled("#/select");
    r.navigate("settings");
    await waitForHashSettled("#/settings");

    r.goBack();
    await waitForHashSettled("#/select");
    expect(container.querySelector("section")?.dataset.page).toBe("select");

    // P5 单槽 lastPageId 在此步会弹回 settings（乒乓）；真栈应回到 launch
    r.goBack();
    await waitForHashSettled("#/launch");
    expect(container.querySelector("section")?.dataset.page).toBe("launch");
  });

  it("刷新直达二级页时 goBack 回主页", async () => {
    const r = await freshRouter();
    r.registerPages([makePage("launch"), makePage("settings")]);
    location.hash = "#/settings";
    const container = document.createElement("main");
    document.body.append(container);
    r.mountRouter(container);

    r.goBack();
    await waitForHashSettled("#/launch");
    expect(container.querySelector("section")?.dataset.page).toBe("launch");
  });

  it("同 hash 重复派发 hashchange 不污染历史，goBack 仍按来路", async () => {
    const r = await freshRouter();
    r.registerPages([makePage("launch"), makePage("settings")]);
    const container = document.createElement("main");
    document.body.append(container);
    r.mountRouter(container);

    r.navigate("settings");
    await waitForHashSettled("#/settings");
    // happy-dom 对一次赋值可能派发多次 hashchange：补发一次模拟。
    // 若栈被重复压入同名页，goBack 的「栈顶-1」将等于当前页 → 卡死不回。
    window.dispatchEvent(new Event("hashchange"));
    await new Promise((resolve) => setTimeout(resolve, 200));

    r.goBack();
    await waitForHashSettled("#/launch");
    expect(container.querySelector("section")?.dataset.page).toBe("launch");
  });
});
