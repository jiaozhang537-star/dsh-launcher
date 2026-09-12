// @vitest-environment happy-dom
import { describe, expect, it } from "vitest";
import { el } from "./dom";

describe("el", () => {
  it("创建带属性的元素并按顺序挂载文本与子节点", () => {
    const child = el("span", { class: "inner" });
    const root = el("button", { id: "ok", "data-nav": "launch" }, ["启动", child]);

    expect(root.tagName).toBe("BUTTON");
    expect(root.id).toBe("ok");
    expect(root.dataset.nav).toBe("launch");
    expect(root.childNodes[0].textContent).toBe("启动");
    expect(root.childNodes[1]).toBe(child);
  });

  it("attrs 缺省为空、children 缺省为空数组", () => {
    const node = el("div");
    expect(node.attributes.length).toBe(0);
    expect(node.childElementCount).toBe(0);
  });
});
