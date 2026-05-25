# PRD: Desktop Spine Client — 桌面端跨设备同步客户端 (Phase 3 续作)

## Introduction

PRD 002《The Spine》交付了一个完整的盲中继同步网关（`services/sync-gateway/`，Go + Hertz + PostgreSQL + Redis），它对密钥、明文与同步包内容**一无所知**。这种零知识架构的代价是把所有"懂业务"的责任全部留给了客户端——配对、密钥派生、加解密、传输、解包、注入索引管道，全部需要在设备本地完成。

本 PRD 定义桌面端作为 Spine 协议的**第一个生产客户端**应当提供的能力：

1. **设备身份与密钥保管：** 在 OS 钥匙串中生成并持久化 Ed25519 身份密钥对，作为 JWT 签名与配对的唯一身份凭证。
2. **配对发起方 UI：** 显示 QR 码与 6 位短码，轮询 `pairing/status` 直到对端完成；与 PRD 002 §US-012 形成完整闭环。
3. **客户端 E2EE：** 派生 `sync_key`、AES-256-GCM 加密/解密、payload hash 校验、versioned plaintext envelope。
4. **网络层：** HTTP 客户端（上传/下载/ACK/Revoke）+ WebSocket 长连接（实时通知 + 心跳 + 指数退避重连 + 轮询兜底）。
5. **本地注入：** 解密后的 Note 落到 `<data-dir>/sync-inbox/`，由现有的 `core/file-watcher` → `core/rag-engine` → `core/storage` 流水线接管；**不修改 `core/storage` 的公共 API**。

桌面端是 Phase 4（移动端）抵达之前唯一能完整跑通 Spine 协议的实体。本 PRD 同时承担"协议回环验证基础设施"的角色：两台桌面端互配，可以替代尚不存在的移动端，对 PRD 002 的端到端假设进行验证。

## Goals

- 在 Tauri 桌面端（`apps/desktop/`）内部新增一组 Rust 模块与一个 Solid 设置面板，让用户能在 5 分钟内完成自托管 Spine 的配对并发出第一条同步 Note。
- 严格遵守 Privacy-is-Absolute：私钥仅存 OS 钥匙串（macOS Keychain / Windows Credential Manager / libsecret），不通过 IPC 暴露给前端，不写入磁盘明文文件。
- 实现 `sync_key = HKDF-SHA256(X25519_shared_secret, salt=session_id, info="syncmind-v1")` 派生 + AES-256-GCM 加解密 + Versioned JSON envelope。
- WebSocket 实时通知 + 30 秒轮询兜底，"重连不丢消息"。
- 解密后的 Note 100% 走文件系统注入，不调用 `core/storage` 的私有 API，复用既有 RAG 管道。
- 不增加新的常驻进程；相对未启用 Sync 的 baseline，进程内存增量 < 20MB。

## User Stories

### US-028: 同步配置与 Spine URL 管理
**Description:** 作为自托管用户，我希望在桌面端设置面板中配置我自己的 Spine 服务地址，并能查看当前同步状态。

**Acceptance Criteria:**
- [ ] 扩展 `core/syncmind-core/src/config.rs` 的 `Config` 结构，新增 `spine: SpineConfig` 字段：
  - `url: Option<String>` — Spine 实例 HTTPS URL（默认 `None`）。
  - `paired_peer_fingerprint: Option<String>` — 对端 Ed25519 公钥 SHA-256 指纹（hex）。
  - `paired_peer_device_type: Option<String>` — `"desktop"` / `"mobile"`。
  - `paired_at: Option<DateTime<Utc>>`。
  - `peer_device_id_uuid: Option<Uuid>` — 服务端分配的 UUID，用于显示与日志。
- [ ] 通过现有 `Config::load`/`Config::save` 机制持久化；旧版本配置文件（无 `[spine]` 段）反序列化时回退到 default 并 emit warning 到 stderr。
- [ ] URL 校验规则：
  - 生产模式：必须为 `https://`。
  - dev 模式（`debug_assertions` 编译开关 或 `SYNCMIND_DEV=1` 环境变量）：允许 `http://localhost` / `http://127.0.0.1`。
  - 否则拒绝写入并向前端 surface error。
- [ ] 新增 Tauri 命令 `spine_get_config()` / `spine_set_url(url: String)`；命令注册位置：`apps/desktop/src-tauri/src/lib.rs:209-225`。
- [ ] 未配置 URL 时，所有其他 Spine 命令应返回 `SPINE_NOT_CONFIGURED` 错误码，前端据此显示引导文案。

### US-029: 设备身份（Ed25519 密钥对）与 OS 钥匙串
**Description:** 作为系统，我需要在设备本地生成一对持久的 Ed25519 身份密钥对，存储于操作系统的安全密钥库中，跨进程重启可读但永远不暴露给前端。

