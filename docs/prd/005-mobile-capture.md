# PRD: Mobile Capture — 移动端轻量捕获客户端 (Phase 4)

## Introduction

PRD 002《The Spine》交付了零知识盲中继同步网关，PRD 004《Desktop Spine Client》让桌面端成为 Spine 协议的第一个生产消费者。Phase 4 引入第二个客户端——**移动捕获端**（`apps/mobile/`），定位为"灵感速记入口"：用户在地铁、会议、走路时随手把一段文字、一张照片、一段语音、或者从其他 App 分享过来的链接灌进 SyncMind 知识库，等回到桌面时这些内容已经被索引完毕，可以通过命令面板召回。

移动端**不是桌面端的小屏复刻**。两端的角色是不对称的：

| 维度 | 桌面端 (`apps/desktop/`) | 移动端 (`apps/mobile/`) |
|---|---|---|
| 算力定位 | Brain（embedding / 索引 / 搜索） | Sensor（采集 / 编码 / 上传） |
| 持久存储 | SQLite + sqlite-vec 全量索引 | 仅会话级缓存与离线发送队列 |
| 网络模式 | 长连接 WS + HTTP | 主要 HTTP + 偶尔 WS（节省电量） |
| 计算密集任务 | STT / OCR / Embedding / Chunking | **无**——原始音频/图像直接加密上传 |

这种切分严格服从 Engineering Directive #1（Privacy is Absolute）与 #3（Frugal Resource Usage）：手机不下载 Whisper 模型，不跑 OCR，不持有用户的全部知识库。它只做"采集 + 加密 + 上传 + 偶尔查询"。所有重计算交还桌面端 Brain。

Phase 4 同时引入一个**协议层增量**：把现有 Spine envelope 从单向 inbox 提升为双向 RPC，让移动端能以 `search-request` / `search-response` 的方式远程查询桌面端 Brain。这是 PRD 005 比 PRD 004 多出来的最重要架构动作。

## Goals

- 在 `apps/mobile/` 内交付一个 Expo (SDK 53+) 双平台（iOS + Android）原生应用，5 分钟内完成与桌面端的扫码配对并发出第一条 capture。
- 严格遵守 Privacy-is-Absolute：
  - 私钥仅存 iOS Keychain / Android Keystore，不进 AsyncStorage / SQLite 明文文件。
  - 原始音频、图像、文本在离开 App 前 100% 完成 AES-256-GCM 加密。
  - 移动端 **不内置** STT/OCR 模型；STT/OCR 全部在桌面端 Brain 执行。
- 协议向后兼容：Spine envelope `v1` 不破坏，新增 6 个 payload type 通过 `payload.kind` 字段区分。
- 离线优先：网络中断时 capture 落入本地加密队列，恢复后自动续传；杀进程 / 重启后队列状态保持。
- 双向能力：mobile 既能 push capture，也能远程 query desktop Brain 并在 10 秒内拿到搜索结果（典型 LAN 场景）。
- 资源占用：冷启动 < 2s，idle 内存 < 80MB，60 秒语音 capture 端到端（录音→加密→上传完成）≤ 5 秒（在 50Mbps 上行 + 自托管 Spine on LAN 场景下）。

## Scope Anchor — 这份 PRD 的范围答疑

| 用户决策 | 选项 | 含义 |
|---|---|---|
| MVP 捕获类型 | **1D** | 文本 + 语音 + 照片 + share-sheet（外部 URL/文本） |
| STT/OCR 处理位置 | **2B** | 委托桌面端 Brain，手机仅上传原始字节 |
| 平台目标 | **3C** | iOS + Android via Expo |
| 配对入口 | **4A** | 复用 Phase 3 Spine pairing API，扫 QR |
| 读侧能力 | **5D** | Write + 本地最近列表 + Spine 远程搜索 |

## User Stories

PRD 004 终止于 US-038，本 PRD 从 **US-039** 开始连号。

### US-039: Expo 工程脚手架与 monorepo 接入

> **Status:** ✅ Implemented via OpenSpec change [`mobile-app-scaffold`](../../openspec/changes/archive/2026-05-27-mobile-app-scaffold/). Merged as commit `cb46d0e`, archived in `cd18ace`.

**Description:** 作为开发者，我需要一个干净的 Expo 工程，能复用 monorepo 已有的 TypeScript / ESLint 配置和 `@syncmind/types`。

**Acceptance Criteria:**
- [x] 在 `apps/mobile/` 初始化 Expo SDK 56（默认 Tabs 模板的 typescript 变体），目标平台 iOS + Android。
- [x] `package.json` 加入根 pnpm workspace，依赖 `@syncmind/types`、`@syncmind/ui-kit`（按需）。
- [x] 复用根 `packages/eslint-config/` 与 `packages/ts-config/`；新增 `apps/mobile/tsconfig.json` extends `@syncmind/ts-config/base.json`。
- [x] `pnpm --filter mobile lint` / `pnpm --filter mobile typecheck` 通过。
- [x] 配置 EAS Build（`eas.json`）的 `development` / `preview` / `production` profile；不强制 native build 在 CI 跑，但配置必须存在并经过 `eas build:configure` 校验。
- [x] README 写明本地启动命令：`pnpm --filter mobile start`（Expo dev server）+ Expo Go 扫码或 dev client。
- [x] **不引入** Redux / MobX / Zustand 以外的全局 state 库；MVP 使用 Zustand（与桌面端 `apps/desktop/src/store.ts` 风格一致）。

