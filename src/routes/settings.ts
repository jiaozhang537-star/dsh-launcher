/** 设置页：左分类（模型提供商 / 配置 / 个性化 / 热键）+ 右内容卡片，全局左右栏布局。 */
import { el, pclCheck, pclRadio } from "../lib/dom";
import { icon } from "../lib/icons";
import type { PageDefinition } from "../router";
import { DSH_SHORTCUTS, DSH_SHORTCUT_SECTIONS } from "../lib/dsh-hotkeys";
import {
  isSettingsCatHidden,
  isTempShowAll,
  loadUiPrefs,
  updateUiPrefs,
  UI_PREFS_EVENT,
  type MoreCatId,
  type SettingsCatId,
  type TitlebarMode,
} from "../lib/uiPrefs";
import { applyBackground, clearBackgroundLayer, setBackgroundOpacity } from "../lib/background";
import {
  clearBackground,
  clearLogo,
  openBackgroundFolder,
  setLogoFromFile,
} from "../services/personalize";
import {
  cancelDownload,
  detectSystemNode,
  downloadNode,
  getDefaultVersion,
  listManagedVersions,
  setDefaultVersion,
  type SystemNodeInfo,
} from "../services/nodeRuntime";
import {
  getGlobalAgentsMd,
  getInstancesDir,
  nativeReconcile,
  setGlobalAgentsMd,
} from "../services/instances";
import { recordProgress, terminalFailure, updateTask, upsertTask } from "../lib/downloadTasks";
import { getMirror, setCommunityMirror, setMirror, testMirror, type MirrorSettingsInfo } from "../services/gateway";
import {
  clearKey,
  forgetVendor,
  getStatus,
  setEnabled,
  setKey,
  setModels,
  setAddress,
  KEY_MASK,
  type KeyvaultStatus,
  type VendorKeyStatus,
} from "../services/keyvault";
import { fetchModels } from "../services/provider";
import { keySaveEligible } from "../lib/vendor";
import { providerIconPath } from "../lib/providerIcon";
import { PRESET_HUES, deriveThemeTokens, hexToHsl, loadTheme, saveTheme, clearThemeOverride, type ThemeColor } from "../lib/theme";
import {
  getDefaultIsolation,
  getLauncherVisibility,
  getMaxDownloads,
  getProxy,
  setDefaultIsolation,
  setLauncherVisibility,
  setMaxDownloads,
  setProxy,
} from "../services/launcherConfig";
import {
  addCustomProvider,
  fetchProvidersView,
  removeCustomProvider,
  setProvidersOrder,
  type ApiStyle,
  type CatalogEntry,
} from "../services/providers";

/** 构建期回退版本号（Tauri 环境下由 getVersion() 覆写为 tauri.conf.json 值）。 */
const APP_VERSION = "0.1.0";

/** 提供方目录条目的统一展示素材（内置与自定义同构）。 */
interface ProviderMeta {
  id: string;
  label: string;
  /** 目录默认地址（卡片地址留空 = 用它）。 */
  base: string;
  style: ApiStyle;
  iconUrl?: string;
  isCustom: boolean;
}

const STYLE_LABELS: Record<ApiStyle, string> = {
  openai: "OpenAI 兼容",
  responses: "OpenAI Responses",
  anthropic: "Anthropic",
  gemini: "Gemini",
  ollama: "Ollama",
};

function formatBytes(n: number): string {
  if (n <= 0) {
    return "0 B";
  }
  const units = ["B", "KB", "MB", "GB"];
  const i = Math.min(Math.floor(Math.log(n) / Math.log(1024)), units.length - 1);
  return `${(n / 1024 ** i).toFixed(i === 0 ? 0 : 1)} ${units[i]}`;
}

function progressLine(label: HTMLElement): HTMLElement {
  return el("div", { class: "settings-progress" }, [label]);
}

// ===== 模型提供商 =====

type ProviderNote = (text: string, isError?: boolean) => void;

/** 单个提供方卡片（FR-20）：默认折叠；头部 = 拖拽把手 + 启用勾选 + 图标 +
 *  名称 + 徽章 + 箭头；展开 = 地址/Key/模型拉取（按接口形态）/模型 ID
 *  （+ 自定义：删除；+ 内置：从列表移除）。`meta` 携带目录素材，内置与
 *  自定义同构。 */
