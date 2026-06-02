## Why

US-041 交付了 QR 扫码配对，用户可以在 10 秒内完成移动端与桌面端的加密配对。但配对之后，用户缺少两个关键能力：(1) 在设置中查看当前配对的对端设备信息；(2) 在不销毁 Ed25519 身份密钥的前提下解除配对。当前 `device_reset()` 是核选项——它同时清除身份密钥、Spine 会话、发送队列和配对状态，用户如果只是想换一台桌面配对，必须重建整个设备身份（丢失历史 device UUID，Spine 侧留下孤儿记录）。

US-042 需要拆出轻量 unpair 流程：通知 Spine 撤销设备 → 清除 secure-store 配对键 → 清空 outbox → UI 回到未配对态，但 **保留** Ed25519 身份密钥，让用户可以立即重新扫码配对。

此外，Spine 侧可能先于客户端感知配对失效（对端 revoke、token 过期），客户端需要在认证失败或 self-device 状态检查确认设备失效时自动退回到 `Unpaired` 状态，而不是进入无限重试循环。

## What Changes

- **新增 Settings "Paired Desktop" 卡片**：显示 peer fingerprint（`sha256:abcdef…wxyz` 前8后4截断）、paired_at 相对时间、Spine URL、最后一次成功 Spine 联系时间（`last_seen_at`）
- **新增轻量 unpair 流程** — `unpair()`：
  1. 调用 Spine `POST /v1/devices/{self_device_uuid}/revoke` 通知服务端撤销
  2. 清除 `expo-secure-store` 中所有配对相关键（保留 Ed25519 身份密钥）
  3. abort 当前 pairing 下正在进行的 Spine 请求 / upload
  4. 清空 outbox 队列
  5. 更新 Zustand store 为 `isPaired: false`
- **新增 Spine device-level status/revoke 端点**：`GET /v1/devices/{self_device_uuid}` 用于启动校验；`POST /v1/devices/{self_device_uuid}/revoke` 撤销当前设备并解除 paired 关系
- **新增 Paired Devices 区段于 Settings 页面**：替代当前 Danger Zone 中唯一的 "Reset Device Identity" 核按钮，提供清晰的 "Unpair" 操作入口
- **新增 `last_seen_at` 追踪**：在 `PersistedPairingState` 中增加可选字段，每次收到 Spine 2xx 响应时更新
- **新增受限 401/404 自动 unpaired 检测**：认证 Spine API 返回 401 时自动 unpair；404 仅在 self-device status/revoke 或明确设备失效错误码时自动 unpair，普通资源 404 不清配对
- **新增 tab 灰度状态**：当 `isPaired === false` 时，底部 tab 中需要配对的入口（Search/List 等）显示为灰色并提示 "Pair with desktop to unlock"；Capture 入口保留为配对入口

## Capabilities

### New Capabilities

- `mobile-pairing-state-management`: 配对状态查看、轻量 unpair 流程、Spine self-device status/revoke、受限 401/404 自动降级为 Unpaired、tab 灰度锁定

### Modified Capabilities

- `mobile-pairing-payload`: 现有 `PersistedPairingState` schema 扩展 `last_seen_at` 字段；旧状态缺少该字段时恢复为 `null`，不能导致已配对用户被误判为 unpaired。
- `device-auth`: 将受保护 Spine API 的 canonical JWT contract 明确为 `iss="syncmind-spine"`、`aud="syncmind-device"`，由 Spine 作为发行方、设备作为受众。
- `mobile-device-identity`: 新增保留 Ed25519 identity 的轻量 `unpair()`，与销毁 identity 的 `device_reset()` 区分。

## Impact

- `apps/mobile/src/spine/session.ts` — `PersistedPairingState` 增加 `lastSeenAt: string | null`；新增 `updateLastSeenAt()` helper
- `apps/mobile/src/spine/client.ts` — 修正 `revokeCurrentDevice()` 端点为 `POST /v1/devices/{self}/revoke`（与 PRD 005 §US-042 对齐）；新增 Ed25519 JWT Bearer 注入和受限 401/404 响应拦截
- `apps/mobile/src/crypto/identity.ts` — 新增 `unpair()` 函数（abort in-flight 请求、清 pairing state、不触达 identity）；保留 `device_reset()` 不变
- `apps/mobile/src/store.ts` — `setUnpaired()` 已存在，无需修改
- `apps/mobile/src/outbox/service.ts` — `clearOutbox()` 已存在，无需修改
- `apps/mobile/app/(tabs)/settings.tsx` — 新增 "Paired Desktop" 卡片 + "Unpair" 按钮；重构 Danger Zone 卡片保留 "Reset Device Identity"
- `apps/mobile/app/(tabs)/_layout.tsx` — 根据 `isPaired` 灰度锁定需要已配对能力的 tab；Capture 保留可访问以承载配对入口
- `services/sync-gateway/internal/handler/device.go`（新增）/ `cmd/spine/main.go` — 增加受保护 self-device status/revoke routes；revoke 撤销当前设备并解除 paired 关系
- 测试 — `apps/mobile/__tests__/pairing-state.test.ts` 覆盖 unpair 流程、401/404 检测、lastSeenAt 更新
- 测试 — `services/sync-gateway/internal/handler/device_test.go` 覆盖 self-device status/revoke 的 200、403/404、401 和 paired-device unlink 行为
