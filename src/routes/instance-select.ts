import { el } from "../lib/dom";
import { icon } from "../lib/icons";
import { enterMsg } from "../lib/anim";
import { sortInstancesByRecency } from "../lib/instanceOrder";
import { dropFavorite, isFavorite, loadFavorites, toggleFavorite } from "../lib/instanceFavorites";
import { getSelectedInstance, setSelectedInstance } from "../lib/selected-instance";
import type { PageDefinition } from "../router";
import { navigate } from "../router";
import {
  deleteInstance,
  listInstances,
  pickFolder,
  registerExternalInstance,
  type Instance,
} from "../services/instances";
import { openModpackImport } from "./modpack-import";

/** 版本图标：DSH 鲸鱼标（public/vendor-logos/deepseek.png）装进正方形小框。 */
function versionIcon(): HTMLElement {
  const img = el("img", {
    class: "ver-icon",
    src: "version-icon/dsh-black.svg",
    alt: "",
    draggable: "false",
  });
  return img;
}

/** 版本选择页（二级页）：左路径区 + 占位入口，右侧收藏夹/已安装实例竖排。 */
export const instanceSelectPage: PageDefinition = {
  id: "instance-select",
  navLabel: "版本选择",
  secondary: true,
  render: () => {
    let instances: Instance[] = [];
    let currentId: string | null = getSelectedInstance();

    // ===== 左栏 =====
    // 语义（需求方定稿）：路径 ≠ dsh 版本。路径区只列两类条目——启动器自己的
    // 默认实例目录（自动创建的实例都落在这里）与玩家日后手动添加/整合包导入
    // 的路径；不逐实例罗列路径。
    const defaultPathSub = el("span", { class: "sub-left__item-sub" }, ["（目录解析中…）"]);
    const pathGroup = el("div", { class: "sub-left__group" }, [
      el("div", { class: "sub-left__label" }, ["路径"]),
      el("div", { class: "sub-left__item" }, [
        el("span", { class: "sub-left__item-main" }, ["默认路径"]),
        defaultPathSub,
      ]),
    ]);

    const addFolderBtn = el(
      "button",
      { type: "button", class: "sub-left__action", title: "注册已有的 DSH 目录为实例" },
      [icon("plus", 14), "添加已有文件夹"],
    );
    addFolderBtn.addEventListener("click", () => {
      void (async () => {
        try {
          const folder = await pickFolder();
          if (!folder) return;
          note("正在注册文件夹…");
          const inst = await registerExternalInstance(folder);
          instances = await listInstances().catch(() => instances);
          renderRight();
          note(`已添加 ${inst.name}（原目录：${inst.dir ?? folder}）`);
        } catch (err) {
          note(`添加失败：${String(err).slice(0, 260)}`, true);
        }
      })();
    });
    const importPackBtn = el(
      "button",
      { type: "button", class: "sub-left__action", title: "选择整合包 tgz 导入" },
      [icon("import", 14), "导入整合包"],
    );
    importPackBtn.addEventListener("click", () => {
      void openModpackImport(null);
    });

    const left = el("aside", { class: "sub-left" }, [
      pathGroup,
      el("div", { class: "sub-left__group" }, [
        el("div", { class: "sub-left__label" }, ["添加或导入"]),
        addFolderBtn,
        importPackBtn,
      ]),
    ]);

    // ===== 右栏：收藏夹卡 + 已安装实例卡 =====
    const noteLine = el("p", { class: "settings-status", hidden: "" }, []);
    let noteTimer: ReturnType<typeof setTimeout> | undefined;
    function note(text: string, isError = false): void {
      noteLine.textContent = text;
      noteLine.classList.toggle("settings-status--error", isError);
      noteLine.hidden = false;
      clearTimeout(noteTimer);
      noteTimer = setTimeout(() => {
        noteLine.hidden = true;
      }, 4000);
    }

    const favCard = el("section", { class: "card settings-card", hidden: "" }, []);
    const installedCard = el("section", { class: "card settings-card sub-right__card" }, []);

    function selectAndBack(inst: Instance): void {
      currentId = inst.id;
      setSelectedInstance(inst.id);
      navigate("launch");
    }

    /** 删除确认卡的目标实例（null = 关闭）。确定后注册表先行、目录由后端
     * 分离进程物理清除（删除立即生效，不卡 UI，启动器退出也会删完）。 */
    let confirmTarget: Instance | null = null;

    async function removeInstance(target: Instance): Promise<void> {
      try {
        await deleteInstance(target.id);
        dropFavorite(target.id);
        note(`已删除 ${target.name}`);
        if (currentId === target.id) {
          instances = instances.filter((i) => i.id !== target.id);
          const next = sortInstancesByRecency(instances)[0];
          currentId = next?.id ?? null;
          if (next) setSelectedInstance(next.id);
          else setSelectedInstance(null);
        }
        instances = await listInstances().catch(() => instances.filter((i) => i.id !== target.id));
      } catch (err) {
        note(`删除失败：${String(err).slice(0, 160)}`, true);
        instances = await listInstances().catch(() => instances);
      }
      renderRight();
    }

    function renderConfirmCard(): void {
      const firstShow = confirmOverlay.hidden && confirmTarget != null;
      confirmOverlay.hidden = !confirmTarget;
      if (confirmTarget) {
        // 外部实例（原地引用）：删除 = 仅解除关联，绝不删用户文件夹。
        confirmBody.replaceChildren(
          el("p", { class: "confirm-card__line" }, [
            `你确定要删除版本 `,
            el("strong", {}, [confirmTarget.name]),
            ` 吗？`,
          ]),
          el("p", { class: "confirm-card__line" }, [
            confirmTarget.dir
              ? "该版本来自外部文件夹：仅解除与启动器的关联，文件夹与其中的数据不会被删除。"
              : "插件、Skill 将会一并删除。",
          ]),
        );
      }
      // 弹窗入场（spec §5 MyMsg：Y40→0 300ms OutBack）——只在由隐转现那一次播。
      if (firstShow) enterMsg(confirmOverlay.querySelector<HTMLElement>(".confirm-card")!);
    }

    /**
     * 单个版本行：图标 + 名称/副标题 + 悬浮操作（收藏/删除/设置）。
     * 点击行 = 选中并跳回启动页（需求方定稿）。
     */
    function buildRow(inst: Instance): HTMLElement {
      const row = el("div", {
        class: `ver-row ${inst.id === currentId ? "ver-row--active" : ""}`,
        role: "button",
        tabindex: "0",
      });
      const text = el("div", { class: "ver-row__text" }, [
        el("span", { class: "ver-row__main" }, [inst.name]),
        el("span", { class: "ver-row__sub" }, [
          inst.dshVersion ? `DSH ${inst.dshVersion}` : "未安装 DSH",
          inst.port ? ` · 固定端口 :${inst.port}` : "",
          inst.id === currentId ? " · 当前" : "",
        ]),
      ]);

      const favBtn = el("button", {
        type: "button",
        class: `ver-act ver-act--fav${isFavorite(inst.id) ? " is-on" : ""}`,
        title: isFavorite(inst.id) ? "取消收藏" : "收藏",
      }, [icon("heart", 15)]);
      favBtn.addEventListener("click", (e) => {
        e.stopPropagation();
        const on = toggleFavorite(inst.id);
        favBtn.classList.toggle("is-on", on);
        favBtn.title = on ? "取消收藏" : "收藏";
        renderRight();
      });

      const delBtn = el("button", {
        type: "button",
        class: "ver-act ver-act--del",
        title: "删除该版本",
      }, [icon("trash", 15)]);
      delBtn.addEventListener("click", (e) => {
        e.stopPropagation();
        confirmTarget = inst;
        renderConfirmCard();
      });

      const setBtn = el("button", {
        type: "button",
        class: "ver-act",
        title: "打开版本设置",
      }, [icon("gear", 15)]);
      setBtn.addEventListener("click", (e) => {
        e.stopPropagation();
        currentId = inst.id;
        setSelectedInstance(inst.id);
        navigate("instance-settings");
      });

      const pick = (): void => selectAndBack(inst);
      row.addEventListener("click", pick);
      row.addEventListener("keydown", (e) => {
        if (e.key === "Enter" || e.key === " ") {
          e.preventDefault();
          pick();
        }
      });

      row.append(versionIcon(), text, el("span", { class: "ver-row__acts" }, [favBtn, delBtn, setBtn]));
      return row;
    }

    function fillInstalledCard(): void {
      installedCard.replaceChildren(el("h2", { class: "card__title" }, ["已安装版本"]));
      if (instances.length === 0) {
        installedCard.append(
          el("p", { class: "settings-status" }, ["还没有已安装的版本——前往「下载」页创建。"]),
        );
        return;
      }
      for (const inst of instances) installedCard.append(buildRow(inst));
    }

    function fillFavCard(): void {
      const favIds = loadFavorites();
      const rows = favIds
        .map((id) => instances.find((i) => i.id === id))
        .filter((i): i is Instance => i !== undefined);
      favCard.replaceChildren();
      favCard.hidden = rows.length === 0;
      if (rows.length === 0) return;
      favCard.append(el("h2", { class: "card__title card__title--fav" }, ["收藏夹"]));
      for (const inst of rows) favCard.append(buildRow(inst));
    }

    function renderRight(): void {
      fillFavCard();
      fillInstalledCard();
    }

    // ===== 初始化 =====
    void (async () => {
      try {
        instances = await listInstances();
        if (!currentId || !instances.some((i) => i.id === currentId)) {
          const sorted = sortInstancesByRecency(instances);
          const first = sorted[0] ?? instances[0];
          currentId = first?.id ?? null;
          if (first) setSelectedInstance(first.id);
        }
      } catch (err) {
        console.error("版本选择页初始化失败", err);
        installedCard.replaceChildren(
          el("h2", { class: "card__title" }, ["已安装实例"]),
          el("p", { class: "settings-status settings-status--error" }, [String(err)]),
        );
      }
      renderRight();
    })();

    void import("../services/instances").then(async ({ getInstancesDir }) => {
      try {
        defaultPathSub.textContent = await getInstancesDir();
      } catch {
        defaultPathSub.textContent = "（目录解析失败）";
      }
    });

    const rightWrap = el("div", { class: "sub-right__stack" }, [
      noteLine,
      favCard,
      installedCard,
    ]);

    // ===== 删除确认卡（PCL2 式：红标题 + 说明 + 确定/取消）=====
    const confirmBody = el("div", { class: "confirm-card__body" }, []);
    const confirmCancel = el("button", { type: "button", class: "btn btn--line" }, ["取消"]);
    confirmCancel.addEventListener("click", () => {
      confirmTarget = null;
      renderConfirmCard();
    });
    const confirmOk = el("button", { type: "button", class: "btn btn--danger" }, ["确定"]);
    confirmOk.addEventListener("click", () => {
      const target = confirmTarget;
      confirmTarget = null;
      renderConfirmCard();
      if (target) void removeInstance(target);
    });
    const confirmOverlay = el("div", { class: "modal-overlay", hidden: "" }, [
      el("div", { class: "confirm-card" }, [
        el("p", { class: "confirm-card__title" }, ["版本删除确认"]),
        confirmBody,
        el("div", { class: "confirm-card__btns" }, [confirmOk, confirmCancel]),
      ]),
    ]);

    return el("section", { class: "page" }, [
      el("div", { class: "sub" }, [left, el("section", { class: "sub-right" }, [rightWrap])]),
      confirmOverlay,
    ]);
  },
};