### US-040: 移动端设备身份（iOS Keychain / Android Keystore）

> **Status:** ✅ Implemented and accepted on 2026-05-31 via OpenSpec change [`mobile-device-identity-native-completion`](../../openspec/changes/archive/2026-05-31-mobile-device-identity-native-completion/). Feature commit `63244d8`, archived in `7dca570`. The original JS + `expo-secure-store` design remains archived in [`2026-05-27-mobile-device-identity`](../../openspec/changes/archive/2026-05-27-mobile-device-identity/) as historical context only.

**Description:** 作为系统，我需要在移动端本地生成持久 Ed25519 身份密钥对，存储在操作系统的安全密钥库中，跨 App 重启可读，但**绝不**通过 JS 桥暴露原始私钥字节。

**Acceptance Criteria:**
- [x] 通过本地 Expo native module `SyncMindDeviceIdentity` 管理身份密钥；iOS 使用 Keychain，Android 使用 Keystore-backed native storage，JS 不生成、不持有、不序列化原始私钥。
- [x] 密钥首次启动时由 native module 生成；JS 侧最多持久化非敏感 `device_identity_meta`（`fingerprint` / `publicKeyHex` / `biometricEnabled`），不得再写入包含私钥的 `device_identity`。
- [x] `apps/mobile/src/crypto/identity.ts` 保持公开 facade；`sign(message)` / `derive_x25519(peer_pub)` 只调用 native module，调用方拿不到 raw private key bytes。
- [x] 生物识别保护默认关闭；设置面板的"启用生物识别保护"开关必须更新 native secure-store 配置，且 `isAuthenticationRequired()` 在 App 重启后仍反映 native 状态。
- [x] 提供 `device_reset()` 操作：清除身份 + 解配对 + 清队列；UI 入口在设置最深处，需二次确认。
- [x] 支持从 legacy `device_identity` blob 一次性迁移到 native identity store；迁移成功后删除 legacy blob，迁移失败时不得继续使用 JS-stored 私钥。
- [x] **Privacy check 单元测试**：在 `apps/mobile/__tests__/crypto.test.ts` 用 jest 验证 private key material 不会泄露到 SecureStore 写入、日志、Error.message 或 JSON.stringify 输出。
- [x] `pnpm --filter mobile typecheck`、`pnpm --filter mobile lint`、`pnpm --filter mobile test --runInBand` 通过，并记录 iOS / Android create、restart persistence、biometric toggle、reset 的手工验证证据后，US-040 才能验收。

### US-041: QR 码配对（扫码端）
**Description:** 作为移动端用户，我希望扫描桌面端 Devices 面板上的二维码就能完成配对，10 秒内进入"已配对"状态。

**Acceptance Criteria:**
- [x] 使用 `expo-camera` 实现扫码界面，相机权限通过 `Camera.requestCameraPermissionsAsync()` 申请；拒绝权限时显示降级 UI：手输配对载荷 base64。
- [x] QR payload schema（与桌面端约定，需要 Phase 3 桌面端配合扩展，见 §US-052）：
  ```json
  {
    "v": 1,
    "kind": "syncmind-pairing",
    "spine_url": "https://spine.example.com:8443",
    "ca_fingerprint": "sha256:HEX...",
    "pairing_token": "<JWT or 64-char hex>",
    "expires_at": "2026-05-26T11:30:00Z",
    "device_a_pubkey": "<base64 ed25519 pubkey>",
    "device_a_fingerprint": "sha256:HEX..."
  }
  ```
- [x] 客户端在解码后必须校验：
  - `v == 1`，否则提示"App 版本过低"。
  - `expires_at` > now（容忍 ±60s 时钟漂移）；过期时提示"二维码已过期，请桌面端重新生成"。
  - `spine_url` 必须 `https://`；dev mode（`__DEV__ === true`）放行 `http://`。
- [x] 调用 Spine `POST /pairing/complete`（PRD 002 §US-012）完成握手：上传 mobile 的 Ed25519 pubkey + X25519 ephemeral pubkey；收到 device_b 的 session_id。
- [x] 派生 `sync_key = HKDF-SHA256(x25519_shared, salt=session_id, info="syncmind-v1")`，与桌面端 §PRD 004 完全一致。
- [x] 持久化到 `expo-secure-store`：`sync_key`、`paired_peer_fingerprint`、`paired_peer_device_type="desktop"`、`paired_at`、`peer_device_id_uuid`、`spine_url`、`ca_fingerprint`。
- [x] 自定义 CA 场景：从 `ca_fingerprint` 校验 TLS 证书指纹（使用 `expo-network` + 自定义 fetch wrapper，或 `react-native-ssl-pinning`）；MVP 接受 system trust + 可选 fingerprint pin。
- [x] 配对成功后跳转到"首条 capture"引导页。