function renderProviderCard(
  vendor: VendorKeyStatus,
  meta: ProviderMeta,
  note: ProviderNote,
  onRemove: () => void,
): HTMLElement {
  let models = [...vendor.models];
  let fetched: string[] = [];
  let open = false;

  const card = el("section", { class: `provider-card ${open ? "provider-card--open" : ""}` });
  card.dataset.providerId = meta.id;
  const body = el("div", { class: "provider-card__body" });
  if (!open) body.hidden = true;

  // ── 头部：整卡可拖拽（走查定稿：六点把手废除）+ 启用勾选（点击不折叠）
  //    + 图标 + 名称（点击折叠）+ 徽章 + 箭头 ──
  const enableCheck = pclCheck({ checked: vendor.enabled });
  const enableBox = enableCheck.input;
  enableCheck.root.addEventListener("click", (event) => event.stopPropagation());
  enableBox.addEventListener("change", () => {
    void (async () => {
      enableBox.disabled = true;
      try {
        await setEnabled(vendor.vendor, enableBox.checked);
      } catch (err) {
        note(`操作失败：${String(err)}`, true);
        enableBox.checked = !enableBox.checked;
      } finally {
        enableBox.disabled = false;
      }
    })();
  });
  const vendorIcon = meta.iconUrl
    ? (() => {
        const img = el("img", { class: "provider-card__icon", src: meta.iconUrl!, alt: "", draggable: "false" });
        img.addEventListener("error", () => img.replaceWith(letterTile(meta.label)));
        return img;
      })()
    : letterTile(meta.label);
  const chevron = el("span", { class: "provider-card__chevron" }, [icon("chevron-down", 12)]);
  const keyReady = vendor.hasKey || vendor.envAuto;
  const badge = el("span", { class: `settings-badge ${keyReady ? "settings-badge--ok" : "settings-badge--bad"}` }, [
    vendor.hasKey ? "保险库已存 Key" : vendor.envAuto ? "环境变量已提供" : "无 Key（可稍后补）",
  ]);
  const styleTag = el("span", { class: "mk-badge" }, [STYLE_LABELS[meta.style]]);
  const head = el(
    "div",
    { class: "provider-card__head", title: "拖拽卡片调整顺序（即启动页下拉顺序）" },
    [enableCheck.root, vendorIcon, el("span", { class: "provider-card__name" }, [meta.label]),
      el("span", { class: "provider-card__spacer" }, []), styleTag, badge, chevron],
  );
  head.addEventListener("click", () => {
    open = !open;
    card.classList.toggle("provider-card--open", open);
    body.hidden = !open;
  });

  // ── 地址 ──
  const addrInput = el("input", { type: "text", class: "settings-input", placeholder: meta.base }) as HTMLInputElement;
  addrInput.value = vendor.address ?? "";
  const addrSave = el("button", { type: "button", class: "btn btn--ghost settings-inline-btn" }, ["保存地址"]);
  addrSave.addEventListener("click", () => {
    void (async () => {
      addrSave.disabled = true;
      try {
        await setAddress(vendor.vendor, addrInput.value);
      } catch (err) {
        note(`保存失败：${String(err)}`, true);
      } finally {
        addrSave.disabled = false;
      }
    })();
  });

  // ── Key（两态掩码）──
  const keyInput = el("input", { type: "password", class: "settings-input" }) as HTMLInputElement;
  if (keyReady) keyInput.value = KEY_MASK;
  keyInput.placeholder = keyReady ? "" : "sk-…（保存后加密落盘）";
  // P3-02：dirty 标志位——只有真实键入/粘贴才算改过；掩码回贴不误存。
  let keyEdited = false;
  keyInput.addEventListener("input", () => {
    keyEdited = true;
  });
  const keySave = el("button", { type: "button", class: "btn btn--ghost settings-inline-btn" }, ["保存 Key"]);
  keySave.addEventListener("click", () => {
    void (async () => {
      const value = keyInput.value.trim();
      if (!keySaveEligible(keyInput.value, keyEdited)) {
        return;
      }
      keySave.disabled = true;
      try {
        await setKey(vendor.vendor, value);
        keyEdited = false;
        keyInput.value = KEY_MASK;
      } catch (err) {
        note(`保存失败：${String(err).slice(0, 200)}`, true);
      } finally {
        keySave.disabled = false;
      }
    })();
  });
  const keyClear = el("button", { type: "button", class: "btn btn--line settings-inline-btn" }, ["清除"]);
  if (!vendor.hasKey) {
    keyClear.disabled = true;
    keyClear.title = vendor.envAuto ? "Key 来自环境变量，无法在此清除" : "尚无已存 Key";
  }
  keyClear.addEventListener("click", () => {
    void (async () => {
      keyClear.disabled = true;
      try {
        await clearKey(vendor.vendor);
        keyEdited = false;
        keyInput.value = "";
        keyInput.placeholder = "sk-…（保存后加密落盘）";
      } catch (err) {
        note(`清除失败：${String(err).slice(0, 200)}`, true);
      } finally {
        keyClear.disabled = false;
      }
    })();
  });

  // ── 模型拉取（OpenAI 兼容 /models；Key 取自保险库）──
  const fetchedRow = el("div", { class: "provider-fetched" }, []);
  const fetchBtn = el("button", { type: "button", class: "btn btn--line settings-inline-btn" }, ["从 API 拉取模型"]);
  fetchBtn.addEventListener("click", () => {
    void (async () => {
      fetchBtn.disabled = true;
      fetchBtn.textContent = "拉取中…";
      try {
        fetched = await fetchModels(vendor.vendor, addrInput.value);
        renderFetched();
      } catch (err) {
        fetchedRow.replaceChildren();
        note(`拉取失败：${String(err).slice(0, 200)}`, true);
      } finally {
        fetchBtn.disabled = false;
        fetchBtn.textContent = "从 API 拉取模型";
      }
    })();
  });
  function renderFetched(): void {
    fetchedRow.replaceChildren();
    for (const model of fetched) {
      const added = models.includes(model);
      const chip = el("button", { type: "button", class: `model-chip ${added ? "model-chip--added" : ""}` }, [
        ...(added ? [icon("check", 11), "已添加"] : [icon("plus", 11), model]),
      ]);
      if (!added) {
        chip.addEventListener("click", () => {
          void (async () => {
            models = [...models, model];
            await setModels(vendor.vendor, models);
            refreshAdded();
            renderFetched();
          })();
        });
      }
      fetchedRow.append(chip);
    }
  }

  // ── 模型 ID（已添加列表 + 手动添加）──
  const addedRow = el("div", { class: "provider-fetched" }, []);
  function refreshAdded(): void {
    addedRow.replaceChildren();
    for (const model of models) {
      const chip = el("span", { class: "model-chip model-chip--added" }, [model]);
      const remove = el("button", { type: "button", class: "model-chip__remove", title: "移除" }, ["×"]);
      remove.addEventListener("click", () => {
        void (async () => {
          models = models.filter((m) => m !== model);
          await setModels(vendor.vendor, models);
          refreshAdded();
          renderFetched();
        })();
      });
      chip.append(remove);
      addedRow.append(chip);
    }
    if (models.length === 0) {
      addedRow.append(el("span", { class: "settings-progress-label" }, ["暂无——拉取或手动添加"]));
    }
  }
  const addInput = el("input", { type: "text", class: "settings-input", placeholder: "模型 ID" }) as HTMLInputElement;
  const addBtn = el("button", { type: "button", class: "btn btn--ghost settings-inline-btn" }, ["添加"]);
  addBtn.addEventListener("click", () => {
    void (async () => {
      const value = addInput.value.trim();
      if (value === "" || models.includes(value)) {
        addInput.value = "";
        return;
      }
      addBtn.disabled = true;
      try {
        models = [...models, value];
        await setModels(vendor.vendor, models);
        addInput.value = "";
        refreshAdded();
        renderFetched();
      } catch (err) {
        note(`添加失败：${String(err)}`, true);
        models = models.filter((m) => m !== value);
      } finally {
        addBtn.disabled = false;
      }
    })();
  });
  refreshAdded();

  if (meta.isCustom) {
    const removeBtn = el("button", { type: "button", class: "btn btn--line settings-inline-btn" }, ["删除此自定义提供方"]);
    removeBtn.addEventListener("click", (event) => {
      event.stopPropagation();
      void removeCustomProvider(meta.id)
        .then(() => {
          onRemove();
        })
        .catch((err) => note(`删除失败：${String(err)}`, true));
    });
    body.append(el("div", { class: "provider-card__row" }, [removeBtn]));
  } else {
    // 内置提供方（按需添加式）：可从列表移除——整条记录（含已存 Key）清除，
    // 之后可在「从预设添加」重新加入。两击确认防误触。
    const removeBtn = el("button", { type: "button", class: "btn btn--line settings-inline-btn" }, ["从列表移除"]);
    let armed = false;
    let resetTimer: ReturnType<typeof setTimeout> | null = null;
    removeBtn.addEventListener("click", (event) => {
      event.stopPropagation();
      if (!armed) {
        armed = true;
        removeBtn.textContent = "确认移除（将清除已存 Key）";
        removeBtn.classList.add("btn--danger");
        resetTimer = setTimeout(() => {
          armed = false;
          removeBtn.textContent = "从列表移除";
          removeBtn.classList.remove("btn--danger");
        }, 3000);
        return;
      }
      if (resetTimer) clearTimeout(resetTimer);
      void forgetVendor(meta.id)
        .then(() => {
          onRemove();
        })
        .catch((err) => note(`移除失败：${String(err)}`, true));
    });
    body.append(el("div", { class: "provider-card__row" }, [removeBtn]));
  }

  body.append(
    el("div", { class: "provider-card__row" }, [el("span", { class: "provider-card__label" }, ["地址"]), addrInput, addrSave]),
    el("div", { class: "provider-card__row" }, [el("span", { class: "provider-card__label" }, ["Key"]), keyInput, keySave, keyClear]),
    el("div", { class: "provider-card__row" }, [el("span", { class: "provider-card__label" }, ["模型拉取"]), fetchBtn]),
    fetchedRow,
    el("div", { class: "provider-card__row" }, [el("span", { class: "provider-card__label" }, ["模型 ID"]), addInput, addBtn]),
    el("div", { class: "provider-card__row" }, [el("span", { class: "provider-card__label" }, ["已添加"]), addedRow]),
  );
  card.append(head, body);
  return card;
}

/** 字母块兜底（无图标 slug 的提供方）。走查修正：此前误用市场组件的
 *  mk-icon（44×44）类，导致无图标厂商的卡片整体偏大——provider 卡内
 *  应用 22×22 的 provider-card__icon 尺寸。 */
function letterTile(label: string): HTMLElement {
  return el("span", { class: "provider-card__icon provider-card__icon--letter" }, [label.charAt(0) || "?"]);
}

/** 按需添加式的「已添加」判定：凭据库里有记录的内置厂商；deepseek 恒在列
 *  （默认视图即启用），自定义提供方全部在列。 */
function addedBuiltinsOf(status: KeyvaultStatus): Set<string> {
  return new Set(status.vendors.map((v) => v.vendor));
}

