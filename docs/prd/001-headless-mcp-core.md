# PRD: Headless MCP Core (Phase 1)

## Introduction

SyncMind Phase 1 的核心目标是构建一个**无头（Headless）的本地上下文引擎**。它作为一个常驻后台的 Rust 进程，能够：
1. 接收用户显式注册的文件路径；
2. 提取内容、语义分块、生成向量嵌入；
3. 将数据持久化到本地 SQLite + sqlite-vec；
4. 通过 **Model Context Protocol (MCP)** 向外部工具（如 Claude Code）暴露 `search_knowledge` 能力。

该阶段**完全不包含 UI**，所有交互通过配置文件、CLI 参数和 MCP 协议完成。

## Goals

- 建立一个内存占用极低（空闲 < 100MB）的常驻 Rust 守护进程。
- 实现从本地文件到向量检索的完整数据管道（File → Text → Chunks → Embeddings → Index）。
- 支持 Markdown、代码文件、PDF 的文本提取。
- 支持通过 MCP Stdio 和 SSE 双协议暴露 `search_knowledge` Tool。
- Embedding 生成优先调用本地 Ollama（使用 `bge-m3`），Ollama 不可用时自动降级为内置 ONNX 模型（`bge-small`）。
- 所有数据（原始文本、向量、元数据）100% 本地存储，零外泄。

## User Stories

### US-001: 工程脚手架与配置系统
**Description:** 作为开发者，我需要稳定的 Rust 项目结构和配置系统，以便后续功能模块化开发。

**Acceptance Criteria:**
- [ ] 初始化 Cargo workspace，核心 crate 名为 `syncmind-core`。
- [ ] 引入 `serde`、`toml`、`anyhow`、`tracing` 等基础依赖。
- [ ] 实现配置加载：从 `~/.config/syncmind/config.toml` 读取用户配置。
- [ ] 配置 Schema 至少包含：`ollama_url`、`ollama_model`、`mcp_transport` (`stdio` | `sse`)、`bind_addr` (SSE 时用)、`registered_files` (路径列表)。
- [ ] Typecheck 通过（`cargo check` / `cargo clippy`）。

### US-002: 文件注册与变更监听
**Description:** 作为用户，我想显式注册一组本地文件路径，并让系统监听其变更，以便我的知识库始终与本地文件同步。

**Acceptance Criteria:**
- [ ] 启动时读取配置中的 `registered_files` 列表。
- [ ] 使用 `notify` crate 对列表中的文件路径建立监听（支持文件修改、删除、重命名事件）。
- [ ] 文件变更后，自动触发对应文件的重新索引流程（异步，不阻塞主线程）。
- [ ] 提供 CLI 子命令 `syncmind register <path>` 和 `syncmind unregister <path>`，动态修改配置并热重载监听器。
- [ ] 新增文件注册时，立即执行一次全量索引。
- [ ] Typecheck / Lint 通过。

### US-003: 多格式文本提取管道
**Description:** 作为用户，我希望系统能自动从我的 Markdown、代码、PDF 中提取纯文本，以便后续向量化。

**Acceptance Criteria:**
- [ ] 实现 `Extractor` trait，统一不同文件类型的提取接口：`fn extract(path: &Path) -> Result<String>`。
- [ ] **Markdown**: 使用 `pulldown-cmark` 提取纯文本（过滤 YAML frontmatter 可选但推荐保留）。
- [ ] **代码文件**: 根据扩展名（`.rs`, `.py`, `.ts`, `.go` 等）直接读取文本，保留格式。
- [ ] **PDF**: 集成 `pdf-extract` 或 `lopdf` 进行基础文本抽取（Phase 1 不要求排版保留，纯文本即可）。
- [ ] **图片 OCR**: **Phase 1 暂不实现**，但需在 `Extractor` 架构中预留扩展点（如 `ImageOcrExtractor` stub）。
- [ ] 提取失败时记录 `tracing::warn`，跳过该文件但不中断管道。
- [ ] Typecheck / Lint 通过。

### US-004: 语义分块引擎 (Chunking)
**Description:** 作为 AI 上下文引擎，我需要将长文本切分为语义连贯的小块，以便生成高质量向量并适配大模型上下文窗口。

**Acceptance Criteria:**
- [ ] 实现 `Chunker` 模块，支持配置 `chunk_size`（默认 512 tokens）和 `chunk_overlap`（默认 50 tokens）。
- [ ] 对代码文件使用语言感知的分块（如按函数/类/结构体边界切分），可使用 `tree-sitter` 进行 AST 级别的粗分，fallback 到按行数/字符数。
- [ ] 对 Markdown 按标题层级进行粗分，超长段落再进行细粒度重叠切分。
- [ ] 每个 Chunk 生成唯一 ID（`file_path + chunk_index` 的哈希），并记录起始/结束行列号。
- [ ] Typecheck / Lint 通过。

