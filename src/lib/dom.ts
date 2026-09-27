/** 创建元素的最小辅助：设置属性并挂载子节点（字符串按文本节点处理）。 */
import { icon } from "./icons";

export function el<K extends keyof HTMLElementTagNameMap>(
  tag: K,
  attrs: Record<string, string> = {},
  children: (Node | string)[] = [],
): HTMLElementTagNameMap[K] {
  const node = document.createElement(tag);
  for (const [key, value] of Object.entries(attrs)) {
    node.setAttribute(key, value);
  }
  for (const child of children) {
    node.append(child);
  }
  return node;
}

/**
 * 自绘勾选框（spec G7：18×18 r3 盒 + SVG 对勾 150ms 弹性；替代原生 checkbox）。
 * input 为视觉隐藏但保留原生语义（label 点击切换、form/测试读 .checked）。
 */
export function pclCheck(opts?: {
  checked?: boolean;
  disabled?: boolean;
  label?: string;
  title?: string;
  name?: string;
  value?: string;
}): { root: HTMLLabelElement; input: HTMLInputElement } {
  const input = document.createElement("input");
  input.type = "checkbox";
  input.className = "sr-only";
  input.checked = opts?.checked ?? false;
  if (opts?.disabled) input.disabled = true;
  if (opts?.name) input.name = opts.name;
  if (opts?.value !== undefined) input.value = opts.value;
  const box = el("span", { class: "pcl-check__box" }, [icon("check", 12)]);
  const root = el("label", { class: "pcl-check", title: opts?.title ?? "" }, [input, box]);
  if (opts?.label) root.append(el("span", { class: "pcl-check__label" }, [opts.label]));
  return { root, input };
}

/**
 * 自绘单选框（spec G7：圆形轨 + 内点 9px，对应 PCL MyRadioBox）。
 * 同名 name 的 radio 由浏览器原生互斥。
 */
export function pclRadio(opts?: {
  checked?: boolean;
  disabled?: boolean;
  label?: string;
  title?: string;
  name?: string;
  value?: string;
}): { root: HTMLLabelElement; input: HTMLInputElement } {
  const input = document.createElement("input");
  input.type = "radio";
  input.className = "sr-only";
  input.checked = opts?.checked ?? false;
  if (opts?.disabled) input.disabled = true;
  if (opts?.name) input.name = opts.name;
  if (opts?.value !== undefined) input.value = opts.value;
  const box = el("span", { class: "pcl-radio__box" });
  const root = el("label", { class: "pcl-radio", title: opts?.title ?? "" }, [input, box]);
  if (opts?.label) root.append(el("span", { class: "pcl-check__label" }, [opts.label]));
  return { root, input };
}

/**
 * 折叠条箭头（spec G5：SVG chevron + 250ms 旋转过渡，替代 ▸/▾ 字符）。
 * sync(true)=折叠（箭头朝右），sync(false)=展开（箭头朝下）。
 */
export function collapseArrow(): { root: HTMLElement; sync: (collapsed: boolean) => void } {
  const root = el("span", { class: "collapse-bar__arrow" });
  root.innerHTML =
    '<svg viewBox="0 0 10 6" width="10" height="6" aria-hidden="true">' +
    '<path d="M1 1 L5 5 L9 1" fill="none" stroke="currentColor" stroke-width="1.5' +
    '" stroke-linecap="round" stroke-linejoin="round"/></svg>';
  const sync = (collapsed: boolean): void => {
    root.classList.toggle("collapse-bar__arrow--collapsed", collapsed);
  };
  return { root, sync };
}