### US-042: 配对状态管理与解配对
**Description:** 作为用户，我希望在设置中清楚看到当前配对的对端设备信息，并能一键解除配对。

**Acceptance Criteria:**
- [x] 设置页 / Devices section 显示：对端 fingerprint（前 8 位 + 后 4 位缩略）、对端 device_type、`paired_at` 相对时间、Spine URL、最后一次成功 Spine 联系时间（`last_seen_at`）。
- [x] "Unpair" 按钮触发：
  1. 调用 Spine `POST /v1/devices/{self}/revoke` 撤销自身设备并解除 paired 关系。
  2. 清除所有 `expo-secure-store` 同步键。
  3. **保留** Ed25519 身份密钥（不重置身份），允许后续重新配对。
  4. 清空发送队列；正在发送的 in-flight 请求 abort。
- [x] 解除配对后 UI 回到"未配对"空状态；底部 tab 中的搜索 / 列表入口变灰并提示需要配对。
- [x] 若 Spine 返回 401（token 失效）/ 404（device 已被对端 revoke），客户端进入 `Unpaired` 状态而不是无限重试。

### US-043: 文本捕获主屏
**Description:** 作为用户，我希望打开 App 就是一个空白文本框，键盘自动弹出，写完点发送即可。

> **Status:** ✅ Implemented via OpenSpec change [`mobile-text-capture-home`](../../openspec/changes/archive/2026-06-04-mobile-text-capture-home/). Text capture now uses the paired default Capture tab, auto-focused multiline input, local status row, 50,000-character limit, optimistic encrypted outbox enqueue, and the US-043 `capture-text` plaintext schema before encryption. Voice-mode capture is implemented by US-044.

**Acceptance Criteria:**
- [x] App 启动后默认进入 `CaptureScreen`（已配对状态下）；`autoFocus={true}` 的多行 `TextInput`。
- [x] 顶部一行薄状态栏显示对端连接状态（绿色圆点 = 已连接 / 灰色 = 队列中 / 红色 = 配对失效）。
- [x] 底部一个明显的 "Send" 按钮 + 一行最近 3 条 capture 的迷你预览（完整列表仍归 §US-049）。
- [x] 输入字数 ≤ 50,000 字符（envelope 限制）；超过时按钮变灰并显示 "Too long — try splitting"。
- [x] Send 触发后 `TextInput` 立即清空（乐观更新）；后台进入 §US-047 的加密队列。
- [x] 支持下拉关闭键盘 / 上滑切到语音模式（§US-044）。下拉/拖动/点击关闭键盘由 US-043 实现；上滑语音模式由 US-044 实现。
- [x] Capture payload schema（明文，加密前）：
  ```json
  {
    "v": 1,
    "kind": "capture-text",
    "id": "<uuid v4>",
    "text": "<user input>",
    "source": "typed",
    "client_ts": "2026-05-26T10:20:00Z",
    "client_device_fingerprint": "sha256:..."
  }
  ```

### US-044: 语音捕获（原始音频上传，不本地转写）

> **Status:** ✅ Implemented via OpenSpec change [`mobile-audio-capture`](../../openspec/changes/archive/2026-06-06-mobile-audio-capture/). Voice capture uses Expo SDK 56 `expo-audio`, encrypts `capture-audio` bundles into the existing outbox, uploads through `/v1/sync/bundle`, and was verified against Spine relay plus desktop `capture-audio` dispatch. iOS microphone permission requires a rebuilt native app because `NSMicrophoneUsageDescription` is bundle metadata, not a Metro-reloadable JS change.

**Description:** 作为用户，我希望长按一个按钮就开始录音，松开自动发送，桌面端会替我转写成文字并索引。

**Acceptance Criteria:**
- [x] 使用 Expo SDK 56 `expo-audio` 录音 API，录音参数：
  - 编码：AAC LC，sample rate 16000Hz，单声道，bit rate 32 kbps（Whisper 的最优输入参数）。
  - 容器格式：`.m4a`。
- [x] 麦克风权限通过 `expo-audio` 权限 API 申请，拒绝时引导到系统设置。
- [x] UI：CaptureScreen 上向上滑切换到语音模式，出现一个圆形按钮，**按住录音 / 松开发送**，录音时显示实时音量波形。
- [x] 最长单次录音 **60 秒**，到点强制停止并提示用户。
- [x] 录音结束后立即读取 m4a 字节，base64 编码塞进 payload；录音器临时文件在加密入队、校验拒绝、丢弃或取消路径 best-effort 删除。
- [x] Capture payload schema：
  ```json
  {
    "v": 1,
    "kind": "capture-audio",
    "id": "<uuid v4>",
    "audio_base64": "<base64 m4a bytes>",
    "audio_mime": "audio/mp4",
    "duration_ms": 23400,
    "client_ts": "...",
    "client_device_fingerprint": "..."
  }
  ```