### US-005: 嵌入生成与降级策略
**Description:** 作为用户，我希望系统优先使用我已安装的 Ollama `bge-m3` 模型生成向量，若 Ollama 未运行则自动使用内置 ONNX 模型，保证系统随时可用。

**Acceptance Criteria:**
- [ ] 实现 `Embedder` trait：`fn embed(texts: &[&str]) -> Result<Vec<Vec<f32>>>`。
- [ ] **Ollama Embedder**: 通过 HTTP POST `ollama_url/api/embed` 批量发送文本，指定模型 `bge-m3`。要求支持 batch 推理以减少网络往返。
- [ ] **ONNX Embedder**: 使用 `ort` crate 加载内置（或首次下载缓存到 `~/.local/share/syncmind/models/`）的 `bge-small-en-v1.5` ONNX 模型。
- [ ] **降级逻辑**: 启动时探测 Ollama（发送健康检查请求）。
  - 若可达且模型存在：使用 Ollama。
  - 若不可达：打印 `tracing::info` 提示降级，加载 ONNX 模型。
  - 若 ONNX 模型文件不存在：自动从 Hugging Face（或预置镜像）下载并缓存。
- [ ] 生成的向量维度需与 sqlite-vec 表定义一致（Ollama `bge-m3` 为 1024 维，ONNX `bge-small` 为 384 维，需在配置中声明 `embedding_dim`，数据库 Schema 需支持动态维度或严格校验）。
- [ ] Typecheck / Lint 通过。

### US-006: 本地向量存储与元数据管理
**Description:** 作为系统，我需要将文本块、原始内容、文件元数据和向量持久化到本地 SQLite，支持高效的向量相似度检索。

**Acceptance Criteria:**
- [ ] 使用 `rusqlite` 连接本地数据库 `~/.local/share/syncmind/syncmind.db`。
- [ ] 加载 `sqlite-vec` 扩展（通过 `rusqlite` 的 `load_extension` 或静态链接）。
- [ ] Schema 设计：
  - `files` 表：`id` (主键), `absolute_path` (唯一), `file_type`, `last_modified`, `last_indexed`。
  - `chunks` 表：`id` (主键), `file_id` (外键), `chunk_index`, `start_line`, `end_line`, `content` (TEXT)。
  - `vec_chunks` 虚拟表（sqlite-vec）：`chunk_id` (主键), `embedding` (FLOAT32 向量)。
- [ ] 实现 `VectorStore` 结构体，提供：
  - `upsert_file(file_meta, chunks, embeddings)`：事务性写入（先删后插）。
  - `search(query_embedding: &[f32], top_k: usize) -> Result<Vec<SearchResult>>`：使用 sqlite-vec 的向量相似度检索。
- [ ] 索引更新时保证原子性（SQLite 事务），崩溃后可安全重试。
- [ ] Typecheck / Lint 通过。

### US-007: MCP 协议服务层
**Description:** 作为 Claude Code 用户，我希望 SyncMind 能作为 MCP Server 被直接调用，通过 `search_knowledge` 工具查询我的本地知识。