async function renderProvidersPane(pane: HTMLElement, pendingNote?: string): Promise<void> {
  pane.replaceChildren();
  // 状态行放面板顶部：启用/保存/添加/移除的成败反馈必须肉眼可见（此前钉在
  // 面板最底部，勾选失败回弹时用户根本看不到原因）。
  const statusLine = el("p", { class: "settings-status" }, []);
  const note: ProviderNote = (text, isError) => {
    statusLine.textContent = text;
    statusLine.classList.toggle("settings-status--error", isError ?? false);
  };

  let view: Awaited<ReturnType<typeof fetchProvidersView>>;
  let status: KeyvaultStatus;
  try {
    // FR-25 S2（进页读一次的非实时口径）：先把实例侧（网页端/手改）新增的
    // 提供商收编成卡（含模型清单与 Key 迁移），下面的目录/状态读取就能看到；
    // 失败不阻塞页面（下次启动的同步链兜底）。
    await nativeReconcile().catch(() => {});
    [view, status] = await Promise.all([fetchProvidersView(), getStatus()]);
  } catch (err) {
    pane.append(
      el("p", { class: "settings-status settings-status--error" }, [`读取提供方目录失败：${String(err)}`]),
      statusLine,
    );
    return;
  }

  // 目录素材映射（内置 + 自定义同构）。
  const metas = new Map<string, ProviderMeta>();
  for (const entry of view.catalog) {
    metas.set(entry.id, {
      id: entry.id,
      label: entry.label,
      base: entry.base,
      style: entry.style,
      iconUrl: providerIconPath(entry.id),
      isCustom: false,
    });
  }
  for (const custom of view.custom) {
    metas.set(custom.id, {
      id: custom.id,
      label: custom.name,
      base: custom.base,
      style: custom.style,
      isCustom: true,
    });
  }

  // 已添加集合与渲染序（按需添加式）：只渲染已添加的卡片，顺序沿用
  // 已存 order → 目录缺省序 → 自定义追加。
  const recorded = addedBuiltinsOf(status);
  const isAdded = (id: string): boolean =>
    metas.get(id)?.isCustom === true || recorded.has(id) || id === "deepseek";
  const ordered: string[] = [];
  for (const id of view.order) {
    if (metas.has(id) && isAdded(id) && !ordered.includes(id)) ordered.push(id);
  }
  for (const entry of view.catalog) {
    if (isAdded(entry.id) && !ordered.includes(entry.id)) ordered.push(entry.id);
  }
  for (const custom of view.custom) {
    if (!ordered.includes(custom.id)) ordered.push(custom.id);
  }
  const vaultOf = (id: string): VendorKeyStatus =>
    status.vendors.find((v) => v.vendor === id) ?? {
      vendor: id,
      enabled: false,
      hasKey: false,
      envAuto: false,
      models: [],
      address: null,
    };

  // 把渲染序持久化（set_order 必须覆盖全部内置 ∪ 自定义 id，未渲染的按
  // 目录缺省序补齐）。`appended` 追加到已渲染末尾（「添加即追加列表末尾」）。
  const persistOrder = async (appended?: string): Promise<void> => {
    const full = [...ordered];
    if (appended && !full.includes(appended)) full.push(appended);
    for (const entry of view.catalog) {
      if (!full.includes(entry.id)) full.push(entry.id);
    }
    for (const custom of view.custom) {
      if (!full.includes(custom.id)) full.push(custom.id);
    }
    await setProvidersOrder(full);
  };

  const listWrap = el("div", { class: "provider-list" }, []);
  for (const id of ordered) {
    const card = renderProviderCard(vaultOf(id), metas.get(id)!, note, () => {
      void renderProvidersPane(pane);
    });
    listWrap.append(card);
  }

  // 拖拽排序（走查定稿）：整卡「拿起」实时跟手——pointerdown 抓住卡片头，
  // 移动超阈值后生成跟随鼠标的浮动影子、原卡半透明占位并实时换位，松手
  // 落位提交（补齐全量 id 走 persistOrder）。六点把手已废除。
  let session: {
    card: HTMLElement;
    startX: number;
    startY: number;
    grabX: number;
    grabY: number;
    ghost: HTMLElement;
    active: boolean;
  } | null = null;
  const endSession = (): void => {
    if (!session) return;
    session.ghost.remove();
    session.card.classList.remove("provider-card--placeholder");
    session = null;
  };
  const commitDrag = (): void => {
    const domOrder = [...listWrap.querySelectorAll<HTMLElement>(".provider-card")]
      .map((c) => c.dataset.providerId ?? "")
      .filter(Boolean);
    ordered.splice(0, ordered.length, ...domOrder);
    void persistOrder()
      .then(() => renderProvidersPane(pane))
      .catch((err) => note(`排序保存失败：${String(err)}`, true));
  };
  listWrap.addEventListener("pointerdown", (event) => {
    if (event.button !== 0) return;
    const target = event.target as HTMLElement;
    if (target.closest("input,button,select,textarea,label,a")) return;
    const headEl = target.closest<HTMLElement>(".provider-card__head");
    const card = target.closest<HTMLElement>(".provider-card");
    if (!headEl || !card?.dataset.providerId) return;
    const rect = card.getBoundingClientRect();
    const ghost = card.cloneNode(true) as HTMLElement;
    ghost.classList.add("provider-card--ghost");
    Object.assign(ghost.style, {
      position: "fixed",
      left: `${rect.left}px`,
      top: `${rect.top}px`,
      width: `${rect.width}px`,
      margin: "0",
    });
    session = {
      card,
      startX: event.clientX,
      startY: event.clientY,
      grabX: event.clientX - rect.left,
      grabY: event.clientY - rect.top,
      ghost,
      active: false,
    };
    // 捕获指针：即便在窗口外松手，pointerup 也保证送达（否则影子残留——
    // 「拖拽完还这么大」的真凶）。
    try {
      headEl.setPointerCapture(event.pointerId);
    } catch {
      // 指针可能已失效；后续 window 监听仍兜底。
    }
    const onMove = (e: PointerEvent): void => {
      if (!session) return;
      if (!session.active) {
        // 阈值内 = 还是点击（折叠）；超阈值 = 卡片被「拿起」。
        if (Math.hypot(e.clientX - session.startX, e.clientY - session.startY) < 5) return;
        session.active = true;
        session.card.classList.add("provider-card--placeholder");
        // 拖过的这趟 pointer 序列结尾的 click 不得再触发折叠。
        session.card.addEventListener("click", (c) => c.stopImmediatePropagation(), {
          capture: true,
          once: true,
        });
        document.body.append(session.ghost);
      }
      session.ghost.style.top = `${e.clientY - session.grabY}px`;
      // 只上下跟手（走查定稿）：left 固定为抓起时的列位，不随鼠标横移。
      // 实时换位：指针纵向落在哪张其它卡上，就把拖拽卡插到它前/后。
      const listRect = listWrap.getBoundingClientRect();
      const inListX = e.clientX >= listRect.left && e.clientX <= listRect.right;
      if (!inListX) return;
      for (const other of listWrap.querySelectorAll<HTMLElement>(".provider-card")) {
        if (other === session.card) continue;
        const r = other.getBoundingClientRect();
        if (e.clientY >= r.top && e.clientY <= r.bottom) {
          const before = e.clientY < r.top + r.height / 2;
          listWrap.insertBefore(session.card, before ? other : other.nextSibling);
          break;
        }
      }
    };
    const onUp = (): void => {
      window.removeEventListener("pointermove", onMove);
      window.removeEventListener("pointerup", onUp);
      window.removeEventListener("pointercancel", onCancel);
      window.removeEventListener("mouseup", onUp);
      window.removeEventListener("blur", onCancel);
      const wasActive = session?.active ?? false;
      endSession();
      if (wasActive) commitDrag();
    };
    const onCancel = (): void => {
      window.removeEventListener("pointermove", onMove);
      window.removeEventListener("pointerup", onUp);
      window.removeEventListener("pointercancel", onCancel);
      window.removeEventListener("mouseup", onUp);
      window.removeEventListener("blur", onCancel);
      endSession();
    };
    window.addEventListener("pointermove", onMove);
    window.addEventListener("pointerup", onUp);
    window.addEventListener("pointercancel", onCancel);
    // 兜底三连：pointer capture 万一失效（窗外松手/失焦/系统抢占），
    // 任一后续事件都能把影子清掉——「拖拽完卡片还变大」的根因就是漏清理。
    window.addEventListener("mouseup", onUp);
    window.addEventListener("blur", onCancel);
  });

  const hooks: ProviderAddHooks = {
    appendOrder: (id) => {
      ordered.push(id);
      return persistOrder();
    },
    refresh: (message) => {
      void renderProvidersPane(pane, message);
    },
  };
  pane.append(
    statusLine,
    listWrap,
    el("div", { class: "settings-row" }, [
      (() => {
        const btn = el("button", { type: "button", class: "btn btn--ghost settings-inline-btn" }, [icon("plus", 13), "添加提供方"]);
        btn.addEventListener("click", () => openPresetProviderModal(view, status, note, hooks));
        return btn;
      })(),
      (() => {
        const btn = el("button", { type: "button", class: "btn btn--ghost settings-inline-btn" }, [icon("plus", 13), "自定义提供方"]);
        btn.addEventListener("click", () => openCustomProviderModal(hooks));
        return btn;
      })(),
    ]),
  );
  if (pendingNote) note(pendingNote);
}

/** 接口形态 → 对话端点路径（地址+格式预览用：壳只负责拉模型清单，对话
 *  端点由 DSH 消费，此处按各形态的通行约定展示完整样貌）。 */
const STYLE_CHAT_SUFFIX: Record<ApiStyle, string> = {
  openai: "/chat/completions",
  responses: "/responses",
  anthropic: "/messages",
  gemini: "/models/{model}:generateContent",
  ollama: "/api/chat",
};

/** 接口形态 → API 地址占位示例（走查定稿：标题放格子外、提示放占位符）。 */
const STYLE_BASE_HINT: Record<ApiStyle, string> = {
  openai: "https://.../v1",
  responses: "https://.../v1",
  anthropic: "https://.../v1",
  gemini: "https://.../v1beta",
  ollama: "http://127.0.0.1:11434",
};

/** 地址+格式完整预览：base（输入值，空则用占位示例）+ 该形态的对话端点。 */
function endpointPreview(base: string, style: ApiStyle): string {
  const trimmed = base.trim().replace(/\/+$/, "");
  const prefix = trimmed || STYLE_BASE_HINT[style];
  return `${prefix}${STYLE_CHAT_SUFFIX[style]}`;
}

/** 一行带外置标题的表单格（走查定稿：标题在格子外、提示在占位符里）。 */
function labeledRow(label: string, control: HTMLElement): HTMLElement {
  return el("div", { class: "settings-row" }, [
    el("span", { class: "provider-card__label" }, [label]),
    control,
  ]);
}

/** 悬浮窗壳：遮罩点击空白处/「取消」关闭；返回遮罩节点供成功后主动关闭。 */
function openModalShell(title: string, bodyChildren: HTMLElement[]): { overlay: HTMLElement; close: () => void } {
  const overlay = el("div", { class: "modal-overlay" }, []);
  const close = (): void => overlay.remove();
  overlay.addEventListener("click", (event) => {
    if (event.target === overlay) close();
  });
  const cancel = el("button", { type: "button", class: "btn btn--line" }, ["取消"]);
  cancel.addEventListener("click", close);
  overlay.append(
    el("div", { class: "error-modal" }, [
      el("h3", { class: "error-modal__title" }, [title]),
      el("div", { class: "error-modal__body" }, bodyChildren),
      el("div", { class: "error-modal__btns" }, [cancel]),
    ]),
  );
  document.body.append(overlay);
  return { overlay, close };
}

interface ProviderAddHooks {
  appendOrder: (id: string) => Promise<void>;
  refresh: (message?: string) => void;
}

