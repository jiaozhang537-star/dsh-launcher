/**
 * PCL2 动效体系的最小前端实现（Phase 5 · spec G5）。
 * 引擎对照 Meloong-Git/PCL ModAnimation.vb：时间驱动补间 + 缓动族；
 * 场景常数表见 docs/design/pcl-visual-spec.md §5，逐场景对号入座。
 * 本模块零依赖；happy-dom/纯浏览器预览下静默降级为无动画。
 */

export type Easing = (t: number) => number;

/** OutFluent：1-(1-t)^p。p：2 Weak / 3 Middle / 4 Strong / 5 ExtraStrong。 */
export function outFluent(power = 3): Easing {
  return (t) => 1 - Math.pow(1 - t, power);
}

/** OutBack：1-(1-t)^p·cos(1.5πt)，p = 3 - Power×0.5（PCL 公式）。 */
export function outBack(power = 2): Easing {
  return (t) => {
    const p = 3 - power * 0.5;
    return 1 - Math.pow(1 - t, p) * Math.cos(1.5 * Math.PI * t);
  };
}

/** OutElastic：PCL PanHint 入场的弹性位移。 */
export function outElastic(power = 3): Easing {
  return (t) => {
    const p = power + 4;
    return (
      1 -
      Math.pow(t, (p - 1) * 0.25) *
        Math.cos((p - 3.5) * Math.PI * Math.pow(1 - t, 1.5))
    );
  };
}

/** 场景常数（spec §5 逐行对应，勿凭感觉改）。 */
export const SCENES = {
  /**
   * 页面进场（H1，逐常数对齐 PCL FormMain/MyPageRight/MyPageLeft 源码）：
   * 右页内容逐控件 Opacity 0→1 100ms + Y 增量两段 -16 +5（250ms）→ +11（350ms）＝回 0；
   * 每控件 +25ms；左页整体 Scale 0.96→1 400ms OutBack(2) + Opacity 仅 100ms。
   */
  pageEnter: {
    swapMs: 110, // FormMain.PageChangeAnim：110ms 后换内容
    triggerMs: 30, // 换内容后再 30ms 触发页面动画（骨架先画一帧）
    colMs: 400,
    colFrom: 0.96,
    colFadeMs: 100, // 左页 Opacity 只有 100ms（PCL 原式，勿改 400——会洗掉级联）
    itemMs: 100,
    itemRiseMs: 350, // Y -16→0 单段 OutBack（PCL 两段并发增量 5+11=16 的净效果）
    staggerMs: 25,
    startFrom: 16,
    // 左栏滑出（主页→子页，PCL PanMainLeft_Resize + 左页 AnimatedControl 变体）：
    leftSlideMs: 180, // RectLeftBackground 宽度 0→145（180ms ExtraStrong）的 transform 近似
    leftSlideFrom: 40, // translateX -40px→0
    leftItemXMs1: 200, // X 增量两段 +5（200ms）→ +20（300ms OutBack Weak）＝自 -25 回 0
    leftItemXMs2: 300,
  },
  /** 页面退场（PCL TriggerExitAnimation/TriggerHideAnimation）：右页逐控件
   *  Opacity→0 + Y-6（70ms，+15ms/控件）；左页 Scale→0.95（110ms）+ 淡出（80ms，延 30）。 */
  pageExit: {
    itemMs: 70,
    itemStaggerMs: 15,
    itemDropY: 6,
    colMs: 110,
    colFadeMs: 80,
    colFadeDelayMs: 30,
    colTo: 0.95,
  },
  /** 弹窗入场：Y40→0 300ms OutBack（MyMsg 三件套）。 */
  msgEnter: { ms: 300, offsetY: 40 },
  /** 折叠条展开/收起：150ms OutFluent ExtraStrong（spec §5「卡片展开」档）。 */
  collapse: { ms: 150 },
  /**
   * 任务卡进/出场（R5 下载任务区增量更新）：进场 Y-8→0 200ms（--anim-norm 档）；
   * 出场淡出 + 高度收起 150ms（spec §5「卡片销毁」的 fade + 高收起档）。
   */
  taskCard: { enterMs: 200, exitMs: 150, offsetY: 8 },
} as const;

