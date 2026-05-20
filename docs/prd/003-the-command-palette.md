# PRD: The Command Palette — Desktop Gateway (Phase 2)

## Introduction

SyncMind Phase 2 的目标是为 Rust 核心引擎穿上一层**桌面交互外壳**。我们将构建一个基于 Tauri + SolidJS 的桌面应用，形态类似 Raycast 的**全局命令面板 (Command Palette)**：用户通过系统级快捷键随时唤醒一个悬浮搜索窗口，对本地知识库执行毫秒级语义检索，并直接预览、复制或打开原始文件。

该阶段**完全依赖 Phase 1 的 Rust 核心能力**，不引入任何新的云端逻辑或数据持久化需求。核心引擎以**库 (Library)** 形式被编译进 Tauri 后端，前端通过 Tauri Command 直接调用，省去 HTTP/MCP 层的序列化开销。

## Goals

- 构建一个内存占用极低（Tauri 进程空闲 < 150MB）的常驻桌面应用。
- 实现系统级全局快捷键唤醒/隐藏命令面板（macOS 首发的悬浮窗口）。
- 提供语义搜索、结果预览、快捷操作（复制、打开文件）的完整闭环。
- 集成 RAG 实验室面板，允许用户调参（top_k、文件类型过滤）并观察检索性能。
- 提供可视化设置界面与索引状态仪表盘，降低非技术用户的配置门槛。
- 支持系统托盘驻留与开机自启，确保核心引擎在后台持续运行。
- 所有数据交互通过本地 IPC 完成，零网络传输，绝对隐私。

## User Stories

### US-020: Tauri 应用脚手架与 SolidJS 前端初始化

**Description:** 作为开发者，我需要一个完整的 Tauri + SolidJS 工程结构，以便在桌面端消费 Rust 核心能力。

**Acceptance Criteria:**

- [ ] 在 `apps/desktop/` 下初始化 Tauri v2 项目结构（`src-tauri/` 后端 + `src/` 前端）。
- [ ] 前端采用 SolidJS + TypeScript，构建工具使用 Vite。
- [ ] 配置 pnpm workspace，使 `apps/desktop` 能引用 `packages/types` 等共享包。
- [ ] 后端 `Cargo.toml` 引入 workspace 依赖：`syncmind-core`、`syncmind-storage`、`syncmind-rag-engine`（作为本地 path 依赖）。
- [ ] 配置 Tauri 能力 (Capability) 文件，仅声明所需的最小权限集（无文件系统写权限暴露给前端）。
- [ ] `pnpm dev` 能正常拉起 Tauri 开发窗口；`cargo check` / `cargo clippy` 通过后端检查。

### US-021: Rust 核心库集成与 Tauri Command 封装

**Description:** 作为桌面应用，我需要通过 Tauri Command 直接调用 Rust 核心的搜索、配置与索引能力。

**Acceptance Criteria:**

- [ ] 在 `src-tauri/src/lib.rs` 中初始化 `syncmind-core` 的 Runtime（加载 `~/.config/syncmind/config.toml`，启动文件监听与索引管道）。
- [ ] 实现 Tauri Command `search_knowledge(query: String, top_k: u32, filter_file_type: Option<Vec<String>>) -> Vec<SearchResult>`，内部直接调用 `syncmind-rag-engine` / `syncmind-storage`。
- [ ] 实现 Tauri Command `get_config() -> Config` 与 `update_config(ConfigPatch) -> Result<()>`，持久化到 `config.toml`。
- [ ] 实现 Tauri Command `get_indexing_status() -> IndexingStatus`，返回已注册文件数、已索引块数、最后更新时间、错误列表。
- [ ] 实现 Tauri Command `trigger_reindex(file_path: Option<String>) -> Result<()>`，可选对单文件或全量重建索引。
- [ ] 所有返回结构体实现 `serde::Serialize`，并生成对应的 TypeScript 类型定义（通过 `specta` 或手动维护在 `packages/types` 中）。
- [ ] 错误处理统一返回 `{ error: String }`，前端负责展示 Toast 提示。

### US-022: 全局快捷键与悬浮窗口管理

