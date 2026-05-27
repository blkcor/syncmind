## Why

Phase 4 移动端（PRD 005）依赖桌面端完成所有"重计算"：音频的语音转写（STT）、图像的 OCR 文字提取、以及对远程搜索请求的响应。当前桌面端的 ingestion dispatcher（US-053）已将 mobile capture 按 kind 分流，但 audio 和 image 的占位 markdown 仍停留在 `"transcription pending"` / `"OCR pending"` 状态，search-request handler 缺乏限速和错误 payload 响应。这是 Phase 4 MVP 能在桌面端完整跑通传输→处理→响应闭环的最后一个前置条件。

## What Changes

- **STT 模块**：新增 `apps/desktop/src-tauri/src/spine/stt.rs`，基于 `whisper-rs`（whisper.cpp Rust 绑定）。默认模型 `ggml-base.en`（~140MB），首次使用时按需下载到 `<data-dir>/models/whisper/`；下载失败时 STT 静默禁用，capture-audio 保留原文件 + 占位 markdown 不变。
- **OCR 引擎重写（core/rag-engine）**：在 `core/rag-engine` 中引入 `ocrs` crate，重写现有基于 `tesseract` 系统命令的 ImageOcrExtractor。新实现不依赖系统 OCR 二进制，但需要本地 RTen OCR 模型文件（通过 `SYNCMIND_OCR_DETECTION_MODEL` / `SYNCMIND_OCR_RECOGNITION_MODEL` 指定）。同时重构 OcrConfig 移除 `ocr_binary_path` 等已不再需要的字段。
- **引入异步后处理管道**：在 `dispatch.rs` 的 `capture-audio` 和 `capture-image` 路由路径中，二进制文件写入 + 占位 markdown 落地后，异步触发后处理：
  - audio → 调用 `stt::transcribe_audio()`
  - image → 调用 `rag_engine::extractor::ImageOcrExtractor`（或新 `rag_engine::ocr` 模块）
  重计算完成后原子更新对应的 `.md` 文件（替换 frontmatter 中的 `source` 和 body）。所有重计算通过 `tokio::spawn` 异步执行，不阻塞主索引流水线。
- **搜索 RPC 限速**：在 `dispatch.rs` 的 `search-request` 路由中添加基于 `peer_fingerprint` 的滑动窗口限速器（单设备 30 req/min）。超限时返回 `kind: "error"` payload（加密信封，保留 `request_id`），而非静默丢弃。
- **Cargo.toml 依赖更新**：
  - `apps/desktop/src-tauri/Cargo.toml`：新增 `whisper-rs`
  - `core/rag-engine/Cargo.toml`：新增 `ocrs`、启用在 `image` 中已依赖的相应 feature

**Not changing:**
- Spine 服务端（`services/sync-gateway/`）不受影响。
- 前端 Devices Tab UI 不受影响（STT/OCR 是全后台处理）。
- search-request 的 routing 本身已在 dispatch.rs 中实现，本次只增强限速和错误响应。
- `core/rag-engine` 的 `Extractor` trait 签名保持兼容（`ImageOcrExtractor::extract` 接口不变）。

## Capabilities

### New Capabilities

- `stt-transcription`: 定义桌面端对移动端 `capture-audio` 执行 Whisper 转写的完整流程：模型按需下载、转写调用、SRT 式分段输出、markdown 更新、失败降级。
- `ocr-text-extraction`: 定义桌面端对移动端 `capture-image` 执行 `ocrs` 文字提取的完整流程：图像加载、多语言 OCR、结果校验（<10 字符回退）、markdown 更新、失败降级。
- `search-rpc-rate-limiting`: 定义来自移动端的 `search-request` 按 peer fingerprint 的滑动窗口限速策略（30 req/min）以及超限错误 payload 格式。

### Modified Capabilities

- `document-extraction-quality`: OCR 后端从 `tesseract` 系统命令替换为 `ocrs` Rust 原生 crate，相关 requirements 需要更新以反映零系统依赖的新约束。

## Impact

- **Code - new files:**
  - `apps/desktop/src-tauri/src/spine/stt.rs` — Whisper STT 模块
  - `apps/desktop/src-tauri/src/spine/ratelimit.rs` — 滑动窗口限速器
  - `core/rag-engine/src/ocr.rs` — ocrs Rust 原生 OCR 包装模块（新）
- **Code - modified files:**
  - `apps/desktop/src-tauri/src/spine/dispatch.rs` — audio/image 后处理 + search 限速
  - `core/rag-engine/src/extractor.rs` — `run_image_ocr`/`run_pdf_ocr` 从 tesseract 迁移到 ocrs；`OcrConfig` 字段精简
  - `core/rag-engine/src/lib.rs` — 若新增 ocr 模块需 pub mod ocr
  - `apps/desktop/src-tauri/Cargo.toml` — 新增 `whisper-rs`
  - `core/rag-engine/Cargo.toml` — 新增 `ocrs`
- **Dependencies:**
  - `whisper-rs`（~20MB 编译产物增量）仅限 desktop crate
  - `ocrs`（~5MB）仅限 rag-engine crate
  - 移除对 `tesseract` 系统命令的运行时依赖
- **Model storage:** `<data-dir>/models/whisper/` + `ggml-base.en`（~140MB 磁盘）、按需下载、失败不崩溃。
- **Performance:** STT/OCR 走 `tokio::spawn` + `tokio::task::spawn_blocking`，单次 OCR < 1s，单次 STT 1-5x 音频时长。不会阻塞 dispatcher 主 async 上下文。
- **Risk:** 中。`whisper-rs` 的 Cargo build 需要系统有 cmake（macOS Xcode CLT 自带）。`ocrs` 在 Apple Silicon 上无特殊系统依赖，但需要本地模型文件；缺失时会降级为 OCR unavailable。`ocrs` 替换 tesseract 的 OCR 质量差异需关注，但 <10 字符回退兜底。