- [x] 单 bundle 大小 hard cap：**8 MB 原始字节 / 11 MB base64**；超过时拒绝发送并提示"片段过长"。
- [x] 录音中断（来电、App 切后台超过 30s）自动停止并保留已录制片段，弹"保留 / 丢弃"两选。

### US-045: 照片捕获（相机 + 相册）

> **Status:** ✅ Implemented and accepted on 2026-06-07 via OpenSpec change [`mobile-photo-capture`](../../openspec/changes/archive/2026-06-07-mobile-photo-capture/). Verification passed: OpenSpec strict validation, focused mobile photo/bundle/outbox/capture tests, mobile typecheck, mobile lint, and desktop spine dispatch tests.

**Description:** 作为用户，我希望从相机或相册选一张图片直接 capture，桌面端会做 OCR 把文字部分加入索引。

**Acceptance Criteria:**
- [x] 入口：CaptureScreen 工具栏的相机图标；点击弹 ActionSheet 选 "Take Photo" / "Pick from Library"。
- [x] 使用 `expo-image-picker`；相机权限 + 相册权限按需申请。
- [x] 图像预处理：
  - 长边超过 2048px 时按比例缩到 2048px（节省带宽 & OCR 准确率边际收益已饱和）。
  - 重新编码为 JPEG quality 85，与原始格式无关（消除 HEIC / RAW 在桌面端的解码依赖）。
  - **不去除 EXIF**——这是 capture 用户的素材，方位/时间 metadata 可能有语义价值；如果未来 Privacy review 否决再调整。
- [x] 单图 hard cap：5 MB 编码后；超过时压缩 quality 到 70 重试，仍超过则拒绝。
- [x] 可选附加文字 caption（与图片同一 bundle，桌面端会在索引时拼接）。
- [x] Capture payload schema：
  ```json
  {
    "v": 1,
    "kind": "capture-image",
    "id": "<uuid v4>",
    "image_base64": "<base64 jpeg>",
    "image_mime": "image/jpeg",
    "width": 2048,
    "height": 1536,
    "caption": null,
    "client_ts": "...",
    "client_device_fingerprint": "..."
  }
  ```

### US-046: Share Extension / Android Share Target
**Description:** 作为用户，我希望在浏览器、Twitter、微信里读到好文章，点系统分享菜单就能把它推进 SyncMind。

**Acceptance Criteria:**
- [ ] iOS：使用 `expo-share-intent`（或必要时 prebuild 切到 bare workflow + 原生 Share Extension target）。
- [ ] Android：在 `app.json` 的 `android.intentFilters` 注册 `android.intent.action.SEND` 的 `text/plain`，App 启动时通过 `Linking.getInitialURL` + `Linking` 监听消费。
- [ ] 支持接收：
  - 纯文本（`text/plain`）→ `kind: "capture-text"`，`source: "shared"`。
  - URL（带或不带文字）→ `kind: "capture-link"`，schema 见下。
  - 图片（`image/*`）→ `kind: "capture-image"`，复用 §US-045 的预处理流水线。
- [ ] Share 触发后**不必打开主 App**：能在 share extension 内直接调起加密 + 入队（最起码做到入队，上传可以委托主 App 后台）。MVP 可以接受"share 后自动打开 App 完成上传"，但用户决策完成（点确认）这一步必须在 share 表单内完成。
- [ ] Capture payload schema（link）：
  ```json
  {
    "v": 1,
    "kind": "capture-link",
    "id": "<uuid>",
    "url": "https://...",
    "shared_text": "<可选的同时分享的文本>",
    "client_ts": "...",
    "client_device_fingerprint": "..."
  }
  ```
- [ ] **不在移动端预抓取 URL 内容**——这是桌面端 RAG engine 的工作（PRD 001 已有 fetcher）。

### US-047: Bundle 加密、离线队列与上传

> **Status:** ✅ Implemented via OpenSpec change [`mobile-capture-outbox-upload`](../../openspec/changes/archive/2026-06-05-mobile-capture-outbox-upload/). Smoke tests 7.4-7.6 passed manually on 2026-06-05.

**Description:** 作为系统，我需要把所有 capture 编码成 Spine envelope，加密、本地排队、按序上传，离线时缓存、上线后续传。

**Acceptance Criteria:**
- [x] 加密层完全复用 PRD 004 的协议：
  - AES-256-GCM with 96-bit random nonce。
  - Envelope plaintext = `JSON.stringify(payload)` 的 UTF-8 字节。
  - Envelope `sha256 = SHA-256(content_utf8 bytes)`，与桌面端 `BundleEnvelope::validate()` 一致；Spine relay 层的 `payload_hash` 仍为 SHA-256(encrypted blob)。
  - Envelope 外层结构与桌面端 `core/storage/src/spine/envelope.rs` **完全一致**（确保桌面端 ingestion 不需要改 envelope 解析代码）。