**Description:** 作为用户，我希望通过全局快捷键随时唤醒或隐藏命令面板，且不干扰当前工作流。

**Acceptance Criteria:**

- [ ] 默认全局快捷键为 `Cmd+Shift+Space`（macOS），可通过设置界面修改。
- [ ] 快捷键由 Tauri Global Shortcut Plugin 注册，即使应用失去焦点也能响应。
- [ ] 窗口特性：
  - 无边框、无标题栏的悬浮面板，居中显示于当前活动屏幕。
  - 固定尺寸（推荐 860x540px），不可手动调整大小。
  - 失去焦点时自动隐藏（动画淡出 150ms），但不退出进程。
  - 再次按下快捷键时，若窗口已隐藏则显示并聚焦搜索框；若已显示则隐藏。
- [ ] `Esc` 键隐藏窗口。
- [ ] macOS 平台首发，代码中预留 Windows/Linux 快捷键配置的 TODO 注释。
- [ ] 窗口显示时，搜索框自动获得焦点且文本全选（便于直接输入新查询）。

### US-023: 命令面板搜索与结果列表

**Description:** 作为用户，我需要在命令面板中输入关键词，快速看到语义相关的代码片段或笔记。

**Acceptance Criteria:**

- [ ] 搜索框置于面板顶部，带占位符文本（如 "Search your local knowledge..."）。
- [ ] 输入防抖 (debounce) 300ms，避免频繁触发 Rust 端向量检索。
- [ ] 结果列表展示以下信息：
  - 文件路径（截断显示，悬浮显示完整路径）。
  - 内容摘要（前 120 个字符，高亮匹配关键词）。
  - 文件类型图标（Markdown、Rust、Python、PDF 等，基于扩展名映射）。
  - 相似度分数（保留两位小数，如 `0.92`）。
- [ ] 键盘导航：
  - `↑` / `↓` 切换选中项。
  - `Enter` 执行默认操作（复制内容到剪贴板）。
  - `Cmd+Enter` 在系统默认编辑器中打开源文件。
- [ ] 空状态：无结果时显示 "No matches found. Try a broader query."。
- [ ] 加载状态：搜索过程中显示不可交互的骨架屏或 Spinner。

### US-024: 预览窗格与快捷操作

**Description:** 作为用户，我希望在不离开面板的情况下预览完整内容，并快速采取行动。

**Acceptance Criteria:**

- [ ] 面板采用左右分栏布局：左侧结果列表（占 40%），右侧预览窗格（占 60%）。
- [ ] 预览窗格显示：
  - 文件路径与行号范围（如 `src/main.rs:42-58`）。
  - 完整块内容，代码块使用等宽字体并保留缩进。
  - 语法高亮（SolidJS 端使用 `shiki` 或 `prismjs`，基于文件扩展名选择语言）。
- [ ] 快捷操作栏（预览窗格底部或悬浮）：
  - **Copy**: 将 `content` 复制到系统剪贴板，显示 "Copied!" 反馈。
  - **Open File**: 调用系统默认程序打开源文件（`open::that` 在 Rust 端实现）。
  - **Reveal in Finder**: 在 Finder 中定位并选中该文件（macOS）。
- [ ] 预览内容超长时支持垂直滚动，且不影响左侧列表滚动。
- [ ] 快捷键绑定：`Cmd+C` 在预览窗格聚焦时复制内容（前端处理）。

### US-025: RAG 实验室面板

**Description:** 作为高级用户，我希望能调优检索参数并观察引擎行为，以便优化我的知识库组织方式。

**Acceptance Criteria:**

- [ ] 通过底部 Tab 或侧边图标切换到 "RAG Lab" 面板。
- [ ] 调参控件：
  - `top_k` 滑块（范围 1–20，默认 5）。
  - `filter_file_type` 多选框（动态列出当前索引中的所有文件类型）。
  - 重置按钮（恢复默认值）。
- [ ] 调试信息区：
  - 单次查询耗时（ms）。
  - 返回结果数。
  - 当前使用的 Embedding 模型名称与维度。
- [ ] 原始 JSON 视图：可折叠展示 `search_knowledge` 的原始返回结果，便于排查问题。
- [ ] 参数变更实时生效，无需重启应用。