**Acceptance Criteria:**
- [ ] 实现 MCP Server，遵循 [Model Context Protocol Specification](https://modelcontextprotocol.io/specification)。
- [ ] 暴露 Capability：`tools`，其中包含 `search_knowledge`。
- [ ] `search_knowledge` 的 Input Schema（JSON）：
  ```json
  {
    "query": "string",
    "top_k": "integer (default 5)",
    "filter_file_type": "string[] (optional)"
  }
  ```
- [ ] `search_knowledge` 的处理流程：
  1. 接收 `query` 字符串。
  2. 使用当前激活的 `Embedder` 将 `query` 转为向量。
  3. 调用 `VectorStore::search`。
  4. 返回 JSON 数组，每项包含：`chunk_id`, `file_path`, `start_line`, `end_line`, `content`, `score`。
- [ ] **Transport - Stdio**: 监听 stdin 的 JSON-RPC 消息，响应写入 stdout。兼容 Claude Code 默认的 MCP 调用方式。
- [ ] **Transport - SSE**: 启动 HTTP 服务器（如 `axum` 或 `tokio::net` + `hyper`），暴露 `/sse` 端点供客户端连接；通过 HTTP POST 回传消息。
- [ ] 通过配置文件 `mcp_transport` 字段切换传输层，启动时加载。
- [ ] 提供 MCP `initialize` 握手，返回 server info 和 capabilities。
- [ ] Typecheck / Lint 通过。

### US-008: CLI 与守护进程模式
**Description:** 作为用户，我希望能通过命令行启动 SyncMind 作为后台守护进程，或在前台运行以便调试。

**Acceptance Criteria:**
- [ ] 使用 `clap` 构建 CLI，支持以下子命令：
  - `syncmind daemon`：以后台模式运行（可配合 `--foreground` 前台运行）。
  - `syncmind register <path>`：注册新文件并热重载。
  - `syncmind unregister <path>`：移除文件监听。
  - `syncmind status`：显示已注册文件数、已索引 chunk 数、当前 Embedder 模式。
  - `syncmind search "<query>" [--top-k N]`：命令行直接测试语义检索，无需 MCP 客户端。
- [ ] `daemon` 启动时：
  - 初始化配置、数据库、文件监听器。
  - 对 `registered_files` 执行一次全量索引。
  - 启动 MCP Server（根据配置选择 Stdio 或 SSE）。
- [ ] 前台运行 (`--foreground`) 时，日志通过 `tracing-subscriber` 彩色输出到 stderr。
- [ ] Typecheck / Lint 通过。

### US-009: Claude Code 集成与端到端测试
**Description:** 作为开发者，我需要验证整个数据管道和 MCP 协议能被 Claude Code 正确调用。

**Acceptance Criteria:**
- [ ] 在 `docs/examples/` 下提供 `claude_desktop_config.json` 或 `claude_code_config.json` 示例，展示如何配置 SyncMind MCP Server。
- [ ] 手动或通过脚本测试：Claude Code 提问 "我关于某某功能之前写过什么？"，Claude Code 调用 `search_knowledge`，返回结果正确且相关。
- [ ] 测试文件修改后，系统自动重新索引，二次查询结果反映最新内容。
- [ ] 所有测试通过后，更新 README 中的 Phase 1 使用说明。

## Functional Requirements

- **FR-1:** 系统必须支持通过 `config.toml` 显式注册文件路径，不允许自动全盘扫描用户目录。
- **FR-2:** 系统必须监听已注册文件的修改、删除、重命名事件，并触发增量或全量重新索引。
- **FR-3:** 系统必须支持从 `.md`、代码文件（`.rs`, `.py`, `.ts`, `.js`, `.go`, `.java`, `.c`, `.cpp`, `.h`, `.hpp` 等）、`.pdf` 中提取纯文本。
- **FR-4:** 系统必须实现语义分块，代码文件优先按 AST 结构体/函数边界切分，Markdown 按标题层级切分，Fallback 到固定重叠窗口。
- **FR-5:** 系统必须实现双路 Embedder：优先 Ollama (`bge-m3`)，不可用时自动降级为本地 ONNX (`bge-small`)。
- **FR-6:** 系统必须使用 SQLite + `sqlite-vec` 作为本地向量与元数据存储引擎，数据文件存储在 `~/.local/share/syncmind/`。
- **FR-7:** 系统必须通过 MCP 协议暴露 `search_knowledge` Tool，支持 `query`、`top_k`、`filter_file_type` 参数。
- **FR-8:** 系统必须同时支持 MCP Stdio 和 SSE 两种传输方式，并通过配置项切换。
- **FR-9:** 系统必须提供 CLI 入口（`syncmind`），支持 `daemon`、`register`、`unregister`、`status`、`search` 子命令。
- **FR-10:** 所有用户原始文本和生成的向量数据必须仅保存在本地文件系统，任何流程不得将数据发送至外部网络（除本地 Ollama HTTP 接口外）。

## Non-Goals (Out of Scope)

- **NG-1:** 不实现任何 UI（Web、Desktop、Mobile）。Phase 1 是纯 Headless 核心。
- **NG-2:** 不实现目录级别的递归监听。Phase 1 仅支持显式文件路径注册（目录递归监听属于 Phase 2 或后期优化）。
- **NG-3:** 不实现图片 OCR。虽然架构预留接口，但 Phase 1 不落地实现。
- **NG-4:** 不实现云端同步、跨端传输、Go 网关。这属于 Phase 3 (The Spine)。
- **NG-5:** 不实现复杂的关系图谱（Graph RAG）或多跳推理检索。Phase 1 仅支持基于向量相似度的单跳检索。
- **NG-6:** 不实现权限管理或多用户支持。Phase 1 假设单用户本地运行。
- **NG-7:** 不实现自然语言到结构化查询（NL2SQL/NL2Cypher）的转换。

## Design Considerations

- **模块划分:** 核心 crate 内部按职责拆分为 `config`、`watcher`、`extractor`、`chunker`、`embedder`、`store`、`mcp`、`cli` 等模块。
- **错误处理:** 使用 `anyhow` 进行上层错误传播，关键路径使用 `thiserror` 定义结构化错误，确保 MCP 调用失败时返回标准的 JSON-RPC Error。
- **并发模型:** 文件监听、索引管道、MCP Server 均基于 `tokio` 异步运行时。文本提取和 ONNX 推理可能涉及 CPU 密集型任务，考虑使用 `tokio::task::spawn_blocking`。
- **配置热重载:** `registered_files` 的变更（通过 `register`/`unregister` CLI）应通过 `tokio::sync::watch` 或 `notify` 配置文件变更来触发监听器重建，无需重启进程。
- **日志:** 使用 `tracing` 结构化日志，日志文件可轮转写入 `~/.local/share/syncmind/logs/`。

## Technical Considerations

- **Rust 依赖选型:**
  - Async Runtime: `tokio` (rt-multi-thread)
  - CLI: `clap`
  - Config: `serde` + `toml`
  - DB: `rusqlite` + `libsqlite3-sys`（需开启 `bundled` feature）+ `sqlite-vec` 静态链接或动态加载
  - File Watch: `notify`
  - Markdown: `pulldown-cmark`
  - PDF: `pdf-extract`（或 `lopdf`）
  - AST/Code: `tree-sitter` + `tree-sitter-rust`, `tree-sitter-python`, `tree-sitter-javascript` 等
  - HTTP (Ollama & SSE): `reqwest` (客户端) + `axum` (服务端，SSE 时)
  - ONNX: `ort` (Rust ONNX Runtime 绑定)
  - JSON-RPC: `jsonrpsee`（若兼容）或手写极简 JSON-RPC 2.0 处理层
- **Embedding 维度对齐:**
  - `bge-m3` 输出 1024 维（或取决于 Ollama 配置）。
  - `bge-small` 输出 384 维。
  - sqlite-vec 建表时必须声明维度。考虑到 Phase 1 需要支持两种模型，**建议**在配置中强制用户声明 `embedding_dim`，初始化时根据该值建表；切换模型时若维度不匹配，需报错或触发重建索引。
- **MCP 协议细节:**
  - Stdio 模式需注意 stdout 只能输出 JSON-RPC 消息，所有日志必须写入 stderr 或文件。
  - SSE 模式需处理客户端连接断开、消息重传等边缘情况。
- **性能基线:**
  - 空载内存 < 100MB。
  - 1000 个文件、总计 10MB 文本的全量索引耗时 < 5 分钟（在 Apple M 系列或同等性能 CPU 上）。
  - 单次 `search_knowledge` 查询（含嵌入生成 + 向量检索）耗时 < 500ms。

## Success Metrics

- **功能闭环:** 用户可以通过 Claude Code 直接调用 `search_knowledge` 查询本地已注册文件的内容，且结果相关度可接受。
- **资源占用:** 守护进程空闲时 RSS 内存 < 100MB；索引过程中 CPU 占用平滑，不导致系统卡顿。
- **稳定性:** 文件频繁修改（如 Git 切换分支）时，系统不崩溃，最终索引状态与文件系统一致。
- **隐私合规:** 代码审计确认无任何硬编码云端 API Key，无数据外发逻辑。

## Open Questions

1. **PDF 提取质量:** `pdf-extract` 对复杂排版 PDF 的提取效果可能不佳，Phase 1 是否可接受纯文本乱序，还是需要引入更重的 OCR/布局分析库（如 `tesseract` / `pdf2image`）？
2. **Code AST 分块深度:** `tree-sitter` 支持的语言需要各自引入语法库，是否需要在 Phase 1 就支持 10+ 种语言，还是先聚焦 Rust/TypeScript/Python 三种？
3. **ONNX 模型分发:** `bge-small` 的 ONNX 模型文件（约 50-100MB）是打包进二进制（`include_bytes!` 不推荐，会导致体积过大），还是首次运行时从网络下载到 `~/.local/share/syncmind/models/`？考虑到“纯离线”原则，可能需要用户在联网环境下首次初始化，或者提供手动放置指南。
4. **SQLite-Vec 的并发:** sqlite-vec 基于 SQLite，写操作是库级锁。频繁文件变更可能导致写队列堆积，是否需要引入 `tokio::sync::Semaphore` 限制并发写入，或异步批处理队列？
5. **MCP Stdio + SSE 同时开启:** 配置是否允许同时开启两种传输，还是严格二选一？同时开启会增加架构复杂度（两个 Server 入口），但对高级用户可能更方便。