**Acceptance Criteria:**
- [ ] 引入 `keyring = "3"` crate（`apps/desktop/src-tauri/Cargo.toml`）。
- [ ] 模块 `apps/desktop/src-tauri/src/spine/identity.rs` 提供：
  - `ensure_identity() -> Result<Ed25519Identity>` — 首次启动生成，后续读取。
  - `Ed25519Identity { fingerprint: String, public_key: [u8; 32], device_uuid: Uuid }`，**不暴露 `SigningKey`**；签名通过 `identity::sign(data: &[u8])` 间接调用。
  - 私钥在 keychain 中的位置：`service = "syncmind"`, `account = "device-identity"`，value 为 PKCS#8 v2 序列化的 Ed25519 私钥（base64）。
- [ ] `fingerprint = lower_hex(SHA-256(public_key))`（64 字符）。
- [ ] 缓存可公开元数据到 `<data-dir>/device.json`：
  - `{ fingerprint, device_type: "desktop", created_at, last_known_device_uuid? }`
  - **不含私钥、不含 sync_key**。
- [ ] Linux 缺少 libsecret 时回退到文件存储：
  - 路径 `<data-dir>/keys/device.ed25519`，权限 `0600`，目录权限 `0700`。
  - 启动时 emit warning 到 stderr：`"keyring unavailable, falling back to filesystem; ensure data dir is on encrypted storage"`。
- [ ] 命令 `spine_get_identity()` 仅返回 `{ fingerprint, device_uuid?, created_at }`；私钥永远不通过 IPC 跨进程边界。
- [ ] 单元测试：keyring `mock` provider 下的 generate-then-read 闭环；签名/验证黄金向量。

### US-030: 配对发起（QR / 短码显示 + 状态轮询）
**Description:** 作为用户，我希望在桌面端点击"开始配对"，看到一个 QR 码与 6 位短码，让我的另一台设备扫描或手输完成配对，无需任何账号。

**Acceptance Criteria:**
- [ ] 模块 `apps/desktop/src-tauri/src/spine/pairing.rs` 提供：
  - `initiate(spine_url: &Url) -> Result<PairingHandle>` — 调用 `POST /v1/pairing/initiate`：
    - body：`{ "initiator_pubkey": "<base64url(ed25519_pubkey)>", "device_type": "desktop" }`
    - 返回：`PairingHandle { session_id, qr_payload, short_code, expires_at }`。
  - `cancel(handle: &PairingHandle)` — 本地停止轮询；服务端无显式 cancel 端点，由 `expires_at` 自然回收。
- [ ] QR 图像渲染：`qrcode = "0.14"` + `image = "0.25"` 生成 PNG bytes（边长 ≥ 256px，纠错级别 `M`），通过 Tauri 命令返回 base64 data URL 给前端。
- [ ] 轮询任务 `poll_status(session_id)`：
  - 间隔 1 秒，调用 `GET /v1/pairing/:session_id/status`。
  - 状态机：`pending` → 继续；`completed` → 触发 US-031；`expired` / `cancelled` → 向前端 emit `spine://pairing/expired`。
  - 上限：TTL 内（5 分钟）持续轮询；TTL 到期自动停止。
- [ ] 命令：
  - `spine_start_pairing() -> { session_id, short_code, qr_png_base64, expires_at }`。
  - `spine_pair_status() -> { state: "idle"|"pending"|"completed"|"expired", session_id?, peer_fingerprint? }`。
  - `spine_cancel_pairing()`。
- [ ] 错误处理：
  - URL 未配置 → `SPINE_NOT_CONFIGURED`。
  - 已配对状态下再次发起 → `ALREADY_PAIRED`（前端需先调用 unpair）。
  - 网络错误 → `SPINE_UNREACHABLE`，UI 显示重试按钮。
- [ ] 单元测试：QR payload 解析与生成的对称性；轮询任务对 `expired` 状态的优雅停止。

### US-031: 共享密钥派生与缓存（`sync_key`）
**Description:** 作为系统，配对完成后我需要根据双方的 Ed25519 公钥派生出对称密钥 `sync_key`，并安全缓存以供后续 Bundle 加解密。

**Acceptance Criteria:**
- [ ] Ed25519 → X25519 转换：
  - 私钥侧：使用 `ed25519-dalek::SigningKey::to_scalar_bytes()`（v2 起内置），将持久 Ed25519 私钥导出为对应的 Curve25519 标量。
  - 公钥侧：使用 `curve25519-dalek::edwards::CompressedEdwardsY::decompress(...).to_montgomery()` 将对端 Ed25519 公钥转换为 Curve25519 公钥。
  - 上述转换需在 PR 中附带 dalek 官方文档引用，并被 code-reviewer agent 单独审计。
- [ ] `shared_secret = X25519(local_x25519_sk, peer_x25519_pk)`（32 字节）。
- [ ] `sync_key = HKDF-SHA256(ikm = shared_secret, salt = session_id.as_bytes(), info = b"syncmind-v1")`，输出 32 字节。
  - `session_id` 来自配对会话（UUID 字符串），与 PRD 002 §US-012 一致。
- [ ] 缓存：
  - 写入 keychain：`service = "syncmind"`, `account = "sync-key:<peer_fingerprint>"`，value 为 base64(`sync_key`)。
  - 选择 keychain 而非每次重新派生的理由：unpair 时可单点擦除；私钥短暂泄露时 sync_key 也能独立轮换（再次配对即可）。
