# Design: Desktop Spine Pairing Payload

## Context

桌面端目前在 `apps/desktop/src-tauri/src/spine/pairing.rs::initiate` 中直接调用 `client.pairing_initiate(...)`、拿到 Spine 返回的 `qr_payload: String`（格式 `spine://pair/{session_id}?pk={b64url_pubkey}`），传给 `render_qr_png_base64` 后回到前端。整条链路是字符串透传，没有版本字段，没有载荷扩展空间。

Phase 4 移动端 (`apps/mobile/`) 在 PRD 005 §US-041 里规定的 QR payload 是 JSON object，需要包含：

| 字段 | 来源 |
|---|---|
| `v` | 客户端约定，当前 `1` |
| `kind` | 常量 `"syncmind-pairing"` |
| `spine_url` | `Config.spine.url`（桌面端已持久化） |
| `ca_fingerprint` | `Config.spine.ca_fingerprint`（如有，自签 CA 场景） |
| `pairing_token` | 当前 `qr_payload` 中的 `?pk=` 段（即 `initiator_pubkey_b64`）；MVP 直接复用此字段作为 token，避免 Spine 服务端再加一个端点 |
| `expires_at` | Spine `pairing_initiate` 响应中的 `expires_at` |
| `device_a_pubkey` | 桌面 Ed25519 公钥的 base64 编码（不是 X25519 ephemeral） |
| `device_a_fingerprint` | `SHA-256(ed25519_pub) → hex`，前后端约定缩略显示前 8 位 + 后 4 位 |

`device-pairing` 服务端契约不变，所有变更落在桌面客户端。

## Goals / Non-Goals

**Goals:**

- 把 QR 内容从 URI 字符串升级为版本化 JSON object，且**不破坏现有的 Spine 服务端协议**。
- 给前端额外提供原始 JSON 文本（便于 Devices Tab 的 "Copy payload" 按钮 / debug 视图），同时继续提供 QR 图像 base64。
- 提供 `parse_mobile_pairing_payload` 的反向解码能力，未来桌面端如果增加扫码场景，能识别 JSON v:1 与遗留 URI 两种格式。
- 所有变更对 PRD 004 的 desktop ↔ desktop 配对路径**零回归**。

**Non-Goals:**

- 不修改 Spine 服务端 `pairing/initiate` / `pairing/status` / `pairing/complete` 端点。
- 不引入 Spine 服务端的 `pairing_token` 概念（目前用 `initiator_pubkey_b64` 直接承担此角色）。
- 不实现移动端任何代码——这是后续 OpenSpec change 的范畴。
- 不引入新的二维码图像格式（继续 PNG base64，由 `qrcode` crate 渲染）。
- 不引入加密包装（payload 是明文 JSON，依赖 QR 物理可见性 + 5 分钟 TTL + 一次性 token 的组合保护）。

## Decisions

### D1：QR 内容编码——纯 JSON 字符串

**选项 A（采纳）:** QR 内容直接是 `serde_json::to_string(&payload)?` 的输出，UTF-8 字符串。  
**选项 B:** 使用 CBOR + base64url 包装。  
**选项 C:** 使用 `data:application/json;base64,...` 自定义 URI。

选 A 的原因：
- QR Code 在 ~700 字符以内仍能保持中等密度的可扫性；估算 payload 长度（spine_url ~50 + b64_pubkey 44 + fingerprint 64 + iso8601 25 + 其余固定结构 ~100）约 **300-350 字符**，远低于阈值。
- 选 A 让 mobile 端无需任何额外解码步骤，`JSON.parse(scanned)` 即可，与 PRD 005 §US-041 描述完全一致。
- 选 B 引入额外 Rust + TS 端依赖（CBOR 编码器）；收益不明显。
- 选 C 看似规范，实际上 mobile 端要写一个 URI 解析器再 atob 再 JSON.parse，凭空多两步。

### D2：`pairing_token` 字段的语义——复用 `initiator_pubkey_b64`

Spine `pairing_initiate` 当前没有"pairing token"这个独立概念，而是把 `initiator_pubkey` 作为 session 的隐式凭证。PRD 005 §US-041 schema 里的 `pairing_token` 字段为了客户端语义清晰命名为 token，但 MVP **直接用 `initiator_pubkey_b64` 填充**。

