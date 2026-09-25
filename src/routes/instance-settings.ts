import { el } from "../lib/dom";
import { getSelectedInstance } from "../lib/selected-instance";
import type { PageDefinition } from "../router";
import {
  getInjectOverride,
  getGlobalAgentsMd,
  listInstances,
  openInstanceSubdir,
  renameInstance,
  setInjectOverride,
  type Instance,
} from "../services/instances";
import { DEEPSEEK_MODELS } from "../services/keyvault";
import {
  pluginCheckUpdates,
  pluginList,
  pluginRemove,
  pluginSetChildEnabled,
  pluginSetEnabled,
  type PluginRow,
} from "../services/plugins";
import {
  deleteInstanceSources,
  downloadSourceArchive,
  instanceSourcesDownloaded,
  openSourceDir,
} from "../services/sourceArchive";

/** 版本设置页（二级页）四分区。 */
type SectionId = "overview" | "settings" | "plugins" | "export";

const SECTION_LABELS: Record<SectionId, string> = {
  overview: "概览",
  settings: "设置",
  plugins: "插件管理",
  export: "导出",
};

/** 折叠组：标题行 + 可折叠列表（对标 PCL 折叠条）。 */
function collapsibleGroup(title: string, count: number, collapsed: boolean, rows: HTMLElement[]): HTMLElement {
  const list = el("div", { class: "collapse-group__list" }, rows);
  const arrow = el("span", { class: "collapse-bar__arrow" }, [collapsed ? "▸" : "▾"]);
  const header = el(
    "button",
    { type: "button", class: "collapse-bar" },
    [el("span", { class: "collapse-bar__title" }, [`${title}（${count}）`]), arrow],
  );
  list.hidden = collapsed;
  header.addEventListener("click", () => {
    list.hidden = !list.hidden;
    arrow.textContent = list.hidden ? "▸" : "▾";
  });
  return el("div", { class: "collapse-group" }, [header, list]);
}

