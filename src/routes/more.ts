/** 更多页：左分类（关于 / 使用帮助 / 检查更新）+ 右内容，全局左右栏布局。 */
import { el } from "../lib/dom";
import type { PageDefinition } from "../router";

const APP_VERSION = "0.1.0";

type Category = "about" | "help" | "update";

const CATEGORIES: { id: Category; label: string }[] = [
  { id: "about", label: "关于" },
  { id: "help", label: "使用帮助" },
  { id: "update", label: "检查更新" },
];

function renderAbout(): HTMLElement {
  return el("div", { class: "settings-pane" }, [
    el("section", { class: "card settings-card" }, [
      el("h2", { class: "card__title" }, ["关于"]),
      el("div", { class: "settings-node-card" }, [
        el("span", { class: "settings-badge settings-badge--ok" }, [`v${APP_VERSION}`]),
        el("span", { class: "settings-path" }, ["DeepSeek Harness 的图形化启动器——像 PCL 管理 Minecraft 一样管理你的 DSH。"]),
      ]),
      el("p", { class: "settings-path" }, [
        "开源协议：MIT（仅需保留作者署名，允许商业使用与二次修改）。壳不修改 DSH 本体，一切设置经 DSH 原生配置机制生效。",
      ]),
    ]),
  ]);
}

function renderHelp(): HTMLElement {
  const steps = [
    "「下载」页选择 DSH 版本下载为独立实例（同名自动 -2/-3 后缀）。",
    "「设置 · 模型提供商」勾选启用厂商，按需填写 API Key（也可用环境变量 DEEPSEEK_API_KEY 自动配置）并添加模型。",
    "「启动」页选厂商与默认模型，点「启动」；运行中条目可停止、看日志、打开网页版。",
    "下载缓慢时在「设置 · 配置」切换镜像源；官方直连始终可选。",
  ];
  const pane = el("div", { class: "settings-pane" }, [
    el("section", { class: "card settings-card" }, [el("h2", { class: "card__title" }, ["使用帮助"])]),
  ]);
  const card = pane.querySelector(".card")!;
  for (const [index, text] of steps.entries()) {
    card.append(el("p", { class: "settings-path" }, [`${index + 1}. ${text}`]));
  }
  return pane;
}

function renderUpdate(): HTMLElement {
  const statusLine = el("p", { class: "settings-status" }, ["当前已是最新版本检查通道——Phase 6 交付。"]);
  const button = el("button", { type: "button", class: "btn btn--ghost settings-inline-btn", disabled: "" }, [
    "检查更新",
  ]);
  return el("div", { class: "settings-pane" }, [
    el("section", { class: "card settings-card" }, [
      el("h2", { class: "card__title" }, ["检查更新"]),
      el("div", { class: "settings-row" }, [button]),
      statusLine,
    ]),
  ]);
}

export const morePage: PageDefinition = {
  id: "more",
  navLabel: "更多",
  render: () => {
    let category: Category = "about";

    const leftNav = el("div", { class: "sub-left__group" }, []);
    leftNav.append(el("div", { class: "sub-left__label" }, ["更多"]));
    const pane = el("div", { class: "settings-pane" }, []);

    function renderRight(): void {
      if (category === "about") pane.replaceChildren(renderAbout());
      else if (category === "help") pane.replaceChildren(renderHelp());
      else pane.replaceChildren(renderUpdate());
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