- [x] 离线队列：使用 `expo-sqlite` 持久化（一张 `outbox` 表），字段 `id`, `created_at`, `state: pending|sending|failed|done`, `attempts`, `last_error`, `encrypted_blob`。
- [x] 加密后立即删除明文：payload object 被 GC 前不进入任何 console.log / sentry breadcrumb；包一层 `secureSerialize()` 在 dev 模式下 panic 阻止 stringify。
- [x] 上传：HTTP POST `/v1/sync/bundle` 到 Spine，带 `Idempotency-Key: <bundle.id>`，遵循 PRD 002 §US-014 的协议（最大 3 次重试，指数退避 1s/4s/16s）。
- [x] 杀进程 / 切后台时正在 `sending` 的 bundle 自动回退到 `pending`，下次启动从队列头部继续。
- [x] iOS 后台任务：使用 `expo-task-manager` 的 background fetch（间隔由 OS 决定，约 15min~hours）尝试 flush 队列；Android 使用 `expo-background-fetch`。**不承诺** 后台秒级上传——这是 OS 的限制。
- [x] 队列长度上限 1000 条，仅统计 `pending|sending|failed`；`done` 不计入容量。超过时拒绝新 capture 并提示"Capture queue is full - connect to upload or retry failed captures"。

### US-048: Capture 发送状态 UI 与重试
**Description:** 作为用户，我希望每条 capture 旁边能看到它的状态（已发送 / 队列中 / 失败），失败的可以一键重试。

**Acceptance Criteria:**
- [ ] CaptureScreen 底部的"最近 3 条预览"显示状态图标：✅ done / 🔄 sending / ⏸️ pending / ❌ failed。
- [ ] 进入完整列表（§US-049）可以看到每条的尝试次数与 last_error（仅本地显示，不发任何遥测）。
- [ ] 失败的 capture 长按出菜单：Retry / Delete / Copy as text。
- [ ] 状态来源：从 `outbox` 表查询，10s 轮询 + bundle 状态变化时的 event emitter 推送（不依赖 WS，移动端 MVP 不开 WS，见 §Technical Considerations）。
- [ ] 配对失效时所有 `pending` bundle 暂停（state 不变），UI 顶部条带显示 "Pairing lost — re-pair to resume"。

### US-049: 最近捕获列表（本地缓存）
**Description:** 作为用户，我希望看到自己最近发过的 capture 列表，确认确实发出去了。

**Acceptance Criteria:**
- [ ] 独立 Tab "Recent"；显示 `outbox` 表的 done + 最近 3 天的所有记录（pending / failed 也显示）。
- [ ] 列表项：
  - 第一行：kind icon + 内容预览（text 显示前 60 字符 / audio 显示时长 / image 显示缩略图 / link 显示 hostname）。
  - 第二行：相对时间 + 状态 icon。
- [ ] 仅本地数据，**不**反查桌面端确认对方是否真正索引完毕——这是 MVP 范围之外的双向 ACK 工作。
- [ ] 列表支持下拉刷新（触发 §US-047 的 flush）。
- [ ] 7 天后的 done 记录自动清理；用户可以在设置里改保留期（7 / 30 / 90 天 / 永久）。
- [ ] 缩略图：image kind 的 capture 完成上传后，本地保留一份压缩到 256x256 的缩略图（jpeg quality 60，~10 KB）；不再保留原图。

### US-050: 远程搜索 RPC（mobile → desktop via Spine）
**Description:** 作为用户，我希望在手机上搜"上周写的 Go 错误处理笔记"，10 秒内拿到桌面端 Brain 的搜索结果。

**Acceptance Criteria:**
- [ ] 协议：mobile 发送 `kind: "search-request"` bundle → Spine inbox → desktop 接收 → 不走 RAG ingestion，直接调用 `core/mcp-server` 的 `search_knowledge` handler → 把结果包成 `kind: "search-response"` bundle → 推回 mobile inbox。
- [ ] Request schema:
  ```json
  {
    "v": 1,
    "kind": "search-request",
    "request_id": "<uuid v4>",
    "query": "Go error handling",
    "top_k": 5,
    "filter_file_type": null,
    "client_ts": "..."
  }
  ```
- [ ] Response schema:
  ```json
  {
    "v": 1,
    "kind": "search-response",
    "request_id": "<对应 request 的 uuid>",
    "results": [
      {
        "chunk_id": "...",
        "file_path": "/Users/.../note.md",
        "start_line": 12,
        "end_line": 28,
        "content": "...",
        "score": 0.83
      }
    ],
    "server_ts": "..."
  }
  ```