- [ ] `sync_key` 永远不通过 IPC 暴露给前端，不写入磁盘其他位置，不出现在任何日志中。
- [ ] 单元测试：HKDF 黄金向量；keychain 缓存写入/读取闭环；unpair 后读取应失败。
- [ ] **本 US 与 PRD 002 §Impl Note 1 强相关**：PRD 002 实现使用 Ed25519 公钥而非 X25519 作为配对会话存储。本 US 是对应客户端转换流程的权威定义。如转换细节与 PRD 002 不一致，应向 PRD 002 提交 amendment（参见 Open Question 1）。

### US-032: JWT 签发、轮换与撤销
**Description:** 作为系统，我需要使用本地 Ed25519 私钥签发短期 JWT 作为所有 Spine 请求的认证凭证。

**Acceptance Criteria:**
- [ ] 模块 `apps/desktop/src-tauri/src/spine/crypto.rs::jwt` 提供：
  - `mint(identity: &Ed25519Identity) -> Result<MintedJwt>` — 返回 `{ token, jti, exp }`。
  - Claims：`{ sub: device_uuid, iat, exp: iat + 3600, jti: uuid_v4(), iss: "syncmind-client", aud: "syncmind-spine" }`。
  - 算法：`EdDSA`（`jsonwebtoken = "9"` 的 `Algorithm::EdDSA`）。
- [ ] JWT 在内存中持有：`tokio::sync::RwLock<Option<MintedJwt>>`；进程退出即丢失，**永不写盘**。
- [ ] 自动刷新：
  - 冷启动且 keychain 中存在已配对状态 → 立即签发首张 JWT。
  - 距离 `exp` 不足 5 分钟时，后台 task 重新签发一张并原子替换。
- [ ] 401 处理：HTTP 客户端拦截器收到 `AUTH_INVALID` 时清空当前 JWT，强制重签一次；二次仍 401 则向前端 emit `spine://auth/failed` 并停止所有 Spine 后台任务。
- [ ] 用户 unpair 时，best-effort 调用 `POST /v1/auth/revoke`（携带当前 JWT），失败不阻塞本地清理流程。
- [ ] 设备 UUID 来源：首次成功调用任意已认证端点（如 `GET /v1/sync/bundles`）后，从服务端响应中读取并缓存（服务端目前未直接返回 UUID — 见 Open Question 3，临时方案是从 `sub` claim 自验证 / 或调用专用 `/me` 端点；本 US 假设有可用来源）。
- [ ] 单元测试：claim 字段完整性；过期前 5 分钟触发刷新；EdDSA 签名可被独立 `jsonwebtoken` 验证。

### US-033: Bundle 加密与上传客户端
**Description:** 作为桌面端，我需要把一段 Note 文本封装成 versioned envelope，AES-256-GCM 加密后上传到 Spine，使其能被对端解密并注入 RAG 管道。

**Acceptance Criteria:**
- [ ] 模块 `apps/desktop/src-tauri/src/spine/bundle.rs` 定义 plaintext envelope（v1）：
  ```json
  {
    "schema_version": 1,
    "kind": "note",
    "filename": "<sanitized utf-8 filename, ≤ 255 bytes>",
    "content_utf8": "<the note body>",
    "source_path": "<original path on sender, optional>",
    "captured_at": "<RFC3339 UTC>",
    "sha256": "<lower-hex SHA-256 of content_utf8 bytes>"
  }
  ```
- [ ] envelope 序列化为 UTF-8 JSON，UTF-8 NFC 归一化 `filename` 与 `content_utf8`。
- [ ] AES-256-GCM 加密：
  - Key：`sync_key`（来自 US-031）。
  - Nonce：12 字节随机（`OsRng`）。
  - AAD：**对端 fingerprint 的 32 字节原始 SHA-256 值**（防止跨配对会话的密文误用）。
  - 输出 wire format：`bundle_blob = nonce(12) ‖ ciphertext_and_tag(N+16)`。
- [ ] 客户端预校验：
  - `bundle_blob.len() ≤ max_bundle_size_mb * 1024 * 1024`（默认 50MB，从首次成功的 server hello / 配置中获取；保守起见 hardcode 50MB 上限）。
  - `content_utf8.is_empty()` → 拒绝（`EMPTY_NOTE`）。
- [ ] 模块 `apps/desktop/src-tauri/src/spine/client.rs::upload_bundle`：
  - `POST <spine_url>/v1/sync/bundle`
  - Headers：
    - `Authorization: Bearer <jwt>`
    - `X-Syncmind-Content-Type: application/syncmind.note+json`（**服务端可见；不放任何敏感信息**）
    - `Idempotency-Key: <uuid_v4>`（每次发送独立 UUID，重试复用同一个）
    - `Content-Type: application/octet-stream`
  - Body：`bundle_blob`。
  - 期望响应：`201 Created` + `{ "bundle_id": "<uuid>" }`。
- [ ] 失败重试策略：
  - `429` / `5xx`：指数退避 1s → 2s → 4s → 8s → 16s，最多 5 次，复用同一 `Idempotency-Key`。
  - `4xx`（非 429）：不重试，将错误码 surface 到前端。