### US-026: 设置界面与索引状态仪表盘

**Description:** 作为用户，我需要可视化地管理配置和监控索引健康度，而不是手动编辑 TOML 文件。

**Acceptance Criteria:**

- [ ] 通过底部 Tab 或侧边图标切换到 "Settings" 面板。
- [ ] 基础设置表单：
  - `ollama_url` 文本输入（带格式校验）。
  - `ollama_model` 下拉选择（支持自定义输入）。
  - `mcp_transport` 单选（stdio / sse，仅展示，因桌面端不直接对外暴露 MCP）。
  - 修改后自动保存并热重载核心配置。
- [ ] 已注册文件管理：
  - 列表展示 `registered_files`，支持删除单条。
  - "Add File" 按钮调用 Tauri 的文件选择对话框（`dialog::open`），支持多选。
  - 新增文件后立即触发对该文件的增量索引。
- [ ] 索引状态仪表盘：
  - 卡片展示：总文件数、总块数、上次更新时间。
  - 错误日志列表：展示最近 10 条索引错误（文件路径 + 错误信息 + 时间戳）。
  - "Rebuild All" 按钮：触发全量重建索引，需二次确认（防止误触）。

### US-027: 系统托盘驻留与开机自启

**Description:** 作为用户，我希望 SyncMind 像后台守护进程一样常驻，随时响应我的搜索需求。

**Acceptance Criteria:**

- [ ] 应用启动后默认不显示主窗口，仅在系统托盘显示图标（macOS 菜单栏右侧）。
- [ ] 托盘菜单项：
  - **Open Palette**（点击等效于全局快捷键）。
  - **Settings...**（打开面板并切换到设置 Tab）。
  - **Indexing Status**（小图标提示：🟢 正常 / 🔴 有错误）。
  - **Quit**（完全退出进程）。
- [ ] 开机自启选项：设置面板中提供 "Launch at login" 开关，使用 `auto-launch` crate 或 Tauri Plugin 实现。
- [ ] 点击 Dock 图标时，若面板已隐藏则显示面板；若已显示则聚焦。
- [ ] `Cmd+Q` 或托盘 Quit 彻底退出应用，释放 Rust 核心资源（关闭 SQLite 连接、停止文件监听）。

## Functional Requirements

- FR-1: 桌面应用必须能够在 Tauri 后端直接初始化并运行 `syncmind-core` 的完整数据管道（文件监听 → 提取 → 分块 → 嵌入 → 存储）。
- FR-2: `search_knowledge` Tauri Command 必须直接调用本地向量库，返回延迟 < 200ms（95th percentile，本地 SSD 环境）。
- FR-3: 全局快捷键必须在 macOS 上始终可响应，无论当前活跃应用是什么。
- FR-4: 命令面板失去焦点后必须在 150ms 内完成隐藏动画并释放窗口焦点，不干扰用户后续操作。
- FR-5: 所有用户配置变更必须通过 Tauri Command 回写至 `~/.config/syncmind/config.toml`，并触发核心配置热重载。
- FR-6: 剪贴板操作、文件打开、Finder 定位等敏感系统交互必须在 Rust 后端完成，前端仅发送指令。
- FR-7: 设置面板中的 "Rebuild All" 操作必须在后台线程执行，不阻塞 UI，且进度通过 Tauri Event 推送到前端。
- FR-8: 系统托盘必须提供视觉状态指示：核心引擎运行中、索引正在进行、最后索引出错。

## Non-Goals

- **跨平台支持：** 本阶段仅支持 macOS。Windows 和 Linux 的快捷键、窗口行为、系统托盘逻辑在代码中预留扩展点，但不做实现。
- **Web Dashboard：** 基于 Next.js 的知识图谱和 RAG 实验室 Web 界面不属于本 PRD 范围，将在后续独立 PRD 中定义。
- **移动端应用：** React Native / Expo 的灵感捕获器不在本阶段。
- **云端同步：** 与 The Spine (Go 网关) 的 E2EE 同步逻辑不在本阶段；桌面端仅操作本地数据。
- **插件/扩展系统：** 不支持第三方自定义提取器、主题或快捷操作。
- **MCP 服务端暴露：** 桌面应用内部不启动 MCP Stdio/SSE 服务端；MCP 能力仍由独立的 `syncmind` CLI 守护进程提供（Phase 1）。未来可考虑在桌面应用中内嵌 MCP Server 模式作为可选功能。