/** 「⊕ 添加提供方」悬浮窗：选预设 + 全字段配置（地址预填可改、Key 可当场
 *  填）——添加 = 建档默认启用 + 按需写地址覆盖/Key，追加列表末尾。 */
function openPresetProviderModal(
  view: Awaited<ReturnType<typeof fetchProvidersView>>,
  status: KeyvaultStatus,
  note: ProviderNote,
  hooks: ProviderAddHooks,
): void {
  const recorded = addedBuiltinsOf(status);
  const presets = view.catalog.filter((entry) => entry.id !== "deepseek" && !recorded.has(entry.id));
  if (presets.length === 0) {
    note("内置提供方均已添加");
    return;
  }
  const presetSel = document.createElement("select");
  presetSel.className = "settings-select";
  // 分组下拉：精选在前，DSH 本体 llm 层原生实装的提供方在后。
  for (const [groupId, groupLabel] of [
    ["curated", "常用"],
    ["dsh", "DSH 内置"],
  ] as const) {
    const inGroup = presets.filter((entry) => (entry.group || "curated") === groupId);
    if (inGroup.length === 0) continue;
    const group = document.createElement("optgroup");
    group.label = groupLabel;
    for (const entry of inGroup) {
      const o = document.createElement("option");
      o.value = entry.id;
      o.textContent = `${entry.label}（${STYLE_LABELS[entry.style]}）`;
      group.append(o);
    }
    presetSel.append(group);
  }
  const current = (): CatalogEntry => presets.find((e) => e.id === presetSel.value)!;
  const baseInput = el("input", { type: "text", class: "settings-input" }) as HTMLInputElement;
  const keyInput = el("input", { type: "password", class: "settings-input", placeholder: "可留空——稍后在卡片配置" }) as HTMLInputElement;
  const preview = el("p", { class: "settings-path" }, []);
  const sync = (): void => {
    const entry = current();
    preview.textContent = `完整端点 ${endpointPreview(baseInput.value || entry.base, entry.style)}`;
  };
  presetSel.addEventListener("change", () => {
    baseInput.value = current().base; // 换预设 = 重置为该提供方默认地址
    baseInput.placeholder = STYLE_BASE_HINT[current().style];
    sync();
  });
  baseInput.addEventListener("input", sync);
  baseInput.value = current().base;
  sync();

  const errLine = el("p", { class: "settings-status settings-status--error" }, []);
  const addBtn = el("button", { type: "button", class: "btn btn--ghost settings-inline-btn" }, [icon("plus", 13), "添加到列表"]);
  const { overlay, close } = openModalShell("添加提供方", [
    labeledRow("提供方", presetSel),
    labeledRow("API 地址", baseInput),
    labeledRow("API Key", keyInput),
    preview,
    errLine,
  ]);
  const btns = overlay.querySelector(".error-modal__btns")!;
  btns.prepend(addBtn);
  addBtn.addEventListener("click", () => {
    const entry = current();
    void (async () => {
      addBtn.disabled = true;
      errLine.textContent = "";
      try {
        await setEnabled(entry.id, true); // 建档 + 默认启用（启用即进启动页）
        const key = keyInput.value.trim();
        if (key) await setKey(entry.id, key);
        const address = baseInput.value.trim();
        if (address && address !== entry.base) await setAddress(entry.id, address); // 中转覆盖
        await hooks.appendOrder(entry.id); // 追加列表末尾
        close();
        hooks.refresh();
      } catch (err) {
        errLine.textContent = `添加失败：${String(err)}`;
        addBtn.disabled = false;
      }
    })();
  });
}

/** 「⊕ 自定义提供方」悬浮窗：名称 + API 地址（占位随形态变化、底部
 *  地址+格式完整预览）+ 接口形态五选 + Key 可当场填；创建即默认启用。 */
function openCustomProviderModal(hooks: ProviderAddHooks): void {
  const nameInput = el("input", { type: "text", class: "settings-input", placeholder: "我的中转" }) as HTMLInputElement;
  const baseInput = el("input", { type: "text", class: "settings-input", placeholder: STYLE_BASE_HINT.openai }) as HTMLInputElement;
  const keyInput = el("input", { type: "password", class: "settings-input", placeholder: "可留空——稍后在卡片配置" }) as HTMLInputElement;
  const styleSel = document.createElement("select");
  styleSel.className = "settings-select";
  for (const [value, label] of Object.entries(STYLE_LABELS)) {
    const o = document.createElement("option");
    o.value = value;
    o.textContent = label;
    styleSel.append(o);
  }
  const preview = el("p", { class: "settings-path" }, []);
  const syncPreview = (): void => {
    baseInput.placeholder = STYLE_BASE_HINT[styleSel.value as ApiStyle];
    preview.textContent = `完整端点 ${endpointPreview(baseInput.value, styleSel.value as ApiStyle)}`;
  };
  styleSel.addEventListener("change", syncPreview);
  baseInput.addEventListener("input", syncPreview);
  syncPreview();

  const errLine = el("p", { class: "settings-status settings-status--error" }, []);
  const addBtn = el("button", { type: "button", class: "btn btn--ghost settings-inline-btn" }, [icon("plus", 13), "添加自定义提供方"]);
  const { overlay, close } = openModalShell("自定义提供方", [
    labeledRow("名称", nameInput),
    labeledRow("API 地址", baseInput),
    labeledRow("接口形态", styleSel),
    labeledRow("API Key", keyInput),
    preview,
    errLine,
  ]);
  const btns = overlay.querySelector(".error-modal__btns")!;
  btns.prepend(addBtn);
  addBtn.addEventListener("click", () => {
    void (async () => {
      addBtn.disabled = true;
      errLine.textContent = "";
      try {
        const name = nameInput.value.trim();
        const base = baseInput.value.trim();
        if (!name) throw new Error("名称不能为空");
        if (!base) throw new Error("API 地址不能为空");
        const added = await addCustomProvider(name, base, styleSel.value as ApiStyle);
        await setEnabled(added.id, true); // 添加即默认启用
        const key = keyInput.value.trim();
        if (key) await setKey(added.id, key);
        await hooks.appendOrder(added.id);
        close();
        hooks.refresh();
      } catch (err) {
        errLine.textContent = `添加失败：${String(err)}`;
        addBtn.disabled = false;
      }
    })();
  });
}

// ===== 配置 =====

/** Node 运行时面板：检测卡 + 托管版本 + 默认版本 + 下载入口。
 *  `note`：重渲染后要展示的状态行（如「已托管 x ✓」）——重渲染会清掉旧 DOM，
 *  完成/取消类反馈必须经此传入才能存活。 */