> 注：这意味着 mobile 客户端拿到 `pairing_token` 后还是要把它作为 `initiator_pubkey` 字段 POST 到 `/pairing/complete`。这是约定，PRD 005 §US-041 的 mobile 实现层会处理。

如果未来 Spine 增加真正的 token 抽象，可以在 v2 schema 里拆分两个字段；目前 v1 保持简化。

### D3：`ca_fingerprint` 缺省值——`null` 而非空串

序列化时使用 `Option<String>`，None → JSON `null`。空串可能让 mobile 端误判为"需要 pin 一个空指纹"。`null` 明确表示"使用系统 trust store"，与 mobile 端 TLS 校验逻辑（见 PRD 005 §US-041 验收点）匹配。

### D4：保留遗留 URI 反向解析

`parse_mobile_pairing_payload` 第一步是 `serde_json::from_str`，失败时回退到字符串 prefix 检查 `spine://pair/`。这样：

- 未来如果桌面端实现扫码场景（例如"扫另一台桌面的 QR 完成配对"），无须区分两套 API。
- 当前并不在 Devices Tab 暴露扫码 UI，所以这一段代码仅有单元测试覆盖，是预留能力。

### D5：Tauri 命令返回结构——新增字段而非替换

`PairingHandleView` 现有字段（`session_id`, `short_code`, `qr_png_base64`, `expires_at`）保持原样；新增 `qr_payload_json: String`。

- 前端 Devices Tab 若不读取 `qr_payload_json`，行为完全不变。
- 类型生成器（`tauri-specta` 或 `ts-rs`，取决于工程实际使用）会自动把新字段带到前端 TS 类型，不需要手写 ambient declaration。

### D6：`expires_at` 时区——ISO 8601 UTC，server-authoritative

直接复用 Spine 返回的字符串（已是 RFC 3339 UTC），不在桌面端做时区转换。Mobile 端用 `Date.parse` 解析时按 UTC 比较即可。

### D7：版本字段——主动拒绝未知版本

`MobilePairingPayload::v` 是 `u8`，反序列化时如果 v != 1 直接返回 `SpineErrorCode::UnsupportedVersion`。MVP 不实现"forward-compatible warning"分支——客户端只接受它认识的版本，避免半解析状态。

## Risks / Trade-offs

- **[Risk] QR 字符串变长导致扫码失败率上升** → Mitigation: 把 ECC 级别从默认 `Medium` 调到 `Low`（300 余字符在 Low ECC 下密度合理），并在 Devices Tab 标准化最大显示尺寸 ≥ 256×256 px。同时手动 6 位短码 fallback 始终保留。
- **[Risk] 前端类型生成漂移导致 build 失败** → Mitigation: tasks.md 显式把 `pnpm typecheck` 列为 CI 关键步骤；新增字段在 `PairingHandleView` 上加 `#[serde(default)]` 让旧前端缓存的 TS 类型有过渡空间。
- **[Risk] `device_a_pubkey` 与 X25519 ephemeral 混淆** → Mitigation: 在 `MobilePairingPayload` 字段 doc-comment 里明确写"this is the long-term Ed25519 identity pubkey, NOT the X25519 ephemeral used in HKDF"；mobile 端拿这个 pubkey 仅用于 fingerprint 校验，不参与密钥派生。
- **[Risk] 反序列化老 URI 时被注入畸形数据** → Mitigation: `parse_mobile_pairing_payload` 在 URI 路径里严格校验 scheme + host + query key 白名单，任何额外参数直接 `BadRequest`。
- **[Trade-off] 不引入服务端真 token** → 服务端协议不变换来 PR 简洁，代价是 `pairing_token` 在 schema 里和 `initiator_pubkey` 是同一值；可接受。

## Open Questions

- Devices Tab 是否需要立即新增 "Copy payload JSON" 按钮？倾向**不在本 change 范围**，留给后续 UI 改进 change。
- 未来如果引入桌面端扫码 UI，是否要把 `parse_mobile_pairing_payload` 提到独立模块？暂不必，留在 `pairing.rs` 内部。
