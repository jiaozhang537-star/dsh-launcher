import { collapseArrow, el, pclCheck } from "../lib/dom";
import { collapseToggle } from "../lib/anim";
import { showHint } from "../lib/hint";
import { icon } from "../lib/icons";
import { getSelectedInstance } from "../lib/selected-instance";
import type { PageDefinition } from "../router";
import {
  getInjectOverride,
  instanceSetAdvanced,
  instanceSetIsolation,
  listInstances,
  openInstanceSubdir,
  renameInstance,
  setInjectOverride,
  type Instance,
} from "../services/instances";
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
  const { root: arrow, sync: syncArrow } = collapseArrow();
  syncArrow(collapsed);
  const header = el(
    "button",
    { type: "button", class: "collapse-bar" },
    [el("span", { class: "collapse-bar__title" }, [`${title}（${count}）`]), arrow],
  );
  list.hidden = collapsed;
  header.addEventListener("click", () => {
    const opening = list.hidden;
    // R5：高度动画展开/收起（anim.ts collapseToggle），箭头旋转逻辑不变。
    collapseToggle(list, opening);
    syncArrow(!opening);
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
    // 页头实例标识（对抗审查 R2，P4 挂账「无提示换实例」收口）：本页此前
    // 无任何「正在配置哪个版本」的可见标识（概览首卡 2026-09-20 裁决删除后）。
    const instanceSubject = el("div", { class: "sub-right__subject" }, []);
    // AGENTS.md 防抖计时器提为 render 级：切分区时清除挂起的保存，避免旧
    // 闭包在页面已重绘后照常写盘（写入目标本身正确，状态行已失效）。
    let agentsSaveTimer: ReturnType<typeof setTimeout> | undefined;

    function renderSection(): void {
      for (const [id, btn] of navButtons) {
        btn.classList.toggle("sub-nav__item--active", id === section);
      }
      clearTimeout(agentsSaveTimer);
      instanceSubject.replaceChildren();
      rightArea.replaceChildren();
      if (!currentInstance) {
        rightArea.append(el("p", { class: "settings-status" }, ["未选择实例——请先在「版本选择」页选择。"]));
        return;
      }
      // 每次渲染都重读选中实例：切换实例返回本页时标识自动刷新。
      instanceSubject.append(
        el("span", { class: "sub-right__subject-label" }, ["正在配置"]),
        el("span", { class: "sub-right__subject-name" }, [currentInstance.name]),
      );
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
        ]),
        renderQuickOpenCard(inst),
        renderSourcesCard(inst),
      );
    }

    /** 快捷打开卡（2026-09-12 追加批次）：横排两个按钮直达 skills / 对话文件夹
     *  （「打开源码」已删，2026-10-24 需求方指令——源码入口收归源码归档卡；
     *  点击后的灰字同批删除：成功无文案【文件夹在桌面打开自证】，失败走左下
     *  轻提示 showHint，卡内无常驻/点击灰字行）。 */
    function renderQuickOpenCard(inst: Instance): HTMLElement {
      const makeBtn = (label: string, which: "skills" | "sessions"): HTMLElement => {
        const btn = el("button", { type: "button", class: "btn btn--line" }, [label]);
        btn.addEventListener("click", () => {
          btn.disabled = true;
          void openInstanceSubdir(inst.id, which)
            .catch((err) => {
              showHint(`打开失败：${String(err).slice(0, 200)}`);
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
        ]),
      ]);
    }

    /** 源码归档卡片（FR-22 修订）：为该版本补下载官方源码到 `versions/<id>/sources/`。
     *  三态（需求方 2026-10-24）：无源码 = 只显示「下载源码」；下载中 = 按钮文字
     *  带进度；已下载 = 「下载源码」隐藏、「打开源码目录/删除源码」出现。
     *  状态灰字全删——进度与结果走按钮文字，status 仅承载错误与「本体未装」
     *  禁用原因（错误反馈不可省，成功态废话不可留）。 */
    function renderSourcesCard(inst: Instance): HTMLElement {
      const status = el("span", { class: "settings-path" }, [
        inst.dshVersion ? "" : "该版本尚未安装 DSH 本体——先到「下载」页安装，再下载源码。",
      ]);

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
            setMissing();
            status.textContent = "";
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

      // 两个稳定态 + 下载中的过渡态。downloading 防竞态：初始「已下载」查询
      // 晚于用户点下载返回时，不得把下载中的界面错切成已下载态。
      let downloading = false;
      const setDownloaded = (): void => {
        downloading = false;
        btn.hidden = true;
        openBtn.hidden = false;
        delBtn.hidden = false;
      };
      const setMissing = (): void => {
        btn.hidden = false;
        btn.disabled = !inst.dshVersion;
        btn.textContent = "下载源码";
        openBtn.hidden = true;
        delBtn.hidden = true;
      };

      void instanceSourcesDownloaded(inst.id)
        .then((downloadedAlready) => {
          if (downloadedAlready && !downloading) setDownloaded();
        })
        .catch(() => {});
      btn.addEventListener("click", () => {
        if (!inst.dshVersion || btn.disabled || downloading) return;
        downloading = true;
        btn.disabled = true;
        openBtn.hidden = true;
        delBtn.hidden = true;
        status.textContent = "";
        void downloadSourceArchive(inst.dshVersion, inst.id, ({ received, total }) => {
          if (total > 0) {
            btn.textContent = `下载中 ${Math.min(100, Math.round((received / total) * 100))}%`;
          }
        })
          .then(setDownloaded)
          .catch((err) => {
            downloading = false;
            btn.textContent = "重试下载";
            status.textContent = `下载失败：${String(err).slice(0, 260)}`;
          })
          .finally(() => {
            if (!btn.hidden) btn.disabled = !inst.dshVersion;
          });
      });
      return el("section", { class: "card settings-card" }, [
        el("h2", { class: "card__title" }, ["源码归档"]),
        el("div", { class: "settings-node-card" }, [btn, openBtn, delBtn, status]),
      ]);
    }

    // ===== 设置 =====
    function renderSettings(): void {
      const inst = currentInstance!;

      // 局部 AGENTS.md（FILE-LAYOUT：输入框落点 = versions/<id>/AGENTS.md 的源头，
      // 覆盖存实例 inject.json；未自设时由全局默认物化）。
      // 需求方定稿（2026-09-12）：①覆盖语义 = 局部设置后该版本只用自己的内容
      // （覆盖而非叠加，全局对该版本不再生效）；②只要输入框，输入完即自动保存；
      // ③启动该版本时生效（sync_instance → write_instance_files 物化）。
      const agentsArea = document.createElement("textarea");
      agentsArea.className = "settings-textarea";
      agentsArea.rows = 8;
      // 需求方 2026-10-24 定稿占位符（含与设置页 AGENTS.md 的优先级关系）；
      // 状态灰字只留错误反馈，成功与继承态说明全删（占位符已承载语义）。
      agentsArea.placeholder =
        "该版本的全局系统指令，优先级大于设置页AGENTS.md，只生效一个";
      const agentsStatus = el("p", { class: "settings-path" }, [""]);
      let agentsLoaded = false;
      agentsArea.addEventListener("input", () => {
        if (!agentsLoaded) return;
        clearTimeout(agentsSaveTimer);
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
            } catch (err) {
              agentsStatus.textContent = `保存失败：${String(err).slice(0, 240)}`;
            }
          })();
        }, 600);
      });
      async function loadAgentsState(): Promise<void> {
        try {
          const over = await getInjectOverride(inst.id);
          const own = over.agentsMd ?? null;
          // 输入框只显示实例自己的覆盖，不得回填全局内容：回填会把「继承全局」
          // 伪装成实例设置——删除后重进输入框又出现全局文字，读作「删了又回来」
          // （2026-10-03 需求方报障「AGENTS 是假的」）。
          agentsArea.value = own ?? "";
        } catch (err) {
          agentsStatus.textContent = `读取失败：${String(err).slice(0, 240)}`;
        } finally {
          agentsLoaded = true;
        }
      }

      // FR-12：启动参数 / 工作目录覆盖（下次启动生效）。
      const argsInput = el("input", {
        type: "text",
        class: "settings-input",
        value: inst.launchArgs ?? "",
        placeholder: "如 --verbose（空格分隔；留空 = 无附加参数）",
      });
      const workInput = el("input", {
        type: "text",
        class: "settings-input",
        value: inst.workDir ?? "",
        placeholder: "绝对路径（留空 = 实例内 workspace）",
      });
      const advStatus = el("p", { class: "settings-path" }, [""]);
      const advSave = el("button", { type: "button", class: "btn btn--ghost settings-inline-btn" }, ["保存"]);
      advSave.addEventListener("click", () => {
        advSave.disabled = true;
        void instanceSetAdvanced(inst.id, {
          launchArgs: argsInput.value.trim() || null,
          workDir: workInput.value.trim() || null,
        })
          .then(() => {
            advStatus.textContent = "已保存——下次启动该版本时生效。";
          })
          .catch((err) => {
            advStatus.textContent = `保存失败：${String(err).slice(0, 200)}`;
          })
          .finally(() => {
            advSave.disabled = false;
          });
      });

      // FR-17：实例隔离开关（两击确认——影响明示后再执行；运行中由后端拒绝）。
      const isoStatus = el("p", { class: "settings-path" }, [""]);
      const isoBtn = el("button", { type: "button", class: "btn btn--ghost settings-inline-btn" }, [""]);
      let armed = false;
      let disarmTimer: ReturnType<typeof setTimeout> | undefined;
      const syncIso = (): void => {
        isoBtn.textContent = armed ? "确认切换？" : inst.isolation ? "关闭隔离" : "开启隔离";
        isoStatus.textContent = armed
          ? inst.isolation
            ? "关闭后：插件目录并入共享区（与其他关闭隔离的实例共用，同名内容改名留档）；配置改由全局模板单源；会话与工作区仍独立。运行中的实例需先停止。"
            : "开启后：从共享池复制一份插件到本实例（共享池保留）；配置恢复「全局默认 ⊕ 本实例覆盖」。"
          : inst.isolation
            ? "当前：隔离——插件与配置独立于其他实例。"
            : "当前：共享——插件目录联接至共享池，配置单源全局模板（实例覆盖暂不参与）。";
      };
      syncIso();
      isoBtn.addEventListener("click", () => {
        if (!armed) {
          armed = true;
          syncIso();
          disarmTimer = setTimeout(() => {
            armed = false;
            syncIso();
          }, 6000);
          return;
        }
        clearTimeout(disarmTimer);
        armed = false;
        isoBtn.disabled = true;
        void instanceSetIsolation(inst.id, !inst.isolation)
          .then((report) => {
            inst.isolation = !inst.isolation;
            isoStatus.textContent = report;
            syncIso();
          })
          .catch((err) => {
            isoStatus.textContent = `切换失败：${String(err).slice(0, 200)}`;
          })
          .finally(() => {
            isoBtn.disabled = false;
          });
      });

      rightArea.append(
        el("section", { class: "card settings-card" }, [
          el("h2", { class: "card__title" }, ["AGENTS.md"]),
          agentsArea,
          agentsStatus,
        ]),
        el("section", { class: "card settings-card" }, [
          el("h2", { class: "card__title" }, ["启动参数与工作目录"]),
          el("div", { class: "settings-row" }, [el("span", {}, ["启动参数："]), argsInput]),
          el("div", { class: "settings-row" }, [el("span", {}, ["工作目录："]), workInput]),
          el("div", { class: "settings-row" }, [advSave]),
          advStatus,
        ]),
        el("section", { class: "card settings-card" }, [
          el("h2", { class: "card__title" }, ["实例隔离"]),
          el("div", { class: "settings-row" }, [isoBtn]),
          isoStatus,
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
        const enableCheck = pclCheck({ checked: p.inBundles });
        const enableBox = enableCheck.input;
        if (p.group === "builtin") {
          enableBox.disabled = true;
          enableCheck.root.title = "内置组件，恒启用";
        } else if (!p.declaresPatch) {
          // 纯依赖包（如 UI 库，keywords 打了 dsh-plugin 混进市场的典型）：
          // 没有可加载的插件层，无「启用」概念——禁用灰框如实呈现。
          enableBox.disabled = true;
          enableCheck.root.title = "依赖库，不可作为插件启用";
        } else {
          enableCheck.root.title = p.inBundles
            ? "取消勾选即停用（下次启动生效）"
            : "勾选即启用（下次启动生效）";
          enableBox.addEventListener("change", () => {
            enableBox.disabled = true;
            setStatus(`正在${enableBox.checked ? "启用" : "停用"} ${p.name}…`);
            void pluginSetEnabled(inst.id, p.name, enableBox.checked)
              .then(() => {
                setStatus(`${enableBox.checked ? "已启用" : "已停用"} ${p.name}（下次启动生效）`);
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
        const remove = el("button", { type: "button", class: "plugin-row__remove", title: "删除插件" }, [icon("trash", 15)]);
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
          enableCheck.root,
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
            const check = pclCheck({ checked: !c.disabled });
            const box = check.input;
            check.root.title = c.disabled
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
              check.root,
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

      function rowUpdate(u: { name: string; current: string; latest: string; updatable: boolean }): HTMLElement {
        const badge = u.updatable
          ? el("span", { class: "mk-badge mk-badge--latest", title: `最新 v${u.latest}` }, ["可更新"])
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
              setStatus("该实例还没有已装插件。");
              return;
            }
            setStatus(
              `已扫描 ${list.plugins.length} 个插件（内置 ${builtin.length} / 第三方 ${third.length} / 自建 ${self.length}）。`,
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
            const check = pclCheck({ label: `${p.name} ${p.version ? `v${p.version}` : ""}` });
            check.input.dataset.name = p.name;
            // 行容器沿用 modpack-radio 版式（原 label 包 input 改为 div 包自绘勾选）。
            pluginListArea.append(el("div", { class: "modpack-radio" }, [check.root]));
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
          const check = pclCheck({ label });
          settingChecks.push({ box: check.input, label });
          return el("div", { class: "modpack-radio" }, [check.root]);
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
      el("div", { class: "sub sub--compact" }, [
        el("aside", { class: "sub-left" }, [nav]),
        el("section", { class: "sub-right" }, [instanceSubject, rightArea]),
      ]),
    ]);
  },
};
