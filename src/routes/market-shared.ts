/** 插件市场 / Skill 中心共享件：口语化格式化 + 条目图标 + 实例选择弹窗。
 * 弹窗按「describe 每实例可装性 → run 执行 → 状态行跟进」的契约复用。 */
import { el } from "../lib/dom";
import { enterMsg } from "../lib/anim";
import { icon, type IconName } from "../lib/icons";
import { listInstances, type Instance } from "../services/instances";
import type { MarketCard } from "../services/market";

/** 下载量口语化：3.1 万 / 2.18 亿 / 876。 */
export function fmtCount(n: number): string {
  if (n >= 1e8) return `${(n / 1e8).toFixed(2)} 亿`;
  if (n >= 1e4) return `${(n / 1e4).toFixed(1)} 万`;
  return String(n);
}

/** RFC3339 → 相对时间（「3 天前」；超 30 天显示日期）。 */
export function relTime(rfc3339: string): string {
  if (!rfc3339) return "未知";
  const then = Date.parse(rfc3339);
  if (Number.isNaN(then)) return rfc3339.slice(0, 10);
  const days = Math.floor((Date.now() - then) / 86400000);
  if (days <= 0) return "今天";
  if (days < 30) return `${days} 天前`;
  return rfc3339.slice(0, 10);
}

/** 首字母色块（无头像时的条目图标）。 */
export function letterTile(name: string): HTMLElement {
  const letter = (name.replace(/^[@/]/, "").charAt(0) || "?").toUpperCase();
  return el("span", { class: "mk-icon mk-icon--letter" }, [letter]);
}

/** 条目方形图标：有头像用头像（加载失败回落色块）。 */
export function cardIcon(iconUrl: string, name: string): HTMLElement {
  if (iconUrl) {
    const img = el("img", { class: "mk-icon", src: iconUrl, alt: "", draggable: "false" });
    img.addEventListener("error", () => img.replaceWith(letterTile(name)));
    return img;
  }
  return letterTile(name);
}

/**
 * 元信息行小项（spec R1）：`span.mk-meta__item > 小图标 + 文本`。
 * name 传 "none" 表示纯文本项（不带图标）；图标颜色随 currentColor。
 */
export function metaItem(name: IconName | "none", text: string): HTMLElement {
  const item = el("span", { class: "mk-meta__item" }, []);
  if (name !== "none") item.append(icon(name, 12));
  item.append(el("span", {}, [text]));
  return item;
}

/** 实例选择弹窗的契约。 */
export interface ChooserOptions {
  /** 安装进度/结果写这里（分区状态行）。 */
  statusLine: HTMLElement;
  /** 每实例的可装性描述：disabled 置灰 + note 说明。 */
  describe: (inst: Instance) => { disabled: boolean; note: string };
  /** 用户选定实例后的执行体；resolve = 成功文案，reject = 报错文案。 */
  run: (inst: Instance) => Promise<string>;
}

/** 打开「安装到实例」弹窗：列出全部实例，选定后执行 run 并把过程写进状态行。 */
export function openInstanceChooser(opts: ChooserOptions): void {
  const overlay = el("div", { class: "modal-overlay" }, []);
  const body = el("div", { class: "error-modal__body" }, [
    el("p", { class: "settings-status" }, ["实例列表加载中…"]),
  ]);
  const close = (): void => overlay.remove();
  overlay.addEventListener("click", (event) => {
    if (event.target === overlay) close();
  });
  const card = el("div", { class: "error-modal" }, [
    el("h3", { class: "error-modal__title" }, ["安装到实例"]),
    body,
    el("div", { class: "error-modal__btns" }, [
      (() => {
        const btn = el("button", { type: "button", class: "btn btn--line" }, ["取消"]);
        btn.addEventListener("click", close);
        return btn;
      })(),
    ]),
  ]);
  overlay.append(card);
  document.body.append(overlay);
  // R5：弹窗入场动画（Y40→0 淡入，MyMsg 规格），与其他弹窗一致不再瞬现。
  enterMsg(card);

  void (async () => {
    try {
      const instances = await listInstances();
      body.replaceChildren();
      if (instances.length === 0) {
        body.append(
          el("p", { class: "settings-status" }, ["还没有实例——先到「下载 · dsh 版本」创建一个。"]),
        );
        return;
      }
      for (const inst of instances) {
        const { disabled, note } = opts.describe(inst);
        const btn = el("button", { type: "button", class: "btn btn--ghost ver-act" }, ["安装"]);
        if (disabled) {
          btn.disabled = true;
          btn.title = note;
        }
        btn.addEventListener("click", () => {
          close();
          opts.statusLine.classList.remove("mk-status--error");
          opts.statusLine.textContent = `正在安装到「${inst.name}」…`;
          opts
            .run(inst)
            .then((message) => {
              opts.statusLine.textContent = message;
            })
            .catch((err) => {
              opts.statusLine.classList.add("mk-status--error");
              opts.statusLine.textContent = `安装失败：${String(err).slice(0, 300)}`;
            });
        });
        body.append(
          el("div", { class: "confirm-card__line" }, [
            el("strong", {}, [inst.name]),
            el("span", { class: "settings-path" }, [note]),
            btn,
          ]),
        );
      }
    } catch (err) {
      body.replaceChildren(
        el("p", { class: "settings-status settings-status--error" }, [String(err).slice(0, 300)]),
      );
    }
  })();
}

/** 来源通道的展示名（列表页与详情页共用）。 */
export const SOURCE_LABELS: Record<MarketCard["source"], string> = {
  npm: "npm",
  github: "GitHub",
  dshhub: "dsh-hub",
};

/** 作者名：repo owner 段或 scoped 包名的 @scope 段；取不到为空串。 */
export function authorOf(card: MarketCard): string {
  if (card.repo) return card.repo.split("/")[0] ?? "";
  const npm = card.npmName ?? "";
  return npm.startsWith("@") ? (npm.slice(1).split("/")[0] ?? "") : "";
}

/** 元信息行：作者 · 版本 · 下载 · 更新 · 来源（+星标/验证标记）。 */
export function buildMetaLine(card: MarketCard): HTMLElement {
  const line = el("div", { class: "mk-meta" }, []);
  const author = authorOf(card);
  if (author) line.append(metaItem("pen", author));
  if (card.latestVersion) line.append(metaItem("none", `v${card.latestVersion}`));
  if (card.monthlyDownloads !== null) {
    line.append(metaItem("download", `${fmtCount(card.monthlyDownloads)}/月`));
  } else if (card.stars !== null) {
    line.append(metaItem("star", fmtCount(card.stars)));
  } else {
    line.append(metaItem("download", "未知"));
  }
  line.append(metaItem("arrow-up", relTime(card.updatedAt)));
  line.append(metaItem("globe", SOURCE_LABELS[card.source]));
  if (card.verified === true) {
    line.append(el("span", { class: "mk-verified" }, [icon("check", 11), "已验证"]));
  }
  return line;
}
