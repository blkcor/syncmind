# Tasks: Desktop Spine Pairing Payload

## 1. Rust 类型与结构

- [ ] 1.1 在 `apps/desktop/src-tauri/src/spine/pairing.rs` 顶部新增 `MobilePairingPayload` 结构体，字段顺序与 PRD 005 §US-041 一致（`v`, `kind`, `spine_url`, `ca_fingerprint: Option<String>`, `pairing_token`, `expires_at`, `device_a_pubkey`, `device_a_fingerprint`），派生 `Serialize`/`Deserialize`/`Debug`/`Clone`
- [ ] 1.2 给 `v` 字段加 `#[serde(default)]` + 自定义 `deserialize_with` 拒绝非 1 值，错误归一化到 `SpineErrorCode::UnsupportedVersion`
- [ ] 1.3 给 `kind` 字段加常量校验：反序列化时若不等于 `"syncmind-pairing"` 返回 `SpineErrorCode::BadRequest`
- [ ] 1.4 在 `PairingHandleView` 结构体（既有）新增 `qr_payload_json: String` 字段，保持 `qr_png_base64` 不动
- [ ] 1.5 同步更新 Tauri 类型导出（`tauri-specta` / `ts-rs` 二选一，按工程实际）让前端 TS 类型自动包含新字段

## 2. Payload 生成路径

- [ ] 2.1 在 `pairing.rs` 新增 `build_mobile_pairing_payload(config: &SpineConfig, identity: &Identity, server_resp: &PairingInitiateResponse) -> Result<MobilePairingPayload, SpineError>`
- [ ] 2.2 `device_a_pubkey` 字段从 `identity.ed25519_public_key_base64()` 取值（如该 helper 不存在，新增之；签名密钥已存在 OS 钥匙串）
- [ ] 2.3 `device_a_fingerprint` 字段计算：`format!("sha256:{}", hex::encode(Sha256::digest(ed25519_pub_bytes)))`，使用 `lower-hex`
- [ ] 2.4 `pairing_token` 字段直接复用 server_resp 中 `qr_payload` URI 的 `?pk=` 段（即 initiator_pubkey_b64url）；如 URI 解析失败返回 `SpineErrorCode::Internal`
- [ ] 2.5 `spine_url` 字段来自 `config.url`；若 `None` 返回 `SpineErrorCode::SpineNotConfigured`
- [ ] 2.6 `ca_fingerprint` 字段：`config.ca_fingerprint` 直接 `Option<String>` 透传（None → JSON null）
- [ ] 2.7 修改 `initiate()` 函数：在拿到 `resp` 后调用 `build_mobile_pairing_payload`，把 `serde_json::to_string(&payload)?` 作为 QR 内容传给 `render_qr_png_base64`，同时把 JSON 字符串写入 `PairingHandleView::qr_payload_json`

## 3. 反向解析与遗留兼容

- [ ] 3.1 新增 `pub(crate) fn parse_mobile_pairing_payload(input: &str) -> Result<MobilePairingPayload, SpineError>`
- [ ] 3.2 解析顺序：先 `serde_json::from_str::<MobilePairingPayload>(input)`，成功即返回
- [ ] 3.3 JSON 解析失败时若 `input.starts_with("spine://pair/")` 走遗留 URI 分支：用 `url::Url::parse` 解析、严格校验 scheme=`spine`、host=`pair`、path 段非空、query 只允许键 `pk`
- [ ] 3.4 遗留 URI 解析成功后，从本地 `Config.spine` 拉取 `spine_url`/`ca_fingerprint`/local identity 填充 `MobilePairingPayload` 其余字段；`expires_at` 用 "now + 5 min" 作占位（这条路径目前没有 mobile 消费者，主要为单元测试与未来桌面扫码场景准备）
- [ ] 3.5 任何不匹配两种格式的输入返回 `SpineErrorCode::BadRequest`
- [ ] 3.6 解析 v1 JSON 时额外校验 `expires_at` 在过去 60 秒以上即返回 `SpineErrorCode::PairingExpired`

## 4. QR 渲染参数调整

- [ ] 4.1 在 `render_qr_png_base64` 调用点（或 helper 本身）显式设置 ECC level = `Low`，确保 ~350 字符 payload 仍能维持合理密度
- [ ] 4.2 维持现有最小图像尺寸下限（≥ 256×256 px）；若 helper 内部用比例计算，则验证输出 PNG 实际尺寸不小于阈值，否则放大模块尺寸

