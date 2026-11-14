/** 下载页「Skill」分区（FR-14）：与插件市场同级的资源分类，共用 PCL 搜索
 * Mod 布局（搜索大卡 + 列表卡 + 热度/最新 tab）。Skill 与插件本质不同
 * （T-4 定稿）：是 `<name>/SKILL.md` 文件布局而非 npm 包——无版本维度，
 * 详情页操作行即「安装技能（整仓库）」；安装落点 `<实例>/skills/<name>/`，
 * DSH 自带目录 watcher，下次启动实例即被发现。返工 W：点行进详情二级页。 */
import { el } from "../lib/dom";
import { showHint } from "../lib/hint";
import { fetchSkillFeed, skillInstall, type SkillCard, type SkillFeed } from "../services/skills";
import { cardIcon, fmtCount, metaItem, openInstanceChooser, relTime } from "./market-shared";
import { openMarketDetail } from "./market-detail";

/** 排序：hot = 星标降序；new = 更新时间降序（空殿后）。 */
type SkillSort = "hot" | "new";

function sortCards(cards: SkillCard[], sort: SkillSort): SkillCard[] {
  const sorted = [...cards];
  if (sort === "hot") {
    sorted.sort((a, b) => b.stars - a.stars || a.name.localeCompare(b.name));
  } else {
    sorted.sort((a, b) => {
      const au = (!a.updatedAt ? "" : a.updatedAt) as string;
      const bu = (!b.updatedAt ? "" : b.updatedAt) as string;
      return bu.localeCompare(au) || a.name.localeCompare(b.name);
    });
  }
  return sorted;
}