async function renderNodePanel(card: HTMLElement, note?: string): Promise<void> {
  card.replaceChildren(el("h2", { class: "card__title" }, ["Node"]));

  let system: SystemNodeInfo | null = null;
  try {
    system = await detectSystemNode();
  } catch (err) {
    card.append(
      el("p", { class: "settings-status settings-status--error" }, [`检测失败：${String(err)}`]),
    );
    return;
  }

  if (system) {
    card.append(
      el("div", { class: "settings-node-card" }, [
        el("span", { class: `settings-badge ${system.meetsRange ? "settings-badge--ok" : "settings-badge--bad"}` }, [
          system.meetsRange ? "满足 ^22.19 || >=24" : "不满足版本要求",
        ]),
        el("span", { class: "settings-mono" }, [system.version]),
        el("span", { class: "settings-path" }, [system.path]),
      ]),
    );
  } else {
    card.append(
      el("div", { class: "settings-node-card" }, [
        el("span", { class: "settings-badge settings-badge--bad" }, ["未检测到系统 Node"]),
        el("span", { class: "settings-path" }, ["可代管便携版，不影响启动器使用"]),
      ]),
    );
  }

  let managed: string[] = [];
  let defaultVersion: string | null = null;
  try {
    managed = await listManagedVersions();
    defaultVersion = await getDefaultVersion();
  } catch (err) {
    card.append(el("p", { class: "settings-status settings-status--error" }, [`读取托管列表失败：${String(err)}`]));
    return;
  }

  const select = document.createElement("select");
  select.className = "settings-select";
  if (managed.length === 0) {
    const opt = document.createElement("option");
    opt.textContent = "（尚无托管版本）";
    opt.value = "";
    select.append(opt);
    select.disabled = true;
  } else {
    for (const version of managed) {
      const opt = document.createElement("option");
      opt.value = version;
      opt.textContent = version;
      select.append(opt);
    }
    // 无全局默认时选中最新托管版本：select.value 置空串匹配不到任何 option，
    // 下拉会显示成空白（曾让用户误以为下载没生效）。
    select.value = defaultVersion ?? managed[managed.length - 1] ?? "";
  }

  const statusLine = el("p", { class: "settings-status" }, [""]);
  if (note) {
    statusLine.textContent = note;
    statusLine.classList.remove("settings-status--error");
  }
  const applyBtn = el("button", { type: "button", class: "btn btn--ghost settings-inline-btn" }, ["设为全局默认"]);
  applyBtn.addEventListener("click", async () => {
    if (!select.value) return;
    applyBtn.disabled = true;
    try {
      await setDefaultVersion(select.value);
      statusLine.textContent = "";
      statusLine.classList.remove("settings-status--error");
    } catch (err) {
      statusLine.textContent = `设置失败：${String(err)}`;
      statusLine.classList.add("settings-status--error");
    } finally {
      applyBtn.disabled = false;
    }
  });

  card.append(
    el("div", { class: "settings-row" }, [el("span", {}, [`托管版本（${managed.length}）：`]), select, applyBtn]),
  );

  const input = el("input", { type: "text", class: "settings-input", placeholder: "要下载的 Node 版本，如 24.12.0" });
  const progressLabel = el("span", { class: "settings-progress-label" }, [""]);
  const downloadBtn = el("button", { type: "button", class: "btn btn--ghost settings-inline-btn" }, ["下载并托管"]);
  const cancelBtn = el("button", { type: "button", class: "btn btn--ghost settings-inline-btn", hidden: "" }, ["取消下载"]);
  let activeKey: string | null = null;
  cancelBtn.addEventListener("click", () => {
    if (activeKey) void cancelDownload(activeKey).catch(() => {});
  });
  downloadBtn.addEventListener("click", () => {
    void (async () => {
      const version = input.value.trim().replace(/^v/, "");
      if (!/^\d+\.\d+\.\d+$/.test(version)) {
        progressLabel.textContent = "请输入 x.y.z 形式的版本号";
        return;
      }
      const key = `node-${version}`;
      activeKey = key;
      downloadBtn.disabled = true;
      downloadBtn.textContent = "下载中…";
      cancelBtn.hidden = false;
      // FR-09：Node 下载进统一任务队列（任务管理页可见、可暂停/继续/取消）。
      // phase 沿用 "source"（字节流展示态：任务行显示进度文案而非已运行时长）。
      upsertTask({
        instanceId: key,
        name: `Node v${version} 运行时`,
        version,
        status: "running",
        phase: "source",
        detail: "",
        received: 0,
        total: 0,
        speed: null,
        startedAt: Date.now(),
        kind: "node",
        canPause: true,
      });
      try {
        await downloadNode(version, ({ received, total }) => {
          const text =
            total > 0 ? `下载中 ${formatBytes(received)} / ${formatBytes(total)}` : `已接收 ${formatBytes(received)}`;
          downloadBtn.textContent = text;
          recordProgress(key, received, total);
          updateTask(key, { detail: text });
        });
        updateTask(key, { status: "done", phase: "done", detail: "" });
        await renderNodePanel(card);
      } catch (err) {
        updateTask(key, terminalFailure(err));
        const text = String(err);
        progressLabel.textContent = text.includes("已取消") ? "" : `下载失败：${text}`;
        if (!text.includes("已取消")) progressLabel.classList.add("settings-status--error");
      } finally {
        activeKey = null;
        downloadBtn.disabled = false;
        downloadBtn.textContent = "下载并托管";
        cancelBtn.hidden = true;
      }
    })();
  });

  card.append(
    el("div", { class: "settings-row" }, [input, downloadBtn, cancelBtn]),
    progressLine(progressLabel),
    statusLine,
  );
}

/** 镜像源面板（FR-18）：源切换 + 自定义前缀 + 连通性测试。 */
async function renderMirrorPanel(card: HTMLElement): Promise<void> {
  card.replaceChildren(el("h2", { class: "card__title" }, ["DSH 版本镜像源"]));

  let info: MirrorSettingsInfo;
  try {
    info = await getMirror();
  } catch (err) {
    card.append(el("p", { class: "settings-status settings-status--error" }, [`读取镜像设置失败：${String(err)}`]));
    return;
  }

  const select = document.createElement("select");
  select.className = "settings-select";
  for (const source of info.sources) {
    const opt = document.createElement("option");
    opt.value = source.id;
    opt.textContent = source.label;
    select.append(opt);
  }
  select.value = info.active;

  const prefixInput = el("input", {
    type: "text",
    class: "settings-input",
    placeholder: "自定义前缀，如 https://ghfast.top（作用于 GitHub 类下载）",
  }) as HTMLInputElement;
  prefixInput.value = info.customPrefix;

  const syncPrefixEnabled = (): void => {
    prefixInput.disabled = select.value !== "custom";
  };
  syncPrefixEnabled();
  select.addEventListener("change", syncPrefixEnabled);

  const statusLine = el("p", { class: "settings-status" }, [""]);
  const saveBtn = el("button", { type: "button", class: "btn btn--ghost settings-inline-btn" }, ["保存"]);
  saveBtn.addEventListener("click", () => {
    void (async () => {
      saveBtn.disabled = true;
      try {
        await setMirror(select.value, prefixInput.value.trim());
        statusLine.textContent = "";
        statusLine.classList.remove("settings-status--error");
      } catch (err) {
        statusLine.textContent = `保存失败：${String(err)}`;
        statusLine.classList.add("settings-status--error");
      } finally {
        saveBtn.disabled = false;
      }
    })();
  });

  const testBtn = el("button", { type: "button", class: "btn btn--line settings-inline-btn" }, ["测试此源连通性"]);
  testBtn.addEventListener("click", () => {
    void (async () => {
      testBtn.disabled = true;
      testBtn.textContent = "测试中…";
      statusLine.textContent = "";
      statusLine.classList.remove("settings-status--error");
      try {
        await setMirror(select.value, prefixInput.value.trim());
        const result = await testMirror(select.value, prefixInput.value.trim());
        if (result.ok) {
          statusLine.textContent = `可达：${result.detail}，延迟 ${result.latencyMs} ms`;
        } else {
          statusLine.textContent = `不可达：${result.detail}`;
          statusLine.classList.add("settings-status--error");
        }
      } catch (err) {
        statusLine.textContent = `测试失败：${String(err)}`;
        statusLine.classList.add("settings-status--error");
      } finally {
        testBtn.disabled = false;
        testBtn.textContent = "测试此源连通性";
      }
    })();
  });

  card.append(
    el("div", { class: "settings-row" }, [el("span", {}, ["下载通道："]), select]),
    el("div", { class: "settings-row" }, [prefixInput, saveBtn, testBtn]),
    statusLine,
  );
}

/** 社区资源镜像源面板（FR-18 修订）：官方 GitHub / Gitee / dsh-hub.cc 三选项，
 *  插件市场 / Skill 中心按此取数。 */
async function renderCommunityMirrorPanel(card: HTMLElement): Promise<void> {
  card.replaceChildren(el("h2", { class: "card__title" }, ["社区资源镜像源"]));

  let info: MirrorSettingsInfo;
  try {
    info = await getMirror();
  } catch (err) {
    card.append(el("p", { class: "settings-status settings-status--error" }, [`读取镜像设置失败：${String(err)}`]));
    return;
  }

  const select = document.createElement("select");
  select.className = "settings-select";
  for (const source of info.communitySources) {
    const opt = document.createElement("option");
    opt.value = source.id;
    opt.textContent = source.label;
    select.append(opt);
  }
  select.value = info.communityActive;

  const statusLine = el("p", { class: "settings-status" }, [""]);
  const saveBtn = el("button", { type: "button", class: "btn btn--ghost settings-inline-btn" }, ["保存"]);
  saveBtn.addEventListener("click", () => {
    void (async () => {
      saveBtn.disabled = true;
      try {
        await setCommunityMirror(select.value);
        statusLine.textContent = "";
        statusLine.classList.remove("settings-status--error");
      } catch (err) {
        statusLine.textContent = `保存失败：${String(err)}`;
        statusLine.classList.add("settings-status--error");
      } finally {
        saveBtn.disabled = false;
      }
    })();
  });

  // 社区卡的「官方」探测点是 GitHub（版本卡的 official 是 npm+GitHub 双探测点）。
  const testBtn = el("button", { type: "button", class: "btn btn--line settings-inline-btn" }, ["测试此源连通性"]);
  testBtn.addEventListener("click", () => {
    void (async () => {
      testBtn.disabled = true;
      testBtn.textContent = "测试中…";
      statusLine.textContent = "";
      statusLine.classList.remove("settings-status--error");
      try {
        await setCommunityMirror(select.value);
        const result = await testMirror(select.value === "official" ? "github" : select.value);
        if (result.ok) {
          statusLine.textContent = `可达：${result.detail}，延迟 ${result.latencyMs} ms`;
        } else {
          statusLine.textContent = `不可达：${result.detail}`;
          statusLine.classList.add("settings-status--error");
        }
      } catch (err) {
        statusLine.textContent = `测试失败：${String(err)}`;
        statusLine.classList.add("settings-status--error");
      } finally {
        testBtn.disabled = false;
        testBtn.textContent = "测试此源连通性";
      }
    })();
  });

  card.append(
    el("div", { class: "settings-row" }, [el("span", {}, ["社区通道："]), select]),
    el("div", { class: "settings-row" }, [saveBtn, testBtn]),
    statusLine,
  );
}

