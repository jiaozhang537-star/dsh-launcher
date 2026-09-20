/** 插件市场 / Skill 中心共享件：口语化格式化 + 条目图标 + 实例选择弹窗。
 * 弹窗按「describe 每实例可装性 → run 执行 → 状态行跟进」的契约复用。 */
import { el } from "../lib/dom";
import { listInstances, type Instance } from "../services/instances";

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
              opts.statusLine.textContent = `✓ ${message}`;
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
