/** 启动页：厂商选择 + 方形头像 + key 两态输入 + 模型选择 + 启动大按钮 + 按需出现的运行状态区。 */
import { el } from "../lib/dom";
import { icon } from "../lib/icons";
import { enterMsg } from "../lib/anim";
import { showHint } from "../lib/hint";
import { webEntryUrl } from "../lib/launch-decision";
import { applyVisibilityOnLaunch } from "../lib/visibility";
import { sortInstancesForLaunch } from "../lib/instanceOrder";
import { settledStageText, stepLevelOf, type StepLevel } from "../lib/launchSteps";
import { getSelectedInstance, setSelectedInstance } from "../lib/selected-instance";
import { UI_PREFS_EVENT, versionButtonsHidden } from "../lib/uiPrefs";
import {
  deriveVendorView,
  keySaveEligible,
  launchModelsFor,
  pruneSessionKeyMarks,
  setVendorCatalog,
  type VendorEntry,
} from "../lib/vendor";
import type { PageDefinition } from "../router";
import { navigate } from "../router";
import { getStatus, setKey, KEY_MASK, type KeyvaultStatus } from "../services/keyvault";
import { fetchProvidersView } from "../services/providers";
import { providerIconPath } from "../lib/providerIcon";

import {
  getInjectOverride,
  listInstances,
  setInjectOverride,
  type Instance,
} from "../services/instances";
import { providerRouteKey } from "../services/providers";
import {
  cancelLaunch,
  launchInstance,
  listRunning,
  onConsoleLine,
  onDiagnostic,
  onExit,
  onLaunchProgress,
  onWebReady,
  onWebTimeout,
  type ConsoleLine,
  type DiagnosticCard,
  type Reconcile,
  type RunStatus,
} from "../services/process";
import { parseBlockedReport } from "../services/precheck";

/**
 * 用系统默认浏览器打开实例网页——启动器职责到此为止（需求方定稿）。
 *
 * 传进来的 URL 由 `webEntryUrl` 决定：优先 `dsh web` 打印的带 token 地址（新版
 * DSH 的鉴权栅栏只认它，裸端口 URL 必然 401），旧版本回落裸端口。
 */
async function openInstanceWeb(url: string): Promise<void> {
  try {
    const { openUrl } = await import("@tauri-apps/plugin-opener");
    await openUrl(url);
  } catch {
    window.open(url, "_blank", "noreferrer");
  }
}

const SUB_NO_INSTANCE = "尚未创建实例 —— 前往「下载」页下载";
/** 运行状态读不到时的副标题：把「未知」说出来，不伪造成「未运行」。 */
const SUB_RUNNING_UNKNOWN = "运行状态读取失败 · 本次按未运行呈现";
/** 用户取消启动后的副标题（进程已由后端停掉，如实说明它现在是什么状态）。 */
const SUB_CANCELLED = "已取消启动（实例已停止）";

/**
 * 主页运行呈现（PCL2 式，需求方定稿 + 2026-09-10 走查裁决）：
 * - 主页不出现任何控制台/日志行内 UI（黑控制台废除）；
 * - 左栏是**两个互斥视图**：静止页（厂商胶囊/头像/Key/模型/启动簇）与加载页
 *   （黑鱼动画 + 进度条 + 字幕式校验序列 [+ 取消启动，见提交 B]）。加载时静止页
 *   整体隐藏——原先「两面同格叠放 + 浮层清单」会让清单每多一行就把身份块顶上去，
 *   拆页后这类耦合从结构上不存在；
 * - 校验序列是**字幕式滚动**：一行一个真实阶段/结论，从下往上滚，最新一行始终
 *   完整可见；阶段文案由后端 `launch-progress` 事件驱动，收尾时换成完成态措辞；
 * - 网页就绪即开浏览器、加载页切回静止页——启动器不呈现「运行中」语义；
 * - 点「启动」= 一次独立的完整启动：后端先停旧进程（含上一会话遗留的孤儿）再
 *   按原钉定端口拉起，因此运行期注入的 Key/基址环境变量每次都刷新。启动器侧的
 *   配置改动一律只在下一次启动生效，运行中不改实例文件（K3）。
 * - 仅报错（校验拦截、启动失败、异常退出、哨兵错误）才弹出二级悬浮卡，
 *   卡内是简化日志；卡上「日志」按钮才打开详细日志白底弹窗。
 */

/** 详细日志缓冲：每实例 ≤300 行（弹窗按需渲染，主页从不展示）。 */
const CONSOLE_LIMIT = 300;

/** 悬浮卡内的简化日志条数上限。 */
const CARD_LINES_LIMIT = 8;

/**
 * 本页全局事件（控制台行/诊断/退出）的订阅释放（审查 R1：切页由 router
 * 调 unmount 清理，防止每次进入页面累积注册监听器）。
 */
let eventDisposers: (() => void)[] = [];
/** 页面是否已卸载：卸载后才 resolve 的订阅立即解除，防慢注册泄漏。 */
let launchEventsReleased = false;

function trackLaunchEvent(unlisten: Promise<() => void>): void {
  if (launchEventsReleased) {
    void unlisten.then((dispose) => dispose());
    return;
  }
  void unlisten.then((dispose) => eventDisposers.push(dispose));
}

function releaseLaunchEvents(): void {
  launchEventsReleased = true;
  for (const dispose of eventDisposers.splice(0)) dispose();
}

/**
 * 启动错误居中弹窗（PCL2 式二级弹窗，需求方定稿）：
 * 校验拦截/启动失败/异常退出/运行时诊断统一走此弹窗；canForce=校验拦截时
 * 弹窗内提供「强制启动（已了解风险）」按钮，左栏不再出现任何额外按钮。
 */
interface ErrorModalState {
  title: string;
  lines: string[];
  canForce: boolean;
  /** 后端校验条目给的路由提示（去设置/去下载/去启动）——点了直接跳页，不让用户自己找。 */
  actions?: string[];
}

/** 校验条目的 `action` → 启动器页面 id（后端只说意图，页面 id 归前端）。 */
const ACTION_PAGES: Record<string, string> = {
  settings: "settings",
  downloads: "downloads",
  launch: "launch",
};

/** 路由提示 → 按钮文案。 */
const ACTION_LABELS: Record<string, string> = {
  settings: "去设置",
  downloads: "去下载页",
  launch: "去启动页",
};