const EASE_OUT_CSS = "cubic-bezier(0.215, 0.61, 0.355, 1)"; // ≈ OutFluent
const EASE_BACK_CSS = "cubic-bezier(0.34, 1.56, 0.64, 1)"; // ≈ OutBack
const EASE_STRONG_CSS = "cubic-bezier(0.22, 1, 0.36, 1)"; // ≈ OutFluent ExtraStrong
const EASE_IN_CSS = "cubic-bezier(0.55, 0, 1, 0.45)"; // ≈ InFluent（t^2）

/** enterPage 动画的内联属性全集（清理时按此逐个摘除，交还 CSS 接管）。 */
const ANIM_PROPS = ["opacity", "transform", "transition"] as const;

function animationSupported(): boolean {
  return (
    typeof requestAnimationFrame === "function" &&
    typeof document !== "undefined" &&
    typeof document.createElement === "function"
  );
}

/** 清理内联动画样式，避免残留影响布局调试与后续 transition。 */
function clearInline(items: HTMLElement[], props: readonly string[], delayMs: number): void {
  setTimeout(() => {
    for (const item of items) {
      for (const prop of props) item.style.removeProperty(prop);
    }
  }, delayMs);
}

// ===== 页面切换（H1：逐常数对齐 PCL MyPageRight / MyPageLeft / FormMain）=====
// PCL 原式（源码摘录于 phase-05 H 波次日志）：
// · FormMain.PageChangeAnim：AaCode(换内容, 110) → AaCode(触发页面动画, 30, After)；
// · 右页 MyPageRight.TriggerEnterAnimation 逐控件：Opacity 0→1（100ms OutFluent
//   Weak）；Y 增量两段 +5（250ms OutFluent）→ +11（350ms OutBack）——起点显式置
//   -16，增量合计 +16 ⇒ 恰好回到 0（AaTranslateY 为增量制：Y += Delta，源码
//   ModAnimation.vb:988）；每控件 Delay += 25ms；
// · 左页 MyPageLeft.TriggerShowAnimation：整体 Scale 0.96→1（400ms OutBack(2)）+
//   Opacity 0→1（100ms）；AnimatedControl 变体逐项 X 增量两段 +5（200ms）→ +20
//   （300ms OutBack Weak）＝自 -25 回 0，Delay += Max(15-id,7)*2。
// · 退场：右页逐控件 Opacity→0 + Y-6（70ms，+15ms/控件）；左页 Scale→0.95
//   （110ms InFluent Weak）+ Opacity→0（80ms，delay 30）。
// CSS 无增量补间：两段增量用「两段 transition 定时接力」复刻。

/** 页面进场栏级（左页）靶：各页左/内容栏中按 PCL「左页整体动画」处理的栏。 */
const PAGE_LEFT_COLUMNS = ".home__left, .sub-left, .dl-left";
/** 页面进场逐控件（右页）靶：内容栏的直接子项。 */
const PAGE_RIGHT_ITEMS =
  ".sub-right > *, .dl-right__content > *, .sub-right__stack > *, " +
  ".sub-right__content > *, .mk-detail > *";

/** 每个动画靶当前归属的动画代数：旧清理回调核对代数，不一致即放弃清理。 */
const pageEnterTokens = new Map<HTMLElement, number>();
let pageEnterSeq = 0;

const isVisible = (node: HTMLElement): boolean =>
  !node.closest("[hidden]") && getComputedStyle(node).visibility !== "hidden";

/** 右页控件的两段增量弹入：stage1 Y→-11（250ms OutFluent）＋fade 100ms；
 *  stage2（250ms 后）Y→0（350ms OutBack）。delay 为该控件的级联延迟。 */
function enterRightItem(item: HTMLElement, delay: number): void {
  // 只设初态与 transition；终值由 enterPage 的双 rAF 统一翻转——
  // 单段平滑（无中途换规格、无逐控件定时器），PCL 两段并发增量的净效果。
  item.style.opacity = "0";
  item.style.transform = "translateY(-16px)";
  item.style.transition =
    `opacity ${SCENES.pageEnter.itemMs}ms ${EASE_OUT_CSS} ${delay}ms, ` +
    `transform ${SCENES.pageEnter.itemRiseMs}ms ${EASE_BACK_CSS} ${delay}ms`;
}

/** 右页控件集合的两段级联进场（供 enterPage 与页内重绘 enterItems 共用）。 */
function cascadeRightItems(items: HTMLElement[], owned: Map<HTMLElement, number>, seq: number): void {
  items.forEach((item, i) => {
    const delay = i * SCENES.pageEnter.staggerMs;
    pageEnterTokens.set(item, seq);
    owned.set(item, seq);
    for (const prop of ANIM_PROPS) item.style.removeProperty(prop);
    enterRightItem(item, delay);
  });
}

