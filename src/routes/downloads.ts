import { collapseArrow, el } from "../lib/dom";
import { collapseToggle, enterItems } from "../lib/anim";
import { icon } from "../lib/icons";
import { uniqueInstanceName } from "../lib/instanceOrder";

/** 版本图标：DSH 鲸鱼标装进正方形小框（与版本选择页共用素材）。 */
function versionIcon(): HTMLElement {
  return el("img", {
    class: "ver-icon",
    src: "version-icon/dsh-black.svg",
    alt: "",
    draggable: "false",
  });
}
import { navigate, type PageDefinition } from "../router";
import { createInstance, listInstances } from "../services/instances";
import {
  installDsh,
  listDshVersions,
  onInstallProgress,
  type DshVersion,
} from "../services/dshInstall";
import { getTask, progressMatchesSpec, recordProgress, terminalFailure, updateTask, upsertTask, type DownloadTask } from "../lib/downloadTasks";
import { downloadSourceArchive } from "../services/sourceArchive";
import { onPluginProgress } from "../services/plugins";
import { onSkillProgress } from "../services/skills";
import { renderMarketSection } from "./download-plugins";
import { renderSkillSection } from "./download-skills";
import { renderModpackSection } from "./download-modpacks";

type DlCategory = "dsh" | "plugins" | "skill" | "modpack";

/** 源码任务在任务 store 中的合成 key：`source-<scope>-<version>`——scope =
 *  实例 id（源码落 `versions/<id>/sources/`）或 `global`（落全局 sources/，
 *  重名加副本后缀）。FR-22 修订（2026-09-12，FILE-LAYOUT §5）。 */
const SOURCE_KEY_PREFIX = "source-";

function sourceTaskKey(version: string, instanceId: string | null): string {
  return `${SOURCE_KEY_PREFIX}${instanceId ?? "global"}-${version}`;
}

export function parseSourceTaskKey(key: string): { version: string; instanceId: string | null } | null {
  if (!key.startsWith(SOURCE_KEY_PREFIX)) return null;
  const rest = key.slice(SOURCE_KEY_PREFIX.length);
  const sep = rest.lastIndexOf("-");
  if (sep <= 0) return null;
  const scope = rest.slice(0, sep);
  return { version: rest.slice(sep + 1), instanceId: scope === "global" ? null : scope };
}

/**
 * 本页全局事件（安装/插件/技能进度）与任务区定时器的释放（审查 R1：切页由
 * router 调 unmount 清理，防止每次进入页面累积注册监听器、离页后定时器空转）。
 */
let eventDisposers: (() => void)[] = [];
let downloadsEventsReleased = false;
/** 下载页分区记忆（模块级）：详情页 ← 返回时恢复离开时的分区。 */
let lastDlCategory: DlCategory = "dsh";
let tickTimer: ReturnType<typeof setInterval> | null = null;

function trackDownloadsEvent(unlisten: Promise<() => void>): void {
  if (downloadsEventsReleased) {
    void unlisten.then((dispose) => dispose());
    return;
  }
  void unlisten.then((dispose) => eventDisposers.push(dispose));
}

function releaseDownloadsEvents(): void {
  downloadsEventsReleased = true;
  for (const dispose of eventDisposers.splice(0)) dispose();
  if (tickTimer !== null) {
    clearInterval(tickTimer);
    tickTimer = null;
  }
}

export function isSourceTask(task: DownloadTask): boolean {
  return task.instanceId.startsWith(SOURCE_KEY_PREFIX);
}

/** prerelease 判定：版本号含 `-`（如 0.1.1-rc.2）。 */
function isPrerelease(version: string): boolean {
  return version.includes("-");
}

/** 版本主显示名：FR-19 惯例 `dsh-<官方版本号>`。 */
function displayName(version: string): string {
  return `dsh-${version}`;
}

