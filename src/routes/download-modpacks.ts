/** 下载页「整合包」分区（P3 浏览层，需求方定稿）：聚合型资源目录——
 * 一键装齐一组插件的「全家桶/聚合包/套件」，布局与插件市场同构（搜索大卡
 * + 列表卡 + 热度/最新 tab）；点行进详情二级页（返工 W，PCL 交互）。
 * 注意：整合包的**文件级**导入（拖入启动页）与导出（版本设置＞导出）属
 * P4（FR-04），本栏只做聚合包的浏览/搜索/安装。 */
import { el } from "../lib/dom";
import { fetchModpackFeed, type MarketCard, type MarketFeed, type MarketSource } from "../services/market";
import { buildMetaLine, cardIcon } from "./market-shared";
import { openMarketDetail } from "./market-detail";

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
  /** 卡片行：点行进详情二级页（PCL 交互）。 */
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
        buildMetaLine(card),
      ]),
    ]);
    row.addEventListener("click", () => openMarketDetail({
      kind: "modpack",
      name: card.name,
      subName: card.subName,
      iconUrl: card.iconUrl,
      tags: card.tags,
      description: card.description,
      meta: buildMetaLine(card),
      displayName: card.subName && card.subName !== card.name ? `${card.name} (${card.subName})` : card.name,
      repo: card.repo,
      marketCard: card,
    }));
    return row;
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

