/** 更多页：左分类（关于）+ 右内容，全局左右栏布局。
 *  使用帮助 / 检查更新两卡已按 Phase 5 R3 删除（纯教程散文与占位卡）；
 *  检查更新待 Phase 6（FR-13）交付时重建。 */
import { el } from "../lib/dom";
import type { PageDefinition } from "../router";

const APP_VERSION = "0.1.0";

const CATEGORIES: { id: string; label: string }[] = [{ id: "about", label: "关于" }];

function renderAbout(): HTMLElement {
  return el("div", { class: "settings-pane" }, [
    el("section", { class: "card settings-card" }, [
      el("h2", { class: "card__title" }, ["关于"]),
      el("div", { class: "settings-node-card" }, [
        el("span", { class: "settings-badge settings-badge--ok" }, [`v${APP_VERSION}`]),
      ]),
      el("p", { class: "settings-path" }, ["开源协议：MIT；壳不修改 DSH 本体。"]),
    ]),
  ]);
}

export const morePage: PageDefinition = {
  id: "more",
  navLabel: "更多",
  render: () => {
    let category = "about";

    const leftNav = el("div", { class: "sub-left__group" }, []);
    const pane = el("div", { class: "settings-pane" }, []);

    function renderRight(): void {
      pane.replaceChildren(renderAbout());
    }
    function renderLeft(): void {
      leftNav.replaceChildren();
      leftNav.append(el("div", { class: "sub-left__label" }, ["更多"]));
      for (const item of CATEGORIES) {
        leftNav.append(
          el(
            "button",
            { type: "button", class: `sub-left__item ${item.id === category ? "sub-left__item--active" : ""}` },
            [el("span", { class: "sub-left__item-main" }, [item.label])],
          ),
        );
      }
      for (const button of [...leftNav.querySelectorAll("button")]) {
        button.addEventListener("click", () => {
          const label = button.textContent?.trim() ?? "";
          const found = CATEGORIES.find((c) => c.label === label);
          if (!found || found.id === category) return;
          category = found.id;
          renderLeft();
          renderRight();
        });
      }
    }

    renderLeft();
    renderRight();

    return el("section", { class: "page" }, [
      el("div", { class: "sub sub--compact" }, [
        el("aside", { class: "sub-left" }, [leftNav]),
        el("section", { class: "sub-right" }, [pane]),
      ]),
    ]);
  },
};
