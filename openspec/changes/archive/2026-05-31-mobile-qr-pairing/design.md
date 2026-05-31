## Context

移动端已有 native-backed Ed25519 设备身份（`apps/mobile/src/crypto/identity.ts`，US-040），通过 `SyncMindDeviceIdentity` 管理私钥并提供 `derive_x25519(peer_pub)` facade。Spine 端已实现 `/v1/pairing/complete`，当前真实请求体是 `session_id` 或 `short_code` + `device_uuid` + `responder_pubkey` + `device_type`，响应返回 `initiator_id` / `responder_id` / `initiator_pubkey`。

桌面端已实现 v1 QR pairing payload（US-052），但现有 JSON 只包含 `pairing_token` 与 `device_a_pubkey`，没有可提交给 Spine 的 `session_id`。本 change 因此包含一个最小协议修正：v1 QR payload 必须携带 `session_id`，移动端完成配对时使用该字段作为 `/v1/pairing/complete` locator。

US-041 需要补齐从"扫码 → 校验 → 完成握手 → 派生 sync_key → 持久化会话"的完整移动端配对链路。

## Goals / Non-Goals

**Goals:**
- 实现 `expo-camera` 扫码 UI，含权限降级（粘贴原始 JSON payload）
- 修正桌面端 v1 QR payload schema，确保 payload 包含 `session_id`
- 实现 QR payload 校验器：`v`、`kind`、`session_id` UUID、`expires_at` TTL、`spine_url` https、base64url pubkey、fingerprint
- 实现 `/v1/pairing/complete` 调用：携带 `session_id`、移动端稳定 `device_uuid`、移动端 Ed25519 identity pubkey、`device_type="mobile"`
- 实现 `sync_key = HKDF-SHA256(x25519_shared, salt=session_id, info="syncmind-v1")` 派生，其中 shared secret 由 native identity 的 Ed25519 seed 转换得到
- 持久化配对状态到 `expo-secure-store`（self device UUID、sync_key、peer fingerprint、peer device id、spine_url 等）
- 升级 `spine/session.ts` 为持久化会话模型

**Non-Goals:**
- 不修改 Spine `/v1/pairing/complete` 服务端协议（PRD 002 已实现）
- 不改变桌面端 QR payload 的版本号；只给 v1 schema 增加 required `session_id`
- 不解配对（US-042）
- 不实现 capture 上传（US-047）
- 不实现多设备配对（单桌面绑定是 MVP 约束）
- 不在 MVP 中承诺 TLS certificate pinning；`ca_fingerprint` 仅作为 metadata 与未来 native pinning 输入

## Decisions

### 1. 扫码方案：`expo-camera` `CameraView`

**选择:** 使用 `expo-camera` 的 `CameraView` + `barcodeScannerSettings` 识别 QR code；权限流使用 `useCameraPermissions()`。

**替代方案:**
- `expo-barcode-scanner` — 已被 `expo-camera` 取代，不作为新实现依赖
- `react-native-vision-camera` — 功能强但需要 prebuild / bare workflow 配置，与当前 Expo managed-first 方向冲突

**理由:** `expo-camera` 是 Expo SDK 标准组件，适合当前 mobile scaffold。手动 fallback 接受桌面端 Copy payload 的原始 JSON 字符串；若未来桌面端输出 base64url-wrapped JSON，可在 parser 增加 decode 分支。

### 2. Pairing locator：QR payload 必须携带 `session_id`

**选择:** 在 `mobile-pairing-payload` v1 schema 中增加 required `session_id` 字段，值为 Spine `pairing_initiate` 返回的 UUID 字符串。移动端只使用 `session_id` 调用 `/v1/pairing/complete`，不得把 `pairing_token` 当作 session id。

**理由:** 当前 Spine server 只支持通过 `session_id` 或 `short_code` 查找 pending pairing session。现有桌面端 JSON payload 中的 `pairing_token` 实际来自 legacy URI 的 `pk` query，也就是发起方 Ed25519 pubkey；把它提交为 `session_id` 会被 server 拒绝。