## 5. 单元测试

- [ ] 5.1 `tests/pairing_payload_roundtrip.rs`（或同目录 `#[cfg(test)] mod tests`）：构造 `MobilePairingPayload` → `serde_json::to_string` → `serde_json::from_str` round-trip 等值断言
- [ ] 5.2 `parse_mobile_pairing_payload` 接受合法 v1 JSON
- [ ] 5.3 `parse_mobile_pairing_payload` 拒绝 `v: 2` 输入，错误码 == `UnsupportedVersion`
- [ ] 5.4 `parse_mobile_pairing_payload` 拒绝 `kind: "syncmind-foo"` 输入，错误码 == `BadRequest`
- [ ] 5.5 `parse_mobile_pairing_payload` 接受合法 `spine://pair/{session}?pk={b64url}` URI
- [ ] 5.6 `parse_mobile_pairing_payload` 拒绝带额外 query key 的 URI（如 `&extra=1`），错误码 == `BadRequest`
- [ ] 5.7 `parse_mobile_pairing_payload` 拒绝 `expires_at` 早于 now-60s 的 v1 JSON，错误码 == `PairingExpired`
- [ ] 5.8 `device_a_fingerprint` 计算与 `SHA-256(decode(device_a_pubkey))` 的 hex 完全一致（含 `sha256:` 前缀、小写）
- [ ] 5.9 `ca_fingerprint=None` 时 JSON 输出包含字面量 `"ca_fingerprint":null`，**不**是 `""` 或缺失

## 6. 前端类型与最低限度的 UI 校验

- [ ] 6.1 重新生成 `apps/desktop/src/bindings/`（或等价目录）的 TS 类型，确认 `PairingHandleView` 含 `qr_payload_json: string`
- [ ] 6.2 `apps/desktop/src/components/DevicesTab.tsx` 不强制使用新字段，但确认编译期未因新字段产生 `unused variable` 警告
- [ ] 6.3 `pnpm --filter @syncmind/desktop typecheck` 通过
- [ ] 6.4 `pnpm --filter @syncmind/desktop lint` 通过

## 7. 安全与日志审计

- [ ] 7.1 grep audit：`rg "qr_payload|pairing_token" apps/desktop/src-tauri/src/` 确认没有 `tracing::*` / `eprintln!` / `println!` 调用把 token 或 pubkey 写入日志
- [ ] 7.2 在 `MobilePairingPayload` 的 `Debug` 实现上（若需）对 `pairing_token` 字段使用脱敏输出（仅前 4 后 4），避免 `dbg!()` 意外泄露
- [ ] 7.3 验证 `qr_payload_json` 不会被任何 `tracing::info!` 直接展开打印

## 8. 端到端验证

- [ ] 8.1 桌面端启动后，Devices Tab "Start pairing" 能正常显示 QR
- [ ] 8.2 用任意手机 QR 扫码 App（如系统相机）解码 QR，得到的字符串能被在线 JSON validator 解析为合法 JSON
- [ ] 8.3 解析出的对象 `v == 1`、`kind == "syncmind-pairing"`、其余 7 个字段均存在
- [ ] 8.4 在 5 分钟内可用 6 位短码完成桌面 ↔ 桌面配对（回归既有路径）
- [ ] 8.5 5 分钟 TTL 到期后，再次扫码进入 mobile 端（pseudo 测试，可用一段手写 Rust 解码代码模拟）应得到 `PairingExpired`

## 9. 工作流收尾

- [ ] 9.1 `cargo clippy --workspace --all-targets -- -D warnings` 通过（限定 desktop crate 与 syncmind-core）
- [ ] 9.2 `cargo test --workspace` 通过
- [ ] 9.3 在 `docs/prd/005-mobile-capture.md` §US-052 的位置加一行注脚链接到本 change（`openspec/changes/desktop-spine-pairing-payload/`）
- [ ] 9.4 开 PR 标题 `feat(apps:desktop): emit JSON QR payload for mobile pairing (PRD 005 §US-052)`，描述链接本 change
- [ ] 9.5 PR 合并后运行 `/opsx:archive desktop-spine-pairing-payload`
