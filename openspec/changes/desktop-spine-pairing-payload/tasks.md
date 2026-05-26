# Tasks: Desktop Spine Pairing Payload

## 1. Rust 类型与结构

- [x] 1.1 在 `apps/desktop/src-tauri/src/spine/pairing.rs` 顶部新增 `MobilePairingPayload` 结构体，字段顺序与 PRD 005 §US-041 一致（`v`, `kind`, `spine_url`, `ca_fingerprint: Option<String>`, `pairing_token`, `expires_at`, `device_a_pubkey`, `device_a_fingerprint`），派生 `Serialize`/`Deserialize`/`Clone`（`Debug` 走自定义 impl 做脱敏，见 §7.2）
- [x] 1.2 `v` 字段用 `u8`；版本不匹配在 `parse_mobile_pairing_payload` 走后置校验，归一到既有 `SpineErrorCode::SchemaVersionUnsupported`（语义等价于规划的 `UnsupportedVersion`，避免新增冗余 enum 变体）
- [x] 1.3 `kind` 字段走后置常量校验：解析后若不等于 `"syncmind-pairing"` 返回 `SpineErrorCode::BadRequest`（新增 enum 变体 + `"BAD_REQUEST"` 字符串）
- [x] 1.4 `PairingHandleView` 新增 `qr_payload_json: String` 字段，保持 `qr_png_base64` 不动
- [x] 1.5 同步更新前端 `apps/desktop/src/components/DevicesTab.tsx` 的手写 TS interface（项目未启用 tauri-specta / ts-rs，类型由手写维护）

## 2. Payload 生成路径

- [x] 2.1 新增 `build_mobile_pairing_payload(config: &SpineConfig, identity: &Identity, resp: &InitiateResponse) -> Result<MobilePairingPayload, SpineError>`（实际函数签名采用项目内既有 `InitiateResponse` 类型）
- [x] 2.2 `device_a_pubkey` 由 `B64URL.encode(identity.public_key_bytes())` 得到（base64url-no-pad，与 `pairing_token` 编码一致，方便 mobile 端复用解码结果）
- [x] 2.3 `device_a_fingerprint = format!("sha256:{}", hex::encode(Sha256::digest(pubkey_bytes)))` —— 单元测试 `fingerprint_matches_sha256_of_decoded_pubkey` 锁定行为
- [x] 2.4 `pairing_token` 由 `extract_pk_from_qr_payload(&resp.qr_payload)` 解析 `?pk=` 段；URI 异常返回 `SpineErrorCode::Internal`
- [x] 2.5 `spine_url` 来自 `config.url`；缺失返回 `SpineErrorCode::SpineNotConfigured`
- [x] 2.6 `ca_fingerprint`：若 `config.trust_ca_path` 设置且可读，计算 PEM 第一段 CERTIFICATE 的 `sha256:<hex>`；否则 `None`（透传为 JSON `null`）。读取或解析失败仅 warn-log，不阻塞 payload 生成
- [x] 2.7 `initiate()` 在拿到 `resp` 后调用 `build_mobile_pairing_payload`，把 `serde_json::to_string(&payload)` 同时写入 QR 图与 `qr_payload_json` 字段；签名变为 `initiate(client, identity, config)`，更新 `spine_start_pairing` 命令传入 `config.spine` 快照

## 3. 反向解析与遗留兼容

- [x] 3.1 新增 `pub(crate) fn parse_mobile_pairing_payload(input: &str) -> Result<MobilePairingPayload, SpineError>`（+ `_at(now)` 时钟可注入测试 helper）
- [x] 3.2 解析顺序：先 `serde_json::from_str::<MobilePairingPayload>(input)`，成功即走 `validate_payload`
- [x] 3.3 JSON 解析失败时若 `input.starts_with("spine://pair/")` 走 `parse_legacy_uri`：严格校验 scheme=`spine`、host=`pair`、path 非空、query 只允许键 `pk`
- [x] 3.4 遗留 URI 解析成功后构造 `MobilePairingPayload` 占位：`spine_url`/`ca_fingerprint`/`device_a_fingerprint` 为空字符串/`None`，`expires_at` 用 `now + 5 min`，`device_a_pubkey` 与 `pairing_token` 同值（仅供单元测试与未来桌面扫码场景）
- [x] 3.5 任何不匹配两种格式的输入返回 `SpineErrorCode::BadRequest`
- [x] 3.6 解析 v1 JSON 时通过 `validate_payload` 校验 `expires_at`：解析 RFC3339 → 容忍 `EXPIRY_SKEW = 60s` → 过期返回 `SpineErrorCode::PairingExpired`

