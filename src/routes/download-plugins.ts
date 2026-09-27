/** 下载页「插件」分区（FR-03）：PCL 搜索 Mod 布局——右侧顶部「搜索」大卡片
 * （名称/来源/类型 + 搜索/重置条件），下方列表卡片右上角「热度 / 最新」tab
 * （默认热度=推荐流；搜索时熄灭、按相关度排序）；条目行 = 图标 + 名称｜副名 +
 * 标签徽章 + 摘要 + 元信息行；**点行进详情二级页**（market-detail：信息卡 +
 * 全部版本 + 逐版本直装当前实例，对齐 PCL「点行进详情」交互，返工 W）。
 * 安装走官方 `dsh plugin add` 链，按拍板时序语义提示「下次启动实例时生效」。 */
import { el } from "../lib/dom";
import { fetchMarketFeed, type MarketCard, type MarketFeed, type MarketSource } from "../services/market";
import { buildMetaLine, cardIcon } from "./market-shared";
import { openMarketDetail } from "./market-detail";

const SOURCE_OPTIONS: { value: MarketSource; label: string }[] = [
  { value: "all", label: "全部" },
  { value: "npm", label: "npm" },
  { value: "github", label: "GitHub" },
  { value: "dshhub", label: "dsh-hub" },
];

// 类型筛选的 value 必须与后端白名单同域：`all` + 七个中文分类（market.rs
// CATEGORIES）。此前把中文标签当 value（"全部"）传给后端 → 一旦用户动过类型
// 下拉，整页报「非法类型筛选」。导出供契约测试守住值域。
export const KIND_OPTIONS: { value: string; label: string }[] = [
  { value: "all", label: "全部" },
  { value: "界面", label: "界面" },
  { value: "搜索", label: "搜索" },
  { value: "机器人", label: "机器人" },
  { value: "记忆", label: "记忆" },
  { value: "视觉", label: "视觉" },
  { value: "开发", label: "开发" },
  { value: "效率", label: "效率" },
  { value: "其他", label: "其他" },
];

export function renderMarketSection(container: HTMLElement): HTMLElement {
  // ── 状态 ──
  // 排序三态：relevance（关键词搜索的相关度序，两个 tab 均熄灭——需求方
  // 拍板「接近到相似」）；hot/new 无关键词时走服务端排序，关键词结果上点击
  // tab 则对当前结果做客户端重排（不重新请求）。
  let sortMode: "relevance" | "hot" | "new" = "hot";
  let applied = { keyword: "", source: "all" as MarketSource, kind: "all" };
  let feed: MarketFeed | null = null;
  let loading = false;
  let loadError = "";

  // ── 搜索大卡片（PCL「搜索 Mod」布局）──
  const keywordEl = el("input", { type: "text", class: "settings-input", placeholder: "搜索插件名称或描述…" });
  const sourceSel = document.createElement("select");
  sourceSel.className = "settings-select";
  for (const opt of SOURCE_OPTIONS) {
    const o = document.createElement("option");
    o.value = opt.value;
    o.textContent = opt.label;
    sourceSel.append(o);
  }
  const kindSel = document.createElement("select");
  kindSel.className = "settings-select";
  for (const k of KIND_OPTIONS) {
    const o = document.createElement("option");
    o.value = k.value;
    o.textContent = k.label;
    kindSel.append(o);
  }
  const searchBtn = el("button", { type: "button", class: "btn btn--ghost" }, ["搜索"]);
  const resetBtn = el("button", { type: "button", class: "btn btn--line" }, ["重置条件"]);

  const searchCard = el("section", { class: "card settings-card" }, [
    el("h2", { class: "card__title" }, ["搜索 插件"]),
    el("div", { class: "market-search" }, [
      el("div", { class: "market-field" }, [el("span", {}, ["名称"]), keywordEl]),
      el("div", { class: "market-field" }, [el("span", {}, ["来源"]), sourceSel]),
      el("div", { class: "market-field" }, [el("span", {}, ["类型"]), kindSel]),
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
  /** 客户端重排（关键词结果上点 tab 用；与服务端 hot/new 同口径）。 */
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
  const offlineBanner = el("p", { class: "mk-offline", hidden: "" }, []);
  const statusLine = el("p", { class: "mk-status" }, []);
  const rowsBox = el("div", { class: "mk-rows" }, []);
  const listCard = el("section", { class: "card settings-card" }, [
    el("div", { class: "mk-head" }, [
      el("h2", { class: "card__title" }, ["插件列表"]),
      el("span", { class: "mk-count" }, [""]),
      el("div", { class: "mk-tabs" }, [tabHot, tabNew]),
    ]),
    offlineBanner,
    rowsBox,
    statusLine,
  ]);

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
      kind: "plugin",
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
      rowsBox.append(el("p", { class: "settings-status" }, ["正在聚合 npm 与 GitHub 数据…"]));
      return;
    }
    if (loadError) {
      if (head) head.textContent = "";
      rowsBox.append(
        el("p", { class: "settings-status settings-status--error" }, [
          `市场拉取失败：${loadError.slice(0, 300)}`,
        ]),
        el("p", { class: "settings-status" }, ["可到「设置 · 配置」检查镜像源或配置 GitHub Token 提额后重试。"]),
      );
      return;
    }
    if (!feed || feed.cards.length === 0) {
      if (head) head.textContent = "";
      // 空态可自证（走查定稿）：说清搜了什么、在线还是离线、各通道召回了多少，
      // 并给「重新搜索」按钮——杜绝「是不是纯装饰」式的无从定位。
      const empty = el("p", { class: "settings-status" }, []);
      if (feed?.offline) {
        empty.textContent = applied.keyword
          ? `离线快照中搜索「${applied.keyword}」无结果——联网后点「重新搜索」重试。`
          : "离线快照为空——联网后点「重新搜索」拉取插件目录。";
      } else if (feed) {
        empty.textContent = applied.keyword
          ? `在线搜索「${applied.keyword}」无结果（在线候选 npm ${feed.recall.npm} · GitHub ${feed.recall.github}，已按相关度过滤松散命中）。`
          : "市场目录为空——稍后点「重新搜索」再试。";
      } else {
        empty.textContent = "没有匹配的插件——换个关键词或重置条件试试。";
      }
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
      const result = await fetchMarketFeed({
        sort: sortMode === "new" ? "new" : "hot",
        keyword: applied.keyword,
        source: applied.source,
        kind: applied.kind,
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
    // 需求方拍板：有关键词 = 相关度序（热度/最新同时熄灭）；清空关键词回热度。
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
    kindSel.value = "all";
    applied = { keyword: "", source: "all", kind: "all" };
    sortMode = "hot";
    tabSync();
    void load();
  });
  sourceSel.addEventListener("change", () => {
    applied = { ...applied, source: sourceSel.value as MarketSource };
    if (sortMode === "relevance") sortMode = applied.keyword ? "relevance" : "hot";
    void load();
  });
  kindSel.addEventListener("change", () => {
    applied = { ...applied, kind: kindSel.value };
    if (sortMode === "relevance") sortMode = applied.keyword ? "relevance" : "hot";
    void load();
  });
  tabHot.addEventListener("click", () => {
    if (sortMode === "hot") return;
    sortMode = "hot";
    tabSync();
    if (feed && applied.keyword) {
      // 关键词结果上切 tab：客户端重排当前结果，不再请求。
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