async function renderConfigPane(pane: HTMLElement): Promise<void> {
  pane.replaceChildren();

  // 启动选项（Phase 8.5，PCL PageSetupLaunch 启动选项卡同构——需求方 2026-11-14
  // 裁决：默认版本隔离 + 启动器可见性两项；游戏窗口标题/自定义信息/窗口大小经
  // PCL 源码考证后砍——DSH 网页无启动器可控的窗口面）。
  // 默认版本隔离（FR-17 扩展）：DSH 无 Mod 加载器/快照之分，PCL 五档收敛两档；
  // 仅作新装实例默认，已有实例各走版本设置页（PCL ToolTip 原文改「版本→实例」）。
  const isoDefaultSelect = el(
    "select",
    {
      class: "settings-input",
      title:
        "当安装新实例时，据此自动设置新实例的隔离选项。\n若想调整已有实例的隔离策略，请前往它的版本设置。",
    },
    [],
  );
  isoDefaultSelect.append(
    el("option", { value: "false" }, ["关闭"]),
    el("option", { value: "true" }, ["完全隔离"]),
  );
  const isoDefaultStatus = el("p", { class: "settings-status" }, [""]);
  void getDefaultIsolation()
    .then((v) => {
      isoDefaultSelect.value = String(v);
    })
    .catch((err) => {
      isoDefaultStatus.textContent = `读取失败：${String(err).slice(0, 160)}`;
    });
  isoDefaultSelect.addEventListener("change", () => {
    void setDefaultIsolation(isoDefaultSelect.value === "true")
      .then(() => {
        isoDefaultStatus.textContent = "";
      })
      .catch((err) => {
        isoDefaultStatus.textContent = `保存失败：${String(err).slice(0, 200)}`;
      });
  });

  // 启动器可见性（FR-28）：PCL LaunchArgumentVisible 五档，「游戏」映射「实例」；
  // 值系沿用 PCL（0/2/3/4/5，1 弃用）。触发点 = 网页就绪（lib/visibility.ts）。
  const visibleSelect = el("select", { class: "settings-input" }, []);
  for (const [value, label] of [
    ["0", "实例启动后立即关闭"],
    ["2", "实例启动后隐藏，实例退出后自动关闭"],
    ["3", "实例启动后隐藏，实例退出后重新打开"],
    ["4", "实例启动后最小化"],
    ["5", "实例启动后仍保持不变"],
  ] as Array<[string, string]>) {
    visibleSelect.append(el("option", { value }, [label]));
  }
  const visibleStatus = el("p", { class: "settings-status" }, [""]);
  void getLauncherVisibility()
    .then((v) => {
      visibleSelect.value = String(v);
    })
    .catch((err) => {
      visibleStatus.textContent = `读取失败：${String(err).slice(0, 160)}`;
    });
  visibleSelect.addEventListener("change", () => {
    void setLauncherVisibility(Number(visibleSelect.value))
      .then(() => {
        visibleStatus.textContent = "";
      })
      .catch((err) => {
        visibleStatus.textContent = `保存失败：${String(err).slice(0, 200)}`;
      });
  });

  pane.append(
    el("section", { class: "card settings-card" }, [
      el("h2", { class: "card__title" }, ["启动选项"]),
      el("div", { class: "settings-row" }, [el("span", {}, ["默认版本隔离："]), isoDefaultSelect]),
      el("div", { class: "settings-row" }, [el("span", {}, ["启动器可见性："]), visibleSelect]),
      isoDefaultStatus,
      visibleStatus,
    ]),
  );

  // 全局 AGENTS.md（FILE-LAYOUT：全局输入框落点 = 托管根 AGENTS.md 文件，
  // 物化下发各版本；实例可在「版本设置 → 设置」单独覆盖）。
  // 需求方定稿（2026-09-12）：只要输入框，输入完即自动保存（防抖）。
  const agentsArea = document.createElement("textarea");
  agentsArea.className = "settings-textarea";
  agentsArea.rows = 8;
  agentsArea.placeholder = "所有版本共享的系统指令";
  // 需求方 2026-10-24：状态灰字只留错误反馈（保存/读取失败），成功与说明句全删。
  const agentsStatus = el("p", { class: "settings-path" }, [""]);
  let agentsLoaded = false;
  let agentsTimer: ReturnType<typeof setTimeout> | undefined;
  agentsArea.addEventListener("input", () => {
    if (!agentsLoaded) return;
    clearTimeout(agentsTimer);
    agentsTimer = setTimeout(() => {
      const text = agentsArea.value;
      void setGlobalAgentsMd(text.trim() === "" ? null : text).catch((err) => {
        agentsStatus.textContent = `保存失败：${String(err).slice(0, 240)}`;
      });
    }, 600);
  });
  void (async () => {
    try {
      const global = await getGlobalAgentsMd();
      agentsArea.value = global ?? "";
    } catch (err) {
      agentsStatus.textContent = `读取失败：${String(err).slice(0, 240)}`;
    } finally {
      agentsLoaded = true;
    }
  })();
  pane.append(
    el("section", { class: "card settings-card" }, [
      el("h2", { class: "card__title" }, ["AGENTS.md"]),
      agentsArea,
      agentsStatus,
    ]),
  );

  // 启动器状态
  let instancesDir = "";
  try {
    instancesDir = await getInstancesDir();
  } catch {
    instancesDir = "（未知）";
  }
  const versionBadge = el("span", { class: "settings-badge settings-badge--ok" }, [`v${APP_VERSION}`]);
  // 真实版本号（tauri.conf.json）；非 Tauri 环境保留构建期回退。
  void import("@tauri-apps/api/app")
    .then(({ getVersion }) => getVersion())
    .then((v) => {
      versionBadge.textContent = `v${v}`;
    })
    .catch(() => {});
  pane.append(
    el("section", { class: "card settings-card" }, [
      el("h2", { class: "card__title" }, ["启动器状态"]),
      el("div", { class: "settings-node-card" }, [
        versionBadge,
        el("span", { class: "settings-path" }, [`数据目录：${instancesDir}`]),
      ]),
    ]),
  );

  // 主题色卡已迁「个性化」分类（2026-10-24，FR-27——PCL 个性化含主题）。

  // 下载并发（FR-09 队列额度 / FR-11）。
  const concSelect = el("select", { class: "settings-input" }, []);
  for (let n = 1; n <= 10; n += 1) {
    concSelect.append(el("option", { value: String(n) }, [String(n)]));
  }
  const concStatus = el("p", { class: "settings-status" }, [""]);
  void getMaxDownloads()
    .then((v) => {
      concSelect.value = String(v);
    })
    .catch((err) => {
      concStatus.textContent = `读取失败：${String(err).slice(0, 160)}`;
    });
  concSelect.addEventListener("change", () => {
    void setMaxDownloads(Number(concSelect.value))
      .then(() => {
        concStatus.textContent = "";
      })
      .catch((err) => {
        concStatus.textContent = `保存失败：${String(err).slice(0, 200)}`;
      });
  });
  pane.append(
    el("section", { class: "card settings-card" }, [
      el("h2", { class: "card__title" }, ["下载并发"]),
      el("div", { class: "settings-row" }, [el("span", {}, ["同时下载任务数："]), concSelect]),
      concStatus,
    ]),
  );

  // 网络代理（FR-11）：作用于全部启动器取数（含镜像测试、下载、npm 子进程链）。
  const proxyInput = el("input", {
    type: "text",
    class: "settings-input",
    placeholder: "http://127.0.0.1:7890（留空 = 直连）",
  });
  const proxySave = el("button", { type: "button", class: "btn btn--ghost settings-inline-btn" }, ["保存"]);
  const proxyStatus = el("p", { class: "settings-status" }, [""]);
  void getProxy()
    .then((v) => {
      proxyInput.value = v;
    })
    .catch((err) => {
      proxyStatus.textContent = `读取失败：${String(err).slice(0, 160)}`;
    });
  proxySave.addEventListener("click", () => {
    proxySave.disabled = true;
    void setProxy(proxyInput.value.trim())
      .then(() => {
        proxyStatus.textContent = "";
      })
      .catch((err) => {
        proxyStatus.textContent = `保存失败：${String(err).slice(0, 200)}`;
      })
      .finally(() => {
        proxySave.disabled = false;
      });
  });
  pane.append(
    el("section", { class: "card settings-card" }, [
      el("h2", { class: "card__title" }, ["网络代理"]),
      el("div", { class: "settings-row" }, [proxyInput, proxySave]),
      proxyStatus,
    ]),
  );

  // Node 运行时
  const nodeSection = el("section", { class: "card settings-card" }, [el("p", { class: "settings-status" }, [""])]);
  pane.append(nodeSection);
  await renderNodePanel(nodeSection);

  // DSH 版本镜像源
  const mirrorSection = el("section", { class: "card settings-card" }, [el("p", { class: "settings-status" }, [""])]);
  pane.append(mirrorSection);
  await renderMirrorPanel(mirrorSection);

  // 社区资源镜像源（FR-18 修订：选项落成真实设置，Phase 3 起被消费）
  const communitySection = el("section", { class: "card settings-card" }, [el("p", { class: "settings-status" }, [""])]);
  pane.append(communitySection);
  await renderCommunityMirrorPanel(communitySection);
  // GitHub Token 提额面板已删（2026-10-24 需求方指令「市场提额删了，不需要用到」）
  // —— GitHub 请求回到匿名配额，后端 github_token 注入一并移除。
}

// ===== 个性化（FR-27，P7.5；PCL PageSetupUI.xaml 同构）=====