### 3. Device UUID：移动端本地稳定生成

**选择:** 新增 `ensureMobileDeviceUuid()`，首次配对前生成 UUIDv4，存入 `expo-secure-store`，后续 pairing completion 与 JWT `sub` 复用该值。

**理由:** Spine 已按 PRD 002 §Impl Note 1.2 要求客户端提交 `device_uuid` 并作为 `devices.id`。US-040 当前 identity meta 不包含 UUID，因此 US-041 必须补上这个稳定设备标识。

### 4. X25519 shared secret：复用 native identity，不生成临时 X25519 密钥

**选择:** 移动端解码 QR payload 的 `device_a_pubkey`（base64url-no-pad Ed25519 pubkey），校验 fingerprint 后将其转换为 X25519 public key，再调用 native identity facade `derive_x25519(peer_x25519_pubkey)` 得到 shared secret。

**替代方案:**
- 纯 JS 生成 ephemeral X25519 keypair 并上传给 Spine — 与当前 server contract 不匹配，desktop 也不会用这个 ephemeral key 派生 `sync_key`
- 将移动端 Ed25519 private seed 暴露给 JS 做转换 — 违反 US-040 的 native-backed 私钥边界

**理由:** PRD 002 当前规范性契约是 Ed25519 ↔ X25519 转换后本地 ECDH。移动端必须保持私钥只在 native module 中，JS 侧只处理 public key 转换与 HKDF。

### 5. HKDF 派生：`@noble/hashes`

**选择:** 使用 `@noble/hashes` 的 `hkdf` 函数，`salt = UTF-8(session_id)`，`info = "syncmind-v1"`，输出 32 字节 `sync_key`。

**理由:** 与桌面端 PRD 002 / PRD 004 的 HKDF 参数一致，适合用 golden vector 验证跨端互通。

### 6. 配对状态持久化：`expo-secure-store`

**选择:** 将 `self_device_uuid`、`sync_key`、`paired_peer_fingerprint`、`paired_peer_device_id`、`paired_peer_device_type`、`spine_url`、`ca_fingerprint`、`paired_at` 分别作为独立 key 存入 `expo-secure-store`。

**理由:** `sync_key` 是最敏感的同步密钥，必须进入 SecureStore。`self_device_uuid` 不是密钥，但它参与 JWT `sub` 与 server device row 绑定，也应随 pairing/session state 一起恢复。

### 7. 模块结构：新建 `pairing/` 目录

```
apps/mobile/src/pairing/
  scanner.tsx       — QR scanner 组件 + 权限降级 UI
  payload.ts        — QR payload 解析 & 校验
  device.ts         — self_device_uuid 生成/恢复
  handshake.ts      — /v1/pairing/complete 调用 + sync_key 派生
  index.ts          — 顶层配对流程 orchestrator
```

**理由:** 配对是独立业务域，不应耦合到 `crypto/` 或 `spine/` 中。`spine/session.ts` 升级为可持久化会话后由 `pairing/` 写入。

## Risks / Trade-offs

- **[Risk] 老桌面端 QR payload 缺少 `session_id`** → mobile parser 将返回 "Desktop version too old — update SyncMind Desktop and generate a new QR code"，不尝试用 `pairing_token` 猜测。
- **[Risk] Ed25519 public key → X25519 public key 转换实现跨端不一致** → 加 golden vector：同一 mobile seed + desktop pubkey + session_id 必须与桌面端 Rust fixture 得到相同 `sync_key`。
- **[Risk] `expo-camera` 在低端 Android 设备上扫码速度慢** → 同时提供粘贴原始 JSON payload 降级路径；不要求用户手输长串。
- **[Risk] 时钟漂移导致 expires_at 校验误拒绝** → 容忍 ±60s 偏移，符合 `mobile-pairing-payload` spec 约束。
- **[Risk] TLS certificate pinning 在 Expo managed workflow 中不可可靠实现** → MVP 不承诺 pinning。`ca_fingerprint` 被校验为合法格式并持久化；只有当平台能力可取得证书链时才执行 fail-closed mismatch 校验，否则走系统 trust。