export const instanceSettingsPage: PageDefinition = {
  id: "instance-settings",
  navLabel: "版本设置",
  secondary: true,
  render: () => {
    let currentInstance: Instance | null = null;
    let section: SectionId = "overview";

    // ===== 左栏子导航 =====
    const navButtons = new Map<SectionId, HTMLButtonElement>();
    const rightArea = el("div", { class: "sub-right__content" }, []);
    // AGENTS.md 防抖计时器提为 render 级：切分区时清除挂起的保存，避免旧
    // 闭包在页面已重绘后照常写盘（写入目标本身正确，状态行已失效）。
    let agentsSaveTimer: ReturnType<typeof setTimeout> | undefined;

    function renderSection(): void {
      for (const [id, btn] of navButtons) {
        btn.classList.toggle("sub-nav__item--active", id === section);
      }
      clearTimeout(agentsSaveTimer);
      rightArea.replaceChildren();
      if (!currentInstance) {
        rightArea.append(el("p", { class: "settings-status" }, ["未选择实例——请先在「版本选择」页选择。"]));
        return;
      }
      if (section === "overview") renderOverview();
      else if (section === "settings") renderSettings();
      else if (section === "plugins") renderPlugins();
      else renderExport();
    }

    // ===== 概览 =====
    function renderOverview(): void {
      const inst = currentInstance!;
      const nameStatus = el("span", { class: "settings-path" }, []);
      rightArea.append(
        // 概览首卡（实例名 + 版本/profile/端口 + 目录）按需求方 2026-09-20 判定
        // 无用途删除：这些信息在「版本选择」页卡片上已有，本页只留操作。
        el("section", { class: "card settings-card" }, [
          el("h2", { class: "card__title" }, ["个性化"]),
          el("div", { class: "settings-field" }, [
            el("span", {}, ["实例名称"]),
            el("div", { class: "settings-actions" }, (() => {
              const input = el("input", { type: "text", class: "settings-input", value: inst.name }) as HTMLInputElement;
              const btn = el("button", { type: "button", class: "btn btn--line" }, ["更改版本名"]);
              btn.addEventListener("click", () => {
                const newName = input.value.trim();
                if (!newName) {
                  nameStatus.textContent = "名称不能为空。";
                  return;
                }
                if (newName === inst.name) {
                  nameStatus.textContent = "名称未变化。";
                  return;
                }
                btn.disabled = true;
                void renameInstance(inst.id, newName)
                  .then(() => {
                    inst.name = newName;
                    nameStatus.textContent = "已改名。";
                    renderSection();
                  })
                  .catch((err) => {
                    nameStatus.textContent = `改名失败：${String(err).slice(0, 200)}`;
                  })
                  .finally(() => {
                    btn.disabled = false;
                  });
              });
              return [input, btn];
            })()),
          ]),
          nameStatus,
          el("div", { class: "settings-field" }, [
            el("span", {}, ["置顶（启动页排序置顶）"]),
            (() => {
              const toggle = el("button", { type: "button", class: "toggle-switch", role: "switch" }, []);
              toggle.classList.add("toggle-switch--off");
              toggle.disabled = true;
              toggle.title = "Phase 5 交付";
              return toggle;
            })(),
          ]),
        ]),
        renderQuickOpenCard(inst),
        renderSourcesCard(inst),
      );
    }

    /** 快捷打开卡（2026-09-12 追加批次）：横排三个按钮直达 skills / 对话 / 源码文件夹。 */
    function renderQuickOpenCard(inst: Instance): HTMLElement {
      const status = el("span", { class: "settings-path" }, ["在系统文件管理器中打开对应文件夹。"]);
      const makeBtn = (label: string, which: "skills" | "sessions" | "sources"): HTMLElement => {
        const btn = el("button", { type: "button", class: "btn btn--line" }, [label]);
        btn.addEventListener("click", () => {
          btn.disabled = true;
          void openInstanceSubdir(inst.id, which)
            .then(() => {
              status.textContent = `已打开 ${label.replace(/^打开/, "")}文件夹。`;
            })
            .catch((err) => {
              status.textContent = String(err).slice(0, 240);
            })
            .finally(() => {
              btn.disabled = false;
            });
        });
        return btn;
      };
      return el("section", { class: "card settings-card" }, [
        el("h2", { class: "card__title" }, ["快捷打开"]),
        el("div", { class: "settings-actions" }, [
          makeBtn("打开skills", "skills"),
          makeBtn("打开对话", "sessions"),
          makeBtn("打开源码", "sources"),
        ]),
        status,
      ]);
    }

    /** 源码归档卡片（FR-22 修订）：为该版本补下载官方源码到 `versions/<id>/sources/`。 */
    function renderSourcesCard(inst: Instance): HTMLElement {
      const status = el("span", { class: "settings-path" }, [
        inst.dshVersion
          ? "下载官方源码归档到本版本目录，供 AI 辅助插件开发参考（不可用于启动）。"
          : "该版本尚未安装 DSH 本体——先到「下载」页安装，再下载源码。",
      ]);
      const actions = el("div", { class: "settings-actions" }, []);
      let downloaded = false;

      const openBtn = el("button", { type: "button", class: "btn btn--ghost settings-inline-btn" }, ["打开源码目录"]);
      openBtn.hidden = true;
      openBtn.addEventListener("click", () => {
        void openSourceDir(inst.dshVersion ?? "", inst.id).catch((err) => {
          status.textContent = `打开失败：${String(err).slice(0, 200)}`;
        });
      });

      // 删除源码（两击确认，同插件删除模式）：只删本版本 sources/ 目录。
      const delBtn = el("button", { type: "button", class: "btn btn--line settings-inline-btn" }, ["删除源码"]);
      delBtn.hidden = true;
      let delArmed = false;
      let delTimer: ReturnType<typeof setTimeout> | undefined;
      delBtn.addEventListener("click", () => {
        if (!delArmed) {
          delArmed = true;
          delBtn.classList.add("btn--danger");
          delBtn.textContent = "确认删除";
          delTimer = setTimeout(() => {
            delArmed = false;
            delBtn.classList.remove("btn--danger");
            delBtn.textContent = "删除源码";
          }, 2500);
          return;
        }
        clearTimeout(delTimer);
        delArmed = false;
        delBtn.classList.remove("btn--danger");
        delBtn.textContent = "删除源码";
        delBtn.disabled = true;
        void deleteInstanceSources(inst.id)
          .then(() => {
            downloaded = false;
            openBtn.hidden = true;
            delBtn.hidden = true;
            btn.disabled = !inst.dshVersion;
            btn.textContent = "下载源码";
            status.textContent = "源码归档已删除。";
          })
          .catch((err) => {
            status.textContent = `删除失败：${String(err).slice(0, 240)}`;
          })
          .finally(() => {
            delBtn.disabled = false;
          });
      });

      const btn = el("button", { type: "button", class: "btn btn--line" }, ["下载源码"]);
      btn.disabled = !inst.dshVersion;
      // 状态回显（走查反馈②）：已下载过就直接呈现「已下载 + 打开/删除」，
      // 不再要点一次下载才知道。
      void instanceSourcesDownloaded(inst.id)
        .then((downloadedAlready) => {
          if (!downloadedAlready || downloaded) return;
          downloaded = true;
          btn.disabled = true;
          btn.textContent = "已下载";
          openBtn.hidden = false;
          delBtn.hidden = false;
          status.textContent = "源码归档已在本版本 sources/ 目录，可打开查看。";
        })
        .catch(() => {});
      btn.addEventListener("click", () => {
        if (!inst.dshVersion || btn.disabled) return;
        btn.disabled = true;
        openBtn.hidden = true;
        delBtn.hidden = true;
        status.textContent = "准备下载源码归档…";
        void downloadSourceArchive(inst.dshVersion, inst.id, ({ received, total }) => {
          if (total > 0) {
            status.textContent = `下载源码归档 ${Math.min(100, Math.round((received / total) * 100))}% · ${received} / ${total} 字节`;
          }
        })
          .then(() => {
            downloaded = true;
            status.textContent = "源码归档已就位 ✓（本版本 sources/ 目录，可打开查看）";
            openBtn.hidden = false;
            delBtn.hidden = false;
          })
          .catch((err) => {
            status.textContent = `下载失败：${String(err).slice(0, 260)}`;
          })
          .finally(() => {
            btn.disabled = downloaded;
            btn.textContent = downloaded ? "已下载" : "重试下载";
          });
      });
      actions.append(btn, openBtn, delBtn);
      return el("section", { class: "card settings-card" }, [
        el("h2", { class: "card__title" }, ["源码归档"]),
        el("div", { class: "settings-node-card" }, [btn, openBtn, delBtn, status]),
      ]);
    }

    // ===== 设置 =====
    function renderSettings(): void {
      const inst = currentInstance!;
      const isolation = el("button", { type: "button", class: "toggle-switch toggle-switch--on", role: "switch" }, []);
      isolation.disabled = true;
      isolation.title = "Phase 5 交付";
      const modelSelect = document.createElement("select");
      modelSelect.className = "settings-select";
      for (const m of DEEPSEEK_MODELS) {
        const opt = document.createElement("option");
        opt.value = m;
        opt.textContent = m;
        modelSelect.append(opt);
      }
      modelSelect.disabled = true;
      modelSelect.title = "Phase 5 交付";

      // 局部 AGENTS.md（FILE-LAYOUT：输入框落点 = versions/<id>/AGENTS.md 的源头，
      // 覆盖存实例 inject.json；未自设时由全局默认物化）。
      // 需求方定稿（2026-09-12）：①覆盖语义 = 局部设置后该版本只用自己的内容
      // （覆盖而非叠加，全局对该版本不再生效）；②只要输入框，输入完即自动保存；
      // ③启动该版本时生效（sync_instance → write_instance_files 物化）。
      const agentsArea = document.createElement("textarea");
      agentsArea.className = "settings-textarea";
      agentsArea.rows = 8;
      agentsArea.placeholder = "为这个版本单独填写指令；留空 = 恢复继承全局默认";
      const agentsStatus = el("p", { class: "settings-path" }, ["加载中…"]);
      let agentsLoaded = false;
      agentsArea.addEventListener("input", () => {
        if (!agentsLoaded) return;
        clearTimeout(agentsSaveTimer);
        agentsStatus.textContent = "正在保存…";
        agentsSaveTimer = setTimeout(() => {
          void (async () => {
            try {
              const over = await getInjectOverride(inst.id);
              const text = agentsArea.value;
              await setInjectOverride(inst.id, {
                settings: over.settings ?? {},
                env: over.env ?? {},
                agentsMd: text.trim() === "" ? null : text,
              });
              agentsStatus.textContent =
                text.trim() === ""
                  ? "已恢复继承全局默认——已即时物化到本版本。"
                  : "已保存并即时物化——运行中实例下一条消息起生效（该版本只用自己的内容，全局对它不再生效）。";
            } catch (err) {
              agentsStatus.textContent = `保存失败：${String(err).slice(0, 240)}`;
            }
          })();
        }, 600);
      });
      async function loadAgentsState(): Promise<void> {
        try {
          const [over, global] = await Promise.all([getInjectOverride(inst.id), getGlobalAgentsMd()]);
          const own = over.agentsMd ?? null;
          agentsArea.value = own ?? global ?? "";
          agentsStatus.textContent =
            own != null
              ? "当前：实例独立内容——覆盖全局（非叠加）；修改即保存并即时物化。"
              : global
                ? "当前：继承全局默认——修改即转为实例独立内容并即时物化。"
                : "当前：全局与实例均未设置。";
        } catch (err) {
          agentsStatus.textContent = `读取失败：${String(err).slice(0, 240)}`;
        } finally {
          agentsLoaded = true;
        }
      }

      rightArea.append(
        el("section", { class: "card settings-card" }, [
          el("h2", { class: "card__title" }, ["版本隔离"]),
          el("div", { class: "settings-node-card" }, [
            isolation,
            el("span", { class: "settings-path" }, [
              "按实例关闭隔离（插件与配置回退全局共享区）—— Phase 5 交付",
            ]),
          ]),
        ]),
        el("section", { class: "card settings-card" }, [
          el("h2", { class: "card__title" }, ["默认使用的模型"]),
          el("div", { class: "settings-node-card" }, [
            modelSelect,
            el("span", { class: "settings-path" }, ["写入该实例的注入配置 —— Phase 5 交付"]),
          ]),
        ]),
        el("section", { class: "card settings-card" }, [
          el("h2", { class: "card__title" }, ["AGENTS.md"]),
          el("p", { class: "settings-path" }, [
            "本版本的局部指令；未单独设置时使用「设置 → 配置」里的全局 AGENTS.md。局部设置 = 覆盖全局（非叠加），启动该版本时生效。",
          ]),
          agentsArea,
          agentsStatus,
        ]),
      );
      void loadAgentsState();
    }

    // ===== 插件管理（FR-03 配套：真数据三分组 + 官方链删除 + 更新检查）=====
    function renderPlugins(): void {
      const inst = currentInstance!;
      const GROUP_LABELS: Record<PluginRow["group"], string> = {
        builtin: "dsh 内置",
        thirdparty: "第三方",
        selfmade: "自建",
      };

      const tabAll = el("button", { type: "button", class: "tab-btn tab-btn--active" }, ["全部"]);
      const tabUpdates = el("button", { type: "button", class: "tab-btn" }, ["可更新"]);
      // 「重新扫描」（R7-4）：插件清单每次渲染都重新读盘，但已渲染的节不自动刷新——
      // 网页里 `dsh plugin add` 装上的插件、或从别处复制进来的自建插件，需要用户
      // 主动重扫一次才出现在这一节（真机走查见 2026-09-12 QA 报告 Q4）。
      const rescan = el("button", { type: "button", class: "btn btn--line" }, ["重新扫描"]);
      rescan.title = "重新读取该实例 profile 的插件清单（不会启动实例）";
      rescan.addEventListener("click", () => {
        rescan.disabled = true;
        void load().finally(() => {
          rescan.disabled = false;
        });
      });
      const statusLine = el("p", { class: "settings-status" }, []);
      const listArea = el("div", { class: "plugin-groups" }, []);
      let mode: "all" | "updates" = "all";

      const tabSync = (): void => {
        tabAll.classList.toggle("tab-btn--active", mode === "all");
        tabUpdates.classList.toggle("tab-btn--active", mode === "updates");
      };

      function setStatus(text: string, isError = false): void {
        statusLine.textContent = text;
        statusLine.classList.toggle("settings-status--error", isError);
      }

      /** 插件行（走查定稿）：最左勾选框 = 启用（bundles 层），再名称/小字；
       *  删除 = 两击确认（官方 remove 链）。内置组勾选框禁用态恒勾。 */
      function rowAll(p: PluginRow): HTMLElement {
        const subParts = [p.version || "未知版本", GROUP_LABELS[p.group]];
        if (p.group !== "builtin" && p.spec) subParts.push(p.spec);
        const enableBox = el("input", { type: "checkbox", class: "plugin-row__enable" }) as HTMLInputElement;
        enableBox.checked = p.inBundles;
        if (p.group === "builtin") {
          enableBox.disabled = true;
          enableBox.title = "dsh 内置层随本体分发，恒启用";
        } else if (!p.declaresPatch) {
          // 纯依赖包（如 UI 库，keywords 打了 dsh-plugin 混进市场的典型）：
          // 没有可加载的插件层，无「启用」概念——禁用灰框如实呈现。
          enableBox.disabled = true;
          enableBox.title = "纯依赖包：未声明 dsh.bundle.patch，不作为插件层加载（无需启用，可点右侧删除）";
        } else {
          enableBox.title = p.inBundles
            ? "已启用——取消勾选 = 停用（下次启动实例生效）"
            : "已停用——勾选 = 启用（下次启动实例生效）";
          enableBox.addEventListener("change", () => {
            enableBox.disabled = true;
            setStatus(`正在${enableBox.checked ? "启用" : "停用"} ${p.name}…`);
            void pluginSetEnabled(inst.id, p.name, enableBox.checked)
              .then(() => {
                setStatus(
                  `${enableBox.checked ? "已启用" : "已停用"} ${p.name}——下次启动实例时生效` +
                    `（注意：官方插件增删会让停用项重新启用）`,
                );
                void load();
              })
              .catch((err) => {
                enableBox.disabled = false;
                enableBox.checked = !enableBox.checked;
                setStatus(`操作失败：${String(err).slice(0, 240)}`, true);
              });
          });
        }
        let armed = false;
        const remove = el("button", { type: "button", class: "plugin-row__remove", title: "删除插件" }, ["🗑"]);
        remove.addEventListener("click", (event) => {
          event.stopPropagation();
          if (!armed) {
            armed = true;
            remove.classList.add("plugin-row__remove--hint");
            remove.title = "再点一次确认删除（下次启动实例时彻底生效）";
            setTimeout(() => {
              armed = false;
              remove.classList.remove("plugin-row__remove--hint");
              remove.title = "删除插件";
            }, 2500);
            return;
          }
          remove.disabled = true;
          setStatus(`正在移除 ${p.name}…`);
          void pluginRemove(inst.id, p.name)
            .then(() => {
              setStatus(`已移除 ${p.name}——下次启动实例时彻底生效。`);
              void load();
            })
            .catch((err) => {
              remove.disabled = false;
              setStatus(`移除失败：${String(err).slice(0, 260)}`, true);
            });
        });
        const row = el("div", { class: "ver-row plugin-row" }, [
          enableBox,
          el("span", { class: "ver-row__main" }, [p.name]),
          el("span", { class: "ver-row__sub" }, [subParts.join(" · ")]),
        ]);
        if (p.removable) row.append(remove);
        return row;
      }

      /** 组合包卡片（2026-09-12 追加批次）：每个组合包一张折叠卡（位于自建组
       * 之上），展开列出父包 patch insert 的子插件，各自可启停——启停写版本
       * 用户层 cordis.patch.yml（{id,disabled}），运行中实例经 DSH 热重载即时
       * 生效；未运行则下次启动生效。 */
      function modpackRows(plugins: PluginRow[]): [PluginRow, HTMLElement][] {
        const out: [PluginRow, HTMLElement][] = [];
        for (const p of plugins) {
          const kids = p.modpackChildren;
          if (!kids || kids.length === 0) continue;
          const rows = kids.map((c) => {
            const box = el("input", { type: "checkbox", class: "plugin-row__enable" }) as HTMLInputElement;
            box.checked = !c.disabled;
            box.title = c.disabled
              ? "已停用——勾选 = 启用（运行中实例热重载即时生效；未运行则下次启动生效）"
              : "已启用——取消勾选 = 停用（运行中实例热重载即时生效；未运行则下次启动生效）";
            box.addEventListener("change", () => {
              box.disabled = true;
              setStatus(`正在${box.checked ? "启用" : "停用"} ${c.name}…`);
              void pluginSetChildEnabled(inst.id, c.rowId, box.checked)
                .then(() => {
                  setStatus(
                    `${box.checked ? "已启用" : "已停用"} ${c.name}——运行中实例热重载即时生效，未运行则下次启动生效。`,
                  );
                  void load();
                })
                .catch((err) => {
                  box.disabled = false;
                  box.checked = !box.checked;
                  setStatus(`操作失败：${String(err).slice(0, 240)}`, true);
                });
            });
            return el("div", { class: "ver-row plugin-row" }, [
              box,
              el("span", { class: "ver-row__main" }, [c.name]),
              el("span", { class: "ver-row__sub" }, [c.version ? `v${c.version}` : "未知版本"]),
            ]);
          });
          out.push([
            p,
            collapsibleGroup(`组合包：${p.name}`, kids.length, false, rows),
          ]);
        }
        return out;
      }

      function rowUpdate(u: { name: string; current: string; latest: string; updatable: boolean }): HTMLElement {        const badge = u.updatable
          ? el("span", { class: "mk-badge mk-badge--latest" }, [`可更新 → ${u.latest}`])
          : el("span", { class: "mk-badge" }, ["已是最新"]);
        return el("div", { class: "ver-row plugin-row" }, [
          el("span", { class: "ver-row__main" }, [u.name]),
          el("span", { class: "ver-row__sub" }, [u.current ? `当前 v${u.current}` : "未安装"]),
          badge,
        ]);
      }

      async function load(): Promise<void> {
        if (!inst.dshVersion) {
          listArea.replaceChildren(
            el("p", { class: "settings-status" }, ["该实例尚未安装 DSH 本体——先到「下载」页安装，再管理插件。"]),
          );
          return;
        }
        setStatus("加载中…");
        try {
          if (mode === "all") {
            const list = await pluginList(inst.id);
            const builtin = list.plugins.filter((p) => p.group === "builtin").map(rowAll);
            const third = list.plugins.filter((p) => p.group === "thirdparty").map(rowAll);
            const self = list.plugins.filter((p) => p.group === "selfmade").map(rowAll);
            listArea.replaceChildren();
            if (list.plugins.length === 0) {
              setStatus("该实例还没有已装插件——到「下载 · 插件」市场逛逛吧。");
              return;
            }
            setStatus(
              `已扫描 ${list.plugins.length} 个插件（内置 ${builtin.length} / 第三方 ${third.length} / 自建 ${self.length}）——` +
                "这一节每次进入都重新读盘（读该 profile 的清单），所以在网页会话里跑 `dsh plugin add` 装上的插件，" +
                "点「重新扫描」即可看到；插件本身仍要下次启动该实例才加载。清单里没有记录的包（例如手放进 node_modules 的）不在此列。",
            );
            const modpacks = modpackRows(list.plugins);
            // 空组不出卡：删掉组内最后一个插件后，0 条的折叠组仍渲染标题行 +
            // 空列表，展开就是一片残影卡（真机走查 2026-09-20）。
            const groups: HTMLElement[] = [];
            if (builtin.length > 0) groups.push(collapsibleGroup("dsh 内置", builtin.length, true, builtin));
            if (third.length > 0) groups.push(collapsibleGroup("第三方插件", third.length, false, third));
            groups.push(...modpacks.map(([, group]) => group));
            if (self.length > 0) groups.push(collapsibleGroup("自建插件", self.length, true, self));
            listArea.append(...groups);
          } else {
            const updates = (await pluginCheckUpdates(inst.id)).filter((u) => u.updatable);
            listArea.replaceChildren();
            // 走查定稿：「可更新」只列有更新的——没有就一句话，不刷「已是最新」。
            if (updates.length === 0) {
              setStatus("全部第三方插件均为最新。");
              return;
            }
            setStatus(`发现 ${updates.length} 个可更新项。`);
            listArea.append(
              collapsibleGroup("第三方插件", updates.length, false, updates.map(rowUpdate)),
            );
          }
        } catch (err) {
          setStatus(String(err).slice(0, 300), true);
        }
      }

      tabAll.addEventListener("click", () => {
        if (mode !== "all") {
          mode = "all";
          tabSync();
          void load();
        }
      });
      tabUpdates.addEventListener("click", () => {
        if (mode !== "updates") {
          mode = "updates";
          tabSync();
          void load();
        }
      });

      rightArea.append(
        el("div", { class: "settings-row" }, [tabAll, tabUpdates, rescan]),
        statusLine,
        listArea,
      );
      void load();
    }

    // ===== 导出（P4：勾选插件 + 设置内容 → 自包含离线 tgz）=====
    function renderExport(): void {
      const inst = currentInstance!;

      const nameInput = el("input", {
        type: "text",
        class: "settings-input",
        value: `${inst.name} 整合包`,
      }) as HTMLInputElement;
      const status = el("p", { class: "settings-path" }, ["勾选要打包的内容；插件以 npm 源码包形式离线携带。"]);

      const pluginListArea = el("div", { class: "modpack-list modpack-list--pick" }, [
        el("p", { class: "settings-status" }, ["加载中…"]),
      ]);
      const settingChecks: { box: HTMLInputElement; label: string }[] = [];

      const exportBtn = el("button", { type: "button", class: "btn btn--primary" }, ["导出整合包…"]);
      exportBtn.disabled = true;

      void (async () => {
        try {
          const list = await pluginList(inst.id);
          const third = list.plugins.filter((p) => p.group === "thirdparty");
          pluginListArea.replaceChildren();
          if (third.length === 0) {
            pluginListArea.append(
              el("p", { class: "settings-path" }, ["该实例没有已装的第三方插件——可只导出设置内容。"]),
            );
          }
          for (const p of third) {
            const box = el("input", { type: "checkbox", "data-name": p.name }) as HTMLInputElement;
            const row = el("label", { class: "modpack-radio" }, [
              box,
              el("span", {}, [`${p.name} ${p.version ? `v${p.version}` : ""}`]),
            ]);
            pluginListArea.append(row);
          }
        } catch (err) {
          pluginListArea.replaceChildren(
            el("p", { class: "settings-status settings-status--error" }, [
              `插件清单读取失败：${String(err).slice(0, 220)}`,
            ]),
          );
        }
        exportBtn.disabled = false;
      })();

      const section = el("section", { class: "card settings-card" }, [
        el("h2", { class: "card__title" }, ["插件（第三方）"]),
        pluginListArea,
        el("h2", { class: "card__title" }, ["设置内容"]),
        el("p", { class: "settings-path" }, [
          "提供商不含 API Key；注入配置与环境变量永不入包。",
        ]),
        ...["模型提供商", "注入配置", "AGENTS.md"].map((label) => {
          const box = el("input", { type: "checkbox" }) as HTMLInputElement;
          settingChecks.push({ box, label });
          return el("label", { class: "modpack-radio" }, [box, el("span", {}, [label])]);
        }),
        el("h2", { class: "card__title" }, ["包名与落点"]),
        nameInput,
        el("div", { class: "settings-actions" }, [exportBtn]),
        status,
      ]);
      rightArea.append(section);

      exportBtn.addEventListener("click", () => {
        if (exportBtn.disabled) return;
        const chosenPlugins = Array.from(
          pluginListArea.querySelectorAll<HTMLInputElement>("input[type=checkbox]:checked"),
        ).length;
        const packName = nameInput.value.trim();
        if (packName === "") {
          status.textContent = "包名不能为空。";
          return;
        }
        const includeVendors = settingChecks[0].box.checked;
        const includeInject = settingChecks[1].box.checked;
        const includeAgentsMd = settingChecks[2].box.checked;
        if (chosenPlugins === 0 && !includeVendors && !includeInject && !includeAgentsMd) {
          status.textContent = "请至少勾选一个插件或一项设置内容。";
          return;
        }
        exportBtn.disabled = true;
        void (async () => {
          try {
            const { modpackExportWithProgress, pickSaveTgz } = await import("../services/modpack");
            const dest = await pickSaveTgz(packName);
            if (!dest) return;
            status.textContent = "正在打包…";
            const manifest = await modpackExportWithProgress(
              {
                instanceId: inst.id,
                name: packName,
                plugins: Array.from(
                  pluginListArea.querySelectorAll<HTMLInputElement>("input[type=checkbox]:checked"),
                ).map((box) => box.dataset.name ?? ""),
                includeVendors,
                includeInject,
                includeAgentsMd,
              },
              dest,
              (detail) => {
                status.textContent = detail;
              },
            );
            status.textContent = `已导出：${dest}（${manifest.plugins.length} 个插件 · ${manifest.type}）`;
          } catch (err) {
            status.textContent = `导出失败：${String(err).slice(0, 400)}`;
          } finally {
            exportBtn.disabled = false;
          }
        })();
      });
    }

    // ===== 左栏 =====
    const nav = el("nav", { class: "sub-nav" }, []);
    for (const id of ["overview", "settings", "plugins", "export"] as SectionId[]) {
      const btn = el("button", { type: "button", class: "sub-nav__item" }, [SECTION_LABELS[id]]);
      btn.addEventListener("click", () => {
        section = id;
        renderSection();
      });
      navButtons.set(id, btn);
      nav.append(btn);
    }

    // ===== 初始化 =====
    void (async () => {
      const id = getSelectedInstance();
      try {
        const list = await listInstances();
        currentInstance = list.find((i) => i.id === id) ?? null;
      } catch {
        currentInstance = null;
      }
      renderSection();
    })();

    return el("section", { class: "page" }, [
      el("div", { class: "sub" }, [
        el("aside", { class: "sub-left" }, [nav]),
        el("section", { class: "sub-right" }, [rightArea]),
      ]),
    ]);
  },
};