export function fmtBytes(n: number): string {
  if (n <= 0) return "0 B";
  const units = ["B", "KB", "MB", "GB"];
  const i = Math.min(Math.floor(Math.log(n) / Math.log(1024)), units.length - 1);
  return `${(n / 1024 ** i).toFixed(i === 0 ? 0 : 1)} ${units[i]}`;
}

export function fmtElapsed(startedAt: number): string {
  const secs = Math.max(0, Math.floor((Date.now() - startedAt) / 1000));
  const m = Math.floor(secs / 60);
  const s = secs % 60;
  return m > 0 ? `已运行 ${m}分${s.toString().padStart(2, "0")}秒` : `已运行 ${s} 秒`;
}

/** 任务卡的进度描述：每个阶段都说清「在干嘛 + 到哪了」。 */
/** 创建实例后的安装任务：状态写入全局 store，由订阅者渲染。
 *  FR-09：kind=install（npm 子进程链——可取消、不可暂停）。 */
export async function startInstall(instanceId: string, name: string, version: string): Promise<void> {
  if (getTask(instanceId)?.status === "running") return; // 同实例已在安装，防双击重入（审查 R1）
  upsertTask({
    instanceId,
    name,
    version,
    status: "running",
    phase: "prepare",
    detail: "",
    received: 0,
    total: 0,
    speed: null,
    startedAt: Date.now(),
    kind: "install",
    canPause: false,
  });
  try {
    await installDsh(instanceId, version);
    updateTask(instanceId, { status: "done", phase: "done", detail: "" });
  } catch (err) {
    updateTask(instanceId, terminalFailure(err));
  }
}

/** 源码归档下载任务（FR-22）：合成 key 进同一任务 store，与实例任务卡并列。
 *  `instanceId` = 随实例落版本目录；null = 落全局 sources/。
 *  FR-09：kind=source（字节流——可暂停/继续/取消，断点续传）。 */
export async function startSourceDownload(version: string, instanceId: string | null = null): Promise<void> {
  const key = sourceTaskKey(version, instanceId);
  if (getTask(key)?.status === "running") return; // 同一落点同版本已在下载，不重复发起
  upsertTask({
    instanceId: key,
    name: instanceId ? `源码 dsh-v${version}（随实例）` : `DSH 源码 dsh-v${version}`,
    version,
    status: "running",
    phase: "source",
    detail: "",
    received: 0,
    total: 0,
    speed: null,
    startedAt: Date.now(),
    kind: "source",
    canPause: true,
  });
  try {
    await downloadSourceArchive(version, instanceId, ({ version: eventVersion, instanceId: eventInstance, received, total }) => {
      if (eventVersion !== version || (eventInstance ?? null) !== instanceId) return; // 并发任务按落点分发
      recordProgress(key, received, total);
    });
    updateTask(key, { status: "done", phase: "done", detail: "" });
  } catch (err) {
    updateTask(key, terminalFailure(err));
  }
}

// 模块级进度接线（H3）：dsh 安装阶段事件 → 任务 store（原 render 内处理器逐字上移）。
onInstallProgress(({ instanceId, stage }) => {
  const task = getTask(instanceId);
  if (!task || task.status !== "running") return;
  if (stage === "preparing") {
    updateTask(instanceId, { phase: "prepare", detail: "" });
  } else if (stage === "verifying") {
    updateTask(instanceId, { phase: "verify", detail: "" });
  } else if (stage === "done") {
    updateTask(instanceId, { phase: "done", detail: "" });
  } else if ("autoNode" in stage) {
    updateTask(instanceId, { phase: "node", detail: stage.autoNode.detail });
    recordProgress(instanceId, stage.autoNode.received, stage.autoNode.total);
  } else if ("installing" in stage) {
    const detail = stage.installing.detail;
    if (detail.startsWith("正在从")) {
      // 通道点名事件：存为常驻行，后续 npm 输出行不再覆盖它。
      updateTask(instanceId, { channel: detail, detail: "", phase: "install" });
    } else {
      updateTask(instanceId, { phase: "install", detail });
    }
  }
});

