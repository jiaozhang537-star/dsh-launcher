import { enterPage, exitPage } from "./lib/anim";

/** 单个页面定义：id 同时用作 URL hash（#/id）。 */
export interface PageDefinition {
  id: string;
  /** 顶栏导航按钮显示文字。 */
  navLabel: string;
  /** 渲染页面主体内容。 */
  render: () => HTMLElement;
  /**
   * 卸载钩子：切离本页时在 DOM 替换前调用（审查 R1）——页面在此清理
   * 全局事件订阅与定时器，防止每次进入页面累积注册监听器。
   */
  unmount?: () => void;
  /** 二级页：不进顶栏导航；顶栏左侧显示返回箭头 + 页面名。 */
  secondary?: true;
  /** 动态标题（如市场详情页显示「名称 (副名)」）；缺省用 navLabel。 */
  titleOf?: () => string;
}

export const DEFAULT_PAGE_ID = "launch";

const pages = new Map<string, PageDefinition>();
const listeners = new Set<(pageId: string) => void>();

let container: HTMLElement | null = null;

/** 当前挂载的页面定义：切页时对其调用 unmount。 */
let mountedDef: PageDefinition | null = null;

/** 上一页 id：顶栏 ← 返回用；同页重渲染不更新。 */
let lastPageId: string | null = null;

/** 导航代数：退场动画期间的快速连切以代数守卫，旧换内容定时器作废。 */
let navSeq = 0;

/** 注册全部页面；必须在 mountRouter 之前调用。 */
export function registerPages(defs: PageDefinition[]): void {
  for (const def of defs) {
    pages.set(def.id, def);
  }
}

function currentPageId(): string {
  const id = location.hash.replace(/^#\/?/, "");
  return pages.has(id) ? id : DEFAULT_PAGE_ID;
}

function renderCurrent(): void {
  if (!container) {
    throw new Error("router 未挂载：先调用 mountRouter");
  }
  const id = currentPageId();
  const def = pages.get(id);
  if (!def) {
    throw new Error(`未注册的页面：${id}`);
  }
  // PCL PageChangeAnim 原式：旧页退场动画（70~110ms）跑着，110ms 后换内容，
  // enterPage 内再含 30ms 触发延迟。快速连切以导航代数守卫，旧换内容定时器作废。
  if (!container) {
    throw new Error("router 未挂载：先调用 mountRouter");
  }
  const host = container;
  navSeq += 1;
  const seq = navSeq;
  const doSwap = (): void => {
    if (navSeq !== seq) return; // 已被更新的导航接管
    // 先卸载上一页（清理订阅/定时器），再替换 DOM；卸载失败不阻断导航。
    if (mountedDef?.unmount) {
      try {
        mountedDef.unmount();
      } catch (err) {
        console.error(`页面 ${mountedDef.id} 卸载清理失败：`, err);
      }
    }
    if (mountedDef && mountedDef.id !== id) {
      lastPageId = mountedDef.id;
    }
    mountedDef = def;
    host.replaceChildren();
    host.append(def.render());
    // 来源页无左栏（主页）→ 目标页左栏滑出（PCL PanMainLeft_Resize 原式近似）
    enterPage(host, { fromNoLeftColumn: lastPageId === "launch" });
    for (const notify of listeners) {
      notify(id);
    }
  };
  if (mountedDef) {
    exitPage(container);
    // PCL 原式：换内容固定等 110ms（退场动画被截断亦然，PageChangeAnim 同款）。
    setTimeout(doSwap, 110);
  } else {
    doSwap(); // 首挂无旧页可退场：同步挂载，避免与紧随的导航竞争
  }
}

/** 订阅页面切换；已挂载时立即以当前页 id 回调一次。 */
export function subscribe(listener: (pageId: string) => void): void {
  listeners.add(listener);
}

/** 挂载路由到容器并渲染初始页面。 */
export function mountRouter(target: HTMLElement): void {
  container = target;
  window.addEventListener("hashchange", renderCurrent);
  if (!location.hash) {
    history.replaceState(null, "", `#/${DEFAULT_PAGE_ID}`);
  }
  renderCurrent();
}

/** 切换到指定页面。 */
export function navigate(pageId: string): void {
  location.hash = `#/${pageId}`;
}

/** 返回上一页（顶栏 ←）；无上一页（如刷新直达二级页）时回主页。 */
export function goBack(): void {
  navigate(lastPageId ?? DEFAULT_PAGE_ID);
}

/** 以注册顺序返回全部页面定义（顶栏导航用，含二级页——顶栏自行过滤）。 */
export function registeredPages(): PageDefinition[] {
  return [...pages.values()];
}

/** 按页面 id 取定义；不存在返回 undefined。 */
export function pageById(id: string): PageDefinition | undefined {
  return pages.get(id);
}