/**
 * 页面进场（router 挂载新页面后调用，H1）：
 * ① 左栏整体 Scale 0.96→1（400ms OutBack）+ Opacity 0→1（仅 100ms，PCL 原式——
 *    长 opacity 会与控件级淡入相乘，把级联洗成整页软淡入）；
 * ② 右页内容逐控件两段增量弹入（100ms fade + Y 两段回 0，+25ms/控件）。
 * 页面根与背景不动；隐藏元素不参与、不占 stagger 序号。
 */
export function enterPage(
  root: HTMLElement,
  opts: { fromNoLeftColumn?: boolean } = {},
): void {
  if (!animationSupported()) return;
  const { colMs, colFrom, colFadeMs, triggerMs } = SCENES.pageEnter;

  for (const target of pageEnterTokens.keys()) {
    if (!target.isConnected) pageEnterTokens.delete(target);
  }

  const leftCols: HTMLElement[] = [];
  const rightItems: HTMLElement[] = [];
  const seen = new Set<HTMLElement>();
  for (const node of root.querySelectorAll<HTMLElement>(PAGE_LEFT_COLUMNS)) {
    if (isVisible(node)) {
      seen.add(node);
      leftCols.push(node);
    }
  }
  for (const node of root.querySelectorAll<HTMLElement>(PAGE_RIGHT_ITEMS)) {
    if (seen.has(node) || !isVisible(node)) continue;
    seen.add(node);
    rightItems.push(node);
  }
  if (leftCols.length === 0 && rightItems.length === 0) return;

  const seq = ++pageEnterSeq;
  const owned = new Map<HTMLElement, number>();

  for (const col of leftCols) {
    pageEnterTokens.set(col, seq);
    owned.set(col, seq);
    for (const prop of ANIM_PROPS) col.style.removeProperty(prop);
    if (opts.fromNoLeftColumn) {
      // 主页→子页：左栏滑出（PCL 左栏宽度 0→145 展开的 transform 近似）+ 逐项 X 滑入
      col.style.transform = `translateX(-${SCENES.pageEnter.leftSlideFrom}px)`;
      col.style.transition = `transform ${SCENES.pageEnter.leftSlideMs}ms ${EASE_STRONG_CSS}`;
      Array.from(col.children).forEach((child, i) => {
        const item = child as HTMLElement;
        const delay = Math.max(15 - i, 7) * 2;
        item.style.opacity = "0";
        item.style.transform = "translateX(-25px)";
        item.style.transition =
          `opacity ${SCENES.pageEnter.itemMs}ms ${EASE_OUT_CSS} ${delay}ms, ` +
          `transform ${SCENES.pageEnter.leftItemXMs2}ms ${EASE_BACK_CSS} ${delay}ms`;
      });
    } else {
      col.style.opacity = "0";
      col.style.transform = `scale(${colFrom})`;
      col.style.transition =
        `transform ${colMs}ms ${EASE_BACK_CSS}, opacity ${colFadeMs}ms linear`;
    }
  }
  cascadeRightItems(rightItems, owned, seq);

  // 关键：强制回流一次，把「初态」提交进样式计算——新插入的元素若不经历
  // 一次已计算的初态，翻终值时浏览器合并成一帧，transition 直接不跑。
  void root.offsetWidth;

  // PCL 原式：换内容后再 30ms 才触发页面动画（骨架先画一帧）。
  setTimeout(() => {
    for (const col of leftCols) {
      if (opts.fromNoLeftColumn) {
        col.style.transform = "translateX(0)";
        Array.from(col.children).forEach((child) => {
          (child as HTMLElement).style.transform = "translateX(0)";
          (child as HTMLElement).style.opacity = "1";
        });
      } else {
        col.style.transform = "scale(1)";
        col.style.opacity = "1";
      }
    }
    for (const item of rightItems) {
      item.style.transform = "translateY(0)";
      item.style.opacity = "1";
    }
  }, triggerMs);

  const total =
    Math.max(
      colMs,
      opts.fromNoLeftColumn ? SCENES.pageEnter.leftItemXMs2 + SCENES.pageEnter.leftSlideMs : 0,
    ) + SCENES.pageEnter.staggerMs * rightItems.length + SCENES.pageEnter.itemRiseMs;
  setTimeout(() => {
    for (const [target, token] of owned) {
      if (pageEnterTokens.get(target) !== token) continue;
      for (const prop of ANIM_PROPS) target.style.removeProperty(prop);
      pageEnterTokens.delete(target);
    }
  }, total);
}

