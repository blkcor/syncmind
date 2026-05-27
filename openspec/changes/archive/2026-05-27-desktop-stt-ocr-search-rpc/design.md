## Context

桌面端 ingestion dispatcher（`apps/desktop/src-tauri/src/spine/dispatch.rs`）在 US-053（`desktop-spine-ingestion-dispatch`）中已完成按 kind 分流：
- `capture-audio` / `capture-image` → 二进制文件落地 + 占位 markdown + 索引
- `search-request` → 调用 `rpc` closure（已在 `commands.rs::process_inbound_bundle` 中连接了 `search_local_knowledge` + 加密响应上传）

当前状态：
- Audio/image 占位 markdown 内容为 `[mobile audio capture — transcription pending]` / `[mobile image capture — OCR pending]` — 无实际 STT/OCR 后处理。
- search-request 可正常处理但无速率限制。
- 所有处理在 dispatcher 的 `dispatch_bundle` 同步上下文中完成；audio/image 写入后即返回，没有异步后处理管道。
- `core/rag-engine/src/extractor.rs` 中已有 `ImageOcrExtractor` 和 `PdfExtractor`，但其 OCR 后端依赖系统命令 `tesseract`，跨平台分发负担重。

US-054 要填补的正是这三个空白，同时彻底移除 `tesseract` 系统命令依赖。

## Goals / Non-Goals

**Goals:**
- 在 capture-audio dispatch 后异步触发 Whisper 转写，结果更新到 markdown + 触发重新索引。
- 在 capture-image dispatch 后异步触发 `ocrs` 文字提取（通过 `core/rag-engine` 的 OCR 模块），结果更新到 markdown + 触发重新索引。
- 在 search-request 处理中增加 `peer_fingerprint` 维度的滑动窗口限速（30 req/min），超限返回加密 error payload。
- 将 `core/rag-engine/src/extractor.rs` 中的 OCR 后端从 tesseract 系统命令完全替换为 `ocrs` Rust 原生 crate。
- 所有重计算在非阻塞后台执行（`tokio::spawn_blocking`），不影响 dispatcher 主 async 路径。

**Non-Goals:**
- 不修改现有音频/图像二进制文件落地逻辑。
- 不修改 search-request 的正常处理路径（仅增加限速检查）。
- 不涉及前端 UI 变更（STT/OCR 是全后台操作）。
- 不添加模型监控 / 自动重试 / 模型版本管理。
- 不引入新的 IPC 或跨进程通信。
- 不替换 PDF 渲染环节的 `pdftoppm` 系统命令（仅 OCR 引擎替换为 ocrs，渲染仍用 poppler）。

## Decisions

### D1: 异步后处理模型 — 在 dispatch 路由中 spawn，而非独立 worker 队列

**Choice:** capture-audio 和 capture-image 路由在完成二进制落地和占位 markdown 写入后，同步 spawn 异步后处理任务，而非投入一个全局 worker 队列。

**Rationale:**
- 当前 dispatch.rs 的 `route_bundle` 已经是 async fn，可以直接 `tokio::spawn` 异步任务。
- 独立 worker 队列需要新增模块状态（队列、worker handle）、启动/关闭生命周期，对 MVP 过度设计。
- CPU 密集的 whisper/ocrs 调用走 `tokio::task::spawn_blocking` 不阻塞 tokio 主线程。
- 后续如果需要限并发（如同时只允许 1 个 STT 任务），可以在 `stt.rs` 中用 `tokio::sync::Semaphore` 控制。

### D2: STT 模块位置 — `apps/desktop/src-tauri/src/spine/stt.rs`

**Choice:** STT 模块放在 desktop Tauri crate 的 spine 子系统中。

**Rationale:**
- STT 目前仅用于 mobile audio capture 后处理，属于 Spine 协议层面的功能。
- `whisper-rs` 的编译产物（whisper.cpp C++ 代码）约 20MB，放在 desktop crate 中不会影响 core crates 的编译速度。

### D3: OCR 模块位置 — `core/rag-engine`，而非 desktop crate

**Choice:** OCR 逻辑集成到 `core/rag-engine`，在 `core/rag-engine/src/ocr.rs` 中新增 `ocrs` 包装模块。Desktop dispatch 后处理通过调用 `rag_engine::ocr` 的函数完成。

**Rationale:**
- `core/rag-engine/src/extractor.rs` 已经有 `ImageOcrExtractor`，其 OCR 后端应统一替换为 `ocrs`。将 `ocrs` 放在 rag-engine 中确保两处（本地文件索引 + mobile capture 后处理）共享同一份初始化逻辑。
- 避免在 desktop crate 中重复 OCR 初始化代码。
- `ocrs` 是纯 Rust 依赖，放在 rag-engine 中不增加构建复杂度。
- 符合"计算逻辑在 core，协议逻辑在 spine"的分层原则。

