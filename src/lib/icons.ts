/**
 * 自绘线框图标库（Phase 5 返工 R1 · spec G7/G8）。
 * 语言规范：viewBox 24、stroke currentColor 1.75、圆头圆角连接、fill none——
 * 与 PCL 的矢量图标语言同风格，全部自绘，不复制 PCL 源码/美术资产。
 * 用法：icon("back", 16) 返回 SVGSVGElement，颜色随 currentColor。
 */

const SVG_NS = "http://www.w3.org/2000/svg";

export type IconName =
  | "back" | "win-min" | "win-close" | "trash" | "check" | "x" | "download"
  | "plus" | "spinner" | "star" | "arrow-up" | "globe" | "pen" | "heart"
  | "gear" | "import" | "warning" | "external-link" | "folder" | "refresh"
  | "chevron-down";

/** 每枚图标的路径组（多段 path，均为描边）。 */
const PATHS: Record<IconName, string[]> = {
  back: ["M20 12H4.5", "M11 5.5L4.5 12l6.5 6.5"],
  "win-min": ["M5 12h14"],
  "win-close": ["M6 6l12 12", "M18 6L6 18"],
  trash: [
    "M4 7h16",
    "M9.5 7V5.4A1.4 1.4 0 0 1 10.9 4h2.2a1.4 1.4 0 0 1 1.4 1.4V7",
    "M6.5 7l.9 12.2A1.9 1.9 0 0 0 9.3 21h5.4a1.9 1.9 0 0 0 1.9-1.8L17.5 7",
    "M10 11v6",
    "M14 11v6",
  ],
  check: ["M5 12.5l5 5L19 7"],
  x: ["M6.5 6.5l11 11", "M17.5 6.5l-11 11"],
  download: ["M12 4v11", "M7 10l5 5 5-5", "M5 20h14"],
  plus: ["M12 5v14", "M5 12h14"],
  spinner: ["M12 3.5a8.5 8.5 0 1 0 8.5 8.5"],
  star: [
    "M12 3.8l2.5 5.1 5.6.8-4 4 1 5.6-5.1-2.7-5.1 2.7 1-5.6-4-4 5.6-.8z",
  ],
  "arrow-up": ["M12 20V5", "M6 11l6-6 6 6"],
  globe: [
    "M12 3.8a8.2 8.2 0 1 0 0 16.4 8.2 8.2 0 0 0 0-16.4z",
    "M3.8 12h16.4",
    "M12 3.8c2.5 2.3 3.8 5 3.8 8.2s-1.3 5.9-3.8 8.2c-2.5-2.3-3.8-5-3.8-8.2s1.3-5.9 3.8-8.2z",
  ],
  pen: ["M14.7 5.3l4 4L8.4 19.6 4 20l.4-4.4z", "M12.7 7.3l4 4"],
  heart: [
    "M12 20s-7-4.4-7-9.4C5 7.9 7.1 6 9.5 6c1.4 0 2.5 1 2.5 1S13.1 6 14.5 6C16.9 6 19 7.9 19 10.6c0 5-7 9.4-7 9.4z",
  ],
  gear: [
    "M12 8.8a3.2 3.2 0 1 0 0 6.4 3.2 3.2 0 0 0 0-6.4z",
    "M12 2.8v2.6M12 18.6v2.6M2.8 12h2.6M18.6 12h2.6M5.4 5.4l1.9 1.9M16.7 16.7l1.9 1.9M18.6 5.4l-1.9 1.9M7.3 16.7l-1.9 1.9",
  ],
  import: [
    "M12 3v10",
    "M8.5 9.5L12 13l3.5-3.5",
    "M5 14v5.2A1.8 1.8 0 0 0 6.8 21h10.4a1.8 1.8 0 0 0 1.8-1.8V14",
  ],
  warning: ["M12 4L21 19H3z", "M12 10v4.2", "M12 17.1v.4"],
  "external-link": [
    "M14 5h5v5",
    "M19 5l-8 8",
    "M19 13.5V18a1.5 1.5 0 0 1-1.5 1.5H6.5A1.5 1.5 0 0 1 5 18V7.5A1.5 1.5 0 0 1 6.5 6H11",
  ],
  folder: ["M3.5 7.5A1.5 1.5 0 0 1 5 6h4l2 2.5h8A1.5 1.5 0 0 1 20.5 10v8A1.5 1.5 0 0 1 19 19.5H5A1.5 1.5 0 0 1 3.5 18z"],
  refresh: ["M20 12a8 8 0 1 1-2.3-5.6", "M20 3.5V7h-3.5"],
  "chevron-down": ["M5 9l7 7 7-7"],
};

/** 生成线框图标元素；颜色随 currentColor，尺寸默认 16。 */
export function icon(name: IconName, size = 16): SVGSVGElement {
  const svg = document.createElementNS(SVG_NS, "svg");
  svg.setAttribute("viewBox", "0 0 24 24");
  svg.setAttribute("width", String(size));
  svg.setAttribute("height", String(size));
  svg.setAttribute("aria-hidden", "true");
  svg.classList.add("icon", `icon--${name}`);
  for (const d of PATHS[name]) {
    const p = document.createElementNS(SVG_NS, "path");
    p.setAttribute("d", d);
    p.setAttribute("fill", "none");
    p.setAttribute("stroke", "currentColor");
    p.setAttribute("stroke-width", "1.75");
    p.setAttribute("stroke-linecap", "round");
    p.setAttribute("stroke-linejoin", "round");
    svg.append(p);
  }
  return svg;
}