export const launchPage: PageDefinition = {
  id: "launch",
  navLabel: "启动",
  unmount: releaseLaunchEvents,
  render: () => {
    launchEventsReleased = false;
    let instances: Instance[] = [];
    let selectedId: string | null = getSelectedInstance();
    let runningMap = new Map<string, RunStatus>();
    /** 运行状态是否处于「读不到」状态（见 refreshRunning）：副标题据此说实话。 */
    let runningUnknown = false;
    let vendorStatus: KeyvaultStatus | null = null;
    let activeVendorId = "deepseek";
    /** 详细日志缓冲（弹窗数据源，主页不渲染）。 */
    const consoleLines = new Map<string, string[]>();
    /** 简化诊断（悬浮卡数据源，最新在前，≤8 条）。 */
    const diagCards = new Map<string, DiagnosticCard[]>();
    /** 启动流程进行中（当前选中实例）：左栏切成加载页。 */
    let starting = false;
    let stageText = "";
    /** 加载动画百分比：target 由后端进度事件推进，shown 每帧缓动逼近（观感流畅）。 */
    let progressTarget = 0;
    let progressShown = 0;
    let progressRaf: number | null = null;
    /** 加载页字幕序列（真实阶段与结论；最新在末尾，进行中的那一行 active=true）。 */
    const launchSteps: { text: string; level: StepLevel; active?: boolean; action?: string }[] = [];
    /**
     * 本次启动的序号：每次点启动 +1。上一次启动迟到的答复（尤其取消后紧跟的
     * 再启动）带着旧序号回来时直接丢弃，不碰这一次的界面。
     */
    let launchRun = 0;
    /** 用户点了「取消启动」：据此把后端的取消错误按「用户取消」处置并抑制就绪/超时。 */
    let cancelRequested = false;
    /** 二级悬浮卡（报错才弹出）。 */
    let errorModal: ErrorModalState | null = null;
    /** 详细日志弹窗当前展示的实例（null = 关闭）。 */
    let logModalId: string | null = null;
    /** 用户主动停止的实例 id：其退出事件不算异常，不弹悬浮卡。 */
    let stoppingByUser: string | null = null;
    /** 本会话内已确认保存过 Key 的厂商——掩码显示不依赖刷新链。 */
    const knownKeys = new Set<string>();

    // ===== 厂商区（默认 deepseek 直接呈现、零交互；目录内多于一个厂商才出现折叠）=====
    const pillText = el("span", { class: "vendor-switch__label" }, ["deepseek"]);
    const pillChevron = el("span", { class: "vendor-switch__chevron", hidden: "" }, [icon("chevron-down", 14)]);
    const vendorPill = el(
      "button",
      { type: "button", class: "vendor-switch__pill" },
      [pillText, pillChevron],
    );
    const vendorList = el("div", { class: "vendor-switch__list" }, []);
    vendorPill.addEventListener("click", () => {
      if (!vendorFoldable()) return;
      toggleFloatingList(vendorList, vendorPill, identityMain);
    });
    const avatar = el("div", { class: "vendor-avatar", "aria-hidden": "true" }, []);

    /** 目录内厂商多于一个才允许折叠切换；当前目录恒为单条 → 静态胶囊。 */
    function vendorFoldable(): boolean {
      return deriveVendorView(vendorStatus).entries.length > 1;
    }

    const keyInput = el("input", { type: "password", class: "launch-key__input", placeholder: "sk-" });
    const keySaveBtn = el("button", { type: "button", class: "btn btn--ghost launch-key__save" }, ["保存"]);
    const keyNote = el("p", { class: "launch-key__note" }, [""]);
    // P3-02：用户是否真实编辑过 Key（input 事件置位；程序化置值不触发 input）。
    let keyEdited = false;
    keyInput.addEventListener("input", () => {
      keyEdited = true;
    });

    // ===== 模型选择框（key 下方同款细长样式）：选中即写当前实例默认模型 =====
    const modelInput = el("input", {
      type: "text",
      class: "launch-key__input model-select__input",
      readonly: "",
      placeholder: "默认模型",
    });
    const modelList = el("div", { class: "model-select__list" }, []);
let currentModel = "";
/** 实例当前默认模型的提供方 id（agent-default-model.provider，空 = 老实例没写）。 */
let currentProvider = "";
/** 切厂商联动意图：厂商行点击置位，refreshModelState 读取完成后消费（见 followVendorDefault）。 */
let followPendingVendor: string | null = null;

    keySaveBtn.addEventListener("click", () => {
      void (async () => {
        const entry = activeEntry();
        const value = keyInput.value.trim();
        // dirty 标志为第一判据；掩码哨兵/空值恒判「未修改」（P3-02：复制
        // 掩码回贴不会被当作新 Key）。
        if (!keySaveEligible(keyInput.value, keyEdited)) {
          if (!entry?.hasKey) noteKey("请先输入 Key", true);
          return;
        }
        keySaveBtn.disabled = true;
        try {
          // 明文永不回显：只写入不读取；保存成功立即就地置掩码（不依赖刷新链）。
          // 成功静默（灰字清理 2026-10-31）：掩码回填 + 胶囊点亮即反馈。
          await setKey(activeVendorId, value);
          knownKeys.add(activeVendorId);
          keyEdited = false;
          keyInput.value = KEY_MASK;
          keyInput.placeholder = "";
          vendorPill.classList.add("vendor-switch__pill--configured");
          // 后台同步最新状态；失败不影响已显示的掩码态。状态可信时与保险库
          // 对账（P3-01）：权威已无 Key 的会话标记被撤销。
          const fresh = await getStatus().catch(() => null);
          if (fresh) {
            vendorStatus = fresh;
            pruneSessionKeyMarks(knownKeys, fresh);
          }
          renderVendorArea();
        } catch (err) {
          noteKey(`保存失败：${String(err).slice(0, 160)}`, true);
        } finally {
          keySaveBtn.disabled = false;
        }
      })();
    });
    function noteKey(text: string, isError = false): void {
      keyNote.textContent = text;
      keyNote.classList.toggle("launch-key__note--error", isError);
    }

    function activeEntry(): VendorEntry | undefined {
      const view = deriveVendorView(vendorStatus);
      return view.entries.find((e) => e.id === activeVendorId) ?? view.entries[0];
    }

    /**
     * 读取当前实例的默认模型（agent-default-model 的 provider + model，实例注入）。
     * 两个字段要成对读：只写 model 时 DSH 会拿 DeepSeek 官方通道发这个 id（R7-1）。
     */
    async function refreshModelState(): Promise<void> {
      modelInput.disabled = !selectedId;
      if (!selectedId) {
        currentModel = "";
        currentProvider = "";
        modelInput.value = "";
        modelInput.placeholder = "默认模型（先在下载页创建实例）";
        return;
      }
      modelInput.placeholder = "默认模型";
      try {
        const config = await getInjectOverride(selectedId);
        const ns = config.settings["agent-default-model"] as Record<string, unknown> | undefined;
        currentModel = typeof ns?.model === "string" ? ns.model : "";
        currentProvider = typeof ns?.provider === "string" ? ns.provider : "";
      } catch {
        // 读不到注入覆盖就按「无覆盖」呈现：输入框留空走 placeholder（默认模型），
        // 与真的没有覆盖视觉一致；危害限于此字段显示，启动链路不依赖它。
        currentModel = "";
        currentProvider = "";
      }
      modelInput.value = currentModel;
      // 切厂商联动挂在本次读取完成之后（followPendingVendor 消费点）：先读实例
      // 现状、再判定归属、再写——读写同一条异步序列，不会互相覆盖。
      if (followPendingVendor !== null) {
        const vendorId = followPendingVendor;
        followPendingVendor = null;
        await followVendorDefault(vendorId);
        return;
      }
      await noteModelOwner();
    }

    /** 路由键缓存：厂商 → 提供方 id（null = 落不了路由）；判定由后端单点给出。 */
    const routeKeyCache = new Map<string, string | null>();

    async function routeKeyOf(vendorId: string): Promise<string | null> {
      if (routeKeyCache.has(vendorId)) return routeKeyCache.get(vendorId) ?? null;
      let key: string | null = null;
      try {
        key = await providerRouteKey(vendorId);
      } catch {
        key = null;
      }
      routeKeyCache.set(vendorId, key);
      return key;
    }

    /**
     * 默认模型归属提示（R7-1）：模型与提供方是一对。只写 model 的实例（老版本
     * 启动器写下的）会被 DSH 当成 DeepSeek 官方模型发出去。切厂商后默认模型
     * **无从联动**（新厂商还没有已添加模型）时也要说清楚，别让用户以为换了
     * 厂商就换了模型——有模型时已自动联动（followVendorDefault），不再出这句。
     */
    async function noteModelOwner(): Promise<void> {
      if (!currentModel) return;
      if (!currentProvider) {
        noteKey(`模型「${currentModel}」缺少提供方标记`, true);
        return;
      }
      const activeKey = await routeKeyOf(activeVendorId);
      if (activeKey !== null && currentProvider === activeKey) return;
      const owner = deriveVendorView(vendorStatus).entries.find(
        (entry) => routeKeyCache.get(entry.id) === currentProvider,
      );
      if (owner) noteKey(`当前默认模型「${currentModel}」属于「${owner.label}」`);
    }

    /**
     * 切厂商后默认模型联动（2026-11-01 需求方报障「换提供商输入框不跟着变」，
     * 推翻本文件旧口径「只提示不联动」）：实例默认模型仍属于旧厂商时，自动
     * 应用新厂商的第一个已添加模型（applyModel 成对写 provider+model，启动即
     * 用新厂商——FR-21「选定的厂商与 model ID 写入实例默认模型」）。新厂商
     * 还没有模型时无从联动，保留原值并交归属提示说清。只有显式点击厂商行才
     * 置位 followPendingVendor，页面初始渲染不触发（不得擅自改写实例默认模型）。
     */
    async function followVendorDefault(vendorId: string): Promise<void> {
      const activeKey = await routeKeyOf(vendorId);
      if (activeVendorId !== vendorId) return; // 用户已切走：这条链过期
      if (activeKey === null || currentProvider === activeKey) return;
      const models = launchModelsFor(vendorStatus, vendorId);
      if (models.length === 0) {
        await noteModelOwner();
        return;
      }
      await applyModel(models[0], vendorId);
    }

    function renderModelList(): void {
      modelList.replaceChildren();
      const models = launchModelsFor(vendorStatus, activeVendorId);
      if (models.length === 0) {
        modelList.append(
          el("p", { class: "model-select__empty" }, ["该厂商还没有模型——到「设置页 · 模型厂商」添加"]),
        );
        return;
      }
      // 同名模型可能同时属于多个厂商：只有提供方也对得上才算「当前默认」，否则
      // 这里会把别家的同名模型显示成已选中（引入 provider 后必须区分）。缓存尚未
      // 热起来（首帧竞态）──无从判断──沿用不抑制的旧行为，不闪一下「没选中」。
      const activeKey = routeKeyCache.get(activeVendorId);
      const sameVendor = currentProvider === "" || activeKey === undefined || activeKey === currentProvider;
      for (const model of models) {
        const row = el(
          "button",
          {
            type: "button",
            class: `model-select__item ${model === currentModel && sameVendor ? "model-select__item--active" : ""}`,
          },
          [model],
        );
        row.addEventListener("click", () => {
          void applyModel(model);
        });
        modelList.append(row);
      }
    }

    /**
     * 把所选模型记为该实例的默认模型：**provider + model 一起写**（R7-1）。
     *
     * `agent-default-model` 的字段是 `{ provider, model }`；只写 model 时 DSH 用它
     * 自己的组合基（`deepseek-official`）当提供方，于是「GLM 的模型 id 走 DeepSeek
     * 通道」——真机 `session/modelCatalog` 实测 `default` 就是
     * `{provider: deepseek-official, model: glm-4.6}`（证据：2026-09-12 QA 报告 T2）。
     *
     * 实例侧的 `settings.yaml` **不在运行中改**：启动器侧配置一律只在下一次启动
     * 生效，启动链统一再生实例文件（`llm_bridge::sync_instance`）。
     */
    async function applyModel(model: string, vendorId: string = activeVendorId): Promise<void> {
      if (!selectedId) return;
      modelInput.disabled = true;
      try {
        const provider = await routeKeyOf(vendorId);
        const config = await getInjectOverride(selectedId);
        const ns: Record<string, unknown> = {
          ...((config.settings["agent-default-model"] as Record<string, unknown> | undefined) ?? {}),
          model,
        };
        if (provider) {
          ns.provider = provider;
        } else {
          delete ns.provider;
        }
        config.settings["agent-default-model"] = ns;
        await setInjectOverride(selectedId, config);
        currentModel = model;
        currentProvider = provider ?? "";
        modelInput.value = model;
        closeFloatingList(modelList, modelInput);
        // 成功静默（2026-10-24 需求方指令「破坏界面的灰字」删——原成功句在自定义
        // 提供方上因内部 id「custom-provider」+ 长模型名折三行糊在输入框下）：
        // 可见反馈 = 下拉行高亮 + 输入框回填；不兼容/失败仍走红字。
        if (!provider) {
          noteKey("该模型接口形态不兼容，无法写入实例", true);
        }
      } catch (err) {
        noteKey(`保存失败：${String(err).slice(0, 160)}`, true);
      } finally {
        modelInput.disabled = false;
      }
    }

    modelInput.addEventListener("click", () => {
      if (!selectedId) return;
      toggleFloatingList(modelList, modelInput, launchArea);
    });

    function renderVendorList(): void {
      const view = deriveVendorView(vendorStatus);
      vendorList.replaceChildren();
      for (const entry of view.entries) {
        const stateText = entry.hasKey ? "Key 就绪" : "未配置 Key";
        const row = el(
          "button",
          {
            type: "button",
            class: `vendor-switch__item ${entry.id === activeVendorId ? "vendor-switch__item--active" : ""}`,
            title: stateText,
          },
          [
            el("span", {}, [entry.label]),
            // 行尾状态点（非徽章）：贴右收编整行宽度，替掉原「标签贴左 + 徽章贴右」
            // 的中部大空白；就绪 = CB3 实点（spec §8.2 成功态不用绿），未配置 = 空心。
            el("span", { class: "sr-only" }, [stateText]),
            el("span", {
              class: `vendor-switch__dot ${entry.hasKey ? "vendor-switch__dot--on" : ""}`,
              "aria-hidden": "true",
            }, []),
          ],
        );
        row.addEventListener("click", () => {
          activeVendorId = entry.id;
          followPendingVendor = entry.id; // 显式切厂商才联动默认模型
          renderVendorArea();
          closeFloatingList(vendorList, vendorPill);
        });
        vendorList.append(row);
      }
    }

    /** 一屏可见的列表行数（超出由鼠标滚轮步进自由滚动）。 */
    const FLOATING_ROWS = 3;

    /**
     * 浮动列表的高度上限：恒定 3 行（2026-10-03 需求方裁决「一次显示三个」）。
     * 曾经按「触发器下方到 floor 的空白」收缩，窗口矮时塌成 1 行——下拉本就
     * 该覆盖下方内容，不再按空白收缩；超出的行由滚轮步进滚动。
     */
    function capFloatingList(list: HTMLElement, _trigger: HTMLElement, _floor: HTMLElement): void {
      const style = getComputedStyle(list);
      const gap = Number.parseFloat(style.rowGap) || 0;
      const padding = (Number.parseFloat(style.paddingTop) || 0) + (Number.parseFloat(style.paddingBottom) || 0);
      const row = list.firstElementChild?.getBoundingClientRect().height ?? 0;
      const wanted = row > 0 ? FLOATING_ROWS * row + (FLOATING_ROWS - 1) * gap + padding : Infinity;
      list.style.maxHeight = `${wanted}px`;
    }

    /**
     * 浮层滚轮步进：一格平滑滚一行。原生滚轮一档 ≈100px ≈ 3-4 行，在只有
     * 3 行高的浮层里就是「1-3、4-6」整页卡跳（2026-10-03 需求方报障）——
     * 接管滚轮改按行步进，滚动条本身隐藏（CSS），观感即「正常滑动」。
     */
    function installWheelStep(list: HTMLElement): void {
      list.addEventListener(
        "wheel",
        (event) => {
          if (!list.classList.contains("is-open")) return;
          event.preventDefault();
          const style = getComputedStyle(list);
          const gap = Number.parseFloat(style.rowGap) || 0;
          const row = list.firstElementChild?.getBoundingClientRect().height ?? 0;
          const step = row > 0 ? row + gap : 24;
          list.scrollBy({ top: Math.sign(event.deltaY) * step, behavior: "smooth" });
        },
        { passive: false },
      );
    }
    installWheelStep(vendorList);
    installWheelStep(modelList);

    /** 展开/收起浮动列表（收起 = 摘掉 .is-open + 去掉浮层定位，回到隐藏态；
     *  R5：显隐走 class 过渡 100ms，不再 hidden 硬切）。 */
    function toggleFloatingList(list: HTMLElement, trigger: HTMLElement, floor: HTMLElement): void {
      if (!list.classList.contains("is-open")) {
        openFloatingList(list, trigger, floor);
      } else {
        closeFloatingList(list, trigger);
      }
    }

    function openFloatingList(list: HTMLElement, trigger: HTMLElement, floor: HTMLElement): void {
      // 先渲染内容再量高度：行高由真实行决定，空列表量不出上限。
      if (list === vendorList) renderVendorList();
      else renderModelList();
      list.classList.add("is-floating");
      capFloatingList(list, trigger, floor);
      list.classList.add("is-open");
      if (list === vendorList) pillChevron.classList.add("is-open");
    }

    function closeFloatingList(list: HTMLElement, trigger: HTMLElement): void {
      list.classList.remove("is-open");
      list.classList.remove("is-floating");
      list.style.maxHeight = "";
      if (list === vendorList) pillChevron.classList.remove("is-open");
      else trigger.blur();
    }

    /** 窗口尺寸变化时重算浮层上限与字幕对齐（两者都取决于当前可用空白）。 */
    function relayoutFloatingLists(): void {
      if (vendorList.classList.contains("is-open")) capFloatingList(vendorList, vendorPill, identityMain);
      if (modelList.classList.contains("is-open")) capFloatingList(modelList, modelInput, launchArea);
      if (starting) syncRoll();
    }

    /** 收起所有浮层（点击别处 / Esc）。 */
    function closeFloatingLists(): void {
      if (vendorList.classList.contains("is-open")) closeFloatingList(vendorList, vendorPill);
      if (modelList.classList.contains("is-open")) closeFloatingList(modelList, modelInput);
    }

    function renderVendorArea(): void {
      const active = activeEntry();
      if (!active) return;
      pillText.textContent = active.label;
      vendorPill.classList.toggle("vendor-switch__pill--unconfigured", !active.enabled);
      vendorPill.classList.toggle("vendor-switch__pill--configured", active.hasKey);
      pillChevron.hidden = !vendorFoldable();
      avatar.replaceChildren();
      if (active.icon) {
        // 厂商官方图标（public/vendor-logos/）；加载失败回退字母方块。
        avatar.className = "vendor-avatar vendor-avatar--icon";
        const iconImg = el("img", { src: active.icon, alt: "" });
        iconImg.addEventListener("error", () => {
          avatar.className = "vendor-avatar vendor-avatar--letter";
          avatar.replaceChildren(active.monogram);
        });
        avatar.append(iconImg);
      } else {
        avatar.className = "vendor-avatar vendor-avatar--letter";
        avatar.textContent = active.monogram;
      }
      // key 两态：有 key（保险库、环境变量或本会话刚保存）→ 掩码；无 → sk- 占位。
      const hasKey = active.hasKey || knownKeys.has(active.id);
      keyEdited = false; // 程序化置值不算用户编辑（P3-02）
      if (hasKey) {
        keyInput.value = KEY_MASK;
        keyInput.placeholder = "";
      } else {
        keyInput.value = "";
        keyInput.placeholder = "sk-";
      }
      // 模型选择框跟随厂商与实例。
      modelList.classList.remove("is-open");
      void refreshModelState();
      if (vendorFoldable()) renderVendorList();
    }

    // ===== 启动大按钮与副标题 =====
    const subLabel = el("span", { class: "launch-btn__sub" }, [SUB_NO_INSTANCE]);
    const launchBtn = el("button", { type: "button", class: "launch-btn" }, [
      el("span", { class: "launch-btn__label" }, ["启动"]),
      subLabel,
    ]);

    function nameOf(instanceId: string): string {
      return instances.find((inst) => inst.id === instanceId)?.name ?? instanceId;
    }

    function idleSubtitle(): string {
      if (!selectedId) return SUB_NO_INSTANCE;
      const inst = instances.find((candidate) => candidate.id === selectedId);
      if (!inst) return SUB_NO_INSTANCE;
      // 需求方定稿：启动成功即恢复纯「启动」态——副标题只显实例名，
      // 不常驻「运行中」，避免按钮看似被锁、影响多开。
      return inst.name;
    }

    function setSub(text: string, warn = false): void {
      subLabel.textContent = text;
      subLabel.classList.toggle("is-warn", warn);
    }

    async function refreshRunning(): Promise<void> {
      try {
        const statuses = await listRunning();
        runningMap = new Map(statuses.map((s) => [s.instanceId, s]));
        runningUnknown = false;
      } catch (err) {
        // 读不到运行状态时不能装作「都没在运行」：实例其实在跑却被显示成未
        // 运行，点启动会撞已占端口、报一个与真实原因无关的错。置标志让副标题
        // 如实说出「按未运行呈现」，并把原因留在控制台。
        runningMap = new Map();
        runningUnknown = true;
        console.error("运行状态读取失败", err);
      }
      renderCluster();
    }

    launchBtn.addEventListener("click", () => {
      void (async () => {
        if (!selectedId) {
          setSub(SUB_NO_INSTANCE, true);
          return;
        }
        // 启动链进行中不重复发起（对抗审查 R1 P1-02）：双击/连点会被这里的
        // 早退挡住；后端另有启动占位锁兜底，两层防线互相独立。
        if (starting) return;
        // 每次点启动都是一次**独立的完整启动**：实例仍在跑时由后端先停旧进程
        // 再按原钉定端口拉起（运行期注入的环境变量——厂商 Key、基址覆盖——
        // 因此每次都刷新）。前端不做"运行中只开网页"的决策，也不预查进程状态。
        // 重新启动同一实例：清掉旧缓冲（详细日志/简化诊断）。
        consoleLines.delete(selectedId);
        diagCards.delete(selectedId);
        errorModal = null;
        renderErrorModal();
        resetLaunchAnimation();
        starting = true;
        cancelRequested = false;
        launchRun += 1;
        const run = launchRun;
        stageText = "正在校验运行环境…";
        renderCluster();
        renderLaunchPane();
        // 先让浏览器画一帧：否则同步段里的阶段文案会被紧接着的渲染覆盖，
        // 「校验中」这一格永远看不见（旧实现即此坑）。
        await new Promise<void>((resolve) => requestAnimationFrame(() => resolve()));
        try {
          const result = await launchInstance(selectedId);
          // 进程已拉起：收掉「正在拉起实例进程…」，把校验结论逐条落成字幕行
          // （等级照后端原样映射：Error→未通过、Warning→提示）。
          settleLaunchStep();
          for (const item of result.report.items) {
            launchSteps.push({
              text: item.message,
              level: stepLevelOf(item.level),
              action: item.action ?? undefined,
            });
          }
          for (const step of reconcileSteps(result.reconcile)) launchSteps.push(step);
          renderLaunchSteps();
          await refreshRunning();
          // 进程已拉起但网页未必可访问：这是整条链里最长的一段（DSH 要装载插件
          // 层、起 web 服务），字幕里必须有一行真实反映它在等什么——否则观感
          // 就是「勾完就只剩进度条」。就绪/超时事件到达时收尾这一步。
          pushLaunchStep("等待 DSH 装载插件层并开放网页…");
          progressTarget = Math.max(progressTarget, 70);
          tickProgress();
          setSub(`已启动（端口 ${result.port}）—— 等待网页就绪后自动打开浏览器…`);
        } catch (err) {
          if (run !== launchRun) return; // 上一次启动的迟到答复：丢弃
          // 本次启动已经收过尾就别再处置（取消先回来、就绪、异常退出都算）：
          // 收尾那一步已经如实说过结果，这里再弹一次「启动失败」是自相矛盾。
          if (!starting) return;
          // 取消收尾的两个来源（R2 P2-01）：①前端自己点的取消（cancelRequested）；
          // ②用户在别处点了「停止」——后端 instance_stop 登记取消令牌，启动链
          // 以「已取消」收场，同样静默收尾，不弹「启动失败」。
          if (cancelRequested || String(err) === "已取消") {
            // 取消可能与「校验拦截」撞在同一瞬间：按用户取消处置，但拦截结论
            // 不能丢——写进日志缓冲（下次启动照常还会拦一次）。
            const blocked = parseBlockedReport(err);
            if (blocked) {
              const lines = linesOf(selectedId);
              for (const item of blocked.items) lines.push(`[${item.level}] ${item.message}`);
            }
            await finishCancelled();
            return;
          }
          starting = false;
          renderLaunchPane();
          const blocked = parseBlockedReport(err);
          if (blocked) {
            const errors = blocked.items.filter((i) => i.level === "error");
            // 报告全量进日志缓冲：弹窗只列错误，警告项仍可在「日志」里看到。
            const lines = linesOf(selectedId);
            for (const item of blocked.items) lines.push(`[${item.level}] ${item.message}`);
            settleLaunchStep("note", "校验拦截：已中止启动（见弹窗）");
            showErrorModal(
              "启动前校验未通过",
              errors.map((i) => i.message),
              true,
              [...new Set(errors.map((i) => i.action).filter((a): a is string => Boolean(a)))],
            );
            setSub("未通过校验（见弹窗）", true);
            return;
          }
          settleLaunchStep("note", "启动失败：已中止（见弹窗）");
          showErrorModal("启动失败", [String(err)], false);
          setSub("启动失败（见弹窗）", true);
        }
      })();
    });

    /** 强制启动（弹窗内按钮触发；左栏保持纯净）。 */
    function runForceLaunch(): void {
      if (!selectedId) return;
      resetLaunchAnimation();
      starting = true;
      cancelRequested = false;
      launchRun += 1;
      const run = launchRun;
      stageText = "正在强制启动…";
      renderCluster();
      renderLaunchPane();
      void (async () => {
        try {
          const result = await launchInstance(selectedId, true);
          settleLaunchStep();
          for (const item of result.report.items) {
            launchSteps.push({
              text: item.message,
              level: stepLevelOf(item.level),
              action: item.action ?? undefined,
            });
          }
          renderLaunchSteps();
          await refreshRunning();
          renderLaunchPane();
          pushLaunchStep("等待 DSH 装载插件层并开放网页…");
          progressTarget = Math.max(progressTarget, 70);
          tickProgress();
          setSub(`已启动（端口 ${result.port}）—— 等待网页就绪后自动打开浏览器…`);
        } catch (err) {
          if (run !== launchRun) return; // 上一次启动的迟到答复：丢弃
          if (!starting) return; // 本次启动已收过尾（取消/就绪/退出）：不重复弹窗
          if (cancelRequested) {
            await finishCancelled();
            return;
          }
          starting = false;
          renderLaunchPane();
          settleLaunchStep("note", "强制启动失败：已中止（见弹窗）");
          showErrorModal("强制启动失败", [String(err)], false);
          setSub("强制启动失败（见弹窗）", true);
        }
      })();
    }

    /**
     * 取消本次启动：登记取消标记 → 通知后端（登记令牌 + 停掉已拉起的进程）→
     * 由启动链的答复或本函数自己收尾。
     *
     * 不在这里立刻切回静止页「装作没事」：先等后端把进程真的停掉（命令返回即
     * 停完并等到端口释放），再回静止页并如实写副标题。取消只存在于加载页，
     * 与旧定稿「运行中不设停止入口」不冲突。
     */
    function requestCancel(): void {
      if (!starting || !selectedId) return;
      const id = selectedId;
      cancelRequested = true;
      // 后端停进程会抑制那条 instance-exit；这里仍置位，兜住「取消瞬间进程自己
      // 先崩了」的情形——那是用户主动操作的结果，不该弹「异常退出」。
      stoppingByUser = id;
      cancelBtn.disabled = true;
      // 进行中的那一行**不能**按「完成了」收尾：取消意味着它被打断了，写成
      // 「DSH 已装载插件层并开放网页」就是把没发生的事说成发生（实测踩到）。
      const active = launchSteps[launchSteps.length - 1];
      settleLaunchStep("info", active?.active ? `${active.text}（已取消，未完成）` : "已取消启动");
      pushLaunchStep("正在取消启动…", "info");
      void (async () => {
        try {
          await cancelLaunch(id);
          await finishCancelled();
        } catch (err) {
          if (!starting) return;
          // 取消失败要如实说：进程可能还在跑。给出真实原因，别让按钮变成没反应。
          settleLaunchStep("note", `取消失败：${String(err)}`);
          cancelBtn.disabled = false;
          cancelRequested = false;
          stoppingByUser = null;
        }
      })();
    }

    /** 取消收尾：回静止页 + 如实副标题（幂等——答复与本函数都可能先到）。 */
    async function finishCancelled(): Promise<void> {
      if (!starting) return;
      const id = selectedId;
      if (stoppingByUser === id) stoppingByUser = null;
      cancelRequested = false;
      starting = false;
      settleLaunchStep("info", "已取消启动（实例已停止）");
      // 进程刚停掉：刷新运行表再写副标题，否则副标题会按旧状态说「未在运行」。
      await refreshRunning();
      renderCluster();
      renderLaunchPane();
      setSub(SUB_CANCELLED);
      setTimeout(() => setSub(idleSubtitle()), 4000);
    }

    const selectBtn = el("button", { type: "button", class: "btn btn--line" }, ["版本选择"]);
    selectBtn.addEventListener("click", () => {
      if (selectedId) setSelectedInstance(selectedId);
      navigate("instance-select");
    });
    const configBtn = el("button", { type: "button", class: "btn btn--line" }, ["版本设置"]);
    configBtn.addEventListener("click", () => {
      if (selectedId) setSelectedInstance(selectedId);
      navigate("instance-settings");
    });
    // 功能隐藏（FR-27）：版本管理两钮可被隐藏（PCL「版本管理」）；F12 临时显示
    // 即时生效（偏好变更事件），监听走 eventDisposers 切页清理。
    const syncVersionBtns = (): void => {
      const hide = versionButtonsHidden();
      selectBtn.hidden = hide;
      configBtn.hidden = hide;
    };
    syncVersionBtns();
    const onUiPrefs = (): void => syncVersionBtns();
    window.addEventListener(UI_PREFS_EVENT, onUiPrefs);
    eventDisposers.push(() => window.removeEventListener(UI_PREFS_EVENT, onUiPrefs));

    // ===== 输出与诊断事件（只进缓冲，主页从不直接渲染）=====
    function linesOf(instanceId: string): string[] {
      let lines = consoleLines.get(instanceId);
      if (!lines) {
        lines = [];
        consoleLines.set(instanceId, lines);
      }
      return lines;
    }

    function cardsOf(instanceId: string): DiagnosticCard[] {
      let cards = diagCards.get(instanceId);
      if (!cards) {
        cards = [];
        diagCards.set(instanceId, cards);
      }
      return cards;
    }

    function showErrorModal(
      title: string,
      lines: string[],
      canForce = false,
      actions: string[] = [],
    ): void {
      errorModal = { title, lines: lines.slice(0, CARD_LINES_LIMIT), canForce, actions };
      renderErrorModal();
    }

    trackLaunchEvent(
      onConsoleLine((line: ConsoleLine) => {
        const lines = linesOf(line.instanceId);
        lines.push(line.line);
        while (lines.length > CONSOLE_LIMIT) lines.shift();
      }),
    );

    trackLaunchEvent(
      onDiagnostic(({ instanceId, diagnostic }: { instanceId: string; diagnostic: DiagnosticCard }) => {
        const cards = cardsOf(instanceId);
        cards.unshift(diagnostic);
        while (cards.length > CARD_LINES_LIMIT) cards.pop();
        // 只有错误才打扰用户（警告进详细日志，不打扰）。
        if (diagnostic.level === "error") {
          showErrorModal(`${nameOf(instanceId)} 运行时诊断`, cards.map(cardLine), false);
        }
      }),
    );

    trackLaunchEvent(
      onExit((notice) => {
        void (async () => {
          const cards = cardsOf(notice.instanceId);
          const byUser = stoppingByUser === notice.instanceId;
          const cancelled = cancelRequested && notice.instanceId === selectedId;
          stoppingByUser = null;
          // 加载中进程就退出（例如启动即崩）：动画必须收，否则一直转到超时。
          if (starting && notice.instanceId === selectedId) {
            if (byUser || cancelled) {
              // 用户主动取消导致的退出：按「已取消」收尾，不报成异常退出
              // （后端取消时本就抑制这条 exit，这里兜住抑制窗口外的极端时序）。
              await finishCancelled();
            } else {
              starting = false;
              settleLaunchStep(
                "note",
                `实例进程已退出（exit ${notice.code}）——启动未完成`,
              );
              renderCluster();
              renderLaunchPane();
            }
          }
          if (byUser || cancelled) {
            // 取消引起的退出不再写「已退出（exit N）」副标题：与「已取消启动
            // （实例已停止）」叠着说是自相矛盾的两句话。
            await refreshRunning();
            renderCluster();
            return;
          }
          if (notice.code !== 0 && !byUser) {
            showErrorModal(
              `${nameOf(notice.instanceId)} 已异常退出（exit ${notice.code}）`,
              cards.map(cardLine),
              false,
            );
          }
          setSub(`${nameOf(notice.instanceId)} 已退出（exit ${notice.code}）`, notice.code !== 0);
          setTimeout(() => setSub(idleSubtitle()), 4000);
          await refreshRunning();
          renderCluster();
        })();
      }),
    );

    /** 简化日志的一行：标题 + 摘要（悬浮卡用）。 */
    function cardLine(card: DiagnosticCard): string {
      const head = `[${card.level === "error" ? "错误" : "警告"}] ${card.title}`;
      return card.detail ? `${head} — ${card.detail}` : head;
    }

    // 网页就绪（R6）：后端拿到 `dsh web` 打印的 URL（新版本，带 token）或探到
    // 端口可连（旧版本）才发——此时才开浏览器、进度推到 100%；加载页停留一拍
    // 让 100% 可见，再切回静止页（启动器不呈现「运行中」）。
    trackLaunchEvent(
      onWebReady(({ instanceId, port, url }) => {
        if (instanceId !== selectedId) return;
        // 已取消：绝不因为「后端恰好也拉起了网页」而开浏览器（取消就是取消）。
        if (cancelRequested) return;
        // 带 token 的 URL 优先（新版 DSH 的鉴权栅栏只认它）；旧版本没打印则回落
        // 裸端口 URL——那时裸 URL 正是正确入口。
        const target = webEntryUrl({ webUrl: url ?? null, port });
        if (target === null) return;
        void openInstanceWeb(target);
        // 启动器可见性（FR-28）：网页就绪即「游戏启动」，按设置施加窗口动作
        // （PCL McLaunchEnd 同触发点；保持不变 = 无操作）。
        void applyVisibilityOnLaunch();
        progressTarget = 100;
        stageText = "DSH 网页已就绪";
        settleLaunchStep("ok", "DSH 网页已就绪，已打开浏览器");
        tickProgress();
        setSub(`已启动（端口 ${port}）`);
        if (starting) {
          setTimeout(() => {
            starting = false;
            renderCluster();
            renderLaunchPane();
            setSub(idleSubtitle());
          }, 700);
        } else {
          setTimeout(() => setSub(idleSubtitle()), 4000);
        }
      }),
    );
    trackLaunchEvent(
      onWebTimeout(({ instanceId }) => {
        if (instanceId !== selectedId) return;
        if (cancelRequested) return; // 已取消：超时与用户无关，不再报一次
        if (!runningMap.get(instanceId)) return; // 进程已退出：exit 事件已另行告知
        settleLaunchStep("note", "网页未在 3 分钟内就绪——实例可能仍在初始化，可稍后再点「启动」");
        starting = false;
        renderCluster();
        renderLaunchPane();
        setSub("网页就绪超时——实例可能仍在初始化，可稍后点「启动」再开", true);
      }),
    );

    // 启动进度：后端在「校验 → 拉起进程 → 等待网页就绪 → 就绪」各阶段推真实
    // 百分比；百分比只增不减。**每个新阶段文案都以「进行中」进入字幕序列**，下一个
    // 阶段到来时才收成结论——阶段推进看得见，而不是一上来全部打勾。
    trackLaunchEvent(
      onLaunchProgress(({ instanceId, percent, stage }) => {
        if (instanceId !== selectedId || !starting) return;
        progressTarget = Math.max(progressTarget, percent);
        stageText = stage;
        if (percent >= 100) {
          // 100% 是收尾信号（就绪事件紧随其后并写好结论文案）：这里只把当前
          // 进行中的那一步收掉，不再起新行——否则末尾会留一个永远转不完的
          // 进行中行（就绪之后再没有事件来收它）。
          settleLaunchStep();
        } else {
          const last = launchSteps[launchSteps.length - 1];
          // 同一阶段会反复推（等待期每 500ms 一次）——只在文案变化时新增一行。
          if (!last || last.text !== stage) pushLaunchStep(stage);
        }
        tickProgress();
      }),
    );

    // ===== 按钮簇：运行中与静止态外观完全一致（PCL2 式，需求方定稿）；
    // 仅启动加载阶段禁用按钮、副标题同步阶段。启动器不提供运行控制入口
    // （启动 = 打开网页，一键到底）。副标题在加载页不可见（静止页整体隐藏），
    // 阶段信息由加载页的字幕承载。
    function renderCluster(): void {
      if (starting) {
        launchBtn.disabled = true;
        setSub(stageText || "正在启动…");
      } else {
        launchBtn.disabled = false;
        // 运行状态读取失败优先说清楚（见 refreshRunning）：否则副标题只显实例
        // 名，用户无从知道「看起来没在运行」其实是没读到。
        setSub(runningUnknown ? SUB_RUNNING_UNKNOWN : idleSubtitle(), runningUnknown);
      }
    }

    // ===== 左栏加载页（静止页隐藏、加载页显示；就绪/失败/取消后切回静止页）=====
    /** 百分比缓动：每帧向目标逼近——后端进度是离散的，直接跳数会顿。 */
    function tickProgress(): void {
      if (progressRaf !== null) return;
      const step = (): void => {
        const gap = progressTarget - progressShown;
        if (Math.abs(gap) < 0.2) {
          progressShown = progressTarget;
        } else {
          progressShown += gap * 0.12 + Math.sign(gap) * 0.15;
        }
        const shown = Math.max(0, Math.min(100, Math.round(progressShown)));
        loadingPercent.textContent = `${shown}%`;
        loadingFill.style.width = `${shown}%`;
        if (progressShown === progressTarget && !starting) {
          progressRaf = null;
          return;
        }
        progressRaf = requestAnimationFrame(step);
      };
      progressRaf = requestAnimationFrame(step);
    }

    /**
     * 加载页字幕序列：一行 = 一个**真实阶段或一条真实结论**，随进度事件推进。
     *
     * 需求方 2026-09-10 反馈「啥也没检测就是四个全打上勾，然后只加载进度条」
     * —— 病灶是清单只在 `launchInstance` 返回时一次性 push（4 条结论同时变），
     * 而**耗时最长的那一段（DSH 装载插件层、开放网页）在清单里根本没有条目**，
     * 于是观感就是「一上来全勾 + 之后纯进度条」。现在：
     * 每个阶段先以「进行中」（旋转图标）出现，下一阶段到来时才收成结论；
     * 进程拉起后追加「等待 DSH 装载插件层并开放网页…」这一行真实长阶段，
     * 它一直转到网页就绪（或超时/失败）才收尾。
     */
    function pushLaunchStep(text: string, level: StepLevel = "ok"): void {
      // 上一行被这一行顶掉 = 那一阶段已经过去：措辞同时收成完成态
      // （「正在校验运行环境…」留在一行已完成项里读起来是错的）。
      for (const step of launchSteps) {
        step.active = false;
        step.text = settledStageText(step.text);
      }
      launchSteps.push({ text, level, active: true });
      renderLaunchSteps();
    }

    /** 收尾当前进行中的那一步（旋转图标换成结论图标，进行时措辞换成完成态）。 */
    function settleLaunchStep(level: StepLevel = "ok", text?: string): void {
      const last = launchSteps[launchSteps.length - 1];
      if (!last) return;
      last.active = false;
      last.level = level;
      last.text = text ?? settledStageText(last.text);
      renderLaunchSteps();
    }

    /**
     * 把本次启动的实例配置对账结果转成字幕行。
     *
     * 恒有一行「实例配置核对」报当前构成（插件/Skill/外部提供商），让"启动器里
     * 的记录"和"实例里真实有什么"始终对得上；与上次启动相比有增减时再逐条列出，
     * 这就是「网页里装的东西下次启动会刷新进启动器」的可见出口。
     */
    function reconcileSteps(reconcile: Reconcile | undefined): { text: string; level: StepLevel }[] {
      if (!reconcile) return [];
      const steps: { text: string; level: StepLevel }[] = [];
      const summary =
        `实例配置核对：插件 ${reconcile.pluginsTotal} 个 / Skill ${reconcile.skillsTotal} 个` +
        ` / 外部提供商 ${reconcile.externalRoutesTotal} 个`;
      const changes: { added: string[]; removed: string[]; label: string }[] = [
        { added: reconcile.pluginsAdded, removed: reconcile.pluginsRemoved, label: "插件" },
        { added: reconcile.skillsAdded, removed: reconcile.skillsRemoved, label: "Skill" },
        {
          added: reconcile.externalRoutesAdded,
          removed: reconcile.externalRoutesRemoved,
          label: "外部模型提供商",
        },
      ];
      const changed = changes.some((c) => c.added.length > 0 || c.removed.length > 0);
      if (!reconcile.baseline) {
        steps.push({ text: `${summary}（首次启动，本次作为基线）`, level: "ok" });
      } else if (changed) {
        steps.push({ text: `${summary}——检出变更`, level: "ok" });
        for (const c of changes) {
          if (c.added.length > 0) {
            steps.push({ text: `${c.label} 新增 ${c.added.length} 个：${c.added.join("、")}`, level: "ok" });
          }
          if (c.removed.length > 0) {
            steps.push({ text: `${c.label} 移除 ${c.removed.length} 个：${c.removed.join("、")}`, level: "note" });
          }
        }
      } else {
        steps.push({ text: `${summary}（与上次启动一致）`, level: "ok" });
      }
      if (reconcile.note) steps.push({ text: reconcile.note, level: "note" });
      return steps;
    }

    /**
     * 字幕行的状态图标：内联 SVG，**不用 ✓/· 之类的终端字符**——这一屏是给人看的
     * 字幕，不是日志流。进行中 = 转圈，ok = 对勾，info = 说明点，note = 未通过。
     */
    function stepIcon(active: boolean, level: StepLevel): HTMLElement {
      const paths = {
        active: '<svg viewBox="0 0 16 16"><circle cx="8" cy="8" r="6.2" class="is-arc" /></svg>',
        ok: '<svg viewBox="0 0 16 16"><circle cx="8" cy="8" r="7" /><path d="M5 8.3l2.1 2.1L11 6.4" /></svg>',
        info: '<svg viewBox="0 0 16 16"><circle cx="8" cy="8" r="7" /><path d="M8 7.2v4.2" /><path d="M8 4.7v.01" /></svg>',
        note: '<svg viewBox="0 0 16 16"><circle cx="8" cy="8" r="7" /><path d="M5.4 5.4l5.2 5.2" /><path d="M10.6 5.4l-5.2 5.2" /></svg>',
      };
      const icon = el("span", { class: "launch-loading__step-icon", "aria-hidden": "true" });
      icon.innerHTML = paths[active ? "active" : level];
      return icon;
    }

    /** 字幕序列渲染：**固定三行窗口**（需求方 2026-10-24 定稿口径）——只保留
     *  最近 3 条：最新一条在下（小字进入）、上一条居中（放大）、上上条置顶（小字）；
     *  第 4 条出现时整体上移一行、最旧一条消失。窗口钉在鲸鱼图标正下方
     *  （CSS 绝对定位），不贴底、不占满栏。 */
    function renderLaunchSteps(): void {
      const window3 = launchSteps.slice(-3);
      // 位置类按「距最新」取：最新 → --new（下，小），上一条 → --cur（中，大），
      // 更早 → --old（上，小）。不足三条时从下往上缺省（单条 = --new）。
      const posClass = (fromEnd: number): string =>
        fromEnd === 0 ? "new" : fromEnd === 1 ? "cur" : "old";
      loadingTrack.replaceChildren(
        ...window3.map((step, index) => {
          const fromEnd = window3.length - 1 - index;
          const children: HTMLElement[] = [
            stepIcon(Boolean(step.active), step.level),
            el("span", { class: "launch-loading__step-text" }, [step.text]),
          ];
          // 带路由提示的条目（R7-3）：把「去设置/去下载页」做成行内按钮，新手不必
          // 自己找入口——后端只说意图（action），页面 id 归前端。
          const pageId = step.action ? ACTION_PAGES[step.action] : undefined;
          if (pageId) {
            const go = el("button", { type: "button", class: "launch-loading__step-go" }, [
              ACTION_LABELS[step.action!] ?? "去处理",
            ]);
            go.addEventListener("click", () => navigate(pageId));
            children.push(go);
          }
          return el(
            "li",
            {
              class: `launch-loading__step launch-loading__step--${step.active ? "active" : step.level} launch-loading__step--${posClass(fromEnd)}`,
            },
            children,
          );
        }),
      );
      syncRoll();
    }

    /**
     * 字幕滚动：三行窗口的重渲染伴随一次性「上移入列」动画（新行从下一行高度
     * 滑入、整体升一行，最旧的随之出窗）。动画靠重触发 class 实现，无累计位移。
     */
    function syncRoll(): void {
      loadingTrack.classList.remove("is-entering");
      void loadingTrack.offsetWidth; // 强制 reflow，让动画类可重复触发
      loadingTrack.classList.add("is-entering");
    }

    /** 每次点启动重置加载页：百分比归零、字幕清空、取消按钮重新可用。 */
    function resetLaunchAnimation(): void {
      progressTarget = 0;
      progressShown = 0;
      launchSteps.length = 0;
      renderLaunchSteps();
      loadingPercent.textContent = "0%";
      loadingFill.style.width = "0%";
      cancelBtn.disabled = false;
    }

    /**
     * 左栏两个互斥视图：静止页（厂商/账户/启动簇）与加载页（动画/进度/字幕）。
     * 加载页里没有厂商胶囊、头像、Key、模型、启动按钮与版本入口——那是另一屏的事。
     */
    function renderLaunchPane(): void {
      idlePane.hidden = starting;
      loadingPane.hidden = !starting;
      if (starting) {
        tickProgress();
        syncRoll();
      }
    }

    // ===== 二级悬浮卡（报错才弹出；主页平时不可见）=====
    function renderErrorModal(): void {
      const overlay = document.getElementById("error-modal");
      if (!overlay) return;
      if (!errorModal) {
        overlay.hidden = true;
        return;
      }
      const firstShow = overlay.hidden;
      overlay.hidden = false;
      overlay.querySelector(".error-modal__title")!.textContent = errorModal.title;
      const body = overlay.querySelector(".error-modal__body")!;
      body.replaceChildren(...errorModal.lines.map((line) => el("div", { class: "error-modal__line" }, [line])));
      // G8 延伸设计（spec §6「错误详情」行）：详细信息折叠 + 复制。errorModal.lines
      // 就是该错误的全部文本来源（诊断卡/退出原因），折叠区与复制共用同一份。
      const detailText = errorModal.lines.join("\n");
      const copyBtn = el("button", { type: "button", class: "btn btn--muted" }, ["复制"]);
      copyBtn.addEventListener("click", (event) => {
        // 复制键在 summary 行内：不触发折叠开合（summary 点击的默认行为）。
        event.preventDefault();
        event.stopPropagation();
        navigator.clipboard?.writeText(detailText).then(
          () => showHint("已复制错误信息"),
          (err) => console.error("复制错误信息失败", err),
        );
      });
      body.append(
        el("details", { class: "error-detail" }, [
          el("summary", {}, ["详细信息", copyBtn]),
          el("pre", { class: "error-detail__body" }, [detailText]),
        ]),
      );
      const foot = overlay.querySelector(".error-modal__btns")!;
      foot.replaceChildren();
      if (errorModal.canForce) {
        const force = el("button", { type: "button", class: "btn btn--danger" }, ["强制启动（已了解风险）"]);
        force.addEventListener("click", () => {
          errorModal = null;
          renderErrorModal();
          runForceLaunch();
        });
        foot.append(force);
      }
      const hasLog = selectedId ? (consoleLines.get(selectedId)?.length ?? 0) > 0 : false;
      for (const action of errorModal.actions ?? []) {
        const pageId = ACTION_PAGES[action];
        if (!pageId) continue;
        const btn = el("button", { type: "button", class: "btn btn--line" }, [
          ACTION_LABELS[action] ?? "去处理",
        ]);
        btn.addEventListener("click", () => {
          errorModal = null;
          renderErrorModal();
          navigate(pageId);
        });
        foot.append(btn);
      }
      if (hasLog) {
        const logBtn = el("button", { type: "button", class: "btn btn--line" }, ["日志"]);
        logBtn.addEventListener("click", () => {
          logModalId = selectedId;
          renderLogModal();
        });
        foot.append(logBtn);
      }
      const ok = el("button", { type: "button", class: "btn btn--line" }, ["确定"]);
      ok.addEventListener("click", () => {
        errorModal = null;
        renderErrorModal();
      });
      foot.append(ok);
      // 弹窗入场（spec §5 MyMsg：Y40→0 300ms OutBack）——只在由隐转现那一次播。
      if (firstShow) enterMsg(overlay.querySelector<HTMLElement>(".error-modal")!);
    }

    function renderLogModal(): void {
      if (!logModalId) {
        logModal.hidden = true;
        return;
      }
      const firstShow = logModal.hidden;
      const lines = consoleLines.get(logModalId) ?? [];
      const cards = (diagCards.get(logModalId) ?? []).map(cardLine);
      logModalTitle.textContent = `运行日志 — ${nameOf(logModalId)}`;
      logModalBody.replaceChildren(
        ...(lines.length === 0 && cards.length === 0
          ? [el("p", { class: "log-modal__empty" }, ["（暂无输出——实例运行后的输出会出现在这里）"])]
          : lines.map((line) => el("div", { class: "log-modal__line" }, [line]))),
        ...(cards.length > 0
          ? [el("div", { class: "log-modal__diag-head" }, ["—— 诊断摘要 ——"]), ...cards.map((c) => el("div", { class: "log-modal__line log-modal__line--diag" }, [c]))]
          : []),
      );
      logModalBody.scrollTop = logModalBody.scrollHeight;
      logModal.hidden = false;
      if (firstShow) enterMsg(logModal.querySelector<HTMLElement>(".log-modal__card")!);
    }

    // ===== 初始化 =====
    void (async () => {
      let initError: string | null = null;
      try {
        await refreshRunning();
        try {
          instances = await listInstances();
        } catch (err) {
          initError = String(err);
        }
        if (!selectedId) {
          // FR-10：收藏（= 置顶）优先于最近使用。
          selectedId = sortInstancesForLaunch(instances)[0]?.id ?? null;
        }
        // FR-20/21：注入完整厂商目录（内置 11 类 + 自定义），胶囊随启用厂商扩容。
        try {
          const view = await fetchProvidersView();
          setVendorCatalog([
            ...view.catalog.map((e) => ({
              id: e.id,
              label: e.label,
              icon: providerIconPath(e.id),
              monogram: e.label.charAt(0) || "?",
            })),
            ...view.custom.map((c) => ({
              id: c.id,
              label: c.name,
              monogram: c.name.charAt(0) || "?",
            })),
          ]);
        } catch (err) {
          // 目录拉取失败不阻塞页面：回落内置 deepseek 目录；但回落必须可见——
          // 静默吞掉会让全部自定义/内置厂商从启动页消失而无人知晓（2026-10-03 排查）。
          console.error("厂商目录拉取失败", err);
          showHint("模型厂商目录拉取失败——启动页暂只显示默认厂商");
        }
        try {
          vendorStatus = await getStatus();
          // P3-01：会话掩码标记与保险库对账——权威已无 Key 的标记撤销。
          pruneSessionKeyMarks(knownKeys, vendorStatus);
          activeVendorId = deriveVendorView(vendorStatus).activeId;
        } catch {
          vendorStatus = null; // 读不到保险库时按未配置占位呈现，不阻塞页面
        }
      } catch (err) {
        // 意外异常不允许静默：曾因排序比较器抛 TypeError 且无 catch，
        // 整页落回默认态（有实例显示「尚未创建实例」）而无人知晓。
        initError = String(err);
        console.error("启动页初始化失败", err);
      } finally {
        // 无论上面哪一步被挂起或抛错，厂商区/按钮簇都必须落成完整默认态；
        // 初始化错误必须在副标题可见，不允许被默认文案覆盖。
        renderVendorArea();
        renderCluster();
        renderLaunchPane();
        renderErrorModal();
        setSub(
          initError ? `页面数据加载失败：${initError.slice(0, 80)}` : idleSubtitle(),
          Boolean(initError),
        );
      }
    })();

    // 头像 + key/模型输入居中悬浮（对齐 PCL 账户区在剩余空间的垂直位置），
    // 启动按钮簇钉在栏底——上、中、下三段，没有大片死空间。加载时整块让位给加载页。
    const identityMain = el("div", { class: "identity-main" }, [
      avatar,
      el("div", { class: "launch-key" }, [keyInput, keySaveBtn]),
      // 模型列表挂在触发器自己的容器里：浮层 `top:100%` 才能精确落在输入框下沿。
      el("div", { class: "launch-key model-select" }, [modelInput, modelList]),
      keyNote,
    ]);
    // 加载页：黑鱼动画 + 进度条 + 字幕式校验序列 + 取消启动（取消 = 中止本次
    // 启动并停掉已拉起的进程；它只存在于加载页）。
    const cancelBtn = el("button", { type: "button", class: "launch-cancel" }, ["取消启动"]);
    cancelBtn.addEventListener("click", requestCancel);
    const loadingPercent = el("span", { class: "launch-loading__percent" }, ["0%"]);
    const loadingFill = el("span", { class: "launch-loading__fill" });
    const loadingTrack = el("div", { class: "launch-loading__track" }, []);
    const loadingRoll = el("div", { class: "launch-loading__roll", "aria-live": "polite" }, [loadingTrack]);
    // 三条**同形同长等距**的波浪线（矢量，位移一个周期即无缝循环）+ 朝左游的
    // 黑色鲸鱼（本体标志）：游动 → 跳起 → 潜水 → 游动循环，游动时头顶喷水柱。
    // 内联 SVG 走 innerHTML（与市场卡下载图标同款写法）；鲸鱼用嵌套两层分别
    // 承担横向漂移与纵向弧线，避免同属性动画互相覆盖。
    const loadingScene = el("div", { class: "launch-loading__scene", "aria-hidden": "true" });
    const wavePath = "M0 13 q 25 -13 50 0 t 50 0 t 50 0 t 50 0 t 50 0";
    const wave = (tier: string): string =>
      `<svg class="launch-loading__wave launch-loading__wave--${tier}" viewBox="0 0 250 26" preserveAspectRatio="none"><path d="${wavePath}" /></svg>`;
    loadingScene.innerHTML =
      wave("surface") +
      `<div class="launch-loading__whale-track"><div class="launch-loading__whale">` +
      `<span class="launch-loading__spout"></span>` +
      `<img class="launch-loading__whale-img" src="version-icon/dsh-black.svg" alt="" />` +
      `</div></div>` +
      wave("mid") +
      wave("deep");
    const loadingPane = el("div", { class: "launch-loading", hidden: "" }, [
      // 常规流整组（2026-10-24 终稿）：鲸鱼 → 三行字幕 → 进度条，居中排布；
      // 旧版顺序（进度在字幕前）是「字幕贴底大窗」时代的遗留，随布局终稿调换。
      el("div", { class: "launch-loading__body" }, [
        loadingScene,
        loadingRoll,
        el("div", { class: "launch-loading__progress" }, [
          el("div", { class: "launch-loading__meter" }, [loadingFill]),
          loadingPercent,
        ]),
      ]),
      cancelBtn,
    ]);
    // 启动按钮簇：模型列表浮层的下界——列表最多铺到它上方，绝不盖住启动按钮。
    const launchArea = el("div", { class: "launch-area" }, [
      launchBtn,
      el("div", { class: "launch-subbtns" }, [selectBtn, configBtn]),
    ]);
    // 左栏两个互斥视图（renderLaunchPane 切换）：静止页与加载页互不参与对方布局，
    // 因此加载页里字幕涨多少行都不可能推动静止页的任何元素。
    const idlePane = el("div", { class: "launch-idle" }, [
      el("div", { class: "vendor-switch" }, [vendorPill, vendorList]),
      el("div", { class: "identity-slot" }, [identityMain]),
      launchArea,
    ]);
    const left = el("aside", { class: "home__left" }, [idlePane, loadingPane]);

    // 首帧同步渲染：不依赖任何 invoke 的返回；init 完成后再以真实数据重刷。
    renderVendorArea();
    renderCluster();
    renderLaunchPane();
    setSub(idleSubtitle());

    // 二级悬浮卡：只在报错时出现（悬浮于右栏蓝色区上方，不影响左栏布局）。
    // ===== 启动错误居中弹窗（PCL2 式二级弹窗；由 renderErrorModal 填充）=====
    const errorModalEl = el("div", { id: "error-modal", class: "modal-overlay", hidden: "" }, [
      el("div", { class: "error-modal" }, [
        el("p", { class: "error-modal__title" }, [""]),
        el("div", { class: "error-modal__body" }, []),
        el("div", { class: "error-modal__btns" }, []),
      ]),
    ]);

    // 详细日志弹窗：白底等宽文本，仅从悬浮卡「日志」进入；主页永不直接展示。
    const logModalClose = el("button", { type: "button", class: "log-modal__close", title: "关闭" }, [icon("x", 14)]);
    logModalClose.addEventListener("click", () => {
      logModalId = null;
      renderLogModal();
    });
    const logModalTitle = el("p", { class: "log-modal__title" }, ["运行日志"]);
    const logModalBody = el("div", { class: "log-modal__body" }, []);
    const logModal = el("div", { class: "log-modal", hidden: "" }, [
      el("div", { class: "log-modal__card" }, [
        el("div", { class: "log-modal__head" }, [logModalTitle, logModalClose]),
        logModalBody,
      ]),
    ]);

    // 浮层的全局收尾：点击别处 / Esc 收起；窗口尺寸变化重算高度上限。
    // 与事件订阅同走 eventDisposers（切页由 router 调 unmount 清理）。
    const onDocumentPointerDown = (event: PointerEvent): void => {
      const target = event.target as Node | null;
      if (
        target &&
        (vendorList.contains(target) ||
          vendorPill.contains(target) ||
          modelList.contains(target) ||
          modelInput.contains(target))
      ) {
        return;
      }
      closeFloatingLists();
    };
    const onDocumentKeyDown = (event: KeyboardEvent): void => {
      if (event.key === "Escape") closeFloatingLists();
    };
    document.addEventListener("pointerdown", onDocumentPointerDown);
    document.addEventListener("keydown", onDocumentKeyDown);
    window.addEventListener("resize", relayoutFloatingLists);
    eventDisposers.push(
      () => document.removeEventListener("pointerdown", onDocumentPointerDown),
      () => document.removeEventListener("keydown", onDocumentKeyDown),
      () => window.removeEventListener("resize", relayoutFloatingLists),
    );

    // PCL 主页：右栏是覆盖整块的蓝色背景，报错时浮出二级卡片。
    return el("section", { class: "page page--launch" }, [
      el(
        "div",
        { class: "home" },
        [left, el("section", { class: "home__right home__right--plain" }, [])],
      ),
      logModal,
      errorModalEl,
    ]);
  },
};