/**
 * 页内重绘后的级联进场（downloads 分类切换 / dl-confirm / settings pane 等）：
 * 对给定元素集合施加右页控件同款两段弹入。
 */
export function enterItems(items: HTMLElement[]): void {
  if (!animationSupported() || items.length === 0) return;
  const visible = items.filter(isVisible);
  if (visible.length === 0) return;
  const seq = ++pageEnterSeq;
  const owned = new Map<HTMLElement, number>();
  cascadeRightItems(visible, owned, seq);
  void visible[0].offsetWidth; // 提交初态（同上）
  const total =
    SCENES.pageEnter.staggerMs * visible.length +
    SCENES.pageEnter.itemRiseMs;
  setTimeout(() => {
    for (const [target, token] of owned) {
      if (pageEnterTokens.get(target) !== token) continue;
      for (const prop of ANIM_PROPS) target.style.removeProperty(prop);
      pageEnterTokens.delete(target);
    }
  }, total);
}

/**
 * 旧页退场（router 换内容前调用，PCL TriggerExitAnimation/TriggerHideAnimation
 * 原式）。返回建议的换内容等待毫秒数（PCL 为 110ms）。
 */
export function exitPage(root: HTMLElement): number {
  if (!animationSupported()) return 0;
  const { itemMs, itemStaggerMs, itemDropY, colMs, colFadeMs, colFadeDelayMs, colTo } =
    SCENES.pageExit;
  const leftCols: HTMLElement[] = [];
  const rightItems: HTMLElement[] = [];
  const seen = new Set<HTMLElement>();
  for (const node of root.querySelectorAll<HTMLElement>(PAGE_LEFT_COLUMNS)) {
    if (isVisible(node)) {
      seen.add(node);
      leftCols.push(node);
    }
  }
  for (const node of root.querySelectorAll<HTMLElement>(PAGE_RIGHT_ITEMS)) {
    if (seen.has(node) || !isVisible(node)) continue;
    seen.add(node);
    rightItems.push(node);
  }
  if (leftCols.length === 0 && rightItems.length === 0) return 0;

  for (const col of leftCols) {
    col.style.transition = `transform ${colMs}ms ${EASE_IN_CSS}, opacity ${colFadeMs}ms linear ${colFadeDelayMs}ms`;
    col.style.transform = `scale(${colTo})`;
    col.style.opacity = "0";
  }
  rightItems.forEach((item, i) => {
    const delay = i * itemStaggerMs;
    item.style.transition =
      `opacity ${itemMs}ms ${EASE_OUT_CSS} ${delay}ms, transform ${itemMs}ms ${EASE_OUT_CSS} ${delay}ms`;
    item.style.opacity = "0";
    item.style.transform = `translateY(-${itemDropY}px)`;
  });
  // 退场总时长：左栏 110ms 与右页级联取大；PCL 换内容固定等 110ms。
  const itemTotal = itemMs + itemStaggerMs * rightItems.length;
  return Math.max(colMs + colFadeDelayMs, itemTotal, 110);
}

/** 弹窗/浮层入场：Y40→0 淡入（300ms OutBack）。 */
export function enterMsg(panel: HTMLElement): void {
  if (!animationSupported()) return;
  const { ms, offsetY } = SCENES.msgEnter;
  panel.style.opacity = "0";
  panel.style.transform = `translateY(${offsetY}px)`;
  panel.style.transition = `transform ${ms}ms ${EASE_BACK_CSS}, opacity ${ms}ms ${EASE_OUT_CSS}`;
  requestAnimationFrame(() => {
    requestAnimationFrame(() => {
      panel.style.opacity = "1";
      panel.style.transform = "translateY(0)";
    });
  });
  clearInline([panel], ["opacity", "transform", "transition"], ms + 100);
}

// ===== 折叠高度动画（R5）=====

/** 折叠收尾的小缓冲：防最后一两帧掉帧时 hidden 提前落下造成截断。 */
const COLLAPSE_SETTLE_MS = 30;
const COLLAPSE_PROPS = ["height", "overflow", "transition"] as const;

/** 折叠动画的代数令牌：快速连点时旧收尾定时器核对代数后作废。 */
const collapseTokens = new WeakMap<HTMLElement, number>();
let collapseSeq = 0;

