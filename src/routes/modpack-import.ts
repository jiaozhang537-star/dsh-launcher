/** 整合包导入流程（P4）：预览 → 落点确认 → 导入 → 报告。
 *  两个入口共用：「版本选择 ⊙ 导入整合包」按钮传 null（弹文件选择对话框），
 *  窗口级拖入传文件路径（main.ts 经 webUtils 解析落点路径）。 */

import { el } from "../lib/dom";
import { getSelectedInstance } from "../lib/selected-instance";
import { listInstances, type Instance } from "../services/instances";
import {
  modpackImport,
  modpackPeek,
  pickOpenTgz,
  type ModpackManifest,
} from "../services/modpack";

const TYPE_LABELS: Record<ModpackManifest["type"], string> = {
  plugins: "插件合集",
  settings: "设置包",
  mixed: "混合包",
};

/** 单行预览（最多 max 行 + 「…还有 N 个」）。 */
function previewList(items: string[], max = 8): HTMLElement {
  const shown = items.slice(0, max);
  const rows = shown.map((t) => el("div", { class: "modpack-list__row" }, [t]));
  if (items.length > max) {
    rows.push(el("div", { class: "modpack-list__row modpack-list__row--more" }, [`…还有 ${items.length - max} 项`]));
  }
  return el("div", { class: "modpack-list" }, rows);
}

/** manifest 的摘要与预览节。 */
function buildPreview(manifest: ModpackManifest): HTMLElement[] {
  const nodes: HTMLElement[] = [
    el("p", { class: "modpack-summary" }, [
      el("span", { class: "mk-badge mk-badge--latest" }, [TYPE_LABELS[manifest.type]]),
      ` ${manifest.plugins.length} 个插件` +
        (manifest.settings
          ? ` · 提供商 ${manifest.settings.vendors.length}` +
            (Object.keys(manifest.settings.hotkeys).length > 0 ? " · 热键" : "") +
            (Object.keys(manifest.settings.injectSettings).length > 0 ? " · 注入配置" : "") +
            (manifest.settings.agentsMd ? " · AGENTS.md" : "")
          : ""),
    ]),
    el("p", { class: "settings-path" }, [`来源：${manifest.generator || "未知"} · ${manifest.createdAt}`]),
  ];
  if (manifest.plugins.length > 0) {
    nodes.push(
      el("p", { class: "modpack-section-title" }, ["插件"]),
      previewList(manifest.plugins.map((p) => `${p.name} ${p.version ? `v${p.version}` : ""}`)),
    );
  }
  if (manifest.settings && manifest.settings.vendors.length > 0) {
    nodes.push(
      el("p", { class: "modpack-section-title" }, ["模型提供商（不含 Key）"]),
      previewList(
        manifest.settings.vendors.map((v) => {
          const label = v.name && v.style ? `${v.name}（自定义·${v.style}）` : v.vendor;
          return `${label} · ${v.enabled ? "启用" : "停用"} · ${v.models.length} 个模型`;
        }),
      ),
    );
  }
  return nodes;
}

/**
 * 打开导入流程。`filePath` = 已知的 tgz 绝对路径（拖入）；null = 弹文件选择框。
 * 全程自持模态层；完成/取消后自行移除。
 */