/** 个性化 = 主题 + 背景 + 标题栏 + 功能隐藏（PCL 设置·个性化页四卡同构；
 *  背景音乐 / 主页自定义不在需求方口径内，不做）。 */
function renderPersonalizePane(pane: HTMLElement): void {
  pane.replaceChildren();
  appendThemeCard(pane);
  appendBackgroundCard(pane);
  appendTitlebarCard(pane);
  appendSwitchCard(pane);
}

/** 主题色卡（FR-11；2026-10-24 自「配置」迁入——PCL 个性化含主题）。 */
function appendThemeCard(pane: HTMLElement): void {
  const themeStatus = el("p", { class: "settings-status" }, [""]);
  let currentTheme: ThemeColor | null = loadTheme();
  const swatches = el("div", { class: "theme-swatches" }, []);
  const syncSwatches = (): void => {
    swatches.replaceChildren(
      ...PRESET_HUES.map((hue) => {
        const color = deriveThemeTokens({ hue, sat: 85 })["--cb3"];
        const active = currentTheme?.hue === hue;
        const swatch = el("button", {
          type: "button",
          class: `theme-swatch${active ? " theme-swatch--on" : ""}`,
          style: `background:${color}`,
          title: `主题色 ${hue}°`,
          "aria-label": `主题色 ${hue}°`,
        });
        swatch.addEventListener("click", () => {
          currentTheme = { hue, sat: 85 };
          saveTheme(currentTheme);
          syncSwatches();
        });
        return swatch;
      }),
    );
  };
  syncSwatches();
  const themeInput = el("input", { type: "color", class: "settings-input theme-input", value: "#1370f3" });
  themeInput.addEventListener("input", () => {
    const picked = themeInput.value;
    // 自选色：取其色相与饱和度，明度沿用 PCL 分档（派生公式固定）。
    const hsl = hexToHsl(picked);
    if (!hsl) return;
    currentTheme = { hue: Math.round(hsl.h), sat: Math.max(40, Math.min(100, hsl.s)) };
    saveTheme(currentTheme);
    syncSwatches();
  });
  const themeReset = el("button", { type: "button", class: "btn btn--ghost settings-inline-btn" }, ["恢复默认"]);
  themeReset.addEventListener("click", () => {
    clearThemeOverride();
    currentTheme = null;
    syncSwatches();
  });
  pane.append(
    el("section", { class: "card settings-card" }, [
      el("h2", { class: "card__title" }, ["主题色"]),
      swatches,
      el("div", { class: "settings-row" }, [themeInput, themeReset]),
      themeStatus,
    ]),
  );
}

/** 背景图片卡（PCL CardBackground，PageSetupUI.xaml:104-157）：不透明度滑杆 +
 *  打开文件夹（图片自行放入，每次启动随机加载一张）/ 刷新背景图片 / 清空背景图片。 */
function appendBackgroundCard(pane: HTMLElement): void {
  const status = el("p", { class: "settings-status" }, [""]); // 仅错误反馈
  const slider = el("input", {
    type: "range",
    class: "pcl-slider",
    min: "0",
    max: "100",
    step: "1",
    "aria-label": "背景不透明度",
  }) as HTMLInputElement;
  slider.value = String(loadUiPrefs().backgroundOpacity);
  const paint = (): void => {
    const pct = Number(slider.value);
    slider.style.setProperty(
      "--slider-fill",
      `linear-gradient(to right, var(--cb3) ${pct}%, var(--cb7) ${pct}%)`,
    );
  };
  paint();
  slider.addEventListener("input", () => {
    paint();
    setBackgroundOpacity(Number(slider.value)); // 实时预览，不落盘
  });
  slider.addEventListener("change", () => {
    updateUiPrefs((p) => {
      p.backgroundOpacity = Number(slider.value);
    });
  });

  const openBtn = el("button", { type: "button", class: "btn btn--line settings-inline-btn", title: "将背景图片放到该文件夹，启动器会在每次打开时随机加载其中的一张" }, ["打开文件夹"]);
  openBtn.addEventListener("click", () => {
    void openBackgroundFolder().catch((err) => {
      status.textContent = `打开失败：${String(err).slice(0, 200)}`;
    });
  });
  const refreshBtn = el("button", { type: "button", class: "btn btn--ghost settings-inline-btn", title: "随机加载文件夹内的一张背景图片" }, ["刷新背景图片"]);
  refreshBtn.addEventListener("click", () => {
    status.textContent = "";
    void applyBackground().catch(() => {
      status.textContent = "刷新背景失败。";
    });
  });
  const clearBtn = el("button", { type: "button", class: "btn btn--danger settings-inline-btn" }, ["清空背景图片"]);
  clearBtn.addEventListener("click", () => {
    status.textContent = "";
    void clearBackground()
      .then(() => {
        clearBackgroundLayer();
      })
      .catch((err) => {
        status.textContent = `清空失败：${String(err).slice(0, 200)}`;
      });
  });

  pane.append(
    el("section", { class: "card settings-card" }, [
      el("h2", { class: "card__title" }, ["背景图片"]),
      el("div", { class: "settings-row" }, [el("span", { class: "settings-path" }, ["不透明度"]), slider]),
      el("div", { class: "settings-actions" }, [openBtn, refreshBtn, clearBtn]),
      status,
    ]),
  );
}

/** 标题栏卡（PCL CardLogo，PageSetupUI.xaml:186-219）：无 / 默认 / 文本 / 图片
 *  四态；文本态出标题栏文本输入框（≤100 字），图片态出更改 / 清空图片。 */
function appendTitlebarCard(pane: HTMLElement): void {
  const status = el("p", { class: "settings-status" }, [""]); // 仅错误反馈
  const prefs = loadUiPrefs();

  const modeRow = el("div", { class: "settings-actions" }, []);
  const radios: { mode: TitlebarMode; label: string }[] = [
    { mode: "none", label: "无" },
    { mode: "default", label: "默认" },
    { mode: "text", label: "文本" },
    { mode: "image", label: "图片" },
  ];
  let currentMode: TitlebarMode = prefs.titlebar.mode;
  for (const { mode, label } of radios) {
    const radio = pclRadio({ name: "dsh-titlebar-mode", value: mode, label, checked: mode === currentMode });
    radio.input.addEventListener("change", () => {
      currentMode = mode;
      updateUiPrefs((p) => {
        p.titlebar.mode = mode;
      });
      syncRows();
    });
    modeRow.append(radio.root);
  }

  const textInput = el("input", {
    type: "text",
    class: "settings-input",
    maxlength: "100",
    placeholder: "标题栏文本",
  }) as HTMLInputElement;
  textInput.value = prefs.titlebar.text;
  let textTimer: ReturnType<typeof setTimeout> | undefined;
  textInput.addEventListener("input", () => {
    clearTimeout(textTimer);
    textTimer = setTimeout(() => {
      updateUiPrefs((p) => {
        p.titlebar.text = textInput.value;
      });
    }, 400);
  });

  const changeImgBtn = el("button", { type: "button", class: "btn btn--line settings-inline-btn" }, ["更改图片"]);
  changeImgBtn.addEventListener("click", () => {
    status.textContent = "";
    void (async () => {
      try {
        const { open } = await import("@tauri-apps/plugin-dialog");
        const picked = await open({
          multiple: false,
          filters: [{ name: "图片", extensions: ["png", "jpg", "jpeg", "webp", "gif", "bmp", "avif"] }],
        });
        if (typeof picked !== "string") return; // 取消
        await setLogoFromFile(picked);
        window.dispatchEvent(new CustomEvent(UI_PREFS_EVENT)); // topbar 刷新图片缓存
      } catch (err) {
        status.textContent = `设置失败：${String(err).slice(0, 200)}`;
      }
    })();
  });
  const clearImgBtn = el("button", { type: "button", class: "btn btn--danger settings-inline-btn" }, ["清空图片"]);
  clearImgBtn.addEventListener("click", () => {
    status.textContent = "";
    void clearLogo()
      .then(() => {
        window.dispatchEvent(new CustomEvent(UI_PREFS_EVENT));
      })
      .catch((err) => {
        status.textContent = `清空失败：${String(err).slice(0, 200)}`;
      });
  });
  const textRow = el("div", { class: "settings-row" }, [textInput]);
  const imageRow = el("div", { class: "settings-actions" }, [changeImgBtn, clearImgBtn]);
  const syncRows = (): void => {
    textRow.hidden = currentMode !== "text";
    imageRow.hidden = currentMode !== "image";
  };
  syncRows();

  pane.append(
    el("section", { class: "card settings-card" }, [
      el("h2", { class: "card__title" }, ["标题栏"]),
      modeRow,
      textRow,
      imageRow,
      status,
    ]),
  );
}

/** 功能隐藏卡（PCL CardSwitch，PageSetupUI.xaml:295-339）：勾选隐藏页面/功能；
 *  说明句为 F12 机制的功能性说明（PCL 原文）。 */
