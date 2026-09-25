/** 整合包导入流程（P4）：预览 → 落点确认 → 导入 → 报告。
 *  两个入口共用：「版本选择 ⊙ 导入整合包」按钮传 null（弹文件选择对话框），
 *  窗口级拖入传文件路径（main.ts 经内容通道落临时文件后传入）。
 *  布局定稿（走查反馈②）：蓝色标题（红色是删除确认专用语义）、摘要一行、
 *  内容分区平铺文本（不用窄滚动小框——那是空白框/文字裁切的根因）。 */

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

/** 模块级重入锁：导入弹窗存活期间忽略再次打开（拖入或按钮），避免叠加
 * 两个导入流程并发跑官方安装链、互刷同一实例清单（审查 P3-09）。 */
let modpackModalOpen = false;

/** 平铺预览：分区标题 + 最多 6 行完整换行的文本行 +「…还有 N 项」。 */
function flatSection(title: string, items: string[], max = 6): HTMLElement[] {
  if (items.length === 0) return [];
  const nodes = [el("p", { class: "modpack-section-title" }, [title])];
  for (const text of items.slice(0, max)) {
    nodes.push(el("div", { class: "modpack-flat__row" }, [text]));
  }
  if (items.length > max) {
    nodes.push(el("div", { class: "modpack-flat__row modpack-flat__more" }, [`…还有 ${items.length - max} 项`]));
  }
  return nodes;
}

/** manifest 的摘要行与内容预览节。 */
function buildPreview(manifest: ModpackManifest): { summary: HTMLElement; sections: HTMLElement[] } {
  const stats: string[] = [];
  if (manifest.plugins.length > 0) stats.push(`${manifest.plugins.length} 个插件`);
  if (manifest.settings) {
    if (manifest.settings.vendors.length > 0) stats.push(`提供商 ${manifest.settings.vendors.length}`);
    if (Object.keys(manifest.settings.injectSettings).length > 0) stats.push("注入配置");
    if (manifest.settings.agentsMd) stats.push("AGENTS.md");
  }
  const summary = el("p", { class: "modpack-summary" }, [
    el("span", { class: "mk-badge mk-badge--latest" }, [TYPE_LABELS[manifest.type]]),
    el("strong", {}, [` ${manifest.name}`]),
    stats.length > 0 ? el("span", {}, [` · ${stats.join(" · ")}`]) : el("span", {}, ["（空包）"]),
  ]);

  const sections: HTMLElement[] = [
    el("p", { class: "settings-path" }, [`来源：${manifest.generator || "未知"} · ${manifest.createdAt}`]),
  ];
  sections.push(
    ...flatSection(
      "插件",
      manifest.plugins.map((p) => `${p.name}${p.version ? `  v${p.version}` : ""}`),
    ),
  );
  if (manifest.settings && manifest.settings.vendors.length > 0) {
    sections.push(
      ...flatSection(
        "模型提供商（不含 Key）",
        manifest.settings.vendors.map((v) => {
          const label = v.name && v.style ? `${v.name}（自定义 · ${v.style}）` : v.vendor;
          return `${label} · ${v.enabled ? "启用" : "停用"} · ${v.models.length} 个模型`;
        }),
      ),
    );
  }
  return { summary, sections };
}

/**
 * 打开导入流程。`filePath` = 已知的 tgz 绝对路径（拖入）；null = 弹文件选择框。
 * 全程自持模态层；完成/取消后自行移除。
 */
