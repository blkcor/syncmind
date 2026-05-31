## Why

移动端目前拥有本地 Ed25519 设备身份（US-040），但缺少与桌面端建立加密配对信道的入口。用户无法将手机"连接"到自己的桌面 Brain，整个 mobile capture 链路因此无法闭合。US-041 补上 QR 扫码配对这一个缺失的握手环节，让移动端在 10 秒内完成与桌面端的加密配对。

## What Changes

- 新增 QR 扫码界面：使用 `expo-camera` 实现相机扫码，支持权限降级（粘贴桌面端 Copy payload 的原始 JSON）
- 新增配对载荷验证层：校验 schema version、`session_id` UUID、`expires_at` TTL、`spine_url` https 约束、base64url pubkey 与 fingerprint
- 新增桌面端 QR payload 最小修正：v1 JSON payload 必须携带 Spine `session_id`，移动端不得把 `pairing_token` 当作 session locator
- 新增 `POST /v1/pairing/complete` 的移动端调用：上传 `device_uuid` + Ed25519 responder pubkey + `device_type="mobile"`，获取 `initiator_id` / `responder_id`
- 新增 `sync_key` 派生逻辑：用 native Ed25519 identity 派生 X25519 shared secret，再执行 `HKDF-SHA256(x25519_shared, salt=session_id, info="syncmind-v1")`
- 新增配对状态持久化：将 `self_device_uuid`、`sync_key`、`paired_peer_fingerprint` 等写入 `expo-secure-store`，替代当前仅内存级别的 `SpineSession`
- 新增 CA fingerprint 元数据处理：MVP 不声称 TLS pinning；仅在平台能力可用时校验，否则走系统 trust 并持久化 fingerprint 供后续 native pinning 使用
- 升级 `apps/mobile/src/spine/session.ts`：从纯内存 session 升级为持久化 + 可恢复的配对会话

## Capabilities

### New Capabilities

- `mobile-qr-pairing`: 移动端扫码配对 — 相机权限管理、QR 解码、payload 校验、Spine pairing completion 调用、sync_key 派生、配对状态持久化与恢复

### Modified Capabilities

- `mobile-pairing-payload`: 桌面端 QR payload v1 必须包含 `session_id`，并明确 `pairing_token` 为不参与 `/v1/pairing/complete` lookup 的 opaque/legacy 字段

## Impact

- `apps/mobile/src/` — 新增 `pairing/` 模块（scanner UI + payload 校验 + completion 调用 + key derivation）；修改 `spine/session.ts`（持久化会话）、`store.ts`（补充配对状态字段）
- `apps/mobile/package.json` — 新增依赖 `expo-camera`（扫码）、`@noble/hashes`（HKDF）、`@noble/curves`（Ed25519 public key → X25519 public key 转换）；`expo-secure-store` / `expo-crypto` 已存在
- `apps/desktop/src-tauri/src/spine/pairing.rs` — v1 QR payload 增加 `session_id` 字段（移动端完成配对的 server locator）
- 协议层 — 依赖 Spine `POST /v1/pairing/complete`（PRD 002 §Impl Note 1.2，已实现）；修正桌面端 QR payload v1 schema（PRD 005 §US-052）
- 测试 — `apps/mobile/__tests__/pairing.test.ts` 覆盖 payload 校验、key derivation 正确性