- [ ] Tauri 命令：`spine_send_note(filename: String, content_utf8: String, source_path: Option<String>) -> Result<{ bundle_id: Uuid }>`。
- [ ] 单元测试：
  - envelope 序列化 / 反序列化对称。
  - AES-GCM roundtrip：sender 加密 → receiver 解密 → 原 envelope；篡改 ciphertext 任意 1 byte 应失败。
  - AAD mismatch 解密失败。

### US-034: Bundle 列表 / 下载 / 解密 / ACK
**Description:** 作为桌面端，我需要拉取 Spine 上属于我的未读 Bundle，逐个下载、解密、校验、落地，最后向 Spine 确认。

**Acceptance Criteria:**
- [ ] 模块 `apps/desktop/src-tauri/src/spine/client.rs::list_bundles`：
  - `GET /v1/sync/bundles?limit=50`
  - 返回元数据数组（不含 ciphertext），按 `created_at` 升序处理。
- [ ] 模块 `apps/desktop/src-tauri/src/spine/client.rs::download_bundle(id)`：
  - `GET /v1/sync/bundles/:id` → 拿到 `bundle_blob` + 响应头 `X-Syncmind-Payload-Hash` + `X-Syncmind-Content-Type`。
  - 校验 1：`lower_hex(SHA-256(bundle_blob)) == header.payload_hash`，失败 → 跳过，记录到 `failed_bundles` 持久化集合，**不 ACK**（让服务端 30 天 retention 自然清理）。
  - 校验 2：`X-Syncmind-Content-Type == "application/syncmind.note+json"`（v1 仅接受此类型；其他类型记 warning + 跳过 + 不 ACK）。
- [ ] 解密：
  - 拆出 nonce / ciphertext。
  - AAD：**本设备 fingerprint 的 32 字节原始 SHA-256 值**（与 US-033 发送方使用的对端 fingerprint 对称）。
  - 解密失败（包括 tag mismatch）→ 跳过 + `failed_bundles` + 不 ACK。
- [ ] envelope 校验：
  - `schema_version == 1`，否则记 warning 并跳过（前向兼容预留）。
  - `kind == "note"`。
  - `lower_hex(SHA-256(content_utf8.as_bytes())) == envelope.sha256`，失败 → 跳过 + 不 ACK。
- [ ] 通过 US-036 落地。
- [ ] 落地成功后 `DELETE /v1/sync/bundles/:id`；DELETE 失败不阻塞，已落地内容不重复处理（通过本地 `processed_bundle_ids` 集合判重）。
- [ ] 本地集合持久化：
  - 文件 `<data-dir>/spine-state.json`（单文件，原子写）。
  - 字段：`{ processed_bundle_ids: HashSet<Uuid>, failed_bundle_ids: HashSet<Uuid>, last_pull_at: DateTime<Utc> }`。
  - 上限：每个集合最多保留最近 10,000 条；溢出按时间淘汰。
- [ ] 单元测试：
  - mock HTTP server 返回篡改的 ciphertext / hash mismatch / 解密失败 / envelope 损坏；分别验证不会 ACK 且不会落地。
  - 重复下载同一 bundle_id 时仅落地一次。

### US-035: 实时通知（WebSocket + 心跳 + 轮询兜底）
**Description:** 作为桌面端，我希望在对端上传新 Bundle 后立即收到通知；网络抖动时不丢消息。

**Acceptance Criteria:**
- [ ] 模块 `apps/desktop/src-tauri/src/spine/ws.rs` 使用 `tokio-tungstenite`：
  - URL：`<spine_url>` 的 scheme 替换为 `wss://`（或 dev 模式 `ws://`） + path `/v1/sync/live`。
  - Header `Authorization: Bearer <jwt>` 通过 `http::Request` 注入 upgrade 握手。
- [ ] 消息处理：
  - 收到 `{"type":"ping"}` → 立即回复 `{"type":"pong"}`。
  - 收到 `{"type":"new_bundle", ...}` → 触发一次 US-034 列表拉取（不直接根据消息中的 bundle_id 单点下载，避免乱序与遗漏其他离线积压）。
- [ ] 心跳超时：40 秒未收到任何消息（与 003 §Impl Note 5 对齐）→ 主动 close 连接，进入重连逻辑。
- [ ] 重连：指数退避 1s, 2s, 4s, 8s, 16s, 32s, 60s（cap），每次实际等待 = base × (0.8 + rand × 0.4)（±20% 抖动）。无限重试，直到 unpair 或用户停止。
- [ ] 轮询兜底：
  - WS 处于 `connected` 状态：暂停轮询。
  - WS 处于 `reconnecting` / `offline` 状态：每 30 秒调一次 `GET /v1/sync/bundles`。
  - WS 重连成功后，立即额外触发一次 catch-up 拉取。
- [ ] 状态机：`Connecting | Connected | Reconnecting | Offline | Disabled`。
  - `Disabled`：未配对 / Spine URL 未配置 / 用户主动暂停。
  - 状态变更通过 Tauri event `spine://status` 推送给前端。
- [ ] 单元测试：
  - mock WS server 推 `new_bundle` 触发列表拉取。
  - 抖动间隔的统计分布合理。
  - 40 秒静默后主动重连。