**Architecture:**
```
core/rag-engine/src/ocr.rs          ← ocrs wrapper (init, ocr_image)
  ├── OcrEngine singleton (OnceLock)
  ├── fn ocr_image(path) -> Result<String>
  └── fn ocr_image_from_bytes(bytes, format) -> Result<String>

core/rag-engine/src/extractor.rs    ← refactored extractors
  ├── ImageOcrExtractor::extract()  ← calls ocr::ocr_image()
  └── PdfExtractor::extract()       ← keeps pdftoppm, OCR step calls ocr::ocr_image()

apps/desktop/src-tauri/src/spine/dispatch.rs
  └── capture-image handler         ← calls rag_engine::ocr::ocr_image() after write
```

### D4: ocrs 替换 tesseract — 迁移策略

**Choice:** 用 `ocrs` 完全替换 `tesseract` 系统命令调用。保留 `pdftoppm` 用于 PDF 页面渲染（这是 PDF 渲染问题，不是 OCR 问题）。`ocrs` 本身不内置模型权重，桌面端通过本地 RTen 模型文件初始化 OCR engine；缺失模型时返回 recoverable OCR unavailable。

**Changes to `core/rag-engine/src/extractor.rs`:**
- `run_image_ocr()` — 重写为调用 `ocr::ocr_image()`，不再 spawn tesseract 子进程。
- `run_pdf_ocr()` — 保留 pdftoppm 渲染页面为 PNG，但 OCR 步骤改为调用 `ocr::ocr_image_from_bytes()`。
- `OcrConfig` — 移除 `ocr_binary_path` 字段（不再需要指定 tesseract 路径），移除 `OcrMode::Disabled`。保留 `mode`（用于 PDF `auto`/`force` 策略）、`pdf_renderer_path` 和 `pdf_text_quality_threshold`。
- 移除 `ocr_available()` / `pdf_text_extractor_available()` 中关于 tesseract 存在性的检查。

### D5: STT 模块设计 — `whisper-rs` + 按需下载

**Choice:** 使用 `whisper-rs` crate 直接绑定 whisper.cpp，模型文件从 Hugging Face `ggml-model-whisper-base.en` 仓库按需下载到 `<data-dir>/models/whisper/`。

**Rationale:**
- `whisper-rs` 是 Rust 生态中最成熟的 Whisper 绑定，纯 Rust API（C FFI behind the scenes），无 Python 运行时依赖。
- `ggml-base.en` 模型约 140MB，是准确率/大小平衡点。仅英文支持，符合 MVP scope。
- 下载通过 `reqwest`（已有依赖）发起，失败时 STT 静默降级。

### D6: 搜索限速器 — 内存滑动窗口

**Choice:** 基于 `tokio::sync::Mutex<HashMap<String, VecDeque<Instant>>>` 的滑动窗口限速器，以 peer_fingerprint 为 key。

**Rationale:**
- 简单、可靠、无外部依赖。
- 30 req/min 的低频率意味着窗口大小很小（最多 30 个 entry/peer）。
- 使用 `VecDeque` 的自动淘汰：插入时 `retain(|ts| ts.elapsed() < WINDOW)`。
- 懒淘汰：无 GC 任务，过期 entry 在下次请求时清理。

**Trade-off:** 重启后限速状态丢失。对于桌面端后台进程，重启意味着用户主动操作，reset 是可以接受的行为。

### D7: 模型文件结构

```
<data-dir>/models/whisper/
└── ggml-base.en.bin    # (~140MB) 第一次 STT 时下载

<data-dir>/sync-inbox/
├── audio/<id>.m4a      # 已有（US-053）
├── images/<id>.jpg     # 已有（US-053）
└── captures/<id>.md    # STT/OCR 后更新
```

## Risks / Trade-offs

- **[Model download timing]** → 首次 `capture-audio` dispatch 可能因下载而延迟。Mitigation：下载走 `spawn_blocking` + `reqwest` streaming，前台 dispatcher 不等待；placeholder markdown 在下载完成前保持 "pending" 状态。
- **[whisper.cpp build latency]** → `whisper-rs` 编译时会编译整个 whisper.cpp C++ 代码库，CI 首次构建可能增加 5-10 分钟。Mitigation：无 — 这是合理的编译成本。
- **[ocrs model availability / accuracy regression vs tesseract]** → `ocrs` 需要本地 RTen 模型文件，英文印刷体质量取决于所配置模型，中文/手写体可能有差异。Mitigation：模型缺失时返回 recoverable unavailable；<10 字符阈值过滤虚假阳性；`[image: no text detected]` 是显式降级信号；未来可在 ocrs 基础上替换或叠加新引擎。
- **[STT/OCR memory usage]** → Whisper `ggml-base.en` 加载后约占用 200-300MB RSS。Mitigation：这是单次加载成本；音频转写完成后 Whisper context 可析构（由 `Arc` 引用计数控制）。
- **[并发音频处理]** → 多个 audio capture 同时到达可能导致多路 whisper 实例争 CPU。Mitigation：stt.rs 使用 `tokio::sync::Semaphore::new(1)` 限制同时只有一个转写任务运行；其余排队。
