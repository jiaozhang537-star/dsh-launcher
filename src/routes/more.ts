/** 更多页：左分类（关于 / 使用帮助 / 检查更新）+ 右内容，全局左右栏布局。
 *  使用帮助 = PROJECT §7 定稿的四步指引（帮助内容只允许出现在「更多」页）；
 *  检查更新 = FR-13（GitHub Releases，经统一下载网关；更新下载进统一任务队列）。 */
import { el } from "../lib/dom";
import type { PageDefinition } from "../router";
import { terminalFailure, updateTask, upsertTask } from "../lib/downloadTasks";
import { checkUpdate, downloadUpdate, installUpdate, type UpdateStatus } from "../services/selfUpdate";

/** 构建期回退版本号（Tauri 环境下由 getVersion() 覆写为 tauri.conf.json 值）。 */
const APP_VERSION = "0.1.0";
const UPDATE_TASK_KEY = "update";

const CATEGORIES: { id: string; label: string }[] = [
  { id: "about", label: "关于" },
  { id: "help", label: "使用帮助" },
  { id: "update", label: "检查更新" },
];

/** 当前运行版本（getVersion 优先，非 Tauri 环境回落构建期常量）。 */
async function runtimeVersion(): Promise<string> {
  try {
    const { getVersion } = await import("@tauri-apps/api/app");
    return await getVersion();
  } catch {
    return APP_VERSION;
  }
}

function renderAbout(version: string): HTMLElement {
  return el("div", { class: "settings-pane" }, [
    el("section", { class: "card settings-card" }, [
      el("h2", { class: "card__title" }, ["关于"]),
      el("div", { class: "settings-node-card" }, [
        el("span", { class: "settings-badge settings-badge--ok" }, [`v${version}`]),
      ]),
      el("p", { class: "settings-path" }, ["开源协议：MIT。"]),
      el("p", { class: "settings-path" }, ["本启动器只是壳：不修改 DSH 本体，一切设置经 DSH 原生配置生效。"]),
    ]),
  ]);
}

/** 使用帮助（PROJECT §7 定稿四步：下载 → 启用厂商 → 配 Key → 选模型 → 启动）。 */
function renderHelp(): HTMLElement {
  const steps: { title: string; detail: string }[] = [
    { title: "① 下载", detail: "「下载」页选择 DSH 版本，确认视图点「开始下载」——实例创建后自动安装。" },
    { title: "② 启用厂商", detail: "「设置 → 模型提供商」添加并勾选要用的模型厂商。" },
    { title: "③ 配置 Key", detail: "在厂商卡片里填入 API Key（启动页也可就地填写）。" },
    { title: "④ 选模型并启动", detail: "回到「启动」页选择厂商与模型，点「启动」即可在浏览器中使用。" },
  ];
  return el("div", { class: "settings-pane" }, [
    el("section", { class: "card settings-card" }, [
      el("h2", { class: "card__title" }, ["使用帮助"]),
      ...steps.map((step) =>
        el("div", { class: "settings-row help-step" }, [
          el("span", { class: "help-step__title" }, [step.title]),
          el("span", { class: "settings-path" }, [step.detail]),
        ]),
      ),
    ]),
  ]);
}

