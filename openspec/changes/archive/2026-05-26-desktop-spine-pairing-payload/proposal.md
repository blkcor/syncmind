# Proposal: Desktop Spine Pairing Payload (Mobile-Ready QR)

## Why

Phase 3 桌面端的 Spine 配对 QR 当前直接渲染服务端返回的 `spine://pair/{session_id}?pk={initiator_pubkey}` URI 字符串。该格式只够覆盖桌面 ↔ 桌面场景，缺少移动端（PRD 005 §US-041、§US-052）完成离线扫码配对所必需的元数据：自托管 Spine URL、自签 CA 指纹、`expires_at`、发起方 Ed25519 身份指纹。Phase 4 移动端在桌面端能输出更富的 QR payload 之前无法开工，因此这是 Phase 4 的**最小必要前置**。

## What Changes

- 桌面端 `apps/desktop/src-tauri/src/spine/pairing.rs::initiate` 在调用完 Spine `pairing_initiate` 之后，将服务端返回的 URI 包装为版本化 JSON object 再渲染为 QR；JSON 结构与 PRD 005 §US-041 完全一致。
- 新增 Rust 结构体 `MobilePairingPayload { v, kind, spine_url, ca_fingerprint, pairing_token, expires_at, device_a_pubkey, device_a_fingerprint }`，序列化为 UTF-8 JSON 字符串作为 QR 内容。
- 扩展 `PairingHandleView` 的 Tauri 返回类型：
  - 既有 `qr_png_base64` 字段保留（前端无需立即改动），其内部内容已替换为 JSON payload 的 QR；
  - 新增 `qr_payload_json: String` 字段（前端 Devices Tab 可选择直接展示或复制）。
- `pairing.rs` 内部新增 `parse_mobile_pairing_payload(input: &str) -> Result<MobilePairingPayload, SpineError>`，能区分并解析两种输入：
  - JSON v:1（mobile 端发出）；
  - 旧 `spine://pair/...` URI（桌面 ↔ 桌面场景仍可被未来的 desktop 端扫描/粘贴流复用）。
- 桌面端 `Config::spine` 复用既有 `url` 与 `ca_fingerprint`（如有）字段，作为 JSON payload 的两个数据源；缺失自签 CA 时 `ca_fingerprint` 置 `null` 而非空字符串。
- 测试：新增 unit tests 覆盖（1）正常 JSON 编码 round-trip；（2）合法 v:1 JSON 反序列化；（3）旧 URI 反序列化兼容；（4）未来 `v: 2` 输入返回明确 `UnsupportedVersion` 错误。

**Not changing:** Spine 服务端 (`services/sync-gateway/`) 一行代码不动；`device-pairing` 服务端协议保持原貌；前端 Devices Tab 的现有 UX 行为保持向后兼容（QR 图像照常渲染、复制按钮照常工作）。

## Capabilities

### New Capabilities

- `mobile-pairing-payload`: 定义桌面端为移动扫码场景生成的版本化 JSON QR payload schema（v:1）、字段语义、TTL 约束、与遗留 URI 格式的互操作规则。

### Modified Capabilities

（无服务端 `device-pairing` 行为变更，故不在此列出。）

## Impact

- **Code:** `apps/desktop/src-tauri/src/spine/pairing.rs`、`apps/desktop/src-tauri/src/spine/mod.rs`（如需暴露新结构体）、`apps/desktop/src/components/DevicesTab.tsx`（可选地展示 `qr_payload_json` 文本）。
- **APIs:** Tauri 命令 `spine_pairing_initiate` 的返回类型扩展（新增字段 `qr_payload_json`），前端 TypeScript 类型生成同步更新。
- **Dependencies:** 无新增 Rust crate 依赖；`serde_json` 与 `qrcode` 已在工作区内。
- **Schemas:** JSON payload 仅作为 in-flight 编码，不入库、不入磁盘持久化。
- **Downstream:** 解锁 `desktop-spine-ingestion-dispatch`（US-053）与 `mobile-capture-mvp` 系列后续 OpenSpec change。
- **Risk:** 低。新增字段对老 mobile-不存在 + desktop↔桌面流程零影响；唯一风险是前端 Tauri 类型生成失误导致 build 失败，由 typecheck 把关。
