/** 下载页「插件」分区（FR-03）：PCL 搜索 Mod 布局——右侧顶部「搜索」大卡片
 * （名称/来源/类型 + 搜索/重置条件），下方列表卡片右上角「热度 / 最新」tab
 * （默认热度=推荐流；搜索时熄灭、按相关度排序）；条目行 = 图标 + 名称｜副名 +
 * 标签徽章 + 摘要 + 元信息行；折叠展开历史版本小卡（头像+版本 / 周下载·更新·
 * 来源小字），悬浮右缘浮现下载图标——点击直接装进**当前实例**并启用（不弹
 * 实例选择框，需求方走查定稿），行内显示安装中/已启用状态。
 * 安装走官方 `dsh plugin add` 链，按拍板时序语义提示「下次启动实例时生效」。 */
import { el } from "../lib/dom";
import { getSelectedInstance } from "../lib/selected-instance";
import { sortInstancesByRecency } from "../lib/instanceOrder";
import {
  fetchMarketFeed,
  fetchMarketVersions,
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

const SOURCE_LABELS: Record<MarketCard["source"], string> = {
  npm: "npm",
  github: "GitHub",
  dshhub: "dsh-hub",
};

  /** 作者名：repo owner 段或 scoped 包名的 @scope 段；取不到为空串。 */
  function authorOf(card: MarketCard): string {
    if (card.repo) return card.repo.split("/")[0] ?? "";
    const npm = card.npmName ?? "";
    return npm.startsWith("@") ? npm.slice(1).split("/")[0] ?? "" : "";
  }

  /** 元信息行：作者 · 版本 · 下载 · 更新 · 来源（+星标/验证标记）。 */
  function metaLine(card: MarketCard): HTMLElement {
    const parts: string[] = [];
    const author = authorOf(card);
    if (author) parts.push(`✎ ${author}`);
    if (card.latestVersion) parts.push(`v${card.latestVersion}`);
    if (card.monthlyDownloads !== null) parts.push(`⬇ ${fmtCount(card.monthlyDownloads)}/月`);
    else if (card.stars !== null) parts.push(`★ ${fmtCount(card.stars)}`);
    else parts.push("⬇ 未知");
    parts.push(`↑ ${relTime(card.updatedAt)}`);
    parts.push(`◎ ${SOURCE_LABELS[card.source]}`);
    const line = el("div", { class: "mk-meta" }, [parts.join("  ·  ")]);
    if (card.verified === true) line.append(el("span", { class: "mk-verified" }, ["✔ 已验证"]));
    return line;
  }

/** 下载图标（需求方定稿：向下箭头+底部横线），跟随文字色、悬浮态跟随按钮。 */
function downloadIcon(): HTMLElement {
  const span = el("span", { class: "mk-dl-icon", "aria-hidden": "true" });
  span.innerHTML =
    '<svg viewBox="0 0 24 24" width="17" height="17" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M12 3v12"/><path d="m7 11 5 5 5-5"/><path d="M5 21h14"/></svg>';
  return span;
}

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
  const expanded = new Set<string>(); // 已展开的条目键（重渲染后恢复）
  const versionCache = new Map<string, VersionsReply>();

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

  // ── 安装（需求方走查定稿）：点下载图标直接装进**当前实例**（不弹选择框），
  // 行内显示 安装中/已启用/失败 状态；「启用」= 装入 profile + bundles 层
  // （DSH 原生无启停开关，T-2：下次启动实例即生效）。 ──
  const installStates = new Map<string, { phase: "installing" | "done" | "failed"; detail: string }>();

  /** 当前实例解析：显式选择 > 最近使用（launch 页同口径）> 唯一实例。
   *  内存单例重启即空——不能因此拒绝安装（走查实证的误报场景）。 */
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
      statusLine.textContent = `正在安装 ${spec} 到「${inst.name}」…`;
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

  /** 版本行小卡（走查定稿）：上=头像+版本号（悬浮右缘浮现下载图标），
   *  下=周下载 · 更新时间 · 来源；行内状态：安装中/已安装并启用/失败。
   *  declaresBundle=false（依赖库冒充插件）→ 无下载图标（源头拦截）。 */
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

  /** 展开区的版本列表（npm 卡 = 历史版本小卡；git 卡 = 直装说明）。 */
  function buildExpand(card: MarketCard): HTMLElement {
    const box = el("div", { class: "mk-expand" }, []);
    if (card.npmName) {
      const render = (reply: VersionsReply): void => paintVersions(box, card, reply);
      const cached = versionCache.get(card.npmName);
      if (cached) {
        render(cached);
      } else {
        box.append(el("p", { class: "settings-status" }, ["版本列表加载中…"]));
        void fetchMarketVersions(card.npmName)
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
      // git 直装形态：未发布 npm，按 GitHub 仓库安装（pnpm git 语义）。
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
            if (box.isConnected) buildExpandInto(box, card);
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

  /** 原位重建展开区（安装状态变化后保持展开与滚动位置大致不变）。 */
  function buildExpandInto(box: HTMLElement, card: MarketCard): void {
    const fresh = buildExpand(card);
    box.replaceWith(fresh);
  }

  function paintVersions(box: HTMLElement, card: MarketCard, reply: VersionsReply): void {
    const versions = reply.versions;
    const list = el("div", { class: "mk-expand__list" }, []);
    for (const v of versions) {
      list.append(versionRow(card, v, reply.declaresBundle, () => {
        if (box.isConnected) paintVersions(box, card, reply);
      }));
    }
    const head = el("p", { class: "settings-status" }, [`历史版本（${versions.length}）——按周下载挑甜点：`]);
    if (!reply.declaresBundle) {
      // 源头拦截（走查定稿）：任一版本都没声明 dsh.bundle.patch = 依赖库
      // 冒充插件（keywords 乱打的典型），安装入口整体禁用。
      head.append(
        el("span", { class: "mk-ver-state mk-ver-state--error" }, [
          "　⚠ 该包未声明 dsh.bundle.patch——是依赖库而非可加载的插件，已禁用安装。",
        ]),
      );
    }
    box.replaceChildren(head, list);
  }

  /** 一张可折叠的插件条目行。 */
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
        metaLine(card),
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
    // 重渲染后恢复已展开状态。
    if (expanded.has(key)) {
      expand = buildExpand(card);
      row.append(expand);
      row.classList.add("mk-row--open");
    }
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
