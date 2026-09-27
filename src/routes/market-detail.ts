/**
 * 市场详情二级页（Phase 5 返工 W：对齐 PCL「点行进详情」交互——
 * 信息卡 + 操作行 + 版本卡；行内展开已废除）。
 * hash 无参数位：选中项存模块级 current，刷新直达时显示空态指路（已知限制，登记于 phase 文件）。
 */

import { el } from "../lib/dom";
import { icon } from "../lib/icons";
import { navigate, type PageDefinition } from "../router";
import { showHint } from "../lib/hint";
import { cardIcon, fmtCount, relTime } from "./market-shared";
import {
  fetchReleasesCached,
  fetchVersionsCached,
  installStates,
  installSpecOf,
  installToCurrent,
} from "./market-install";
import type { GithubRelease } from "../services/market";
import type { MarketCard, MarketVersion } from "../services/market";

export type MarketDetailKind = "plugin" | "modpack" | "skill";

/** 详情目标：由列表页构造。marketCard 存在且有 npmName → 渲染版本卡；
 *  无 npmName（GitHub 直装 / Skill 整仓库）→ 安装走 installDirect。 */
export interface MarketDetailTarget {
  kind: MarketDetailKind;
  name: string;
  subName?: string;
  iconUrl?: string;
  tags: string[];
  description?: string;
  meta: HTMLElement;
  /** 顶栏标题：PCL 式「名称 (副名)」。 */
  displayName: string;
  repo?: string | null;
  /** npm 卡：版本卡的来源卡。 */
  marketCard?: MarketCard;
  /** 无版本卡种类的安装动作；onSettled 供按钮态重绘。 */
  installDirect?: (statusLine: HTMLElement, onSettled: () => void) => void;
}

let current: MarketDetailTarget | null = null;

/** 列表页行点击入口：记录目标并跳详情页。 */
export function openMarketDetail(target: MarketDetailTarget): void {
  current = target;
  navigate("market-detail");
}

/** 外链：Tauri opener 优先，浏览器环境回落 window.open（launch 页同款）。 */
async function openExternal(url: string): Promise<void> {
  try {
    const { openUrl } = await import("@tauri-apps/plugin-opener");
    await openUrl(url);
  } catch {
    window.open(url, "_blank", "noreferrer");
  }
}

/** 版本行：v 号 + 最新徽章 + 周下载/时间，右侧 hover 浮现安装钮（mk-ver-dl 语言）。 */
function versionRow(
  kind: "plugin" | "modpack",
  card: MarketCard,
  v: MarketVersion,
  statusLine: HTMLElement,
  onSettled: () => void,
): HTMLElement {
  const spec = installSpecOf(card, v.version);
  const state = installStates.get(spec);
  const row = el("div", { class: "mk-detail__ver" }, [
    el("span", { class: "ver-row__main" }, [`v${v.version}`]),
    v.latest ? el("span", { class: "mk-badge mk-badge--latest" }, ["最新"]) : document.createDocumentFragment(),
    el("span", { class: "mk-detail__ver-sub" }, [
      `${v.weeklyDownloads !== null ? `周下载 ${fmtCount(v.weeklyDownloads)}` : "周下载 --"} · ${relTime(v.publishedAt)}`,
    ]),
  ]);
  const acts = el("div", { class: "mk-ver-card__acts" }, []);
  if (!state || state.phase !== "installing") {
    // 状态徽章（安装完成/失败）优先于按钮；依赖库拦截沿用列表页口径。
  }
  if (state?.phase === "installing") {
    acts.append(el("span", { class: "mk-ver-state" }, [icon("spinner", 12), " 安装中…"]));
  } else {
    if (state?.phase === "done") {
      acts.append(el("span", { class: "mk-ver-state mk-ver-state--done" }, ["已安装并启用"]));
    } else if (state?.phase === "failed") {
      acts.append(
        el("span", { class: "mk-ver-state mk-ver-state--error", title: state.detail }, ["失败（可重试）"]),
      );
    }
    const dl = el("button", { type: "button", class: "ver-act mk-ver-dl", title: "下载并安装到当前实例" }, []);
    dl.append(icon("download", 17));
    dl.addEventListener("click", (event) => {
      event.stopPropagation();
      installToCurrent(kind, card, v.version, statusLine, onSettled);
    });
    acts.append(dl);
  }
  row.append(acts);
  return row;
}

