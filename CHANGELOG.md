# 变更记录

本项目遵循 [语义化版本](https://semver.org/lang/zh-CN/)；发布节奏见 [docs/ROADMAP.md](docs/ROADMAP.md)。

## [0.1.0] - 2026-10-06（内测预发布）

首个公开内测版：DeepSeek Harness（DSH）的图形化启动器——像 PCL 管理 Minecraft 一样管理 DSH。**只是壳**：不修改 DSH 本体一行代码，一切设置通过 DSH 原生配置机制生效。

### 能力

- **下载与实例**：npm 通道下载 DSH 本体进独立实例目录（npmmirror 优先 + 官方兜底、SHA256 校验、断点续传）；Node 运行时自动代管；同一版本可重复安装（自动加序号后缀）。
- **多实例隔离**：独立数据根 / profile / 插件 / 技能 / 会话 / 钉定 Web 端口（3090 起，3080~3089 留给用户自用）；隔离开关（数据入共享区）；外部文件夹原地注册。
- **启动闭环**：启动前校验链（Node → Key → 路由 → 默认模型 → 完整性 → 插件体检）→ 配置对账 → 拉起进程 → 抓取带 token 的网页地址自动打开；取消令牌按代隔离，启动占位互斥；运行记录跨会话识别孤儿进程。
- **模型厂商**：内置厂商目录 + 自定义提供方，Key 走 Windows DPAPI 密文保险库、启动时仅注入进程环境（明文不落盘）；默认模型与 provider 成对写入实例路由。
- **插件市场 / Skill 中心**：聚合 npm `dsh-plugin` 与 GitHub 补星标，热度/最新双排序、逐版本下载量；安装/启停/更新走 DSH 官方链；Skill 装进实例 `skills/` 即被 DSH watcher 发现。
- **整合包**：勾选导出自包含 tgz（明确排除 Key 与环境变量）、拖入自动分辨（插件合集 / 设置包 / 混合）、失败整体回滚、导入留档 `imported/` 支持后续增删。
- **启动器自更新**：GitHub Releases 检查与下载，恒直连官方（不经镜像改写），安装包强制 SHA256SUMS 校验（发布资产须附校验清单）。

### 工程

- 纯前端 Vanilla TS（无 UI 框架）+ Rust 后端（Tauri 2），UI 为 PCL2 的延伸（逐属性对齐开源源码，推导来源全部登记于 `docs/design/pcl-visual-spec.md` §8）；
- 门禁：cargo 单测 233 例（R2 后）+ 前端 vitest 107 例 + tsc/vite 构建，三绿为提交底线；
- Windows 安装包：NSIS（currentUser，简体中文），WebView2 缺失自动引导；卸载时中文数据警示卡（保留 / 一并删除两处数据目录）；
- 已知边界：安装包未做代码签名（SmartScreen 提示为预期态，README 有指引）；一期仅出 Windows 包，路径与进程层保持可移植。