export function renderSkillSection(container: HTMLElement): HTMLElement {
  // ── 状态 ──
  let sort: SkillSort = "hot";
  let appliedKeyword = "";
  let feed: SkillFeed | null = null;
  let loading = false;
  let loadError = "";

  // ── 搜索大卡片（与插件分区同布局；Skill 来源当前仅 GitHub 聚合）──
  const keywordEl = el("input", { type: "text", class: "settings-input", placeholder: "搜索技能名称或描述…" });
  const sourceSel = document.createElement("select");
  sourceSel.className = "settings-select";
  for (const [value, label] of [
    ["all", "全部"],
    ["github", "GitHub"],
  ] as const) {
    const o = document.createElement("option");
    o.value = value;
    o.textContent = label;
    sourceSel.append(o);
  }
  sourceSel.value = "all";
  const searchBtn = el("button", { type: "button", class: "btn btn--ghost" }, ["搜索"]);
  const resetBtn = el("button", { type: "button", class: "btn btn--line" }, ["重置条件"]);

  const searchCard = el("section", { class: "card settings-card" }, [
    el("h2", { class: "card__title" }, ["搜索 Skill"]),
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
    tabHot.classList.toggle("tab-btn--active", sort === "hot");
    tabNew.classList.toggle("tab-btn--active", sort === "new");
  };
  const offlineBanner = el("p", { class: "mk-offline", hidden: "" }, []);
  const statusLine = el("p", { class: "mk-status" }, []);
  const rowsBox = el("div", { class: "mk-rows" }, []);
  const listCard = el("section", { class: "card settings-card" }, [
    el("div", { class: "mk-head" }, [
      el("h2", { class: "card__title" }, ["Skill 列表"]),
      el("span", { class: "mk-count" }, [""]),
      el("div", { class: "mk-tabs" }, [tabHot, tabNew]),
    ]),
    offlineBanner,
    rowsBox,
    statusLine,
  ]);

  /** 元信息行（R1：字符图标换 metaItem 小图标结构）：星标 · 更新 · GitHub。 */
  function metaLine(card: SkillCard): HTMLElement {
    return el("div", { class: "mk-meta" }, [
      metaItem("star", fmtCount(card.stars)),
      metaItem("arrow-up", relTime(card.updatedAt)),
      metaItem("globe", "GitHub"),
    ]);
  }

  function cardRow(card: SkillCard): HTMLElement {
    const row = el("div", { class: "mk-row" }, [
      cardIcon(card.iconUrl, card.name),
      el("div", { class: "mk-row__body" }, [
        el("div", { class: "mk-row__name" }, [
          el("span", { class: "ver-row__main" }, [card.name]),
          el("span", { class: "mk-row__sub" }, [` | ${card.subName}`]),
        ]),
        card.description ? el("div", { class: "mk-row__desc" }, [card.description]) : document.createDocumentFragment(),
        metaLine(card),
      ]),
    ]);
    row.addEventListener("click", () => openMarketDetail({
      kind: "skill",
      name: card.name,
      subName: card.subName,
      iconUrl: card.iconUrl,
      tags: [],
      description: card.description,
      meta: metaLine(card),
      displayName: `${card.name} (${card.subName})`,
      repo: card.repo,
      installDirect: (statusLine) => {
        openInstanceChooser({
          statusLine,
          describe: (inst) => ({ disabled: false, note: inst.dshVersion ? `DSH ${inst.dshVersion} · :${inst.port ?? "-"}` : "未安装 DSH 本体（技能仍可先装）" }),
          run: (inst) =>
            skillInstall(inst.id, card.repo).then((outcome) => {
              // 成功转左下轻提示（灰字清理 2026-10-31）：装到哪、装了什么是结果
              // 数据，但不再以多行灰字常驻状态行。
              const message = outcome.overwritten
                ? `已覆盖安装技能 ${outcome.installed.join("、")} 到「${inst.name}」`
                : `已安装 ${outcome.installed.length} 个技能到「${inst.name}」`;
              showHint(message);
              return "";
            }),
        });
      },
    }));
    return row;
  }

  /** 客户端关键词过滤 + 排序（目录一次拉全量，筛选零额外配额）。 */
  function visible(): SkillCard[] {
    const cards = feed?.cards ?? [];
    const filtered = appliedKeyword
      ? cards.filter((c) => {
          const needle = appliedKeyword.toLowerCase();
          return (
            c.name.toLowerCase().includes(needle) ||
            c.subName.toLowerCase().includes(needle) ||
            c.description.toLowerCase().includes(needle)
          );
        })
      : cards;
    return sortCards(filtered, sort);
  }

  function paintRows(): void {
    rowsBox.replaceChildren();
    const head = listCard.querySelector(".mk-count");
    if (loading) {
      if (head) head.textContent = "";
      rowsBox.append(el("p", { class: "settings-status" }, [""]));
      return;
    }
    if (loadError) {
      if (head) head.textContent = "";
      rowsBox.append(
        el("p", { class: "settings-status settings-status--error" }, [`技能目录拉取失败：${loadError.slice(0, 300)}`]),
        el("p", { class: "settings-status" }, ["可稍后重试，或到「设置 · 配置」检查镜像源。"]),
      );
      return;
    }
    const cards = visible();
    if (cards.length === 0) {
      if (head) head.textContent = "";
      rowsBox.append(el("p", { class: "settings-status" }, [feed ? "没有匹配的技能——换个关键词试试。" : "暂无数据。"]));
      return;
    }
    if (head) head.textContent = `（${cards.length}）`;
    for (const card of cards) rowsBox.append(cardRow(card));
  }

  async function load(): Promise<void> {
    loading = true;
    loadError = "";
    paintRows();
    try {
      const result = await fetchSkillFeed();
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
    appliedKeyword = keywordEl.value.trim();
    paintRows();
  });
  keywordEl.addEventListener("keydown", (event) => {
    if ((event as KeyboardEvent).key === "Enter") searchBtn.click();
  });
  resetBtn.addEventListener("click", () => {
    keywordEl.value = "";
    appliedKeyword = "";
    paintRows();
  });
  tabHot.addEventListener("click", () => {
    if (sort === "hot") return;
    sort = "hot";
    tabSync();
    paintRows();
  });
  tabNew.addEventListener("click", () => {
    if (sort === "new") return;
    sort = "new";
    tabSync();
    paintRows();
  });

  container.append(searchCard, listCard);
  void load();
  return statusLine;
}