/** 版本卡（npm 卡）：全部版本平铺，安装态变化后整卡重绘。 */
function versionsCardOf(
  kind: "plugin" | "modpack",
  card: MarketCard,
  statusLine: HTMLElement,
): HTMLElement {
  const wrap = el("div", { class: "mk-detail__versions" }, [
    el("p", { class: "settings-status" }, ["版本列表加载中…"]),
  ]);
  const load = (): void => {
    void fetchVersionsCached(card.npmName as string)
      .then((reply) => {
        if (!wrap.isConnected) return;
        wrap.replaceChildren();
        if (reply.versions.length === 0) {
          wrap.append(el("p", { class: "settings-status" }, ["registry 上没有可用版本"]));
          return;
        }
        if (!reply.declaresBundle) {
          wrap.append(
            el("p", { class: "settings-status settings-status--error" }, [
              icon("warning", 12),
              " 该包未声明 dsh.bundle.patch——是依赖库而非可加载的插件，已禁用安装。",
            ]),
          );
        }
        for (const v of reply.versions) {
          wrap.append(versionRow(kind, card, v, statusLine, load));
        }
      })
      .catch((err) => {
        if (wrap.isConnected) {
          wrap.replaceChildren(
            el("p", { class: "settings-status settings-status--error" }, [
              `版本列表拉取失败：${String(err).slice(0, 240)}`,
            ]),
          );
        }
      });
  };
  load();
  return el("section", { class: "card settings-card" }, [
    el("h2", { class: "card__title" }, ["全部版本"]),
    wrap,
  ]);
}

/** GitHub Releases 版本卡（git 直装卡的「历代版本」）：tag + 预发布徽章 +
 *  发布时间，hover 浮现安装钮（spec = github:repo#tag，后端原样透传 pnpm）。
 *  安装态变化经 onSettled 整卡重绘。 */
function releasesCardOf(
  kind: "plugin" | "modpack",
  card: MarketCard,
  statusLine: HTMLElement,
): HTMLElement {
  const wrap = el("div", { class: "mk-detail__versions" }, [
    el("p", { class: "settings-status" }, ["版本列表加载中…"]),
  ]);
  const rowOf = (rel: GithubRelease): HTMLElement => {
    const spec = installSpecOf(card, rel.tagName);
    const state = installStates.get(spec);
    const row = el("div", { class: "mk-detail__ver" }, [
      el("span", { class: "ver-row__main" }, [rel.tagName]),
      rel.prerelease ? el("span", { class: "mk-badge" }, ["预发布"]) : document.createDocumentFragment(),
      rel.name && rel.name !== rel.tagName
        ? el("span", { class: "mk-detail__ver-sub" }, [rel.name])
        : document.createDocumentFragment(),
      el("span", { class: "mk-detail__ver-sub" }, [rel.publishedAt ? relTime(rel.publishedAt) : ""]),
    ]);
    const acts = el("div", { class: "mk-ver-card__acts" }, []);
    if (state?.phase === "installing") {
      acts.append(el("span", { class: "mk-ver-state" }, [icon("spinner", 12), " 安装中…"]));
    } else {
      if (state?.phase === "done") {
        acts.append(el("span", { class: "mk-ver-state mk-ver-state--done" }, ["已安装并启用"]));
      } else if (state?.phase === "failed") {
        acts.append(
          el("span", { class: "mk-ver-state mk-ver-state--error", title: state.detail }, ["失败（可重试）"]),
        );
      }
      const dl = el("button", {
        type: "button",
        class: "ver-act mk-ver-dl",
        title: `下载并安装 ${rel.tagName} 到当前实例`,
      }, []);
      dl.append(icon("download", 17));
      dl.addEventListener("click", (event) => {
        event.stopPropagation();
        installToCurrent(kind, card, rel.tagName, statusLine, load);
      });
      acts.append(dl);
    }
    row.append(acts);
    return row;
  };
  const load = (): void => {
    void fetchReleasesCached(card.repo as string)
      .then((list) => {
        if (!wrap.isConnected) return;
        wrap.replaceChildren();
        if (list.length === 0) {
          wrap.append(
            el("p", { class: "settings-status" }, [
              "该仓库没有发布过 Release——顶部「安装」按钮装默认分支最新代码。",
            ]),
          );
          return;
        }
        for (const rel of list) wrap.append(rowOf(rel));
      })
      .catch((err) => {
        if (wrap.isConnected) {
          wrap.replaceChildren(
            el("p", { class: "settings-status settings-status--error" }, [
              "Releases 拉取失败：" + String(err).slice(0, 240),
            ]),
          );
        }
      });
  };
  load();
  return el("section", { class: "card settings-card" }, [
    el("h2", { class: "card__title" }, ["全部版本"]),
    wrap,
  ]);
}