// 模块级进度接线（H3）：市场安装（plugin-install-progress）→ 任务 store。
// 详情页/列表页发起的安装无论停在哪个页面，任务管理页都能看到实时进度。
onPluginProgress(({ detail, spec }) => {
  if (!spec) return;
  const task = getTask(spec);
  if (task?.status === "running") updateTask(spec, { detail });
});

export const downloadsPage: PageDefinition = {
  id: "downloads",
  navLabel: "下载",
  unmount: releaseDownloadsEvents,
  render: () => {
    downloadsEventsReleased = false;
    let versions: DshVersion[] = [];
    // 版本列表拉取的失败原因（空串 = 没失败）：空列表本身区分不出「还在拉」
    // 与「拉失败」，渲染层要靠它说实话。
    let versionsError = "";
    let confirmVersion: string | null = null; // 非空 = 处于该版本的「开始下载」二级视图
    // 分类记忆上移模块级：从详情页 ← 返回时恢复离开时的分区（PCL 同行为）。
    let category: DlCategory = lastDlCategory;

    // ===== 左栏分类 =====
    const leftNav = el("nav", { class: "dl-left__nav" }, []);
    const leftItems: { id: DlCategory; label: string }[] = [
      { id: "dsh", label: "dsh 版本" },
      { id: "plugins", label: "插件" },
      { id: "skill", label: "Skill" },
      { id: "modpack", label: "整合包" },
    ];

    // ===== 右栏骨架 =====
    const rightContent = el("div", { class: "dl-right__content" }, []);

    function renderLeftNav(): void {
      leftNav.replaceChildren();
      const dshBtn = el(
        "button",
        { type: "button", class: `dl-left__item ${category === "dsh" ? "dl-left__item--active" : ""}` },
        ["dsh 版本"],
      );
      dshBtn.addEventListener("click", () => {
        category = "dsh";
        lastDlCategory = "dsh";
        renderLeftNav();
        renderRight();
      });
      leftNav.append(dshBtn);
      leftNav.append(el("div", { class: "dl-left__label" }, ["资源"]));
      for (const item of leftItems.slice(1)) {
        const btn = el(
          "button",
          { type: "button", class: `dl-left__item ${category === item.id ? "dl-left__item--active" : ""}` },
          [item.label],
        );
        btn.addEventListener("click", () => {
          category = item.id;
          lastDlCategory = item.id;
          renderLeftNav();
          renderRight();
        });
        leftNav.append(btn);
      }
    }

    // ===== 任务区：状态在全局 store，切页回来不丢；每秒刷新运行中卡的时长 =====

    /** keyed 增量渲染（R5）：新任务建卡（带入场动画），已有卡只涂变化字段，
     *  消失的任务走退场动画后移除——进度缓动与入场不再被整区重建击穿。 */
    // 插件/技能安装进度：分区渲染时把状态行交给分区组件，这里只在事件到达
    // 时刷新「当前接线的」状态行（分区重渲染会替换引用）。spec 过滤（审查
    // R1）：插件与整合包共用 plugin-install-progress 通道，按发起安装时打在
    // 状态行上的 dataset.spec 各归各，并发安装互不刷写。
    let pluginStatusLine: HTMLElement | null = null;
    trackDownloadsEvent(
      onPluginProgress(({ detail, spec }) => {
        if (pluginStatusLine?.isConnected && progressMatchesSpec(pluginStatusLine.dataset.spec, spec)) {
          pluginStatusLine.textContent = detail;
        }
      }),
    );
    let skillStatusLine: HTMLElement | null = null;
    trackDownloadsEvent(
      onSkillProgress(({ detail }) => {
        if (skillStatusLine?.isConnected) {
          skillStatusLine.textContent = detail;
        }
      }),
    );
    let modpackStatusLine: HTMLElement | null = null;
    trackDownloadsEvent(
      onPluginProgress(({ detail, spec }) => {
        if (modpackStatusLine?.isConnected && progressMatchesSpec(modpackStatusLine.dataset.spec, spec)) {
          modpackStatusLine.textContent = detail;
        }
      }),
    );

    // ===== 右栏渲染 =====
    /** 版本行 → 进入该版本的「开始下载」二级视图。 */
    function openConfirm(version: string): void {
      confirmVersion = version;
      renderRight();
    }

    function renderDsh(): void {
      rightContent.replaceChildren();
      if (confirmVersion) {
        renderConfirm(confirmVersion);
        return;
      }

      if (versions.length === 0) {
        // 拉取失败必须说出来：此前归一成空数组，页面永远停在「加载中…」，
        // 用户看到的是「没版本可下」而不是「接口拉不到」——两类原因指向完全
        // 不同的处置（等一会儿重试 vs 查网络/镜像源）。
        const card = el("section", { class: "card settings-card" }, [
          el("h2", { class: "card__title" }, ["dsh 版本"]),
          el("p", { class: versionsError ? "settings-status settings-status--error" : "settings-status" }, [
            versionsError ? `版本列表拉取失败：${versionsError}` : "版本列表加载中…",
          ]),
        ]);
        if (versionsError) {
          const retry = el("button", { type: "button", class: "btn btn--ghost settings-inline-btn" }, ["重试"]);
          retry.addEventListener("click", () => {
            versionsError = "";
            renderDsh();
            void loadVersions();
          });
          card.append(retry);
        }
        rightContent.append(card);
        return;
      }

      // 最新版本：最新正式 + 最新预览（各一条，缺省省略）；点击行进入下载页。
      const latestStable = versions.find((v) => !isPrerelease(v.version));
      const latestPre = versions.find((v) => isPrerelease(v.version));
      const latestCard = el("section", { class: "card settings-card" }, [el("h2", { class: "card__title" }, ["最新版本"])]);
      const latestEntries = [latestStable, latestPre].filter((v): v is DshVersion => v !== undefined);
      for (const v of latestEntries) {
        const row = el(
          "button",
          { type: "button", class: "ver-row" },
          [
            versionIcon(),
            el("div", { class: "ver-row__text" }, [
              el("span", { class: "ver-row__main" }, [displayName(v.version)]),
              el("span", { class: "ver-row__sub" }, [
                `${isPrerelease(v.version) ? "最新预览版" : "最新正式版"}，发布于 ${v.publishedAt.slice(0, 16).replace("T", " ")}`,
              ]),
            ]),
          ],
        );
        row.addEventListener("click", () => openConfirm(v.version));
        latestCard.append(row);
      }
      if (latestEntries.length === 0) {
        latestCard.append(el("p", { class: "settings-status" }, ["registry 上没有可用版本"]));
      }
      rightContent.append(latestCard);

      // 过往版本：除最新卡两条外的全部版本，降序（越上越新）；点击行进入下载页。
      const excluded = new Set(latestEntries.map((v) => v.version));
      const past = versions.filter((v) => !excluded.has(v.version));
      const pastList = el("div", { class: "collapse-group__list" }, []);
      for (const v of past) {
        const row = el(
          "button",
          { type: "button", class: "ver-row" },
          [
            versionIcon(),
            el("div", { class: "ver-row__text" }, [
              el("span", { class: "ver-row__main" }, [displayName(v.version)]),
              el("span", { class: "ver-row__sub" }, [
                `${isPrerelease(v.version) ? "预览版" : "正式版"}，发布于 ${v.publishedAt.slice(0, 16).replace("T", " ")}`,
              ]),
            ]),
          ],
        );
        row.addEventListener("click", () => openConfirm(v.version));
        pastList.append(row);
      }
      const { root: arrow, sync: syncArrow } = collapseArrow();
      syncArrow(true);
      const pastHeader = el(
        "button",
        { type: "button", class: "collapse-bar" },
        [el("span", { class: "collapse-bar__title" }, [`过往版本（${past.length}）`]), arrow],
      );
      pastList.hidden = true;
      pastHeader.addEventListener("click", () => {
        const opening = pastList.hidden;
        // R5：高度动画展开/收起（anim.ts collapseToggle），箭头旋转逻辑不变。
        collapseToggle(pastList, opening);
        syncArrow(!opening);
      });
      rightContent.append(
        el("section", { class: "card settings-card collapse-group" }, [pastHeader, pastList]),
      );
      __scheduleCascade();
    }

    /** 「开始下载」二级视图：头部（返回 + 可编辑实例名，恒显）+「下载内容」
     *  双勾选项（启动版本默认勾、可同时勾选源码，全不选时按钮禁用）+ 居中开始按钮。 */
    function renderConfirm(version: string): void {
      const baseName = displayName(version);
      const nameInput = el("input", { type: "text", class: "settings-input dl-confirm__input", value: baseName });
      // P3-03：异步回填不得覆盖用户已输入的值——用户动过输入框（input 事件）
      // 即停用预演回填，预演只负责消除「默认名撞名」的落差。
      let nameEdited = false;
      nameInput.addEventListener("input", () => {
        nameEdited = true;
      });

      // 重名预演：与后端 unique_name 同规则（见 lib/instanceOrder），消除
      // 「输入框显示 dsh-x、实际创建 dsh-x-17」的预期落差；后端仍是权威兜底。
      void (async () => {
        try {
          const existing = await listInstances();
          if (nameInput.isConnected && !nameEdited) {
            nameInput.value = uniqueInstanceName(baseName, existing.map((i) => i.name));
          }
        } catch {
          // 拉不到存量列表就保持原始默认名，交由后端兜底。
        }
      })();

      const backBtn = el("button", { type: "button", class: "dl-back", title: "返回版本列表" }, [icon("back", 16)]);
      backBtn.addEventListener("click", () => {
        confirmVersion = null;
        renderRight();
      });

      // ── 下载内容（FR-22 需求方定稿）：双勾选项而非二选一——用源码的人通常
      // 也要装实例测插件冲突，允许 √√ 同下；行 = 纯文字 + 勾选框，选中=蓝色√、
      // 未选=空框；默认 √×，××（全不选）时开始按钮禁用。──
      let wantInstance = true;
      let wantSource = false;

      const startBtn = el("button", { type: "button", class: "dl-start-btn" }, [
        el("span", { class: "dl-start-btn__icon" }, [icon("download", 18)]),
        "开始下载",
      ]);
      const syncStart = (): void => {
        startBtn.disabled = !wantInstance && !wantSource;
      };

      const makeItemRow = (label: string, initial: boolean, onChange: (on: boolean) => void): HTMLElement => {
        let on = initial;
        const box = el("span", { class: `dl-check ${on ? "dl-check--on" : ""}` }, on ? [icon("check", 12)] : []);
        const row = el("button", { type: "button", class: "dl-item" }, [
          el("span", { class: "dl-item__label" }, [label]),
          box,
        ]);
        row.addEventListener("click", () => {
          on = !on;
          box.classList.toggle("dl-check--on", on);
          box.replaceChildren(...(on ? [icon("check", 12)] : []));
          onChange(on);
        });
        return row;
      };
      const rowInstance = makeItemRow("启动版本", true, (on) => {
        wantInstance = on;
        syncStart();
      });
      const rowSource = makeItemRow("下载源码", false, (on) => {
        wantSource = on;
        syncStart();
      });

      // 创建失败就地报错并停留本视图——绝不为此生成任务卡（防卡片无限堆叠）；
      // fail-fast：实例创建失败时不发起任何下载，无半下载状态。
      // 源码下载路径的错误进任务卡（startSourceDownload 内部捕获），不会走到这里。
      const errLine = el("p", {
        class: "settings-status settings-status--error dl-confirm__error",
        hidden: "",
      }, []);
      let starting = false;
      startBtn.addEventListener("click", () => {
        if (starting || (!wantInstance && !wantSource)) return;
        starting = true;
        errLine.hidden = true;
        errLine.textContent = "";
        startBtn.disabled = true;
        void (async () => {
          try {
            let createdInstanceId: string | null = null;
            if (wantInstance) {
              const name = nameInput.value.trim() || displayName(version);
              // FR-19：撞名自动 -2/-3 后缀由后端处理；返回的任务卡以最终名显示。
              const instance = await createInstance(name, "web");
              createdInstanceId = instance.id;
              void startInstall(instance.id, instance.name, version);
            }
            if (wantSource) {
              // FR-22 修订：随实例下载落 `versions/<id>/sources/`；仅源码下载落全局。
              void startSourceDownload(version, createdInstanceId);
            }
            confirmVersion = null;
            // PCL Install_Click 原式：安装发起即切任务管理页（需求方点名的「下载加载页」）。
            void navigate("tasks");
          } catch (err) {
            errLine.textContent = `创建实例失败：${String(err).slice(0, 300)}`;
            errLine.hidden = false;
          } finally {
            starting = false;
            if (startBtn.isConnected) startBtn.disabled = false;
          }
        })();
      });

      rightContent.append(
        el("div", { class: "dl-confirm" }, [
          el("section", { class: "card settings-card dl-confirm__head" }, [
            backBtn,
            nameInput,
          ]),
          el("section", { class: "card settings-card dl-items" }, [
            el("h2", { class: "card__title" }, ["下载内容"]),
            rowInstance,
            rowSource,
          ]),
          el("div", { class: "dl-start-wrap" }, [startBtn]),
          errLine,
        ]),
      );
    }

    // 入场级联开关：版本数据**异步到达**后的重绘不重播——切页入场（router
    // enterPage）已经播过一次，数据回来再整栏 replaceChildren + 级联，等于把
    // 刚落定的内容换成透明副本重来，视觉上「拉出来、消失、再拉出来」闪一道
    // （走查 2026-10-03）。用户点左栏分类回到 dsh 页的重绘是主动换内容，
    // 级联照常。
    let cascadeOnRender = true;
    const __scheduleCascade = () => {
      if (!cascadeOnRender) return;
      enterItems(Array.from(rightContent.children) as HTMLElement[]);
    };
    function renderRight(): void {
      if (category !== "dsh") {
        if (category === "plugins") {
          rightContent.replaceChildren();
          pluginStatusLine = renderMarketSection(rightContent);
        } else if (category === "skill") {
          rightContent.replaceChildren();
          skillStatusLine = renderSkillSection(rightContent);
        } else {
          rightContent.replaceChildren();
          modpackStatusLine = renderModpackSection(rightContent);
        }
        return;
      }
      renderDsh();
    }

    // ===== 初始化 =====
    // 版本列表拉取：失败留痕（versionsError）供渲染层如实呈现，不静默成空列表。
    async function loadVersions(): Promise<void> {
      try {
        versions = await listDshVersions();
        versionsError = "";
      } catch (err) {
        versions = [];
        versionsError = String(err).slice(0, 200);
        console.error("dsh 版本列表拉取失败", err);
      }
      // 数据到达 = 内容就位，不是「换了一个新页面」：静默替换加载卡，不重播入场。
      cascadeOnRender = false;
      renderRight();
      cascadeOnRender = true;
    }

    void (async () => {
      renderLeftNav();
      renderRight();
      await loadVersions();
    })();

    return el("section", { class: "page" }, [
      el("div", { class: "dl" }, [
        el("aside", { class: "dl-left" }, [leftNav]),
        el("section", { class: "dl-right" }, [rightContent]),
      ]),
    ]);
  },
};