## Design Considerations

- **视觉风格：** 深色模式优先（`#0f0f0f` 背景，`#e0e0e0` 文字），极简无边框设计，与 macOS 原生视觉融合。
- **动画：** 窗口显隐使用 `cubic-bezier(0.16, 1, 0.3, 1)` 缓动，时长 150ms；列表项切换使用 80ms 淡入淡出。
- **字体：** 中文使用系统默认 `"PingFang SC"`、`"Hiragino Sans GB"`；英文与代码使用 `"SF Mono"`、`"JetBrains Mono"`。
- **布局参考：** Raycast、Alfred、Arc Command Palette。
- **前端状态管理：** 使用 SolidJS 的 `createStore` 管理全局状态（搜索结果、配置、索引状态），避免引入重量级状态库。

## Technical Considerations

- **核心耦合模式：** 采用 "Library-in-Process" 模式。`apps/desktop/src-tauri` 将 `syncmind-core` 等 crate 作为 `path` 依赖引入。这意味着桌面应用启动即启动核心引擎，退出即停止。此模式简化了部署（单二进制文件），但牺牲了独立守护进程的持久性。若未来需要守护进程独立于 UI 运行，可迁移为 "Sidecar" 模式（Tauri 管理子进程）。
- **内存预算：** Tauri WebView + SolidJS 运行时约占用 60–80MB；Rust 核心（含 SQLite、文件监听、嵌入缓存）约 50–70MB。合计空闲目标 < 150MB，符合工程第一性原理的克制要求。
- **配置一致性：** 桌面应用与 CLI 守护进程共享同一个 `~/.config/syncmind/config.toml`。通过文件系统监听（`notify` crate）实现多进程间的配置热同步；本阶段桌面应用作为唯一写者，暂无需处理冲突。
- **类型安全边界：** Rust 后端与 SolidJS 前端之间的所有数据交换必须通过强类型结构体。推荐使用 `specta` + `tauri-specta` 自动生成 TypeScript 类型，确保重构时类型同步。
- **SQLite 并发：** 由于 Tauri 后端与潜在的外部 CLI 守护进程可能同时访问 `~/.local/share/syncmind/syncmind.db`，必须启用 SQLite WAL 模式（Write-Ahead Logging），并确保所有连接使用 `busy_timeout` 避免锁竞争。
- **stdout 纪律：** Tauri 内部使用 stdout 进行前端加载通信。Rust 后端的所有日志（`tracing`）必须定向到 stderr 或 `~/.local/share/syncmind/logs/desktop.log`，严禁污染 stdout。

## Success Metrics

- 从按下全局快捷键到搜索框可输入的延迟 < 300ms（冷启动后首次唤醒）。
- 语义搜索从输入停止到结果渲染的端到端延迟 < 500ms（95th percentile，查询本地 1000 个块以内）。
- 桌面应用空闲内存占用 < 150MB（Activity Monitor 观察 5 分钟均值）。
- 索引状态仪表盘中，文件变更到块更新在 UI 中反映的延迟 < 5 秒（文件监听 + 重新索引异步完成）。

## Open Questions

- 是否需要为搜索结果提供 "收藏 / Pin" 功能，以便用户固定常用片段？ 需要提供
- RAG Lab 面板中的 `filter_file_type` 是否需要支持通配符或正则（如 `*.rs`）？ 需要支持
- 当用户通过桌面应用修改 `registered_files` 时，是否同时通知外部 MCP 守护进程（如果正在运行）进行配置重载？是否需要文件锁或 IPC 机制？ 需要进行配置重载。需要文件锁和IPC机制。
- 预览窗格的语法高亮库（`shiki` vs `prismjs`）是否对包体积有显著影响？是否需要懒加载语言定义？ 追求极致体验（选 Shiki + 懒加载）
