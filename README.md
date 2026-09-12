# DSH 启动器

DeepSeek Harness（DSH）的图形化启动器：**像 PCL 管理 Minecraft 一样管理你的 DSH**——多实例下载与全套隔离、插件市场、Skill 中心、整合包分享、运行环境（Node）托管、模型厂商配置。

**它只是壳**：不修改 DSH 本体一行代码，一切设置都通过 DSH 的原生配置机制（实例目录下的 `settings.yaml` / `.env` / 进程环境变量）生效。

## 现在能做什么

| 能力 | 说明 |
| --- | --- |
| 下载与安装 | 从 GitHub Releases 列版本下载 DSH 本体（可配镜像前缀），落进独立实例目录；也支持源码包安装 |
| 环境托管 | 缺 Node 时按代管清单下载便携版并纳入统一目录，启动时优先使用；启动前校验会给出人话结论 |
| 多实例隔离 | 每个实例有独立的数据根、profile、插件、Skill、会话与固定 Web 端口，浏览器里可以并存互不干扰 |
| 模型厂商 | 内置 28 家厂商目录（地址/协议/图标）+ 自定义提供方；Key 走 Windows DPAPI 密文保险库，启动时注入进程环境；默认模型与提供方成对写入实例 |
| 插件市场 | 聚合 npm `keywords:dsh-plugin` 主干与 GitHub topic 补星标，热度/最新双排序、每版本下载量、任选版本安装 |
| Skill 中心 | 从 GitHub 仓库把 SKILL.md 布局装进实例的 `skills/`，DSH 自带 watcher 即发现 |
| 整合包 | 浏览层已具备（清单格式与一键安装见 ROADMAP Phase 4） |
| 启动闭环 | 启动前校验 → 实例配置对账 → 拉起进程 → 等待 DSH 打印带 token 的网页地址 → 用系统浏览器打开；加载页以字幕序列如实报告每一步 |

七个常见疑问的逐条回答（含真机证据与据此修掉的缺陷）见 [docs/reviews/2026-09-12-p0-p3-qa.md](docs/reviews/2026-09-12-p0-p3-qa.md)。

## 开发

前置：Node ≥ 22（本仓用系统 Node 构建前端）、Rust 工具链、Tauri 2 的 Windows 依赖（WebView2 + MSVC 构建工具）。

```sh
npm install
npm run tauri dev      # 开发运行（前端热更 + Rust 增量编译）
npm run tauri build    # 出 Windows 安装包（安装包分发见 ROADMAP Phase 6）
```

门禁（提交前必须全绿）：

```sh
cd src-tauri && cargo test --lib          # Rust 单测
npm run test                              # 前端单测（vitest）
npm run build                             # tsc 类型检查 + vite 打包
cd src-tauri && cargo test --lib -- --ignored   # 走网络 / 需本机实例的 e2e
```

> 受限沙箱下 `npm run test` 与 `npm run build` 会因 esbuild 的管道 IPC 报 `spawn EPERM`——这是沙箱边界，不是项目缺陷；需在放宽的主机沙箱下执行。

## 目录

```text
src/                 前端（Vanilla TS + 自研极简路由）
  routes/            页面：启动 / 下载 / 版本设置 / 设置 / 更多
  services/          Tauri invoke 的类型化封装（每个命令一处）
  lib/               纯函数逻辑（可 vitest 直测）
src-tauri/src/       后端（Rust）
  node_runtime.rs    托管 Node：下载、解包、选版
  dsh_install.rs     DSH 本体安装（npm）
  source_archive.rs  源码包安装
  instances.rs       实例注册表与目录布局
  inject.rs          注入配置的合并与落盘（settings.yaml / .env）
  llm_bridge.rs      厂商配置 → 实例 llm-pi-ai 路由 / 默认模型的桥接
  keyvault.rs        DPAPI 密文保险库 + 环境变量名（注入与路由共用）
  precheck.rs        启动前校验链（Node / Key / 路由 / 默认模型 / 完整性 / 插件）
  process.rs         实例进程生命周期（拉起、钉端口、运行记录、停）
  log_sentinel.rs    DSH 输出 → 人话诊断
  market.rs         插件市场聚合（GitHub + npm + 镜像网关）
  plugin_manage.rs  插件清单与增删（复用 DSH 官方 add/remove 链）
  skills.rs         Skill 安装
docs/                项目文档（见下）
```

## 文档

| 文件 | 回答什么 |
| --- | --- |
| [docs/PROJECT.md](docs/PROJECT.md) | 这个项目到底是什么：定位、硬性原则、需求边界、架构取向、完成标准 |
| [docs/ROADMAP.md](docs/ROADMAP.md) | 走到哪了：阶段划分与当前阶段 |
| [docs/plans/](docs/plans/) | 当前阶段的详细计划与「进度与决策日志」（会话中断后从这里接续） |
| [docs/reviews/](docs/reviews/) | 对抗式审查与 QA 报告 |
| [AGENTS.md](AGENTS.md) | 本仓的开工前必读与红线纪律 |

## 许可

MIT（见 [LICENSE](LICENSE)）——仅需保留署名，允许商用与二次分发。
