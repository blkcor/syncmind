## Context

US-041 交付了完整的移动端配对链路：扫码 → 校验 → `/v1/pairing/complete` → sync_key 派生 → 持久化。配对后的状态管理与解配对是 US-042 的范围。

当前代码库中已有 `device_reset()`（`apps/mobile/src/crypto/identity.ts:211`），但它执行的是全量清除——包括 Ed25519 身份密钥。US-042 需要的是保留身份密钥的轻量 unpair。

此外，当前 `revokeCurrentDevice()`（`apps/mobile/src/spine/client.ts:6`）调用的是 `POST /v1/auth/revoke`，只撤销当前 JWT，不会撤销 device row 或解除 paired 关系。US-042 需要补齐 device-level status/revoke API，并让移动端改用该 contract。

## Goals / Non-Goals

**Goals:**
- Settings 页面新增 "Paired Desktop" 卡片，展示 peer 信息（fingerprint 截断、device_type、paired_at 相对时间、Spine URL、last_seen_at）
- 实现轻量 `unpair()` 流程：Spine revoke → abort in-flight Spine 请求/upload → clearPairingState → clearOutbox → setUnpaired
- 保留 Ed25519 身份密钥，允许立即重新配对
- 新增 Spine self-device status/revoke 端点，供移动端启动校验和解配对调用
- 401 响应自动进入 Unpaired 状态；404 仅在 self-device status/revoke 或明确设备失效错误码时进入 Unpaired 状态
- Tab 灰度锁定：未配对时需要已配对能力的入口不可用；Capture 保留为配对入口

**Non-Goals:**
- 不修改 `device_reset()` 的行为（核选项保持不变）
- 不实现多桌面配对（单桌面绑定仍是 MVP 约束）
- 不实现任意设备管理 API（只实现 self-device status/revoke，不提供列出设备、管理 peer 或多设备功能）
- 不实现配对恢复/重连（那是未来的 US-0XX）
- 不修改 QR pairing 流程本身

## Decisions

### 1. Spine self-device API：`GET/POST /v1/devices/{self}`

**选择:** 在 Spine 中新增两个受保护端点：
- `GET /v1/devices/{self_device_uuid}`：仅允许 JWT `sub` 与 path device UUID 相同的设备读取自身状态，返回 `device_uuid`, `device_type`, `paired_device_id`, `is_active`, `last_seen_at`。
- `POST /v1/devices/{self_device_uuid}/revoke`：仅允许 JWT `sub` 与 path device UUID 相同的设备撤销自身；请求体可携带 `{ "device_uuid": "<self_device_uuid>" }` 作为一致性校验。服务端将当前设备 `is_active=false`，并清除对端 `paired_device_id` 中指向当前设备的关系。

**理由:** `POST /v1/auth/revoke` 只 blacklist 当前 JWT，不能表达“解除配对并撤销此设备”。US-042 的用户语义是 device-level unpair，因此 server 端点必须纳入本 change，避免移动端调用不存在的 contract。

**替代方案:**
- 保留 `POST /v1/auth/revoke` — 只能撤销 token，不能撤销 device row 或解除 paired 关系
- 只做移动端本地清理 — 用户可以重新配对，但 Spine 侧留下 active orphan device，不符合 US-042

### 2. Mobile auth：Ed25519 JWT Bearer（非 sync_key auth）

**选择:** `authenticatedFetch(url, options)` 在每次请求前使用 mobile native identity `sign()` 生成 Ed25519 JWT，并注入 `Authorization: Bearer <jwt>`。JWT claims：
- `sub`: `PersistedPairingState.selfDeviceUuid`
- `iat`: 当前 Unix 秒
- `exp`: `iat + 24h`
- `jti`: UUIDv4
- `iss`: `"syncmind-spine"`
- `aud`: `"syncmind-device"`

**理由:** 当前 Spine `AuthMiddleware` 只接受 Ed25519-signed Bearer JWT。`sync_key` 只用于 envelope 加解密，不能作为 HTTP auth header，避免把数据加密密钥扩大为认证凭据。

**Spec delta:** 本 change 将 active `device-auth` capability 的 canonical issuer/audience 更新为 Spine 发行、设备接收：`iss="syncmind-spine"`、`aud="syncmind-device"`。旧文档中 `iss="syncmind-client"`、`aud="syncmind-spine"` 是历史 contract，应由本 change 的 `device-auth` delta 覆盖。

### 3. 401/404 拦截层：fetch wrapper（非 Axios interceptor）

**选择:** 在 `spine/client.ts` 中新增 `authenticatedFetch(url, options)` wrapper，统一注入 Ed25519 JWT Bearer，并处理受限的 unpair 降级：
- 任意 authenticated request 返回 401 → `clearPairingState()` + `setUnpaired()` + throw `UnpairedError`
- 404 仅在请求目标是 `GET /v1/devices/{self}`、`POST /v1/devices/{self}/revoke`，或响应错误码明确为 `DEVICE_REVOKED` / `DEVICE_NOT_FOUND` 时触发 auto-unpair
- 普通资源 404（例如 bundle 不存在或无权访问）按原响应返回，不清 pairing state

**替代方案:**
- 每个调用点自行处理 — 容易遗漏，不符合 DRY 原则
- Axios interceptor — 当前项目不依赖 axios，引入新依赖不合理

**理由:** 401 表示当前设备认证失效，必须回到 unpaired。404 在现有 bundle download/ack 中也表示资源不存在或权限隐藏，不能全局等同于“device revoked”。

### 4. `lastSeenAt` 更新时机：每次收到 Spine 2xx 时