function renderUpdate(): HTMLElement {
  const status = el("p", { class: "settings-path" }, ["点击「检查更新」获取最新版本信息。"]);
  const notesBox = el("div", { class: "settings-path" }, []);
  const actionRow = el("div", { class: "settings-row" }, []);
  const card = el("section", { class: "card settings-card" }, [
    el("h2", { class: "card__title" }, ["检查更新"]),
    status,
    notesBox,
    actionRow,
  ]);

  const renderResult = (result: UpdateStatus): void => {
    notesBox.replaceChildren();
    actionRow.replaceChildren();
    if (!result.configured) {
      status.textContent = "未配置更新源——启动器正式发布后可用（更新源在 config.json 的 update_repo）。";
      return;
    }
    if (!result.hasUpdate) {
      status.textContent = `已是最新版本（v${result.currentVersion}）。`;
      return;
    }
    status.textContent = `发现新版本 v${result.latestVersion ?? ""}（当前 v${result.currentVersion}）。`;
    if (result.releaseNotes) {
      const details = el("details", { class: "error-detail" }, [
        el("summary", {}, ["更新说明"]),
        el("pre", { class: "error-detail__body" }, [result.releaseNotes]),
      ]);
      notesBox.append(details);
    }
    if (!result.assetName) {
      status.textContent += "（该发布未提供 Windows 安装包资产）";
      return;
    }
    const downloadBtn = el("button", { type: "button", class: "btn btn--ghost settings-inline-btn" }, ["下载更新"]);
    const installBtn = el("button", {
      type: "button",
      class: "btn btn--line settings-inline-btn",
      hidden: "",
    }, ["打开安装包"]);
    let installerPath: string | null = null;
    installBtn.addEventListener("click", () => {
      if (!installerPath) return;
      void installUpdate(installerPath).catch((err) => {
        status.textContent = `打开安装包失败：${String(err).slice(0, 200)}`;
      });
    });
    downloadBtn.addEventListener("click", () => {
      downloadBtn.disabled = true;
      status.textContent = "正在下载更新（任务管理页可见进度）…";
      // 进统一任务队列（任务键 update），任务管理页可暂停/继续/取消。
      upsertTask({
        instanceId: UPDATE_TASK_KEY,
        name: `更新 v${result.latestVersion ?? ""}`,
        version: result.latestVersion ?? "",
        status: "running",
        phase: "source",
        detail: "",
        received: 0,
        total: 0,
        speed: null,
        startedAt: Date.now(),
        kind: "update",
        canPause: true,
      });
      void (async () => {
        try {
          installerPath = await downloadUpdate(result.currentVersion);
          updateTask(UPDATE_TASK_KEY, { status: "done", phase: "done", detail: "" });
          status.textContent = "下载完成——打开安装包，按系统安装器指引完成更新。";
          installBtn.hidden = false;
        } catch (err) {
          updateTask(UPDATE_TASK_KEY, terminalFailure(err));
          status.textContent = `下载失败：${String(err).slice(0, 200)}`;
          downloadBtn.disabled = false;
        }
      })();
    });
    actionRow.append(downloadBtn, installBtn);
  };

  const checkBtn = el("button", { type: "button", class: "btn btn--ghost settings-inline-btn" }, ["检查更新"]);
  checkBtn.addEventListener("click", () => {
    checkBtn.disabled = true;
    status.textContent = "正在检查更新…";
    notesBox.replaceChildren();
    actionRow.replaceChildren();
    void (async () => {
      try {
        const version = await runtimeVersion();
        const result = await checkUpdate(version);
        renderResult(result);
      } catch (err) {
        status.textContent = `检查失败：${String(err).slice(0, 200)}`;
      } finally {
        checkBtn.disabled = false;
      }
    })();
  });
  card.append(checkBtn);
  return el("div", { class: "settings-pane" }, [card]);
}

export const morePage: PageDefinition = {
  id: "more",
  navLabel: "更多",
  render: () => {
    let category = "about";
    let version = APP_VERSION;

    const leftNav = el("div", { class: "sub-left__group" }, []);
    const pane = el("div", { class: "settings-pane" }, []);

    function renderRight(): void {
      if (category === "help") {
        pane.replaceChildren(renderHelp());
      } else if (category === "update") {
        pane.replaceChildren(renderUpdate());
      } else {
        pane.replaceChildren(renderAbout(version));
      }
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
    void runtimeVersion().then((v) => {
      if (v === version) return;
      version = v;
      if (category === "about") renderRight();
    });

    return el("section", { class: "page" }, [
      el("div", { class: "sub sub--compact" }, [
        el("aside", { class: "sub-left" }, [leftNav]),
        el("section", { class: "sub-right" }, [pane]),
      ]),
    ]);
  },
};
