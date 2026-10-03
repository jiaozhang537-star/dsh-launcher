/** 设置页：左分类（模型提供商 / 配置 / 热键）+ 右内容卡片，全局左右栏布局。 */
import { el, pclCheck } from "../lib/dom";
import { icon } from "../lib/icons";
import type { PageDefinition } from "../router";
import { DSH_HOTKEY_GROUPS, DSH_HOTKEYS } from "../lib/dsh-hotkeys";
import {
  cancelDownload,
  detectSystemNode,
  downloadNode,
  getDefaultVersion,
  listManagedVersions,
  setDefaultVersion,
  type SystemNodeInfo,
} from "../services/nodeRuntime";
import { getGlobalAgentsMd, getInstancesDir, setGlobalAgentsMd } from "../services/instances";
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
import { getMaxDownloads, getProxy, setMaxDownloads, setProxy } from "../services/launcherConfig";
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
        note(enableBox.checked ? `已启用 ${meta.label}` : `已停用 ${meta.label}`);
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
        note("地址已保存（留空 = 默认地址）");
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
        note("未修改");
        return;
      }
      keySave.disabled = true;
      try {
        await setKey(vendor.vendor, value);
        keyEdited = false;
        keyInput.value = KEY_MASK;
        note("Key 已加密保存");
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
        note("已清除");
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
      fetchedRow.replaceChildren(el("span", { class: "settings-progress-label" }, ["拉取中…"]));
      try {
        fetched = await fetchModels(vendor.vendor, addrInput.value);
        renderFetched();
        note(`拉取到 ${fetched.length} 个模型，点击“+”添加`);
      } catch (err) {
        fetchedRow.replaceChildren();
        note(`拉取失败：${String(err).slice(0, 200)}`, true);
      } finally {
        fetchBtn.disabled = false;
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
            note(`已添加 ${model}`);
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
          note(`已移除 ${model}`);
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
        note(`已添加 ${value}`);
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
          note(`已删除 ${meta.label}`);
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
          note(`已从列表移除 ${meta.label}（可随时重新添加）`);
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
        hooks.refresh(`已添加 ${entry.label}（默认已启用）`);
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
        hooks.refresh(`已添加自定义提供方 ${added.name}（默认已启用）`);
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
      statusLine.textContent = `全局默认已设为 ${select.value}`;
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
    progressLabel.textContent = "正在取消…";
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
      cancelBtn.hidden = false;
      progressLabel.textContent = "准备下载…";
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
          progressLabel.textContent = text;
          recordProgress(key, received, total);
          updateTask(key, { detail: text });
        });
        updateTask(key, { status: "done", phase: "done", detail: "" });
        // 重渲染清掉进度行，完成提示经 note 在新面板中显示。
        await renderNodePanel(card, `已托管 ${version}`);
      } catch (err) {
        updateTask(key, terminalFailure(err));
        const text = String(err);
        progressLabel.textContent = text.includes("已取消") ? "已取消下载" : `下载失败：${text}`;
      } finally {
        activeKey = null;
        downloadBtn.disabled = false;
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
        statusLine.textContent = "镜像源已保存";
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
      statusLine.textContent = "测试中…";
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
        statusLine.textContent = "社区资源镜像源已保存";
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
      statusLine.textContent = "测试中…";
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

  // 全局 AGENTS.md（FILE-LAYOUT：全局输入框落点 = 托管根 AGENTS.md 文件，
  // 物化下发各版本；实例可在「版本设置 → 设置」单独覆盖）。
  // 需求方定稿（2026-09-12）：只要输入框，输入完即自动保存（防抖）。
  const agentsArea = document.createElement("textarea");
  agentsArea.className = "settings-textarea";
  agentsArea.rows = 8;
  agentsArea.placeholder = "对所有版本生效的全局指令；留空 = 清除";
  const agentsStatus = el("p", { class: "settings-path" }, ["加载中…"]);
  let agentsLoaded = false;
  let agentsTimer: ReturnType<typeof setTimeout> | undefined;
  agentsArea.addEventListener("input", () => {
    if (!agentsLoaded) return;
    clearTimeout(agentsTimer);
    agentsStatus.textContent = "正在保存…";
    agentsTimer = setTimeout(() => {
      const text = agentsArea.value;
      void setGlobalAgentsMd(text.trim() === "" ? null : text)
        .then(() => {
          agentsStatus.textContent =
            text.trim() === ""
              ? "已清除——已从未自设的版本移除 AGENTS.md。"
              : "已保存——对所有版本即时生效（运行中实例下一条消息起读取新内容）。";
        })
        .catch((err) => {
          agentsStatus.textContent = `保存失败：${String(err).slice(0, 240)}`;
        });
    }, 600);
  });
  void (async () => {
    try {
      const global = await getGlobalAgentsMd();
      agentsArea.value = global ?? "";
      agentsStatus.textContent =
        global == null
          ? "当前未设置。对所有版本共享生效。"
          : "已设置——修改即自动保存，对全部版本的下次启动生效。";
    } catch (err) {
      agentsStatus.textContent = `读取失败：${String(err).slice(0, 240)}`;
    } finally {
      agentsLoaded = true;
    }
  })();
  pane.append(
    el("section", { class: "card settings-card" }, [
      el("h2", { class: "card__title" }, ["全局 AGENTS.md"]),
      el("p", { class: "settings-path" }, ["对所有版本共享生效；单个版本可在版本设置覆盖。"]),
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

  // 主题色（FR-11）：PCL ModSecret 同式派生——选色即整套色板联动；
  // 未选自定义主题时保持 phase-05 对齐的静态默认蓝。
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
          themeStatus.textContent = "已切换主题色。";
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
    themeStatus.textContent = `已切换主题色：${picked}`;
  });
  const themeReset = el("button", { type: "button", class: "btn btn--ghost settings-inline-btn" }, ["恢复默认"]);
  themeReset.addEventListener("click", () => {
    clearThemeOverride();
    currentTheme = null;
    syncSwatches();
    themeStatus.textContent = "已恢复默认蓝主题。";
  });
  pane.append(
    el("section", { class: "card settings-card" }, [
      el("h2", { class: "card__title" }, ["主题色"]),
      swatches,
      el("div", { class: "settings-row" }, [themeInput, themeReset]),
      themeStatus,
    ]),
  );

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
        concStatus.textContent = "已保存——对之后的下载任务生效。";
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
        proxyStatus.textContent = "已保存——对之后的下载任务生效，可用镜像源的「测试」验证连通。";
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
  const nodeSection = el("section", { class: "card settings-card" }, [el("p", { class: "settings-status" }, ["加载中…"])]);
  pane.append(nodeSection);
  await renderNodePanel(nodeSection);

  // DSH 版本镜像源
  const mirrorSection = el("section", { class: "card settings-card" }, [el("p", { class: "settings-status" }, ["加载中…"])]);
  pane.append(mirrorSection);
  await renderMirrorPanel(mirrorSection);

  // 社区资源镜像源（FR-18 修订：选项落成真实设置，Phase 3 起被消费）
  const communitySection = el("section", { class: "card settings-card" }, [el("p", { class: "settings-status" }, ["加载中…"])]);
  pane.append(communitySection);
  await renderCommunityMirrorPanel(communitySection);

  // GitHub Token（FR-03 配额提额；DPAPI 加密落凭据库）
  const tokenSection = el("section", { class: "card settings-card" }, [el("p", { class: "settings-status" }, ["加载中…"])]);
  pane.append(tokenSection);
  await renderGithubTokenPanel(tokenSection);
}

/** GitHub Token 面板（市场/Skill 聚合 API 提额，匿名 60 次/小时 → 配置后大幅提升）。 */
async function renderGithubTokenPanel(card: HTMLElement): Promise<void> {
  card.replaceChildren(el("h2", { class: "card__title" }, ["GitHub Token（市场提额）"]));
  let status: KeyvaultStatus;
  try {
    status = await getStatus();
  } catch (err) {
    card.append(el("p", { class: "settings-status settings-status--error" }, [String(err)]));
    return;
  }
  const statusLine = el("p", { class: "settings-status" }, [""]);
  const tokenInput = el("input", {
    type: "password",
    class: "settings-input",
    placeholder: status.githubTokenConfigured ? KEY_MASK : "ghp_…（可选，加密落盘，不明文回读）",
  }) as HTMLInputElement;
  if (status.githubTokenConfigured) tokenInput.value = KEY_MASK;
  const saveBtn = el("button", { type: "button", class: "btn btn--ghost settings-inline-btn" }, ["保存 Token"]);
  saveBtn.addEventListener("click", () => {
    void (async () => {
      const value = tokenInput.value.trim();
      if (value === KEY_MASK || value === "") {
        statusLine.textContent = "未修改";
        statusLine.classList.remove("settings-status--error");
        return;
      }
      saveBtn.disabled = true;
      try {
        await setKey("github", value);
        tokenInput.value = KEY_MASK;
        statusLine.textContent = "Token 已加密保存（市场与 Skill 目录的 API 配额即刻提升）";
        statusLine.classList.remove("settings-status--error");
      } catch (err) {
        statusLine.textContent = `保存失败：${String(err)}`;
        statusLine.classList.add("settings-status--error");
      } finally {
        saveBtn.disabled = false;
      }
    })();
  });
  const clearBtn = el("button", { type: "button", class: "btn btn--line settings-inline-btn" }, ["清除"]);
  if (!status.githubTokenConfigured) {
    clearBtn.disabled = true;
    clearBtn.title = status.githubTokenEnvAuto ? "Token 来自环境变量，无法在此清除" : "尚无已存 Token";
  }
  clearBtn.addEventListener("click", () => {
    void (async () => {
      clearBtn.disabled = true;
      try {
        await clearKey("github");
        tokenInput.value = "";
        tokenInput.placeholder = "ghp_…（可选，加密落盘，不明文回读）";
        statusLine.textContent = "已清除";
        statusLine.classList.remove("settings-status--error");
      } catch (err) {
        statusLine.textContent = `清除失败：${String(err)}`;
        statusLine.classList.add("settings-status--error");
      } finally {
        clearBtn.disabled = false;
      }
    })();
  });
  card.append(
    el("p", { class: "settings-path" }, ["未配置时以匿名配额运行；配置后提升额度。"]),
    el("div", { class: "settings-row" }, [tokenInput, saveBtn, clearBtn]),
    statusLine,
  );
}

// ===== 热键 =====

/** 设置·热键 = DSH 网页端原版快捷键的只读速查（2026-09-25 需求方裁决）：
 * 热键只有「DSH 网页端操作快捷键」一个含义，启动器不设自身快捷键；
 * DSH 键位硬编码、无配置机制，整页只读、无任何编辑控件。
 * 目录基线 = BASELINE_DSH_VERSION（BASELINE_VERIFIED_RANGE 实测一致，DSH 升级后复检），数据见 lib/dsh-hotkeys.ts。 */
function renderHotkeysPane(pane: HTMLElement): void {
  pane.replaceChildren();
  pane.append(el("p", { class: "settings-path" }, ["DSH 网页端原版快捷键（只读，由 DSH 本体决定）。"]));
  for (const group of DSH_HOTKEY_GROUPS) {
    const list = el("div", { class: "hotkey-list" }, []);
    for (const entry of DSH_HOTKEYS.filter((e) => e.group === group.id)) {
      list.append(
        el("div", { class: "hotkey-row" }, [
          el("span", { class: "hotkey-row__label" }, [entry.label]),
          el("span", { class: "hotkey-row__keys" }, entry.keys.map((key) => el("kbd", { class: "hotkey-kbd" }, [key]))),
        ]),
      );
    }
    pane.append(el("h2", { class: "card__title" }, [group.label]), list);
  }
}

// ===== 页面壳 =====

export const settingsPage: PageDefinition = {
  id: "settings",
  navLabel: "设置",
  render: () => {
    type Category = "providers" | "config" | "hotkeys";
    let category: Category = "providers";
    const categories: { id: Category; label: string }[] = [
      { id: "providers", label: "模型提供商" },
      { id: "config", label: "配置" },
      { id: "hotkeys", label: "热键" },
    ];

    const leftNav = el("div", { class: "sub-left__group" }, []);
    leftNav.append(el("div", { class: "sub-left__label" }, ["设置"]));
    const pane = el("div", { class: "settings-pane" }, []);

    function renderRight(): void {
      if (category === "providers") {
        void renderProvidersPane(pane);
      } else if (category === "config") {
        void renderConfigPane(pane);
      } else {
        renderHotkeysPane(pane);
      }
    }
    function renderLeft(): void {
      leftNav.replaceChildren();
      leftNav.append(el("div", { class: "sub-left__label" }, ["设置"]));
      for (const item of categories) {
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
          renderLeft();
          renderRight();
        });
      }
    }

    renderLeft();
    renderRight();

    return el("section", { class: "page" }, [
      el("div", { class: "sub sub--compact" }, [
        el("aside", { class: "sub-left" }, [leftNav]),
        el("section", { class: "sub-right" }, [pane]),
      ]),
    ]);
  },
};