function appendSwitchCard(pane: HTMLElement): void {
  if (loadUiPrefs().hidden.hideSwitchCard && !isTempShowAll()) return; // 「功能隐藏」自身被隐藏（设置依然有效）
  const grid = el("div", { class: "settings-switch-grid" }, []);
  const group = (label: string): HTMLElement =>
    el("span", { class: "settings-switch-group" }, [label]);
  const toggleIn = (list: string[], id: string, on: boolean): void => {
    const idx = list.indexOf(id);
    if (on && idx === -1) list.push(id);
    if (!on && idx !== -1) list.splice(idx, 1);
  };
  const check = (label: string, checked: boolean, onChange: (on: boolean) => void, title?: string): HTMLElement => {
    const box = pclCheck({ label, checked, title });
    box.input.addEventListener("change", () => onChange(box.input.checked));
    return box.root;
  };
  const hidden = loadUiPrefs().hidden;
  const pageCheck = (id: string, label: string): HTMLElement =>
    check(label, hidden.pages.includes(id), (on) => {
      updateUiPrefs((p) => toggleIn(p.hidden.pages, id, on));
    });
  const catCheck = (id: SettingsCatId, label: string): HTMLElement =>
    check(label, hidden.settingsCats.includes(id), (on) => {
      updateUiPrefs((p) => toggleIn(p.hidden.settingsCats as string[], id, on));
    });
  const moreCheck = (id: MoreCatId, label: string): HTMLElement =>
    check(label, hidden.moreCats.includes(id), (on) => {
      updateUiPrefs((p) => toggleIn(p.hidden.moreCats as string[], id, on));
    });

  grid.append(
    group("主页面"),
    pageCheck("downloads", "下载"),
    pageCheck("settings", "设置"),
    pageCheck("more", "更多"),
    group("设置 子页面"),
    catCheck("providers", "模型提供商"),
    catCheck("config", "配置"),
    catCheck("personalize", "个性化"),
    catCheck("hotkeys", "热键"),
    group("更多 子页面"),
    moreCheck("about", "关于"),
    moreCheck("help", "使用帮助"),
    moreCheck("update", "检查更新"),
    group("特定功能"),
    check("版本管理", hidden.versionButtons, (on) => {
      updateUiPrefs((p) => {
        p.hidden.versionButtons = on;
      });
    }, "隐藏启动按钮下方的版本选择与版本设置入口。"),
    check("功能隐藏", hidden.hideSwitchCard, (on) => {
      updateUiPrefs((p) => {
        p.hidden.hideSwitchCard = on;
      });
    }, "仅隐藏本选项卡，设置的禁用内容依然有效。"),
  );

  pane.append(
    el("section", { class: "card settings-card" }, [
      el("h2", { class: "card__title" }, ["功能隐藏"]),
      el("p", { class: "settings-path" }, [
        "你可以隐藏不需要的页面或关闭特定功能。在任意界面按 F12 可以暂时显示被隐藏的功能。",
      ]),
      grid,
    ]),
  );
}

// ===== 热键 =====

/** 设置·热键 = DSH 0.2.0 快捷键体系速查（2026-10-04 需求方指令，推翻 P4.5「键位硬编码」
 * 旧结论——旧侦察只覆盖 0.1.x web 源码，0.2.0-rc 起官方有 dsh-client-shortcuts 快捷键系统）。
 * 版式（需求方指定，PCL 语言）：**一个快捷键一张卡**——名字加大加粗深色（PCL 卡标题语言），
 * 右侧键位为蓝描边方框按钮（PCL MyButton 蓝皮肤 = .btn--ghost 同款：半透白底 + CB2 描边 +
 * 文字同色；纯展示不可点，无编辑通道）。键位取 web:windows 档默认（启动器实例是 web 运行时）；
 * 编辑在实例网页 设置 → 快捷键 原生编辑器进行（桌面壳存 keybindings.json、web 存浏览器
 * localStorage，启动器无存储桥不代管）。数据见 lib/dsh-hotkeys.ts。 */
function renderHotkeysPane(pane: HTMLElement): void {
  pane.replaceChildren();
  for (const section of DSH_SHORTCUT_SECTIONS) {
    // 小标只保留「固定」：它标记不可编辑区的起点；「可编辑」小标删除（2026-10-04 需求方指令）——
    // 固定之前的全部即默认可编辑，无需赘述。
    if (section.id === "fixed") {
      pane.append(el("p", { class: "hotkey-group" }, [section.label]));
    }
    for (const entry of DSH_SHORTCUTS.filter((entry) => entry.section === section.id)) {
      pane.append(
        el("div", { class: "hotkey-item" }, [
          el("span", { class: "hotkey-item__name" }, [entry.label]),
          el("span", { class: "hotkey-keybtn" }, renderComboKeys(entry.keys)),
        ]),
      );
    }
  }
}

/** 键位组合 → 蓝描边按钮内容：组合内键用 + 分隔，多组合间用 / 分隔（如 Esc / Esc）。 */
function renderComboKeys(keys: string[]): (Node | string)[] {
  const out: (Node | string)[] = [];
  keys.forEach((combo, comboIndex) => {
    if (comboIndex > 0) out.push("/");
    combo
      .split("+")
      .map((key) => key.trim())
      .filter((key) => key !== "")
      .forEach((key, keyIndex) => {
        if (keyIndex > 0) out.push(el("span", { class: "hotkey-keybtn__plus" }, ["+"]));
        out.push(key);
      });
  });
  return out;
}

// ===== 页面壳 =====

type SettingsCategory = "providers" | "config" | "hotkeys" | "personalize";

/** 左栏分类记忆：跨页面重挂载保留（同下载页 lastDlCategory 先例，2026-10-04 走查修复——
 *  闭包局部变量会在 启动页↔设置 往返时把分类重置回模型提供商）。 */
let lastSettingsCategory: SettingsCategory = "providers";

/** 页面级监听清理（功能隐藏：勾选会即时改左栏，切页时移除监听防累积）。 */
let cleanupSettingsListener: (() => void) | null = null;

export const settingsPage: PageDefinition = {
  id: "settings",
  navLabel: "设置",
  render: () => {
    type Category = SettingsCategory;
    const categories: { id: Category; label: string }[] = [
      { id: "providers", label: "模型提供商" },
      { id: "config", label: "配置" },
      { id: "personalize", label: "个性化" },
      { id: "hotkeys", label: "热键" },
    ];
    let category: Category = lastSettingsCategory;
    // 功能隐藏（FR-27）：被隐藏的分类不进左栏；记忆的分类被藏时回落第一个可见。
    if (isSettingsCatHidden(category)) {
      category = categories.find((c) => !isSettingsCatHidden(c.id))?.id ?? "providers";
    }

    const leftNav = el("div", { class: "sub-left__group" }, []);
    leftNav.append(el("div", { class: "sub-left__label" }, ["设置"]));
    const pane = el("div", { class: "settings-pane" }, []);

    function renderRight(): void {
      if (category === "providers") {
        void renderProvidersPane(pane);
      } else if (category === "config") {
        void renderConfigPane(pane);
      } else if (category === "personalize") {
        renderPersonalizePane(pane);
      } else {
        renderHotkeysPane(pane);
      }
    }
    function renderLeft(): void {
      leftNav.replaceChildren();
      leftNav.append(el("div", { class: "sub-left__label" }, ["设置"]));
      for (const item of categories) {
        if (isSettingsCatHidden(item.id)) continue;
        leftNav.append(
          el(
            "button",
            { type: "button", class: `sub-left__item ${item.id === category ? "sub-left__item--active" : ""}` },
            [el("span", { class: "sub-left__item-main" }, [item.label])],
          ),
        );
      }
      for (const button of [...leftNav.querySelectorAll("button")]) {
        button.addEventListener("click", () => {
          const label = button.textContent?.trim() ?? "";
          const found = categories.find((c) => c.label === label);
          if (!found || found.id === category) return;
          category = found.id;
          lastSettingsCategory = category;
          renderLeft();
          renderRight();
        });
      }
    }

    renderLeft();
    renderRight();

    // 偏好变更（功能隐藏勾选 / F12）：左栏即时过滤；当前分类被藏 → 自动切走；
    // 「功能隐藏自身」开关变化 → 重画右栏让本卡消失/回归。
    const hiddenSig = (): string => {
      const h = loadUiPrefs().hidden;
      return JSON.stringify([h.settingsCats, h.hideSwitchCard]);
    };
    let lastSig = hiddenSig();
    const onPrefs = (): void => {
      renderLeft();
      if (isSettingsCatHidden(category)) {
        category = categories.find((c) => !isSettingsCatHidden(c.id))?.id ?? "providers";
        lastSettingsCategory = category;
        renderRight();
        return;
      }
      if (hiddenSig() !== lastSig) {
        lastSig = hiddenSig();
        renderRight();
      }
    };
    window.addEventListener(UI_PREFS_EVENT, onPrefs);
    cleanupSettingsListener = () => window.removeEventListener(UI_PREFS_EVENT, onPrefs);

    return el("section", { class: "page" }, [
      el("div", { class: "sub sub--compact" }, [
        el("aside", { class: "sub-left" }, [leftNav]),
        el("section", { class: "sub-right" }, [pane]),
      ]),
    ]);
  },
  unmount: () => {
    cleanupSettingsListener?.();
    cleanupSettingsListener = null;
  },
};