export async function openModpackImport(filePath: string | null): Promise<void> {
  const path = filePath ?? (await pickOpenTgz());
  if (!path) return;

  // ── 预览 ──
  let manifest: ModpackManifest;
  try {
    manifest = await modpackPeek(path);
  } catch (err) {
    alert(`无法读取整合包：\n${String(err).slice(0, 300)}`);
    return;
  }

  let instances: Instance[] = [];
  try {
    instances = await listInstances();
  } catch {
    instances = [];
  }
  const selectedId = getSelectedInstance();
  const defaultTarget =
    instances.find((i) => i.id === selectedId) ?? instances[0] ?? null;

  const needsInstance = manifest.plugins.length > 0;
  const hasSettings = manifest.settings != null;
  let settingsTarget: "global" | "instance" | null = hasSettings ? "global" : null;

  const body = el("div", { class: "confirm-card__body modpack-body" }, []);
  const okBtn = el("button", { type: "button", class: "btn btn--primary" }, ["导入"]);
  const cancelBtn = el("button", { type: "button", class: "btn btn--muted" }, ["取消"]);
  const overlay = el("div", { class: "modal-overlay" }, [
    el("div", { class: "confirm-card modpack-card" }, [
      el("p", { class: "confirm-card__title" }, ["导入整合包"]),
      body,
      el("div", { class: "confirm-card__btns" }, [okBtn, cancelBtn]),
    ]),
  ]);
  const close = (): void => overlay.remove();
  cancelBtn.addEventListener("click", close);
  document.body.append(overlay);

  // 目标实例下拉（插件落点；设置选「随实例」时同用）。
  const targetSelect = document.createElement("select");
  targetSelect.className = "settings-select modpack-target";
  for (const inst of instances) {
    const opt = document.createElement("option");
    opt.value = inst.id;
    opt.textContent = `${inst.name}${inst.dshVersion ? `（DSH ${inst.dshVersion}）` : "（未装本体）"}`;
    targetSelect.append(opt);
  }
  if (defaultTarget) targetSelect.value = defaultTarget.id;
  targetSelect.disabled = instances.length === 0;

  const globalRadio = el("input", { type: "radio", name: "modpack-target-mode" }) as HTMLInputElement;
  const instanceRadio = el("input", { type: "radio", name: "modpack-target-mode" }) as HTMLInputElement;
  const globalLabel = el("label", { class: "modpack-radio" }, [
    globalRadio,
    el("span", {}, ["写入全局默认（新版本自动继承）"]),
  ]);
  const instanceLabel = el("label", { class: "modpack-radio" }, [
    instanceRadio,
    el("span", {}, ["随下方所选版本写入"]),
  ]);
  globalRadio.checked = true;
  globalRadio.addEventListener("change", () => {
    if (globalRadio.checked) settingsTarget = "global";
  });
  instanceRadio.addEventListener("change", () => {
    if (instanceRadio.checked) settingsTarget = "instance";
  });

  body.append(
    el("p", { class: "confirm-card__line" }, [
      el("strong", {}, [manifest.name]),
    ]),
    ...buildPreview(manifest),
  );

  if (needsInstance) {
    body.append(
      el("p", { class: "modpack-section-title" }, ["安装到版本"]),
      targetSelect,
      instances.length === 0
        ? el("p", { class: "settings-status settings-status--error" }, ["还没有任何版本——先到「下载」页创建一个。"])
        : el("p", { class: "settings-path" }, ["包内插件将经离线安装链装入所选版本；失败会整体回滚。"]),
    );
  }
  if (hasSettings) {
    body.append(
      el("p", { class: "modpack-section-title" }, ["设置落点"]),
      globalLabel,
      instanceLabel,
      el("p", { class: "settings-path" }, [
        "热键始终作用于启动器全局；提供商不含 API Key（导入后需自行填写）。",
      ]),
    );
  }

  okBtn.disabled = needsInstance && instances.length === 0;
  okBtn.addEventListener("click", () => {
    const targetId = targetSelect.value;
    if (needsInstance && !targetId) return;
    okBtn.disabled = true;
    cancelBtn.disabled = true;
    const status = el("p", { class: "settings-status" }, ["正在导入…"]);
    body.append(status);
    const settingsArg = hasSettings ? settingsTarget : null;
    modpackImport(path, targetId || "", settingsArg)
      .then((report) => {
        const lines: string[] = [];
        if (report.installed.length > 0) {
          lines.push(`已安装 ${report.installed.length} 个插件：`);
          for (const p of report.installed) {
            lines.push(`  · ${p.packageName} v${p.version || "?"}${p.bundleActivated ? "（已进 bundles 层）" : ""}`);
          }
        }
        if (report.applied.length > 0) {
          lines.push(`已应用设置：${report.applied.join("、")}`);
        }
        for (const w of report.warnings) lines.push(`警告：${w}`);
        status.classList.add("settings-status--ok");
        status.textContent = lines.length > 0 ? lines.join("\n") : "导入完成（包内没有可安装内容）。";
        status.classList.add("modpack-report");
        okBtn.textContent = "完成";
        okBtn.disabled = false;
        okBtn.removeEventListener("click", close);
        okBtn.addEventListener("click", close);
        cancelBtn.remove();
      })
      .catch((err) => {
        status.classList.add("settings-status--error");
        status.textContent = String(err).slice(0, 600);
        status.classList.add("modpack-report");
        cancelBtn.textContent = "关闭";
        cancelBtn.disabled = false;
        cancelBtn.removeEventListener("click", close);
        cancelBtn.addEventListener("click", close);
        okBtn.remove();
      });
  });
}