### US-036: 本地落地（sync-inbox + watcher 集成）
**Description:** 作为系统，我希望解密后的 Note 自动进入 SyncMind 的索引流水线，无需用户手动导入。

**Acceptance Criteria:**
- [ ] 启动时确保目录存在：`<data-dir>/sync-inbox/`（权限 `0700`）。
- [ ] 将该目录注册到 `core/file-watcher`：
  - **前置条件验证**（实现前必须确认 — 见 Open Question 2）：当前 `core/file-watcher` 是否支持目录级 watch 且能递归发现新文件。
  - 若仅支持文件列表：US-036 退化为"写入后主动调用 indexing 流水线"，需要新增 `core::syncmind_indexing::index_single_file` 之类的入口；该退化路径需要在 PRD 实施阶段补充。
- [ ] 模块 `apps/desktop/src-tauri/src/spine/inbox.rs::write_note(envelope, bundle_id) -> PathBuf`：
  - 文件名构造：`{captured_at_unix_ms}-{sanitized_filename}`，扩展名沿用 envelope 中的 filename（如 `.md`、`.txt`）；若 envelope filename 无扩展名，默认 `.md`。
  - `sanitize`：保留 ASCII 字母数字 / `-` / `_` / `.`；其他字符替换为 `_`；上限 200 字节。
  - 冲突时追加 `(2)`, `(3)`, ... 直到不冲突。
  - 写入流程：写入 `*.tmp` → `fsync` → `rename` 到最终路径（原子可见）。
- [ ] 落地成功后才向 US-034 返回 OK，触发 DELETE ACK；防止"已 ACK 但内容未持久化"的丢失。
- [ ] **不删除** sync-inbox 文件（保留作为本地审计副本）。设置 UI 提供"清理 sync-inbox"按钮（默认保留所有；清理需二次确认）。
- [ ] 元数据落盘：每个 inbox 文件旁可选 `*.meta.json`，记录 `{ bundle_id, from_device_uuid, captured_at, sha256, source_path? }`，供未来知识图谱回溯（非必需，但本 US 推荐实现）。
- [ ] 单元测试：
  - 文件名 sanitize 黄金向量（含路径穿越 `../`、CR/LF、unicode 控制字符）。
  - 冲突计数器正确。
  - 异常中断（写 tmp 后未 rename）不应被 watcher 拾起。

### US-037: Devices / Sync 设置 UI
**Description:** 作为用户，我希望在桌面端设置面板有一个独立的"Devices"标签页，能看到我的设备身份、配对状态、连接状态，并能发起配对或解除配对。

**Acceptance Criteria:**
- [ ] 新增 Solid 组件 `apps/desktop/src/components/DevicesTab.tsx`。
- [ ] 在 `apps/desktop/src/App.tsx:12-16` 的 tab 数组中追加 `{ key: 'devices', label: 'Devices' }`，作为第 5 个 tab。
- [ ] 同时在托盘菜单（`apps/desktop/src-tauri/src/lib.rs:417-431`）追加 "Sync devices…" 项，点击直接打开主窗口并切到 Devices tab。
- [ ] UI 区块：
  - **Spine URL** 卡片：输入框 + 保存按钮；空时整 tab 显示"未配置 Spine — 请先填写服务地址"。
  - **本机身份** 卡片：fingerprint（默认显示前 16 字符 + 复制按钮，hover 显示完整 64 字符）、device_type、创建时间。
  - **配对状态** 卡片：
    - 未配对：单一 "开始配对" 主按钮。
    - 已配对：peer fingerprint（同样缩略 + 复制）、device_type、配对时间、last_seen_at（来自 server / 本地推算）、连接状态徽章（绿色 connected / 黄色 reconnecting / 灰色 offline）、"重新生成 sync_key"（即重新配对）按钮、"解除配对"危险按钮。
  - **配对面板**（模态对话框）：QR PNG 居中、6 位短码加粗大字、TTL 倒计时（mm:ss）、"取消"按钮；配对完成自动关闭并切换主面板到 Paired 状态。
  - **危险区**：解除配对二次确认 dialog，列出"将擦除 sync_key、撤销当前 JWT、断开 WebSocket、保留 sync-inbox 历史文件"四条副作用，复选框"同时清空 sync-inbox（不可恢复）"默认未勾选。
- [ ] 前端不持有任何密钥材料 / JWT / sync_key；所有敏感数据通过 Tauri 命令按需查询，且响应中只含派生指纹与状态。
- [ ] 与现有 `src/store.ts:48` Solid store 集成：新增 `spineState: { url, fingerprint, deviceUuid?, paired: boolean, peer?: {...}, connectionStatus, pairing?: {...} }`。
- [ ] 错误显示：所有 Spine 错误码（`SPINE_NOT_CONFIGURED` / `ALREADY_PAIRED` / `SPINE_UNREACHABLE` / `AUTH_INVALID` / `RATE_LIMITED` / ...）映射为可读中文文案。

### US-038: 解配对、密钥轮换与设备 Reset
**Description:** 作为用户，我希望能随时切断与对端的同步关系，并在丢机或换设备时彻底清除身份。