/** 操作行：打开仓库 / 复制名称 /（无版本卡时）安装；安装态变化整行重绘。 */
function buildActions(target: MarketDetailTarget, statusLine: HTMLElement): HTMLElement {
  const actions = el("div", { class: "settings-actions" }, []);
  const render = (): void => {
    actions.replaceChildren();
    if (target.repo) {
      const repoBtn = el("button", { type: "button", class: "btn btn--line" }, ["打开仓库"]);
      repoBtn.addEventListener("click", () => void openExternal(`https://github.com/${target.repo}`));
      actions.append(repoBtn);
    }
    const copyBtn = el("button", { type: "button", class: "btn btn--line" }, ["复制名称"]);
    copyBtn.addEventListener("click", () => {
      navigator.clipboard?.writeText(target.name).then(
        () => showHint("已复制名称"),
        (err) => console.error("复制名称失败", err),
      );
    });
    actions.append(copyBtn);
    const gitCard = target.marketCard && !target.marketCard.npmName &&
      (target.kind === "plugin" || target.kind === "modpack") ? target.marketCard : null;
    if (gitCard) {
      // GitHub 来源的插件/整合包：无 npm 版本，详情页操作行直装当前实例。
      const spec = installSpecOf(gitCard, null);
      const state = installStates.get(spec);
      if (state?.phase === "installing") {
        actions.append(el("span", { class: "mk-ver-state" }, [icon("spinner", 12), " 安装中…"]));
      } else {
        if (state?.phase === "done") {
          actions.append(el("span", { class: "mk-ver-state mk-ver-state--done" }, ["已安装并启用"]));
        } else if (state?.phase === "failed") {
          actions.append(
            el("span", { class: "mk-ver-state mk-ver-state--error", title: state.detail }, ["失败（可重试）"]),
          );
        }
        const installBtn = el("button", { type: "button", class: "btn btn--ghost" }, [
          icon("download", 14),
          "安装",
        ]);
        installBtn.addEventListener("click", () => {
          installToCurrent(target.kind === "modpack" ? "modpack" : "plugin", gitCard, null, statusLine, render);
        });
        actions.append(installBtn);
      }
    } else if (target.installDirect) {
      const spec = `skill:${target.repo ?? ""}`;
      const state = installStates.get(spec);
      if (state?.phase === "installing") {
        actions.append(el("span", { class: "mk-ver-state" }, [icon("spinner", 12), " 安装中…"]));
      } else {
        if (state?.phase === "done") {
          actions.append(el("span", { class: "mk-ver-state mk-ver-state--done" }, ["已安装"]));
        } else if (state?.phase === "failed") {
          actions.append(
            el("span", { class: "mk-ver-state mk-ver-state--error", title: state.detail }, ["失败（可重试）"]),
          );
        }
        const installBtn = el("button", { type: "button", class: "btn btn--ghost" }, [
          icon("download", 14),
          "安装",
        ]);
        installBtn.addEventListener("click", () => target.installDirect?.(statusLine, render));
        actions.append(installBtn);
      }
    }
  };
  render();
  return actions;
}

function renderDetail(): HTMLElement {
  if (!current) {
    // 刷新直达（hash 无参数位）：已知限制，指路回下载页。
    const backBtn = el("button", { type: "button", class: "btn btn--line" }, ["回到下载页"]);
    backBtn.addEventListener("click", () => navigate("downloads"));
    return el("section", { class: "page" }, [
      el("div", { class: "mk-detail" }, [
        el("section", { class: "card settings-card" }, [
          el("h2", { class: "card__title" }, ["没有选中的资源"]),
          el("p", { class: "settings-path" }, ["回到「下载」页，点任意条目进入详情。"]),
          el("div", { class: "settings-actions" }, [backBtn]),
        ]),
      ]),
    ]);
  }
  const target = current;
  const statusLine = el("p", { class: "mk-status" }, []);
  const infoCard = el("section", { class: "card settings-card mk-detail__info" }, [
    el("div", { class: "mk-detail__head" }, [
      cardIcon(target.iconUrl ?? "", target.name),
      el("div", { class: "mk-detail__title" }, [
        el("div", { class: "mk-row__name" }, [
          el("span", { class: "ver-row__main" }, [target.name]),
          target.subName && target.subName !== target.name
            ? el("span", { class: "mk-row__sub" }, [` | ${target.subName}`])
            : document.createDocumentFragment(),
          ...target.tags.map((t) => el("span", { class: "mk-badge" }, [t])),
        ]),
        target.description
          ? el("div", { class: "mk-detail__desc", title: target.description }, [target.description])
          : document.createDocumentFragment(),
        target.meta,
      ]),
    ]),
    buildActions(target, statusLine),
    statusLine,
  ]);
  const children: HTMLElement[] = [infoCard];
  if (target.marketCard?.npmName && (target.kind === "plugin" || target.kind === "modpack")) {
    children.push(versionsCardOf(target.kind, target.marketCard, statusLine));
  } else if (target.marketCard?.repo && (target.kind === "plugin" || target.kind === "modpack")) {
    // GitHub 来源卡：历代版本 = Releases（无 Release 时兜底文案 + 顶部安装）。
    children.push(releasesCardOf(target.kind, target.marketCard, statusLine));
  }
  return el("section", { class: "page" }, [el("div", { class: "mk-detail" }, children)]);
}

/** 详情二级页（secondary：不进导航；标题随选中项变化）。 */
export const marketDetailPage: PageDefinition = {
  id: "market-detail",
  navLabel: "详情",
  secondary: true,
  titleOf: () => current?.displayName ?? "详情",
  render: renderDetail,
};
