/** 下载页「整合包」分区（P3 浏览层，需求方定稿）：聚合型资源目录——
 * 一键装齐一组插件的「全家桶/聚合包/套件」，布局与插件市场同构（搜索大卡
 * + 列表卡 + 热度/最新 tab + 版本行小卡直装当前实例）。
 * 注意：整合包的**文件级**导入（拖入启动页）与导出（版本设置＞导出）属
 * P4（FR-04），本栏只做聚合包的浏览/搜索/安装。 */
import { el } from "../lib/dom";
import { getSelectedInstance } from "../lib/selected-instance";
import { sortInstancesByRecency } from "../lib/instanceOrder";
import {
  fetchModpackFeed,
  type MarketCard,
  type MarketFeed,
  type MarketSource,
  type MarketVersion,
  type VersionsReply,
} from "../services/market";
import { pluginInstall } from "../services/plugins";
import { listInstances, type Instance } from "../services/instances";
import { cardIcon, fmtCount, relTime } from "./market-shared";

const SOURCE_OPTIONS: { value: MarketSource; label: string }[] = [
  { value: "all", label: "全部" },
  { value: "npm", label: "npm" },
  { value: "github", label: "GitHub" },
];

export function renderModpackSection(container: HTMLElement): HTMLElement {
  // ── 状态（与插件分区同构）──
  let sortMode: "relevance" | "hot" | "new" = "hot";
  let applied = { keyword: "", source: "all" as MarketSource };
  let feed: MarketFeed | null = null;
  let loading = false;
  let loadError = "";
  const expanded = new Set<string>();
  const versionCache = new Map<string, VersionsReply>();
  const installStates = new Map<string, { phase: "installing" | "done" | "failed"; detail: string }>();

  // ── 搜索大卡片 ──
  const keywordEl = el("input", { type: "text", class: "settings-input", placeholder: "搜索整合包名称或描述…" });
  const sourceSel = document.createElement("select");
  sourceSel.className = "settings-select";
  for (const opt of SOURCE_OPTIONS) {
    const o = document.createElement("option");
    o.value = opt.value;
    o.textContent = opt.label;
    sourceSel.append(o);
  }
  const searchBtn = el("button", { type: "button", class: "btn btn--ghost" }, ["搜索"]);
  const resetBtn = el("button", { type: "button", class: "btn btn--line" }, ["重置条件"]);

  const searchCard = el("section", { class: "card settings-card" }, [
    el("h2", { class: "card__title" }, ["搜索 整合包"]),
    el("div", { class: "market-search" }, [
      el("div", { class: "market-field" }, [el("span", {}, ["名称"]), keywordEl]),
      el("div", { class: "market-field" }, [el("span", {}, ["来源"]), sourceSel]),
      el("div", { class: "market-field" }, [
        el("span", {}, ["说明"]),
        el("span", { class: "settings-path" }, ["聚合包 = 一键装齐一组插件；文件级导入/导出在 P4"]),
      ]),
      el("div", { class: "market-actions" }, [searchBtn, resetBtn]),
    ]),
  ]);

  // ── 列表卡片 ──
  const tabHot = el("button", { type: "button", class: "tab-btn tab-btn--active" }, ["热度"]);
  const tabNew = el("button", { type: "button", class: "tab-btn" }, ["最新"]);
  const tabSync = (): void => {
    tabHot.classList.toggle("tab-btn--active", sortMode === "hot");
    tabNew.classList.toggle("tab-btn--active", sortMode === "new");
  };
  const offlineBanner = el("p", { class: "mk-offline", hidden: "" }, []);
  const statusLine = el("p", { class: "mk-status" }, []);
  const rowsBox = el("div", { class: "mk-rows" }, []);
  const listCard = el("section", { class: "card settings-card" }, [
    el("div", { class: "mk-head" }, [
      el("h2", { class: "card__title" }, ["整合包列表"]),
      el("span", { class: "mk-count" }, [""]),
      el("div", { class: "mk-tabs" }, [tabHot, tabNew]),
    ]),
    offlineBanner,
    rowsBox,
    statusLine,
  ]);

  function sortClient(cards: MarketCard[], mode: "hot" | "new"): MarketCard[] {
    const list = [...cards];
    if (mode === "hot") {
      list.sort(
        (a, b) =>
          (b.monthlyDownloads ?? 0) - (a.monthlyDownloads ?? 0) ||
          (b.stars ?? 0) - (a.stars ?? 0) ||
          a.name.localeCompare(b.name),
      );
    } else {
      list.sort(
        (a, b) =>
          (b.updatedAt || "").localeCompare(a.updatedAt || "") ||
          (b.monthlyDownloads ?? 0) - (a.monthlyDownloads ?? 0) ||
          a.name.localeCompare(b.name),
      );
    }
    return list;
  }

  // ── 安装（直装当前实例，解析链同插件分区）──
  async function resolveCurrentInstance(): Promise<Instance | null> {
    const instances = await listInstances();
    if (instances.length === 0) return null;
    const selectedId = getSelectedInstance();
    if (selectedId) {
      const picked = instances.find((i) => i.id === selectedId);
      if (picked) return picked;
    }
    return sortInstancesByRecency(instances)[0] ?? null;
  }

  function installToCurrent(card: MarketCard, version: string | null, onSettled: () => void): void {
    const spec = version ? `${card.npmName}@${version}` : `github:${card.repo}`;
    statusLine.classList.remove("mk-status--error");
    statusLine.textContent = "正在确定目标实例…";
    void (async () => {
      const inst = await resolveCurrentInstance();
      if (!inst) {
        statusLine.classList.add("mk-status--error");
        statusLine.textContent = "还没有任何实例——先到「下载 · dsh 版本」创建一个，再回来安装。";
        return;
      }
      installStates.set(spec, { phase: "installing", detail: "" });
      onSettled();
      // 打标发起的 spec：downloads 页全局进度行按它各归各（审查 R1）。
      statusLine.dataset.spec = spec;
      statusLine.textContent = `正在安装 ${spec} 到「${inst.name}」…（聚合包会一并装齐它声明的全部插件）`;
      try {
        const outcome = await pluginInstall(inst.id, spec);
        installStates.set(spec, { phase: "done", detail: outcome.version });
        statusLine.textContent =
          `✓ ${outcome.packageName} ${outcome.version} 已安装到「${inst.name}」并启用` +
          `——下次启动该实例时生效（首次启动可能稍慢）`;
      } catch (err) {
        installStates.set(spec, { phase: "failed", detail: String(err).slice(0, 160) });
        statusLine.classList.add("mk-status--error");
        statusLine.textContent = `安装失败：${String(err).slice(0, 260)}`;
      }
      onSettled();
    })();
  }

  function versionRow(
    card: MarketCard,
    v: MarketVersion,
    declaresBundle: boolean,
    onSettled: () => void,
  ): HTMLElement {
    const spec = `${card.npmName}@${v.version}`;
    const row = el("div", { class: "mk-ver-card" }, []);
    const state = installStates.get(spec);
    const acts = el("div", { class: "mk-ver-card__acts" }, []);
    if (!declaresBundle) {
      acts.append(el("span", { class: "mk-ver-state" }, ["依赖库·禁装"]));
    } else if (state?.phase === "installing") {
      acts.append(el("span", { class: "mk-ver-state" }, ["⏳ 安装中…"]));
    } else {
      if (state?.phase === "done") {
        acts.append(el("span", { class: "mk-ver-state mk-ver-state--done" }, ["✓ 已安装并启用"]));
      } else if (state?.phase === "failed") {
        acts.append(
          el("span", { class: "mk-ver-state mk-ver-state--error", title: state.detail }, ["✗ 失败（悬浮点 ⬇ 重试）"]),
        );
      }
      const dl = el("button", { type: "button", class: "ver-act mk-ver-dl", title: "下载并安装到当前实例" }, []);
      dl.append(downloadIcon());
      dl.addEventListener("click", (event) => {
        event.stopPropagation();
        installToCurrent(card, v.version, onSettled);
      });
      acts.append(dl);
    }
    row.append(
      el("div", { class: "mk-ver-card__main" }, [
        cardIcon(card.iconUrl, card.name),
        el("span", { class: "ver-row__main" }, [`v${v.version}`]),
        v.latest ? el("span", { class: "mk-badge mk-badge--latest" }, ["最新"]) : document.createDocumentFragment(),
        acts,
      ]),
      el("div", { class: "mk-ver-card__sub" }, [
        `${v.weeklyDownloads !== null ? `周下载 ${fmtCount(v.weeklyDownloads)}` : "周下载 --"} · ${relTime(v.publishedAt)} · npm`,
      ]),
    );
    return row;
  }

  /** 展开区的版本列表（npm 卡；git 卡 = 直装说明）。 */
  function buildExpand(card: MarketCard): HTMLElement {
    const box = el("div", { class: "mk-expand" }, []);
    if (card.npmName) {
      const render = (reply: VersionsReply): void => paintVersions(box, card, reply);
      const cached = versionCache.get(card.npmName);
      if (cached) {
        render(cached);
      } else {
        box.append(el("p", { class: "settings-status" }, ["版本列表加载中…"]));
        void fetchModpackVersions(card.npmName)
          .then((reply) => {
            versionCache.set(card.npmName!, reply);
            if (box.isConnected) render(reply);
          })
          .catch((err) => {
            if (box.isConnected) {
              box.replaceChildren(
                el("p", { class: "settings-status settings-status--error" }, [
                  `版本列表拉取失败：${String(err).slice(0, 240)}`,
                ]),
              );
            }
          });
      }
    } else {
      const spec = `github:${card.repo ?? ""}`;
      const state = installStates.get(spec);
      const acts = el("div", { class: "mk-ver-card__acts" }, []);
      if (state?.phase === "installing") {
        acts.append(el("span", { class: "mk-ver-state" }, ["⏳ 安装中…"]));
      } else {
        if (state?.phase === "done") {
          acts.append(el("span", { class: "mk-ver-state mk-ver-state--done" }, ["✓ 已安装并启用"]));
        } else if (state?.phase === "failed") {
          acts.append(el("span", { class: "mk-ver-state mk-ver-state--error", title: state.detail }, ["✗ 失败"]));
        }
        const dl = el("button", { type: "button", class: "ver-act mk-ver-dl", title: "下载并安装到当前实例" }, []);
        dl.append(downloadIcon());
        dl.addEventListener("click", (event) => {
          event.stopPropagation();
          installToCurrent(card, null, () => {
            if (box.isConnected) {
              const fresh = buildExpand(card);
              box.replaceWith(fresh);
            }
          });
        });
        acts.append(dl);
      }
      box.append(
        el("p", { class: "settings-status" }, [
          "未发布 npm——git 直装（经仓库构建，首次安装较慢；如遇 pnpm 构建拦截按输出提示操作）。",
        ]),
        acts,
      );
    }
    return box;
  }

  function paintVersions(box: HTMLElement, card: MarketCard, reply: VersionsReply): void {
    const versions = reply.versions;
    const list = el("div", { class: "mk-expand__list" }, []);
    for (const v of versions) {
      list.append(versionRow(card, v, reply.declaresBundle, () => {
        if (box.isConnected) paintVersions(box, card, reply);
      }));
    }
    box.replaceChildren(
      el("p", { class: "settings-status" }, [`历史版本（${versions.length}）——按周下载挑甜点：`]),
      list,
    );
  }

  /** 卡片行（同插件分区）。 */
  function cardRow(card: MarketCard): HTMLElement {
    const row = el("div", { class: "mk-row" }, [
      cardIcon(card.iconUrl, card.name),
      el("div", { class: "mk-row__body" }, [
        el("div", { class: "mk-row__name" }, [
          el("span", { class: "ver-row__main" }, [card.name]),
          card.subName && card.subName !== card.name
            ? el("span", { class: "mk-row__sub" }, [` | ${card.subName}`])
            : document.createDocumentFragment(),
          ...card.tags.map((t) => el("span", { class: "mk-badge" }, [t])),
        ]),
        card.description ? el("div", { class: "mk-row__desc" }, [card.description]) : document.createDocumentFragment(),
        el("div", { class: "mk-meta" }, [metaText(card)]),
      ]),
    ]);
    let expand: HTMLElement | null = null;
    const key = card.npmName ?? card.repo ?? card.name;
    row.addEventListener("click", () => {
      if (expand) {
        expand.remove();
        expand = null;
        row.classList.remove("mk-row--open");
        expanded.delete(key);
        return;
      }
      expand = buildExpand(card);
      expanded.add(key);
      row.classList.add("mk-row--open");
      row.append(expand);
    });
    if (expanded.has(key)) {
      expand = buildExpand(card);
      row.append(expand);
      row.classList.add("mk-row--open");
    }
    return row;
  }

  function metaText(card: MarketCard): string {
    const parts: string[] = [];
    const author = card.repo
      ? card.repo.split("/")[0] ?? ""
      : (card.npmName ?? "").startsWith("@")
        ? (card.npmName ?? "").slice(1).split("/")[0] ?? ""
        : "";
    if (author) parts.push(`✎ ${author}`);
    if (card.latestVersion) parts.push(`v${card.latestVersion}`);
    if (card.monthlyDownloads !== null) parts.push(`⬇ ${fmtCount(card.monthlyDownloads)}/月`);
    else if (card.stars !== null) parts.push(`★ ${fmtCount(card.stars)}`);
    else parts.push("⬇ 未知");
    parts.push(`↑ ${relTime(card.updatedAt)}`);
    parts.push(`◎ ${card.source === "npm" ? "npm" : "GitHub"}`);
    return parts.join("  ·  ");
  }

  function paintRows(): void {
    rowsBox.replaceChildren();
    const head = listCard.querySelector(".mk-count");
    if (loading) {
      if (head) head.textContent = "";
      rowsBox.append(el("p", { class: "settings-status" }, ["正在聚合整合包目录…"]));
      return;
    }
    if (loadError) {
      if (head) head.textContent = "";
      rowsBox.append(
        el("p", { class: "settings-status settings-status--error" }, [`整合包目录拉取失败：${loadError.slice(0, 300)}`]),
      );
      return;
    }
    if (!feed || feed.cards.length === 0) {
      if (head) head.textContent = "";
      const empty = el("p", { class: "settings-status" }, []);
      empty.textContent = feed?.offline
        ? applied.keyword
          ? `离线快照中搜索「${applied.keyword}」无结果——联网后点「重新搜索」重试。`
          : "离线快照为空——联网后点「重新搜索」拉取整合包目录。"
        : applied.keyword
          ? `搜索「${applied.keyword}」无聚合型结果——换个关键词试试。`
          : "暂无聚合型整合包收录。";
      const retry = el("button", { type: "button", class: "btn btn--ghost settings-inline-btn" }, ["重新搜索"]);
      retry.addEventListener("click", () => void load());
      rowsBox.append(empty, retry);
      return;
    }
    if (head) head.textContent = `（${feed.cards.length}）`;
    for (const card of feed.cards) rowsBox.append(cardRow(card));
  }

  async function load(): Promise<void> {
    loading = true;
    loadError = "";
    paintRows();
    try {
      const result = await fetchModpackFeed({
        sort: sortMode === "new" ? "new" : "hot",
        keyword: applied.keyword,
        source: applied.source,
      });
      feed = result;
      offlineBanner.hidden = !result.offline;
      if (result.offline) {
        offlineBanner.textContent = `离线快照 · 数据截至 ${result.fetchedAt.slice(0, 16).replace("T", " ")}（当前仅可浏览，联网后自动恢复）`;
      }
    } catch (err) {
      feed = null;
      loadError = String(err);
      offlineBanner.hidden = true;
    } finally {
      loading = false;
      paintRows();
    }
  }

  searchBtn.addEventListener("click", () => {
    applied = { ...applied, keyword: keywordEl.value.trim() };
    sortMode = applied.keyword ? "relevance" : "hot";
    tabSync();
    void load();
  });
  keywordEl.addEventListener("keydown", (event) => {
    if ((event as KeyboardEvent).key === "Enter") searchBtn.click();
  });
  resetBtn.addEventListener("click", () => {
    keywordEl.value = "";
    sourceSel.value = "all";
    applied = { keyword: "", source: "all" };
    sortMode = "hot";
    tabSync();
    void load();
  });
  sourceSel.addEventListener("change", () => {
    applied = { ...applied, source: sourceSel.value as MarketSource };
    void load();
  });
  tabHot.addEventListener("click", () => {
    if (sortMode === "hot") return;
    sortMode = "hot";
    tabSync();
    if (feed && applied.keyword) {
      feed = { ...feed, cards: sortClient(feed.cards, "hot") };
      paintRows();
    } else {
      void load();
    }
  });
  tabNew.addEventListener("click", () => {
    if (sortMode === "new") return;
    sortMode = "new";
    tabSync();
    if (feed && applied.keyword) {
      feed = { ...feed, cards: sortClient(feed.cards, "new") };
      paintRows();
    } else {
      void load();
    }
  });

  container.append(searchCard, listCard);
  void load();
  return statusLine;
}

/** 版本枚举经同一命令（聚合包与插件共用 packument 通道）。 */
function fetchModpackVersions(packageName: string): Promise<VersionsReply> {
  return import("../services/market").then((m) => m.fetchMarketVersions(packageName));
}

/** 下载图标（与插件分区同款 SVG）。 */
function downloadIcon(): HTMLElement {
  const span = el("span", { class: "mk-dl-icon", "aria-hidden": "true" });
  span.innerHTML =
    '<svg viewBox="0 0 24 24" width="17" height="17" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M12 3v12"/><path d="m7 11 5 5 5-5"/><path d="M5 21h14"/></svg>';
  return span;
}