- [ ] Mobile 侧：
  - 发起搜索时把 request 入 outbox，但优先级最高（插队到 head）。
  - 启动一个 10s 超时；同时拉取 inbox 检查匹配 `request_id` 的 response（HTTP 短轮询 1.5s 间隔，因为这是用户阻塞的交互）。
  - 超时时 UI 显示"Desktop offline — try again later"，不抛错。
- [ ] Desktop 侧：在 §US-054 中实现 handler。
- [ ] **搜索请求不进入本地 outbox 的 done 持久化**——查询是临时的，结果显示一次后丢弃。

### US-051: 搜索结果展示
**Description:** 作为用户，我希望搜索结果以可读的卡片形式展示，能复制内容、能看到来源文件。

**Acceptance Criteria:**
- [ ] 搜索 Tab：顶部搜索框 + 历史查询（最近 10 条，本地存储）。
- [ ] 结果卡片：file_path（缩短显示，文件名加粗 + 父目录灰色）、score 百分比、行号范围、content 段落（markdown 简单渲染：代码块用等宽字体，其它纯文本）。
- [ ] 点击卡片：复制 content 到剪贴板 + 显示 toast "Copied"；不打开任何"原文跳转"——移动端没有桌面端的本地文件路径访问能力。
- [ ] 空结果显示"No matches on your desktop"；超时显示"Desktop unreachable"；配对失效显示"Pair a desktop first"。

---

### 桌面端协议侧调整（与 Phase 4 同节奏交付）

US-052 至 US-054 涉及桌面端 `apps/desktop/` 与 `core/`，**不属于 mobile 工作树本身**，但是移动端 MVP 能跑起来的前置依赖。建议作为一组 PR 在 `feat/desktop-spine-mobile-support` 分支推进。

### US-052: Desktop 端 QR pairing payload 扩展

> **Status:** ✅ Implemented via OpenSpec change [`desktop-spine-pairing-payload`](../../openspec/changes/archive/2026-05-26-desktop-spine-pairing-payload/). Merged as commit `29c864d`, archived in `91b9d0b`.

**Description:** 作为桌面端，我需要在 Devices 面板生成的 QR 中包含 mobile 配对所需的全部信息（CA fingerprint、device_a pubkey、spine_url、TTL token）。

**Acceptance Criteria:**
- [x] 扩展 `apps/desktop/src-tauri/src/spine/pairing.rs` 的 `pairing_start` 命令，返回的 QR payload 改为 JSON object（与 §US-041 schema 一致），而不是当前的纯 token。
- [x] 前端 Devices Tab 渲染 QR 时直接 stringify 该 object。
- [x] payload TTL：`expires_at = now + 5 min`；超时由 Spine 侧自动清理 pairing_token。
- [x] 桌面端二维码下方仍显示 6 位短码用于手动 fallback（已有逻辑）。
- [x] 向后兼容：当 mobile 端 schema `v: 1`，桌面端可继续接受老的纯 token（用于桌面↔桌面配对场景）。

### US-053: Desktop 端识别新 `capture-*` 和 `search-*` payload kinds
**Description:** 作为桌面端 ingestion 管道，我需要识别 `payload.kind` 字段并分流到不同处理器。

**Acceptance Criteria:**
- [x] 修改 `apps/desktop/src-tauri/src/spine/dispatch.rs` 的 ingestion dispatcher（对应 PRD 原路径 `core/storage/src/spine/inbox.rs`，实际实现在 desktop Tauri crate）：
  - `kind: "note"` → 现有 RAG 管道（向后兼容 Phase 3）。[✅ desktop-spine-ingestion-dispatch]
  - `kind: "capture-text"` / `"capture-link"` → 包装成 markdown 文件落到 `<data-dir>/sync-inbox/captures/<id>.md`，复用现有 file-watcher → rag-engine 流水线。[✅ desktop-spine-ingestion-dispatch]
  - `kind: "capture-audio"` → 落到 `<data-dir>/sync-inbox/audio/<id>.m4a`；同时写占位 `.md` 供索引，STT 唤醒待 §US-054。[✅ desktop-spine-ingestion-dispatch]
  - `kind: "capture-image"` → 落到 `<data-dir>/sync-inbox/images/<id>.jpg`；同时写占位 `.md` 供索引，OCR 唤醒待 §US-054。[✅ desktop-spine-ingestion-dispatch]
  - `kind: "search-request"` → 不入索引，直接调用 RPC handler（§US-054）。[✅ desktop-spine-ingestion-dispatch]
  - `kind: "search-response"` → 不入索引（桌面端是 sender 不是 receiver）；记 warning。[✅ desktop-spine-ingestion-dispatch]
  - 未知 kind → 丢入 `<data-dir>/sync-inbox/_unknown/`，记 warning，不 crash。[✅ desktop-spine-ingestion-dispatch]
- [x] 单元测试覆盖每个 kind 的 dispatch 路径（24 tests）。[✅ desktop-spine-ingestion-dispatch]

### US-054: Desktop 端 STT / OCR / 搜索 RPC handler