**Acceptance Criteria:**
- [ ] `spine_unpair()` 命令：
  1. 调 `POST /v1/auth/revoke`（best-effort）。
  2. 主动 close WebSocket。
  3. 擦除 keychain 中所有 `account = "sync-key:*"` 条目。
  4. 清空 `Config.spine.paired_*` 字段并 save。
  5. 清空内存中的 `processed_bundle_ids` / `failed_bundle_ids`（保留 `<data-dir>/sync-inbox/` 本身，除非用户在 US-037 勾选清空）。
  6. emit `spine://unpaired` 给前端。
- [ ] `spine_reset_identity()` 命令（位于 US-037 危险区之下的更深层"Advanced"折叠面板）：
  1. 先执行 unpair 流程。
  2. 擦除 keychain `account = "device-identity"`。
  3. 删除 `<data-dir>/device.json`。
  4. 下次启动将生成全新身份。
  5. 服务端会留下一行孤儿 `devices`，依赖服务端的 `last_seen_at` 老化逻辑（PRD 002 §Impl Note 7）。
- [ ] Reset 期间禁止其他 Spine 命令并发执行（命令级互斥锁）。
- [ ] 单元测试：unpair 后再次调用 `spine_send_note` 应返回 `NOT_PAIRED`；keychain 中 sync-key 条目应不存在。

## Functional Requirements

- **FR-21:** 所有 Ed25519 / X25519 / AES-GCM / HKDF / JWT 操作必须在 Tauri Rust 后端完成；任何密钥材料、明文 envelope、JWT 都不得通过 IPC 边界暴露给前端 SolidJS 层（前端只能拿到指纹、状态、错误码）。
- **FR-22:** Ed25519 私钥优先存放在 OS 钥匙串；当且仅当 keychain 不可用时降级到磁盘文件（`0600`）并 emit warning。
- **FR-23:** 每次 Bundle 加密必须使用全新随机 96-bit nonce（`OsRng`），AAD 必须包含对端 fingerprint 的 32 字节 SHA-256 原始值。
- **FR-24:** JWT 仅在进程内存持有，不写入任何文件、数据库或日志；进程重启后必须重新签发。
- **FR-25:** 所有 HTTP 请求使用 rustls + TLS 1.2+；生产模式拒绝 `http://` 的 Spine URL，dev 模式仅放行 `localhost` / `127.0.0.1`。
- **FR-26:** 客户端必须在 ACK (`DELETE /v1/sync/bundles/:id`) **之前**完成 sync-inbox 文件的 `fsync + rename`；防止已 ACK 但本地未持久化导致的丢内容。
- **FR-27:** 错误码字符串必须与 PRD 002 §Impl Note 6 的服务端约定保持一致（`AUTH_INVALID` / `DEVICE_NOT_PAIRED` / `RATE_LIMITED` / `BUNDLE_TOO_LARGE` / ...）。本 PRD 在客户端侧扩展若干新码：`SPINE_NOT_CONFIGURED` / `SPINE_UNREACHABLE` / `ALREADY_PAIRED` / `NOT_PAIRED` / `EMPTY_NOTE` / `KEYCHAIN_UNAVAILABLE`。
- **FR-28:** WebSocket 与 HTTP 客户端必须共享同一 JWT 管理器，确保认证状态一致；任一通道收到 `AUTH_INVALID` 时全局强制重签 JWT。
- **FR-29:** 客户端不得修改 `core/storage` 的公共 API；所有同步内容必须通过 `<data-dir>/sync-inbox/` + 现有文件水位线进入索引流水线。
- **FR-30:** 桌面端不得直接读写 `services/sync-gateway/` 的 PostgreSQL；唯一的服务端接触点是 Spine 的 HTTPS / WSS API。

## Non-Goals (Out of Scope)

- **NG-15:** 不实现 QR 扫描 / 摄像头能力。桌面端仅作为配对发起方（initiator），显示 QR + 短码并轮询完成状态。
- **NG-16:** 不实现多对端配对（与 PRD 002 §NG-10 一致，单 `paired_peer`）。
- **NG-17:** 不实现端到端的冲突合并 (Conflict Resolution)。同名 Note 通过文件名前缀（`captured_at_unix_ms`）天然避免覆盖，由用户人工决断。
- **NG-18:** 不实现 Double Ratchet / 前向保密；本期 `sync_key` 在一次配对生命周期内固定（与 PRD 002 §Open Question 2 一致）。
- **NG-19:** 不实现密钥找回 / 托管流程。丢机即丢密钥；用户必须重新配对。
- **NG-20:** 不接收/解析任何媒体类型 Bundle（`image/*` / `audio/*`），仅接受 `application/syncmind.note+json`。媒体接收能力延后到 PRD 005（Phase 4 移动端）。
- **NG-21:** 不实现桌面之间的 P2P 直连或局域网发现；所有流量经 Spine。
- **NG-22:** 不实现多用户隔离；与 PRD 002 §NG-14 一致，一个桌面安装对应一个用户身份。
- **NG-23:** 不集成 Tauri Updater / 自动升级；密钥 schema 变更通过本 PRD 的 `schema_version` 字段而非应用层版本号管理。

## Design Considerations

