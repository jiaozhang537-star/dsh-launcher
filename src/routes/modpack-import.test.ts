// @vitest-environment happy-dom
/** 导入弹窗落点逻辑回归（2026-09-20 二轮实机验证发现的问题域）：
 *  纯设置包此前不渲染目标版本下拉，「只写入下方所选版本」会静默落到默认实例
 *  （用户不可选不可见）——修复后：纯设置包也渲染「设置写入到版本」下拉，且
 *  「随版本」落点未选目标时拦截并给可读提示。routes 层首个测试。 */
import { beforeEach, describe, expect, it, vi } from "vitest";

const h = vi.hoisted(() => ({
  peek: vi.fn(),
  importFn: vi.fn(),
  pickOpen: vi.fn(),
  listInstances: vi.fn(),
}));

vi.mock("../services/modpack", () => ({
  modpackPeek: h.peek,
  modpackImport: h.importFn,
  pickOpenTgz: h.pickOpen,
}));
vi.mock("../services/instances", () => ({
  listInstances: h.listInstances,
}));

/** 每用例独立模块实例：modpack-import 持有模块级重入锁（弹窗存活期忽略再次打开）。 */
async function freshOpen(): Promise<(filePath: string | null) => Promise<void>> {
  vi.resetModules();
  return (await import("./modpack-import")).openModpackImport;
}

const INST_A = { id: "i-a", name: "甲", profile: "web", nodeVersion: null, dshVersion: "0.1.0", lastLaunchedAt: null, port: 3090, createdAt: "2026-09-01T00:00:00Z", dir: null };
const INST_B = { id: "i-b", name: "乙", profile: "web", nodeVersion: null, dshVersion: null, lastLaunchedAt: null, port: 3091, createdAt: "2026-09-02T00:00:00Z", dir: null };

function settingsManifest() {
  return {
    formatVersion: 1,
    type: "settings" as const,
    name: "探针设置包",
    generator: "test",
    createdAt: "2026-09-20T00:00:00Z",
    plugins: [],
    settings: {
      vendors: [],
      hotkeys: {},
      injectSettings: { "llm-deepseek": { thinking: "disabled" } },
      agentsMd: null,
    },
  };
}

function pluginsManifest() {
  return {
    formatVersion: 1,
    type: "plugins" as const,
    name: "探针插件包",
    generator: "test",
    createdAt: "2026-09-20T00:00:00Z",
    plugins: [{ name: "dsh-x", version: "1.0.0", source: "npm", file: "packages/dsh-x-1.0.0.tgz" }],
    settings: null,
  };
}

const tick = (): Promise<void> => new Promise((resolve) => setTimeout(resolve, 0));
async function settle(): Promise<void> {
  for (let i = 0; i < 10; i++) await tick();
}

const findAll = <T extends Element>(sel: string): T[] => [...document.querySelectorAll<T>(sel)];
const buttonByText = (text: string): HTMLButtonElement | undefined =>
  findAll<HTMLButtonElement>("button").find((b) => b.textContent === text);
const hasText = (text: string): boolean => (document.body.textContent ?? "").includes(text);

describe("openModpackImport 落点逻辑", () => {
  beforeEach(() => {
    document.body.replaceChildren();
    h.peek.mockReset();
    h.importFn.mockReset();
    h.pickOpen.mockReset();
    h.listInstances.mockReset();
  });

  it("纯设置包渲染「设置写入到版本」下拉（修复回归），可选任意实例并按其落点导入", async () => {
    h.peek.mockResolvedValue(settingsManifest());
    h.listInstances.mockResolvedValue([INST_A, INST_B]);
    h.importFn.mockResolvedValue({ manifestName: "探针设置包", installed: [], applied: ["注入配置（实例覆盖）"], warnings: [] });

    const open = await freshOpen();
    void open("/x/probe.tgz");
    await settle();

    // 修复点：纯设置包也必须出现版本下拉，且默认选中「当前选中实例」。
    expect(hasText("设置写入到版本")).toBe(true);
    const select = document.querySelector<HTMLSelectElement>("select.modpack-target");
    expect(select).not.toBeNull();
    expect(findAll<HTMLOptionElement>("select.modpack-target option").map((o) => o.value)).toEqual(["i-a", "i-b"]);
    expect(select!.value).toBe("i-a");

    // 选「只写入下方所选版本」→ 显式挑 i-b → 导入应以实例落点调用。
    const radios = findAll<HTMLInputElement>('input[type="radio"]');
    const instanceRadio = radios[1];
    instanceRadio.checked = true;
    instanceRadio.dispatchEvent(new Event("change"));
    select!.value = "i-b";
    select!.dispatchEvent(new Event("change"));
    buttonByText("导入")!.click();
    await settle();

    expect(h.importFn).toHaveBeenCalledTimes(1);
    expect(h.importFn.mock.calls[0].slice(0, 3)).toEqual(["/x/probe.tgz", "i-b", "instance"]);
  });

  it("「随版本」落点未选目标版本时拦截并提示（不再静默兜底）", async () => {
    h.peek.mockResolvedValue(settingsManifest());
    h.listInstances.mockResolvedValue([INST_A, INST_B]);
    h.importFn.mockResolvedValue({ manifestName: "探针设置包", installed: [], applied: [], warnings: [] });

    const open = await freshOpen();
    void open("/x/probe.tgz");
    await settle();
    const radios = findAll<HTMLInputElement>('input[type="radio"]');
    radios[1].checked = true;
    radios[1].dispatchEvent(new Event("change"));
    // 模拟无目标可用：清空下拉选项（value 变空串）
    document.querySelector<HTMLSelectElement>("select.modpack-target")!.replaceChildren();
    buttonByText("导入")!.click();
    await settle();

    expect(h.importFn).not.toHaveBeenCalled();
    expect(hasText("请先在「设置写入到版本」里选择目标版本")).toBe(true);
  });

  it("插件包 + 零实例：按钮禁用、给指路错误行、空下拉禁用", async () => {
    h.peek.mockResolvedValue(pluginsManifest());
    h.listInstances.mockResolvedValue([]);

    const open = await freshOpen();
    void open("/x/plugins.tgz");
    await settle();

    expect(hasText("还没有任何版本")).toBe(true);
    const sel = document.querySelector<HTMLSelectElement>("select.modpack-target");
    expect(sel).not.toBeNull();
    expect(sel!.disabled).toBe(true); // 无实例可选的禁用空下拉（既有形态）
    expect(buttonByText("导入")!.disabled).toBe(true);
  });

  it("纯设置包 + 零实例：无版本下拉、「随版本」radio 禁用（global 仍可导）", async () => {
    h.peek.mockResolvedValue(settingsManifest());
    h.listInstances.mockResolvedValue([]);
    h.importFn.mockResolvedValue({ manifestName: "探针设置包", installed: [], applied: ["注入配置（全局默认）"], warnings: [] });

    const open = await freshOpen();
    void open("/x/probe.tgz");
    await settle();

    expect(hasText("设置写入到版本")).toBe(false);
    expect(document.querySelector("select.modpack-target")).toBeNull();
    const radios = findAll<HTMLInputElement>('input[type="radio"]');
    expect(radios[1].disabled).toBe(true);

    buttonByText("导入")!.click();
    await settle();
    expect(h.importFn).toHaveBeenCalledTimes(1);
    expect(h.importFn.mock.calls[0].slice(0, 3)).toEqual(["/x/probe.tgz", "", "global"]);
  });
});