## 4. QR 渲染参数调整

- [x] 4.1 `render_qr_png_base64` 显式切到 `qrcode::EcLevel::L`，能容纳 ~350 字符 payload
- [x] 4.2 module scale 改用 `div_ceil`（替代原 `/`），保证 `side >= QR_PNG_SIDE_PX = 320`；新增 `render_qr_png_handles_full_json_payload` 测试验证 200+ 字符 payload 仍能渲染

## 5. 单元测试

- [x] 5.1 `payload_roundtrip_json`
- [x] 5.2 `parse_accepts_valid_v1_json`
- [x] 5.3 `parse_rejects_unsupported_version`（断言 code == `SCHEMA_VERSION_UNSUPPORTED`）
- [x] 5.4 `parse_rejects_unknown_kind`
- [x] 5.5 `parse_accepts_legacy_uri`
- [x] 5.6 `parse_rejects_uri_with_extra_query`（+ `parse_rejects_uri_without_pk` / `parse_rejects_uri_without_session` 防呆）
- [x] 5.7 `parse_rejects_expired_payload` + `parse_accepts_payload_within_clock_skew`
- [x] 5.8 `fingerprint_matches_sha256_of_decoded_pubkey`（含 lowercase + `sha256:` prefix 断言）
- [x] 5.9 `null_ca_fingerprint_serializes_as_json_null`

补充测试：`extract_pk_from_qr_payload_*`、`redact_token_*`、`debug_impl_redacts_pairing_token`、`parse_rejects_unrelated_input`。

最终 `cargo test --lib spine`：**66 passed; 0 failed**。

## 6. 前端类型与最低限度的 UI 校验

- [x] 6.1 项目内无 `apps/desktop/src/bindings/`；TS 类型由 `DevicesTab.tsx` 手写维护，已同步添加 `qr_payload_json: string`
- [x] 6.2 `DevicesTab.tsx` 当前不读取 `qr_payload_json`，未引入 unused-var 警告
- [x] 6.3 `pnpm exec tsc --noEmit` 在 `apps/desktop` 通过（无输出 == 无错）
- [x] 6.4 `pnpm exec eslint src/components/DevicesTab.tsx --max-warnings 0` 通过；仓库 `pnpm lint` 整体仍因 **预存在** `RagLabTab.tsx:171` 的 `solid/reactivity` 警告失败（与本 change 无关，主仓 main 上同样未通过）

## 7. 安全与日志审计

- [x] 7.1 `rg "qr_payload|pairing_token" apps/desktop/src-tauri/src/`：没有 `tracing::*` / `eprintln!` / `println!` 把 token 或 pubkey 打到日志，唯一引用都是字段访问或错误构造的上下文字符串（不含值）
- [x] 7.2 `MobilePairingPayload` 实现自定义 `Debug`：`pairing_token` 走 `redact_token`（前 4 + `…` + 后 4），`device_a_pubkey` 同样脱敏；测试 `debug_impl_redacts_pairing_token` 断言原值不出现在 `{:?}` 输出
- [x] 7.3 `qr_payload_json` 没有出现在任何 `tracing::info!` / `dbg!` 调用中

## 8. 端到端验证

> 以下需要 `docker compose up` 起 sync-gateway + 真机扫码，留待具备实机条件时执行。本 PR 内已用代码层等价测试（§5）覆盖每条 AC 的行为契约。

- [ ] 8.1 桌面端启动后，Devices Tab "Start pairing" 能正常显示 QR（人工，待真机验证）
- [ ] 8.2 用任意手机 QR 扫码 App（如系统相机）解码 QR，得到字符串可被 JSON validator 解析（人工，待真机验证）
- [ ] 8.3 解析出的对象 `v == 1` / `kind == "syncmind-pairing"` / 其余字段齐全（人工，等价代码测试见 `payload_roundtrip_json`）
- [ ] 8.4 5 分钟内 6 位短码完成桌面 ↔ 桌面配对（既有路径回归，人工）
- [ ] 8.5 TTL 过期场景（等价单元测试已通过：`parse_rejects_expired_payload`）

## 9. 工作流收尾

- [x] 9.1 `cargo clippy --all-targets -- -D warnings` 在 `apps/desktop/src-tauri` 通过
- [x] 9.2 `cargo test --workspace` 在 `core/` 全部通过（含 desktop 68 个测试）
- [x] 9.3 在 `docs/prd/005-mobile-capture.md` §US-052 加注脚链接到本 change
- [ ] 9.4 开 PR（紧随本提交执行）
- [ ] 9.5 PR 合并后 `/opsx:archive desktop-spine-pairing-payload`