export async function openModpackImport(filePath: string | null): Promise<void> {
  if (modpackModalOpen) return;
  const path = filePath ?? (await pickOpenTgz());
  if (!path) return;
  modpackModalOpen = true;

  // ── 预览 ──
  let manifest: ModpackManifest;
  try {
    manifest = await modpackPeek(path);
  } catch (err) {
    alert(`无法读取整合包：\n${String(err).slice(0, 300)}`);
    modpackModalOpen = false;
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

  const needsPluginTarget = manifest.plugins.length > 0;
  const hasSettings = manifest.settings != null;
  let settingsTarget: "global" | "instance" | null = hasSettings ? "global" : null;

  const body = el("div", { class: "confirm-card__body modpack-body" }, []);
  const okBtn = el("button", { type: "button", class: "btn btn--primary" }, ["导入"]);
  const cancelBtn = el("button", { type: "button", class: "btn btn--line" }, ["取消"]);
  const overlay = el("div", { class: "modal-overlay" }, [
    el("div", { class: "confirm-card modpack-card" }, [
      el("p", { class: "modpack-card__title" }, ["导入整合包"]),
      body,
      el("div", { class: "confirm-card__btns" }, [okBtn, cancelBtn]),
    ]),
  ]);
  const close = (): void => {
    overlay.remove();
    modpackModalOpen = false;
  };
  cancelBtn.addEventListener("click", close);
  document.body.append(overlay);

  const { summary, sections } = buildPreview(manifest);

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
    el("span", {}, ["写入全局默认（所有版本共享，新版本自动继承）"]),
  ]);
  const instanceLabel = el("label", { class: "modpack-radio" }, [
    instanceRadio,
    el("span", {}, ["只写入下方所选版本"]),
  ]);
  globalRadio.checked = true;
  // 零实例时「只写入下方所选版本」无目标可选，禁用（审查 P3-10）。
  instanceRadio.disabled = instances.length === 0;
  globalRadio.addEventListener("change", () => {
    if (globalRadio.checked) settingsTarget = "global";
  });
  instanceRadio.addEventListener("change", () => {
    if (instanceRadio.checked) settingsTarget = "instance";
  });

  body.append(summary, ...sections);

  const validation = el("p", { class: "settings-status" }, []) as HTMLParagraphElement;
  validation.hidden = true;

  // 目标版本选择块：插件落点必用；设置包选「只写入下方所选版本」时也必须有
  // （2026-09-20 二轮实机发现：纯设置包此前不渲染该下拉，落点静默取默认实例、
  // 用户不可选不可见——设置写入到哪个版本无从确认）。
  if (needsPluginTarget) {
    body.append(
      el("p", { class: "modpack-section-title" }, ["安装到版本"]),
      targetSelect,
      instances.length === 0
        ? el("p", { class: "settings-status settings-status--error" }, ["还没有任何版本——先到「下载」页创建一个。"])
        : el("p", { class: "settings-path" }, ["包内插件将经离线安装链装入所选版本；失败会整体回滚。"]),
    );
  } else if (hasSettings && instances.length > 0) {
    body.append(
      el("p", { class: "modpack-section-title" }, ["设置写入到版本"]),
      targetSelect,
      el("p", { class: "settings-path" }, [
        "选择「只写入下方所选版本」时，设置写入该版本；选「写入全局默认」时本选择不参与导入。",
      ]),
    );
  }
  if (hasSettings) {
    body.append(
      el("p", { class: "modpack-section-title" }, ["设置写入位置"]),
      globalLabel,
      instanceLabel,
      el("p", { class: "settings-path" }, [
        "提供商不含 API Key（导入后需自行填写）；AGENTS.md 保存即生效。",
      ]),
    );
  }
  body.append(validation);

  const clearValidation = (): void => {
    validation.hidden = true;
    validation.textContent = "";
  };
  targetSelect.addEventListener("change", clearValidation);
  globalRadio.addEventListener("change", clearValidation);
  instanceRadio.addEventListener("change", clearValidation);

  okBtn.disabled = needsPluginTarget && instances.length === 0;
  okBtn.addEventListener("click", () => {
    const targetId = targetSelect.value;
    // 「随版本」落点（插件或设置）必须显式选定目标版本，不再允许空值静默兜底。
    const requireTarget = needsPluginTarget || (hasSettings && settingsTarget === "instance");
    if (requireTarget && !targetId) {
      validation.hidden = false;
      validation.classList.add("settings-status--error");
      validation.textContent = needsPluginTarget
        ? "请先在「安装到版本」里选择目标版本。"
        : "请先在「设置写入到版本」里选择目标版本（或改为「写入全局默认」）。";
      return;
    }
    clearValidation();
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