- **模块划分:** `apps/desktop/src-tauri/src/spine/` 新增子模块：
  - `mod.rs` — 公共导出与 `SpineState` 单例（`OnceCell` / `tokio::sync::Mutex`）。
  - `identity.rs` — keychain 私钥 I/O、fingerprint 派生、签名接口。
  - `crypto.rs` — HKDF、AES-GCM、JWT EdDSA、Ed25519↔X25519 转换。
  - `pairing.rs` — initiate / poll-status / QR PNG 渲染。
  - `client.rs` — `reqwest::Client`（rustls）+ 端点封装 + idempotency / 重试拦截器。
  - `ws.rs` — `tokio-tungstenite` 长连接、心跳、指数退避、轮询兜底。
  - `bundle.rs` — envelope 序列化、加密、解密、AAD 计算。
  - `inbox.rs` — sync-inbox 写盘、文件名 sanitize、原子写。
  - `commands.rs` — Tauri 命令注册（合并到 `apps/desktop/src-tauri/src/commands.rs` 现有数组）。
- **状态机:** 模块化为三个独立状态机：
  - `IdentityState`: `NotInitialized | Loaded(fingerprint, uuid?)`
  - `PairingState`: `Idle | Pending(session_id, expires_at) | Completing | Paired(peer_fp) | Failed(reason)`
  - `ConnectionState`: `Disabled | Connecting | Connected | Reconnecting(attempt) | Offline`
  - 状态变更必经 `SpineState::transition()`，原子推送事件 `spine://status` 给前端。
- **后台任务管理:** 所有长跑 task（WS 重连循环、轮询兜底、JWT 刷新、bundle 拉取处理）由 `tokio::task::JoinSet` 集中管理；unpair / reset 时统一 abort，杜绝 goroutine 泄漏。
- **错误处理:** 自定义错误类型 `SpineError`（含 `code: &'static str` + `message: String`），实现 `serde::Serialize`，通过 Tauri 命令直接传给前端；前端按 `code` 走文案映射。
- **日志安全:** `tracing` instrumentation 中过滤 `Authorization` header、`bundle_blob` body、`sync_key`、`shared_secret`；CI 跑 `cargo clippy -W clippy::print_stdout -W clippy::print_stderr` + 自定义 lint 阻止意外打印。
- **配置文件向后兼容:** 旧版本 `config.toml` 无 `[spine]` 段时，`serde(default)` 应回退到全 `None`；保存时再次写入完整段。
- **可测试性:** `client.rs` / `ws.rs` 抽象 `Transport` trait（HTTP / WS），单元测试用 mock 实现；crypto 单元测试使用确定性 `ChaChaRng::seed_from_u64`。

## Technical Considerations

- **新增 Rust crate 依赖**（`apps/desktop/src-tauri/Cargo.toml`）：
  - `keyring = "3"` — OS 钥匙串
  - `ed25519-dalek = { version = "2", features = ["pkcs8", "rand_core"] }` — Ed25519
  - `x25519-dalek = "2"` — X25519 ECDH
  - `curve25519-dalek = "4"` — Ed25519↔Curve25519 转换辅助
  - `aes-gcm = "0.10"` — AES-256-GCM
  - `hkdf = "0.12"` + `sha2 = "0.10"` — HKDF-SHA256
  - `qrcode = "0.14"` + `image = "0.25"` — QR PNG 渲染
  - `reqwest = { version = "0.12", default-features = false, features = ["rustls-tls", "json", "stream"] }`
  - `tokio-tungstenite = { version = "0.24", features = ["rustls-tls-webpki-roots"] }`
  - `jsonwebtoken = "9"` — EdDSA JWT
  - `uuid = { version = "1", features = ["v4", "serde"] }`
  - `base64 = "0.22"` — base64url
  - `chrono = { version = "0.4", features = ["serde"] }`
  - `tracing = "0.1"`
  - 现有依赖 `serde` / `serde_json` / `tokio` 不变。
- **核心 Cargo.toml 改动:** `core/syncmind-core/src/config.rs` 的 `Config` 结构扩 `[spine]` 段；不改任何其他 crate 公共 API。
- **AAD 选择理由:** 把对端 fingerprint 的 SHA-256 原始字节放进 GCM AAD 是深度防御 — 即使两个不同的配对会话偶然共享 sync_key（理论上不可能，但属于零信任假设），AAD 不匹配会让 GCM 解密失败。
- **Nonce 预算:** 每个 `sync_key` 寿命内 96-bit 随机 nonce，按 birthday bound `2^32` 个 bundle 时碰撞概率 `~2^-32`。本 PRD 假定单次配对生命周期内的 bundle 数远小于此；spec 明确"累计 ≥ 10^7 bundles 时强烈建议主动 unpair → re-pair 触发 sync_key 轮换"。
- **JWT 时钟漂移:** 客户端使用本机 UTC，与服务端 PRD 002 §US-013 约定的 ±300s leeway 兼容；不实现 NTP 同步。
- **平台差异:**
  - **macOS:** Keychain 直接可用，无额外依赖。
  - **Windows:** Credential Manager 直接可用。
  - **Linux:** 需运行时存在 libsecret-1（GNOME Keyring / KeePassXC / kwallet 提供）。若缺失，按 US-029 降级路径。
  - CI 使用 `keyring` 的 mock provider；GitHub Actions Ubuntu runner 需 `apt install libsecret-1-dev` 才能跑非 mock 集成测试。