**选择:** 在 `authenticatedFetch` 中，任何 2xx 响应返回前更新 `PersistedPairingState.lastSeenAt` 为当前 UTC ISO 8601 时间戳，并异步写入 `expo-secure-store`。

**理由:** US-042 设置页显示的是 "Last seen"，语义为“最后一次成功与 Spine 通信的时间”。移动端目前没有独立的 upload tracker，且 US-043（capture 上传）尚未实现；不要把该字段命名或展示为“最后一次上传时间”。

**Compatibility:** US-041 已持久化的 pairing state 没有 `syncmind.pairing.last_seen_at`。`restorePairingState()` 必须把缺失值恢复为 `null`，不能将旧状态判定为 corrupted。

### 5. Fingerprint 截断格式：`sha256:abcd1234…wxyz`

**选择:** 显示算法——取 `sha256:` 前缀后的 hex 部分，前 8 个字符 + `…` + 后 4 个字符。完整 fingerprint 通过 `selectable` 属性可复制。

**例子:** `sha256:a1b2c3d4e5f6…a1b2c3d4e5f6…` → 显示为 `sha256:a1b2c3d4…e5f6`

**理由:** 前 8 后 4 是 PRD 中明确指定的截断规则。完整 fingerprint 可选中复制用于手动比对。

### 6. Tab 灰度锁定：保留 Capture 配对入口

**选择:** 在 `(tabs)/_layout.tsx` 中读取 `useAppStore().isPaired`。当 `false` 时，保留 Capture tab 可导航，因为当前 `index.tsx` 承载 QR pairing scanner；只锁定需要已配对能力的入口（当前占位 Graph/未来 Search/List tab），将其 `href` 设为 `null` 或用 `onPress` 阻止导航，并在 tab bar label 旁显示锁定图标，透明度降至 0.4。

**替代方案:**
- Tab bar badge 提示 "locked" — 语义不清晰
- 锁定 Capture — 会切断新用户主要配对入口，除非 Settings 同时新增完整配对入口；本 change 不做这类入口迁移

**理由:** 禁用导航 + 视觉降级是最清晰的 "此功能需要配对" 信号，但不能阻断配对入口。Capture 在未配对时显示 pairing scanner 或空状态 CTA；Search/List 在未配对时显示 "Pair with a desktop..." 的空状态并可跳转 Settings。

### 7. `unpair()` 返回结构化 revoke warning

**选择:** `unpair()` 始终执行本地 cleanup（`clearPairingState()`、`clearOutbox()`、`setUnpaired()`）并保留 identity。若 remote revoke 发生网络错误、DNS、timeout 或 5xx，`unpair()` 返回 `{ revokeWarning: "network_error" }`（或等价 typed result）供 Settings 显示 "Could not notify desktop — unpaired locally"。401、404 self-device not found/revoked 静默视为成功。

**理由:** 用户已经确认 unpair，本地退出配对应优先完成；但 Settings UI 仍需要知道远端通知失败，以便诚实提示。

### 8. In-flight abort：解除当前 pairing 的活动网络任务

**选择:** `unpair()` 在本地 cleanup 阶段必须 abort 当前 pairing 下仍在进行的 Spine 请求 / upload。abort 只作用于当前 mobile pairing 会话的活动请求，不销毁 Ed25519 identity，也不影响未来重新配对后的新请求。若 remote revoke 本身已经完成或失败，abort 仍需执行，保证用户确认 unpair 后不会继续使用旧 pairing 上传或重试。

**理由:** PRD 005 §US-042 明确要求清空发送队列并 abort 正在发送的 in-flight 请求。只清 outbox 不足以阻止已经发出的请求继续消耗旧 pairing state。

### 9. 模块结构：少量 server 文件 + 现有 mobile 模块扩展

**选择:** 移动端不在 `apps/mobile/src/` 下创建新的 `unpair/` 或 `pairing-state/` 目录。unpair 逻辑放在 `crypto/identity.ts`（与 `device_reset()` 并列），JWT/authenticated fetch 放在 `spine/client.ts`，UI 放在已有 `settings.tsx`。Spine server 新增一个小的 `internal/handler/device.go`，并在 `cmd/spine/main.go` 注册受保护路由。

**理由:** US-042 是现有模块的增强，不是新业务域。`unpair()` 是 `device_reset()` 的轻量变体，放在同一文件便于对比和维护；server 端点属于 device resource，单独 handler 能避免把 device-level 语义塞进 auth handler。

## Risks / Trade-offs

- **[Risk] 新增 Spine self-device API 扩大 US-042 范围** → 只实现 self-device status/revoke，不增加任意设备管理、设备列表或多配对功能；测试限定 path `device_uuid` 必须等于 JWT `sub`。
- **[Risk] 401 拦截可能与正常的 auth 刷新逻辑冲突** → MVP 没有 token 刷新机制（JWT 长有效期）。当未来引入 refresh token 时，401 拦截应首先尝试刷新，只有刷新失败才降级 Unpaired。
- **[Risk] `lastSeenAt` 频繁写入 `expo-secure-store`** → `expo-secure-store` 在 iOS 上每次写入涉及 Keychain 操作，频繁写入可能有性能影响。MVP 采用 throttle：最多每 30s 写入一次，内存中实时更新。
- **[Risk] 404 语义混淆导致误 unpair** → wrapper 只对 self-device status/revoke 或明确设备失效错误码触发 404 auto-unpair；bundle/resource 404 不清配对。
- **[Risk] Tab 灰度锁定可能让用户困惑** → Capture 保留配对入口；被锁定页面显示 "Pair with a desktop..." 空状态并提供 Settings 跳转。