/**
 * 折叠列表展开/收起的高度动画（spec §5「卡片展开」：150ms OutFluent ExtraStrong）。
 * opening=true：移除 hidden → height 0→h → 清内联样式；
 * opening=false：height h→0 → 结束后置回 hidden。
 * `.collapse-group__list[hidden]{display:none}` 的 CSS 规则保留兜底。
 */
export function collapseToggle(list: HTMLElement, opening: boolean): void {
  if (!animationSupported()) {
    list.hidden = !opening;
    return;
  }
  const { ms } = SCENES.collapse;
  const seq = ++collapseSeq;
  collapseTokens.set(list, seq);
  // 上一轮动画（半途而废的展开/收起）残留的内联样式先清掉，回到基态再起舞。
  for (const prop of COLLAPSE_PROPS) list.style.removeProperty(prop);

  if (opening) {
    list.hidden = false;
    const target = list.offsetHeight;
    list.style.overflow = "hidden";
    list.style.height = "0px";
    list.style.transition = `height ${ms}ms ${EASE_STRONG_CSS}`;
    void list.offsetHeight; // 强制回流：先注册 0 起点，下一行才会产生高度过渡
    list.style.height = `${target}px`;
    setTimeout(() => {
      if (collapseTokens.get(list) !== seq) return;
      for (const prop of COLLAPSE_PROPS) list.style.removeProperty(prop);
    }, ms + COLLAPSE_SETTLE_MS);
  } else {
    const start = list.offsetHeight;
    list.style.overflow = "hidden";
    list.style.height = `${start}px`;
    list.style.transition = `height ${ms}ms ${EASE_STRONG_CSS}`;
    void list.offsetHeight;
    list.style.height = "0px";
    setTimeout(() => {
      if (collapseTokens.get(list) !== seq) return;
      list.hidden = true;
      for (const prop of COLLAPSE_PROPS) list.style.removeProperty(prop);
    }, ms + COLLAPSE_SETTLE_MS);
  }
}

// ===== 任务卡进/出场（R5 下载任务区增量更新）=====

/**
 * 任务卡进场：淡入 + Y-8→0（200ms，--anim-norm 档）。只做入场，
 * 后续内容更新由调用方增量涂写，不再动这里设置的内联样式。
 */
export function taskCardEnter(card: HTMLElement): void {
  if (!animationSupported()) return;
  const { enterMs, offsetY } = SCENES.taskCard;
  card.style.opacity = "0";
  card.style.transform = `translateY(-${offsetY}px)`;
  card.style.transition = `opacity ${enterMs}ms ${EASE_OUT_CSS}, transform ${enterMs}ms ${EASE_OUT_CSS}`;
  requestAnimationFrame(() => {
    requestAnimationFrame(() => {
      card.style.opacity = "1";
      card.style.transform = "translateY(0)";
    });
  });
  // 卡片若在清理前被移除，对游离节点清内联样式是无害空操作。
  clearInline([card], ["opacity", "transform", "transition"], enterMs + 100);
}

/**
 * 任务卡退场：淡出 + 高度收起（150ms，spec §5「卡片销毁」档），结束后回调移除。
 * 高度动画连 padding/border 一起收（box-sizing: border-box 下 height 收不到
 * padding 以下），否则会留一条空壳残影。
 */
export function taskCardExit(card: HTMLElement, onDone: () => void): void {
  if (!animationSupported()) {
    onDone();
    return;
  }
  const { exitMs } = SCENES.taskCard;
  card.style.overflow = "hidden";
  card.style.height = `${card.offsetHeight}px`;
  card.style.transition =
    `opacity ${exitMs}ms ${EASE_OUT_CSS}, height ${exitMs}ms ${EASE_OUT_CSS}, ` +
    `padding ${exitMs}ms ${EASE_OUT_CSS}, border-width ${exitMs}ms ${EASE_OUT_CSS}`;
  void card.offsetHeight;
  card.style.opacity = "0";
  card.style.height = "0px";
  card.style.paddingTop = "0";
  card.style.paddingBottom = "0";
  card.style.borderTopWidth = "0";
  card.style.borderBottomWidth = "0";
  setTimeout(() => {
    for (const prop of [
      "opacity",
      "height",
      "padding-top",
      "padding-bottom",
      "border-top-width",
      "border-bottom-width",
      "overflow",
      "transition",
    ]) {
      card.style.removeProperty(prop);
    }
    onDone();
  }, exitMs + COLLAPSE_SETTLE_MS);
}