> **Status:** ✅ Implemented via OpenSpec change [`desktop-stt-ocr-search-rpc`](../../openspec/changes/archive/2026-05-27-desktop-stt-ocr-search-rpc/). Merged as commit `03ca05b`.

**Description:** 作为桌面端 Brain，我需要为移动端的音频做 STT、为图像做 OCR、为搜索请求返回结果。

**Acceptance Criteria:**
- [x] STT：引入 `whisper-rs`（绑定到 whisper.cpp）；默认模型 `ggml-base.en`（~140MB），首次启动时按需下载到 `<data-dir>/models/whisper/`。下载失败时 STT 静默禁用，audio capture 仍保留原文件并加 `# Transcription unavailable` 占位 markdown。
- [x] STT 输出：转写文本 + 时间戳段落（SRT-like 结构）→ 落成 `<data-dir>/sync-inbox/captures/<id>.md`，frontmatter 标 `source: mobile-audio`，正文为转写结果，附加块标 `audio_file: ../audio/<id>.m4a`。
- [x] OCR：引入 `ocrs` crate（Rust 原生，无 Python 依赖）；初版仅做英文 + 中文。OCR 失败或文字过少（< 10 字符）时落到 `<data-dir>/sync-inbox/images/<id>.jpg` 旁边的 `.md` 加占位"[image: no text detected]"。
- [x] 搜索 RPC handler：
  - 监听 `kind: "search-request"`。
  - 调用 `core/mcp-server` 现有的 `search_knowledge(query, top_k, filter_file_type)`。
  - 包装结果为 `kind: "search-response"` envelope（保留 `request_id`），加密后推送到对端 inbox。
  - 限速：单设备 30 req/min；超过返回 `kind: "error"` payload。
- [x] 所有重计算（STT / OCR）必须**异步**进行，不阻塞主索引流水线；走现有的 `core/syncmind-indexing` 任务队列。

## Functional Requirements

- **FR-1**：移动端应用必须支持 iOS 16+ 和 Android 11+（API level 30+），通过单一 Expo 工程交付。
- **FR-2**：移动端必须在 OS 安全密钥库（iOS Keychain / Android Keystore）中持有唯一 Ed25519 身份密钥对，且永不通过任何序列化路径泄露私钥字节。
- **FR-3**：所有 capture payload 必须在离开设备前完成 AES-256-GCM 加密，envelope 格式与桌面端 `core/storage/src/spine/envelope.rs` 完全一致。
- **FR-4**：移动端必须支持 4 种 capture kind：`capture-text` / `capture-audio` / `capture-image` / `capture-link`，并且**所有 4 种**都通过同一个 outbox 队列与上传链路。
- **FR-5**：移动端**不得**包含任何 STT、OCR、Embedding 模型或推理库（无 ONNX Runtime、无 Whisper、无 Tesseract）。
- **FR-6**：移动端必须支持 Spine reverse-channel RPC（`search-request` / `search-response`），在已配对状态下查询桌面 Brain。
- **FR-7**：QR 配对必须在 5 分钟 token TTL 内完成，过期后桌面端必须能重新出码。
- **FR-8**：离线时所有 capture 必须落入持久化 outbox（`expo-sqlite` 表），杀进程后恢复。
- **FR-9**：单个 bundle 加密后大小 ≤ 12 MB；超过时上传层必须拒绝并将状态置为 `failed` with reason `oversize`。
- **FR-10**：桌面端必须扩展 ingestion dispatcher 识别 5 种新 kind（`capture-text/audio/image/link` + `search-request`），并向后兼容旧 `kind: "note"`。

## Non-Goals

- **No on-device STT/OCR/embedding**：所有重计算交还桌面端，由 §US-053/054 完成。
- **No knowledge graph UI**：3D 知识图谱属于 Phase 5（`apps/web/`），不在本期。
- **No bidirectional ACK**：移动端不需要知道桌面端是否已完成索引（只知道 Spine 已收到）；"已索引"反馈留给未来。
- **No edit / delete of past captures from mobile**：MVP 只能 push，不能修改 / 删除已上传内容；这是设计选择，不是 bug。
- **No Web PWA fallback in MVP**：用户决策为 3C，Web 推迟。
- **No iOS Lock Screen widget / Android quick tile**：好东西，但留到 Phase 4.5。
- **No multi-pair**：MVP 移动端只能配对 1 台桌面；多桌面同步留待 Phase 5。
- **No background polling for new desktop pushes**：移动端只 push capture + 主动 query；桌面端的 RAG 更新不会自动同步回移动端。这是 Privacy 与电量的双重妥协。
- **No telemetry / crash reporting**：不接 Sentry / Firebase Crashlytics（违反 Privacy directive）。本地 logfile 仅用 `expo-file-system` 写到 sandbox，用户手动导出。

## Design Considerations