- **测试策略:**
  - 单元测试：crypto 黄金向量、envelope serde、idempotency key 一致性、状态机不可达状态拒绝。
  - 集成测试：起本地 `docker compose up` 服务端（`services/sync-gateway/docker-compose.yml`），跑 e2e：
    1. 桌面 A 启动 → 生成身份 → 调用 initiate → 拿到 QR。
    2. 桌面 B 启动 → 解析 QR payload（测试态绕过 UI）→ 调用 complete。
    3. A 与 B 各自派生 sync_key → 互发一条 Note → 各自的 sync-inbox 出现对方文件。
    4. 校验 server 数据库中 `encrypted_payload` 不包含 plaintext 关键字（grep audit）。
  - 模糊测试：envelope 反序列化对随机 bytes / 截断 / 错位 schema_version 的 panic-free 表现。
- **资源开销:** 静态分析新增 crate 二进制贡献预计 ~3MB；WS 长连接稳态占用 < 200KB；后台 task 数 ≤ 5；进程稳态新增 RSS < 20MB（与 Goal 一致）。
- **审计点（与 PRD 002 §"E2EE 审计点"对偶）:** 客户端代码必须能通过以下静态搜索验证：
  - 不存在 `eprintln!.*sync_key` / `eprintln!.*shared_secret` / `dbg!.*Signing` 等模式。
  - 所有 keychain 写入路径只接受 `service = "syncmind"` 与受白名单 account 前缀。
  - Tauri 命令注册数组中不包含任何返回 `Vec<u8>` 私钥的命令。

## Success Metrics

- **配对体验:** 两台已知 Spine URL 的桌面端，从用户点击 "开始配对" 到 paired 状态确认，p50 < 30s（局域网到自托管 Spine）。
- **同步延迟:** 一条 1 KB Note 端到端（A `spine_send_note` → B sync-inbox 文件落地），WS 在线时 p50 < 2s，p95 < 5s；WS 离线兜底轮询场景 p95 < 35s。
- **可靠性:** 30 分钟"网络抖动"测试（每 30 秒强制断开 5 秒）下，所有 Bundle 最终落地，无丢失、无重复落地。
- **安全:** Spine 服务端的 PostgreSQL `pg_dump` 中，对任意明文关键词（如发送的测试 Note 内容）grep 返回 0 行。
- **资源:** 桌面端进程在 100 条积压 Bundle 拉取场景下，峰值 RSS 增量 < 80MB；稳态 < 20MB。
- **错误可见性:** UI 在任何 Spine 异常下都能显示明确文案，不出现"加载中…"无限旋转。

## Implementation Notes & Divergences

> 本节预留给实施阶段；PR 作者按 PRD 002 同名章节的格式记录与本 spec 的实际偏差。空段表示尚未开始实施。

## Open Questions

1. **Ed25519 → X25519 转换的权威定义:** PRD 002 §Impl Note 1 仅说明"X25519 ECDH 在客户端本地完成"，未指定具体的密钥转换算法。本 PRD §US-031 提议使用 dalek 推荐的 `SigningKey::to_scalar_bytes()` + `CompressedEdwardsY::to_montgomery()`。建议向 PRD 002 提交 amendment，把该算法选择固化为协议层契约，以保证未来其他客户端（mobile / web）实现一致。
2. **file-watcher 的目录监听能力:** `core/file-watcher` 当前的 `registered_files` 是否支持目录级递归监听？若仅支持文件列表，US-036 必须退化为"写入后主动调用 indexing 流水线"，并在 `core/syncmind-indexing` 暴露一个 `index_single_file` 入口；该改动需要在实施阶段先于 spine 客户端落地，或者作为本 PRD 的隐含前置条件。**实施前必须验证。**
3. **设备 UUID 来源:** PRD 002 服务端在配对完成时为每台设备分配一个 UUID（`devices.id`），但目前的客户端流程中并未明确这个 UUID 如何回传给设备本身。临时方案是从已签发的 JWT `sub` claim 中解码自身设置的 UUID（即客户端在签发时自定 UUID 后服务端会接受？需核对），或服务端补一个 `GET /v1/me` 端点。该问题不阻塞配对，但影响 UI 文案与日志的可读性。
4. **Spine URL 校验严格度:** 是否在 dev 模式之外也放行 IP + 自签证书？某些自托管场景（家用 NAS）没有域名也没有公网证书。建议在设置面板提供"信任自签 CA"开关 + PEM 文件路径输入，由 reqwest `add_root_certificate` 加载；该能力在 US-028 中未明确，需在实施前确认是否纳入本期范围。
5. **sync-inbox 文件生命周期:** 本 PRD 默认保留所有 inbox 文件作为审计副本。若用户使用 1+ 年后积累数 GB 体积，是否需要内置 LRU 自动清理（按文件年龄）？或仅提供手动清理 UI（US-037）？倾向后者，但需要对长期用户做体积估算后定夺。