- **视觉一致性**：桌面端命令面板是 Raycast 风格的纯键盘驱动；移动端则是**触屏优先**的极简 capture 表单。两者审美一致（深色主题 + 单色调强调色），但**交互模型不同**，不要强求 1:1 复刻桌面 UI。
- **空状态**：每个 Tab 都有明确空状态（未配对 / 无 capture / 无搜索结果），文案统一在 `apps/mobile/src/i18n/`。
- **键盘行为**：CaptureScreen 进入立即聚焦输入；Search Screen 进入聚焦搜索框；其它屏不强制 keyboard。
- **复用 `@syncmind/ui-kit`**：MVP 期可以 fork 桌面端的 Token（spacing / color），但**不直接 import React 组件**（RN 与 React DOM 组件不互通）；UI 层独立实现。

## Technical Considerations

- **WebSocket on mobile**：MVP **不启用** WS 长连接。原因：iOS 后台限制 + 电量成本 + 当前 use case（用户主动查询）不需要秒级 push。改为：
  - 用户在 App 前台时，10s 短轮询 `/v1/sync/bundles?limit=20`（仅用于 search response）。
  - 后台时不轮询；OS-scheduled background fetch 触发时拉一次。
- **Search request 与 outbox 的优先级**：search-request 在 outbox 中标 `priority: high`，跳过普通 capture 的 FIFO 顺序立即上传。
- **Hermes 兼容性**：所有 JS crypto 库必须经过 Hermes JS engine 验证；US-040 身份密钥不再依赖 JS crypto，而是通过 native `SyncMindDeviceIdentity` module 使用 Keychain / Keystore-backed storage。
- **二进制传输**：base64 内联是 MVP 折中。如果未来发现大图/长音频上传成本过高，再升级到 multipart blob endpoint（Spine 协议层改动）。
- **EAS 与签名**：MVP 阶段只走 EAS internal distribution；用户自托管时可以用 sideloading / TestFlight；App Store 发布留到 Phase 4.5。
- **monorepo 引用**：`apps/mobile/` 依赖 `packages/types`；不依赖 `apps/desktop/` 的源码（保持工作树清晰）。
- **测试策略**：
  - Crypto / envelope 层：jest 单元测试，与桌面端 `core/storage/src/spine/envelope.rs` 共享 fixture（同一个 plaintext + nonce 输出同一个 ciphertext）。
  - Outbox 队列：jest + 内存 sqlite mock。
  - UI：Detox E2E（可选，MVP 不强制 CI 跑）。
  - 跨端集成：手工本地测试 + 一份 `docs/manual/phase4-acceptance.md` checklist。

## Success Metrics

- 用户从打开 App → 完成首条 capture ≤ **15 秒**（已配对状态）。
- 文本 capture 端到端（按 Send → Spine ACK）≤ **2 秒**（4G + 自托管 Spine）。
- 60 秒语音 capture 端到端（含 base64 + 加密 + 上传）≤ **5 秒**（50Mbps + LAN Spine）。
- 远程搜索（mobile → desktop → mobile）p95 ≤ **8 秒**（同 WLAN）。
- App idle 内存 ≤ **80 MB**。
- 离线 24 小时内积压 100 条 capture，恢复网络后 5 分钟内全部上传完毕。
- 30 天 crash-free session ≥ 99%（本地崩溃日志统计，不上传）。

## Open Questions

1. **App Store 合规**：mobile 端连接用户自托管 HTTP 服务器（dev mode），是否需要在 App Store 描述中显式声明"connects to self-hosted endpoints only"？需要在提审前与 Apple 审核指南对照。
2. **STT 模型大小 vs 准确率**：whisper `base.en` 140MB 还是 `small.en` 466MB？这是桌面端用户体验问题，建议默认 base，提供配置项升级。
3. **iOS Share Extension 的工作流**：选 `expo-share-intent` 还是 prebuild 切到 bare workflow + 手写 SE target？前者维护简单，后者控制力强。倾向前者，除非实测不稳定。
4. **生物识别保护私钥的 UX**：每次 capture 都弹 FaceID 太烦，但完全不保护私钥又有"手机被拿走即可发 capture"的风险。MVP 决策：默认关，提供开关；正式发布前 review 一次。
5. **`capture-link` 是否预抓取**：在桌面端再抓取（PRD 001 fetcher 已实现）vs 在移动端预抓取（节省桌面端外网请求）。当前决策为桌面端抓取（移动端只传 URL），但桌面是否能访问该 URL（防火墙、登录态）是另一个问题，可能需要补一个"link unreachable"反馈机制。
6. **搜索响应能否带上 file preview**：移动端展示搜索结果时，桌面端的 file_path 在 mobile 上没意义；要不要在 response 里附加更多上下文（前后段落、文件标题）？可能需要在 §US-054 实现时再决定。
7. **OCR 多语言**：`ocrs` 当前支持英文为主；中文 OCR 是否切到其它 crate（如 `tesseract-rs`）？这是 PRD 4.5 的事，MVP 接受英文 OCR + 中文图像降级为"image stored, no text"。
