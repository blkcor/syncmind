# PRD: The Spine — Cross-Device Sync Gateway (Phase 3)

## Introduction

SyncMind Phase 3 的核心目标是构建 **The Spine**：一个去中心化、隐私优先的跨设备同步网关。它作为 SyncMind 生态系统的"云端脊柱"，承担以下核心职责：

1. **设备配对与密钥交换：** 为移动端与桌面端提供安全的配对通道，通过 QR 码或短码完成 X25519 ECDH 密钥交换，**无需任何用户账户系统**。
2. **盲中继 (Blind Relay)：** 作为端到端加密 (E2EE) 的中继节点，Spine **永远无法访问任何明文数据**。它仅存储加密后的数据包 (SyncBundle) 和元数据，负责在设备间进行可靠的路由与投递。
3. **移动端素材接入：** 接收来自移动端的加密语音、图像等原始素材，将其作为不透明加密 Blob 暂存，并通知桌面端拉取。

The Spine 是 SyncMind 从"单设备本地引擎"迈向"多端协同知识网络"的关键基础设施。它与 Phase 1 的 Rust Core 完全解耦：桌面端 Core 负责本地索引与推理，Spine 只负责安全的数据搬运。

## Goals

- 建立一个零信任、无账户的跨设备同步网关，Spine 运营方（甚至自托管用户自身）无法解密传输内容。
- 实现基于 X25519 + AES-256-GCM 的端到端加密同步协议。
- 支持移动端加密上传素材，桌面端异步拉取并注入本地 RAG 管道。
- 提供可自托管的 Docker Compose 部署方案，内存占用 < 200MB。
- 所有代码、配置、文档遵循 Spec-Driven 与 Privacy-is-Absolute 原则。

## User Stories

### US-010: Go 服务脚手架与配置系统
**Description:** 作为开发者，我需要稳定的 Go 微服务结构和配置系统，以便后续功能模块化开发。

**Acceptance Criteria:**
- [ ] 在 `services/sync-gateway/` 下初始化 Go 模块（`go.mod`），模块名 `github.com/blkcor/syncmind/spine`。
- [ ] 引入 `go-zero` 作为服务脚手架（`core/conf`, `rest`, `zrpc` 等）。
- [ ] 引入 `hertz` 作为高性能 HTTP/WebSocket 服务器（`github.com/cloudwego/hertz`）。
- [ ] 配置加载：支持从环境变量和 `spine.yaml` 读取。
- [ ] 配置 Schema 至少包含：
  - `database_url` (PostgreSQL DSN)
  - `redis_addr`
  - `bind_addr` (HTTP 服务监听地址，默认 `:8080`)
  - `tls_cert`, `tls_key` (mTLS/TLS 证书路径)
  - `pairing_session_ttl` (配对会话有效期，默认 5 分钟)
  - `bundle_retention_days` (同步包保留天数，默认 30 天)
  - `max_bundle_size_mb` (最大单包大小，默认 50MB)
  - `jwt_issuer`, `jwt_audience`
- [ ] 提供健康检查端点 `GET /health`。
- [ ] `go build` / `go vet` 通过。

### US-011: 数据库 Schema 与迁移
**Description:** 作为系统，我需要持久化存储设备元数据、加密同步包和配对会话，并支持 Schema 版本管理。

**Acceptance Criteria:**
- [ ] 使用 `golang-migrate/migrate` 或 `pressly/goose` 管理 PostgreSQL 迁移脚本，存放在 `services/sync-gateway/migrations/`。
- [ ] Schema V1 设计：
  - `devices` 表：
    - `id` UUID PRIMARY KEY
    - `public_key_fingerprint` VARCHAR(64) UNIQUE NOT NULL (SHA-256 公钥指纹)
    - `public_key` BYTEA NOT NULL (原始 Ed25519 公钥)
    - `paired_device_id` UUID NULL (指向另一台配对设备)
    - `device_type` VARCHAR(20) CHECK (`desktop`, `mobile`)
    - `created_at`, `last_seen_at` TIMESTAMPTZ
    - `is_active` BOOLEAN DEFAULT TRUE
  - `pairing_sessions` 表：
    - `id` UUID PRIMARY KEY
    - `initiator_device_id` UUID NOT NULL
    - `initiator_pubkey` BYTEA NOT NULL (临时 X25519 公钥)
    - `responder_pubkey` BYTEA NULL (响应方 X25519 公钥)
    - `status` VARCHAR(20) CHECK (`pending`, `completed`, `expired`, `cancelled`)
    - `expires_at` TIMESTAMPTZ NOT NULL
    - `created_at` TIMESTAMPTZ DEFAULT NOW()
  - `sync_bundles` 表：
    - `id` UUID PRIMARY KEY
    - `from_device_id` UUID NOT NULL
    - `to_device_id` UUID NOT NULL
    - `encrypted_payload` BYTEA NOT NULL (AES-256-GCM 加密后的数据)
    - `payload_hash` VARCHAR(64) NOT NULL (SHA-256 密文哈希，用于完整性校验)
    - `payload_size_bytes` INT NOT NULL
    - `content_type` VARCHAR(50) DEFAULT `application/octet-stream` (如 `text/markdown`, `image/jpeg`)
    - `created_at`, `expires_at` TIMESTAMPTZ
    - `acked_at` TIMESTAMPTZ NULL (确认接收时间)
    - `deleted_at` TIMESTAMPTZ NULL (软删除)
- [ ] 创建必要索引：
  - `idx_sync_bundles_to_device_acked` (`to_device_id`, `acked_at`)
  - `idx_pairing_sessions_expires` (`expires_at`) (用于清理过期会话)
- [ ] 编写 `make migrate-up` / `make migrate-down` 命令。
- [ ] `go test` 通过（至少包含 Schema 创建的集成测试）。

### US-012: 设备配对 (QR 码 / X25519 ECDH)
**Description:** 作为用户，我希望通过扫描桌面端显示的 QR 码，将我的手机与电脑安全配对，无需注册账户或输入密码。

**Acceptance Criteria:**
- [ ] 桌面端调用 `POST /v1/pairing/initiate`，传入其临时 X25519 公钥。
- [ ] 服务端生成 `pairing_session` 记录，状态为 `pending`，返回 `{session_id, qr_payload}`。
  - `qr_payload` 结构：`spine://pair/{session_id}?pk={base64url_initiator_pubkey}`
- [ ] 服务端自动生成短码（6 位数字，如 `482-193`）作为 QR 扫描失败时的手动输入备选。
- [ ] 移动端扫描 QR 后，调用 `POST /v1/pairing/complete`，传入 `session_id` 和其临时 X25519 公钥。
- [ ] 服务端验证会话未过期，将 `responder_pubkey` 写入记录，状态变为 `completed`。
- [ ] 配对完成后，**双方设备各自在本地**通过 X25519 ECDH 计算共享对称密钥：
  - `shared_secret = X25519(my_private_key, peer_public_key)`
  - `sync_key = HKDF-SHA256(shared_secret, salt=session_id, info="syncmind-v1")`
- [ ] 服务端在 `devices` 表中注册两台设备，互相记录 `paired_device_id`。
- [ ] 提供 `GET /v1/pairing/:session_id/status` 供轮询状态。
- [ ] 配对会话过期后自动清理（Cron job 或基于 `expires_at` 的惰性删除）。
- [ ] 单元测试覆盖完整的配对流程（密钥交换逻辑可用测试桩模拟）。

### US-013: 设备认证 (Ed25519 JWT)
**Description:** 作为系统，我需要确保只有已配对的合法设备才能上传或下载同步数据。

**Acceptance Criteria:**
- [ ] 每台设备在本地生成持久的 Ed25519 身份密钥对（与临时的 X25519 配对密钥分离）。
- [ ] 设备在注册时（配对完成时）将 Ed25519 公钥提交给 Spine，存入 `devices.public_key`。
- [ ] 设备每次请求时，在 HTTP Header `Authorization: Bearer <jwt>` 中携带 JWT。
  - JWT 使用设备的 Ed25519 私钥签名。
  - Claims 至少包含：`sub` (device_id), `iat`, `exp` (有效期 ≤ 24h), `jti` (防止重放)。
- [ ] Spine 中间件验证 JWT：
  - 解析 `sub` 获取 device_id。
  - 从数据库加载对应设备的 `public_key`。
  - 使用 Ed25519 公钥验证签名。
  - 检查 `exp` 和 `jti` 未重放（可选 Redis 缓存 `jti` 黑名单）。
- [ ] 认证失败返回 `401 Unauthorized`，不泄露设备是否存在。
- [ ] WebSocket 连接升级时同样执行 JWT 验证（通过 `Sec-WebSocket-Protocol` 子协议头或 Query Param 传递 token）。
- [ ] `go test` 覆盖认证中间件。

### US-014: 同步包上传与存储
**Description:** 作为桌面端 Rust Core，我需要将本地新生成的知识块（加密后）上传到 Spine，以便同步到移动端。

**Acceptance Criteria:**
- [ ] 实现 `POST /v1/sync/bundle` 端点。
  - 请求体为 `multipart/form-data` 或直接 `application/octet-stream`（需确定）。
  - 请求头必须包含 `X-Syncmind-Content-Type`（明文的内容类型，如 `text/markdown`）。
  - 单包大小受 `max_bundle_size_mb` 限制，超限返回 `413 Payload Too Large`。
- [ ] 服务端验证 `from_device_id` 与 JWT 中的 `sub` 一致。
- [ ] 服务端查询 `devices` 表，确认 `from_device_id` 的 `paired_device_id` 存在且 `is_active = TRUE`。
- [ ] 将加密 Payload 写入 PostgreSQL `sync_bundles` 表。
  - `payload_hash` 在服务端计算（SHA-256 of the encrypted blob），防止传输损坏。
  - `expires_at = NOW() + bundle_retention_days`。
- [ ] 写入成功后，通过 Redis Pub/Sub 向 `to_device_id` 对应的通道发布 `SyncNotification`：
  ```json
  {
    "type": "new_bundle",
    "bundle_id": "uuid",
    "from_device": "uuid",
    "payload_size": 12345,
    "content_type": "text/markdown"
  }
  ```
- [ ] 返回 `201 Created`，响应体包含 `bundle_id`。
- [ ] 提供幂等性支持：客户端可通过 `Idempotency-Key` 头避免重复上传。

### US-015: 实时同步通知 (WebSocket + Redis)
**Description:** 作为桌面端，我希望在手机上传新笔记后能立即收到通知，而不是被动轮询。

**Acceptance Criteria:**
- [ ] 实现 `WS /v1/sync/live` 端点（Hertz WebSocket Upgrade）。
- [ ] 连接建立时验证 JWT，并将 `device_id` 与 WebSocket Conn 关联。
- [ ] 维护一个内存中的映射：`device_id -> []WebSocketConn`（支持单设备多连接）。
- [ ] 启动一个 Redis Subscriber Goroutine，订阅 `sync:notify:{device_id}` 通道。
- [ ] 当收到 Redis 消息时，遍历该 device 的所有 WebSocket 连接，推送 JSON 通知。
- [ ] 实现心跳机制：服务端每 30 秒发送 `{"type":"ping"}`，客户端需在 10 秒内回复 `pong`，否则断开连接。
- [ ] 连接断开时清理内存映射，避免 Goroutine 泄漏。
- [ ] 移动端离线时，Redis 消息在通道中不保留（简化设计）；设备重连后通过 `GET /v1/sync/bundles` 拉取未读包。
- [ ] 压力测试：单 Spine 实例支持至少 10,000 并发 WebSocket 连接。

### US-016: 移动端素材上传 (加密 Blob)
**Description:** 作为移动端用户，我希望快速拍摄一张照片或录制一段语音，加密后上传到 Spine，随后在我的桌面端知识库中看到它。

**Acceptance Criteria:**
- [ ] 实现 `POST /v1/media/upload` 端点。
  - 请求体为 `multipart/form-data`：`file` (加密后的 Blob) + `metadata` (JSON，包含明文内容类型、文件名、文件大小)。
  - 接受的内容类型：`image/jpeg`, `image/png`, `image/heic`, `audio/m4a`, `audio/wav`。
- [ ] 文件大小限制同 `max_bundle_size_mb`。
- [ ] 服务端将加密 Blob 视为特殊的 `sync_bundles` 记录，其中：
  - `content_type` 为客户端声明的原始类型（如 `image/jpeg`）。
  - `encrypted_payload` 为文件内容。
  - 插入时 `to_device_id` 为配对桌面端的 ID。
- [ ] 上传完成后立即触发 Redis 通知。
- [ ] 移动端获得响应 `{media_id, bundle_id, expires_at}`。
- [ ] **Phase 3 中 Spine 不执行任何内容处理**（无缩略图生成、无 OCR、无语音转文字）。素材保持加密状态，等待桌面端下载后处理。

### US-017: 同步包下载与确认
**Description:** 作为桌面端 Rust Core，我需要拉取 Spine 上暂存的加密同步包，本地解密后注入索引管道，然后通知 Spine 可以删除该包。

**Acceptance Criteria:**
- [ ] 实现 `GET /v1/sync/bundles` 端点。
  - 查询参数：`?limit=20&before_id=<uuid>`（分页）。
  - 仅返回 `to_device_id = 当前设备` 且 `acked_at IS NULL` 的记录。
  - 返回 JSON 数组，**不包含 `encrypted_payload`**（仅返回元数据列表）：
    ```json
    [
      {
        "bundle_id": "uuid",
        "from_device": "uuid",
        "payload_size": 12345,
        "content_type": "text/markdown",
        "created_at": "2026-01-01T00:00:00Z",
        "payload_hash": "sha256..."
      }
    ]
    ```
- [ ] 实现 `GET /v1/sync/bundles/:id` 端点。
  - 验证当前设备的 JWT `sub` 等于 `to_device_id`。
  - 返回完整的加密 Payload（`Content-Type: application/octet-stream`）。
  - 响应头包含 `X-Syncmind-Content-Type`（原始内容类型）。
  - 响应头包含 `X-Syncmind-Payload-Hash`（供客户端校验）。
- [ ] 实现 `DELETE /v1/sync/bundles/:id` 端点。
  - 验证权限后，将 `acked_at` 设为当前时间（软删除）。
  - 可选：真正的物理删除由后台 Cron 在 `bundle_retention_days` 后执行。
- [ ] 桌面端下载流程：
  1. 收到 WebSocket 通知或轮询到列表。
  2. 逐个调用 `GET /v1/sync/bundles/:id` 下载。
  3. 本地使用 `sync_key` 解密 Payload。
  4. 验证 SHA-256 哈希。
  5. 将解密后的内容注入本地 Rust Core 的索引管道。
  6. 调用 `DELETE` 确认。

### US-018: Docker Compose 部署与可观测性
**Description:** 作为自托管用户，我希望能通过一条命令启动 Spine 及其依赖（PostgreSQL + Redis）。

**Acceptance Criteria:**
- [ ] 在 `services/sync-gateway/` 下提供 `docker-compose.yml`：
  - `spine` 服务：基于 `golang:1.24-alpine` 多阶段构建，暴露 8080 端口。
  - `postgres` 服务：`postgres:17-alpine`，持久化卷 `spine_pg_data`。
  - `redis` 服务：`redis:7-alpine`。
- [ ] 提供 `Makefile` 包含以下命令：
  - `make build` — 构建 Go 二进制。
  - `make migrate-up` — 执行数据库迁移。
  - `make dev` — 启动 docker-compose 本地开发环境。
  - `make test` — 运行单元测试与集成测试。
- [ ] 集成 `go-zero` 内置的 Prometheus metrics 端点 `/metrics`。
- [ ] 关键指标暴露：
  - `spine_sync_bundles_total` (Counter) — 上传总包数。
  - `spine_sync_bundle_size_bytes` (Histogram) — 包大小分布。
  - `spine_active_websockets` (Gauge) — 当前 WebSocket 连接数。
  - `spine_pairing_sessions_total` (Counter) — 配对请求数（按状态 label）。
- [ ] 使用 `zap` 进行结构化日志输出，包含 `trace_id` 以便跨服务追踪。
- [ ] 提供一份 `docs/examples/spine-docker-compose.md` 快速部署指南。

## Functional Requirements

- **FR-11:** 系统必须支持通过 QR 码或 6 位短码完成设备配对，**不允许任何用户名/密码账户系统**。
- **FR-12:** 系统必须在配对过程中使用 X25519 ECDH 交换临时公钥，**Spine 不得存储或访问任何私钥或派生的共享对称密钥**。
- **FR-13:** 系统必须对同步包 Payload 使用 AES-256-GCM 加密，且密钥完全由客户端在本地派生和管理。
- **FR-14:** 系统必须通过 Ed25519 签名 JWT 认证所有设备请求，Spine 仅验证签名，不持有任何设备私钥。
- **FR-15:** 系统必须将加密后的同步包存储在 PostgreSQL 中，元数据（`from_device`, `to_device`, `payload_hash`, `content_type`）与密文分离存储，但 Spine 不得尝试解析密文。
- **FR-16:** 系统必须通过 Redis Pub/Sub + WebSocket 向目标设备推送实时同步通知，确保低延迟（目标 < 2s）。
- **FR-17:** 系统必须支持移动端上传加密图像/音频 Blob，Spine 仅作为不透明存储，**不得进行任何内容分析、OCR 或缩略图生成**。
- **FR-18:** 系统必须提供分页拉取未确认同步包的 API，并支持客户端通过确认 (ACK) 机制安全删除已处理的包。
- **FR-19:** 系统必须提供自托管的 Docker Compose 部署方案，包含 PostgreSQL、Redis 和 Spine 服务。
- **FR-20:** 系统必须保证 Spine 进程本身、日志输出和数据库转储中**不包含任何用户明文数据或可用于解密的密钥材料**。

## Non-Goals (Out of Scope)

- **NG-8:** 不实现任何用户名/密码注册、OAuth 或第三方登录系统。SyncMind 的哲学是"零账户"。
- **NG-9:** Spine 不执行任何内容处理（OCR、语音识别、Embedding 生成、格式转换）。所有内容处理由桌面端 Rust Core 在解密后完成。
- **NG-10:** 不实现复杂的多跳同步（如 A → B → C 的链式同步）。Phase 3 仅支持一对一设备配对（一台桌面 + 一台移动端）。
- **NG-11:** 不实现离线消息队列的持久化重传。如果目标设备长期离线，Spine 仅在保留期内保存包，超期自动清理。
- **NG-12:** 不实现冲突解决 (Conflict Resolution) 算法。如果两端同时修改同一份知识，Spine 传递两份加密包，由桌面端 Rust Core 在应用层解决冲突。
- **NG-13:** 不实现 NAT 穿透或 P2P 直连。所有流量必须通过 Spine 中继（这是设计选择，为了简化防火墙/NAT 兼容性）。
- **NG-14:** 不实现多用户隔离或 RBAC 权限系统。Phase 3 假设 Spine 实例仅服务于一个用户的一对设备。

## Design Considerations

- **模块划分:** `services/sync-gateway/` 内部按职责拆分为：
  - `cmd/` — 主入口与 CLI 命令。
  - `internal/config/` — 配置解析与校验。
  - `internal/handler/` — Hertz HTTP 处理器（REST + WebSocket）。
  - `internal/logic/` — 业务逻辑层（go-zero 风格）。
  - `internal/model/` — 数据模型与数据库操作（go-zero `sqlx` / `gorm`)。
  - `internal/middleware/` — JWT 认证、请求日志、限流。
  - `internal/pkg/crypto/` — Ed25519 / X25519 辅助函数（注意：Spine 仅做验证，不做密钥派生）。
  - `internal/pkg/websocket/` — WebSocket Hub 管理（连接注册、广播、心跳）。
  - `internal/scheduler/` — 定时任务（清理过期配对会话、清理已 ACK 的 Bundle）。
- **错误处理:** 使用 `go-zero` 的 `xerr` 或标准 `errors` + `fmt.Errorf("%w")`。对外返回统一的 JSON 错误体：`{ "code": 10001, "message": "..." }`。
- **并发模型:** HTTP  handler 使用 Hertz 的 Goroutine-per-request 模型。WebSocket Hub 使用单独的 Goroutine + Channel 管理连接。Redis Subscriber 使用独立 Goroutine。
- **限流与防护:**
  - 单设备上传频率限制：每分钟最多 100 个 Bundle。
  - 全局配对请求限流：每分钟最多 20 次（防爆破）。
  - WebSocket 单 IP 最大连接数限制。
- **数据清理:**
  - 过期配对会话：每 5 分钟清理一次 `expires_at < NOW()` 的记录。
  - 已确认 Bundle：每天凌晨物理删除 `acked_at IS NOT NULL AND deleted_at < NOW() - INTERVAL '7 days'` 的记录。
  - 未确认但超期 Bundle：物理删除 `expires_at < NOW()` 的记录。
- **日志安全:** 所有日志必须过滤敏感字段（如 `encrypted_payload` 不得出现在日志中，即使被截断）。

## Technical Considerations

- **Go 依赖选型:**
  - Service Scaffold: `github.com/zeromicro/go-zero` (core, rest)
  - HTTP/WebSocket: `github.com/cloudwego/hertz`
  - Database: `github.com/jackc/pgx/v5` (PostgreSQL driver) + `github.com/pressly/goose` (migrations)
  - ORM / SQL Builder: `github.com/zeromicro/go-zero/core/stores/sqlx` 或 `github.com/uptrace/bun`
  - Redis: `github.com/redis/go-redis/v9`
  - Crypto: `golang.org/x/crypto/curve25519`, `golang.org/x/crypto/ed25519`, `golang.org/x/crypto/hkdf`
  - JWT: `github.com/golang-jwt/jwt/v5` (支持 Ed25519)
  - Config: `github.com/zeromicro/go-zero/core/conf` (YAML)
  - Logging: `github.com/zeromicro/go-zero/core/logx` 或 `go.uber.org/zap`
  - Metrics: `github.com/prometheus/client_golang/prometheus` (go-zero 内置)
- **PostgreSQL 性能:**
  - `sync_bundles` 表可能积累大量加密 Blob。建议对频繁查询的元数据列加索引，但 `encrypted_payload` 不加索引。
  - 大 Bundle（如 50MB 图片）直接存入 PostgreSQL `BYTEA` 是否合适？Phase 3 假设是。若未来扩展到百 MB 级文件，需迁移至对象存储（如 MinIO）。
- **WebSocket 扩展性:**
  - 单实例 Hertz 可支撑数万连接。若需水平扩展，Redis Pub/Sub 天然的广播特性支持多 Spine 实例。每个实例订阅 `sync:notify:*`，收到消息后推送到本地维护的 WebSocket 连接。
- **TLS / mTLS:**
  - 默认要求 TLS 1.3。自托管用户可使用 Let's Encrypt 或自签名证书。
  - 可选 mTLS：设备客户端证书由用户在本地 CA 签发，Spine 校验客户端证书中的 device_id。
- **E2EE 审计点:**
  - 代码审查必须确认：任何代码路径不得将 `encrypted_payload` 传入解密函数、字符串解析函数或日志输出。
  - 数据库备份（`pg_dump`）应仅包含密文，审查者可通过搜索 `encrypt`/`decrypt` 关键字确认无解密逻辑。

## Success Metrics

- **安全合规:** 通过代码审计，确认 Spine 源码中无任何明文数据访问路径，无硬编码密钥。
- **配对体验:** 桌面端展示 QR 码 → 手机扫描 → 配对完成，整个流程在 10 秒内完成（局域网环境下）。
- **同步延迟:** 移动端上传加密笔记 → Spine 接收 → Redis 通知 → 桌面端 WebSocket 收到通知，端到端延迟 < 2 秒（同一 Region）。
- **资源占用:** Spine 进程在 1000 对活跃设备、10,000 并发 WebSocket 连接下，内存占用 < 1GB；空闲时（无连接）< 200MB。
- **稳定性:** 设备频繁重连（如网络切换）时，Spine 不泄露 Goroutine，WebSocket Hub 连接映射最终一致性。

## Implementation Notes & Divergences

> 本节记录实际实现与 PRD 原始设计之间的差异，便于后续维护者理解设计决策。

### 1. 配对会话存储的是 Ed25519 公钥，而非 X25519
- **PRD 原设计 (US-011 / US-012):** `pairing_sessions.initiator_pubkey` 和 `responder_pubkey` 被描述为"临时 X25519 公钥"。
- **实际实现:** 配对会话存储的是设备的**持久 Ed25519 身份公钥**。X25519 ECDH 交换及 `sync_key` 派生（`HKDF-SHA256`）完全在客户端本地完成，Spine 服务端不接触任何 X25519 密钥或派生出的对称密钥。
- **原因:** 这更符合 FR-12 的要求（Spine 不得存储或访问任何私钥或派生的共享对称密钥），同时简化了服务端逻辑。Ed25519 公钥在配对完成后直接写入 `devices` 表作为设备身份标识。

### 2. Go-Zero 使用范围缩小
- **PRD 原设计 (US-010 / US-018):** 计划引入 `go-zero` 作为完整服务脚手架，包括 `rest`、`zrpc` 等组件。
- **实际实现:** 仅使用 `github.com/zeromicro/go-zero/core/conf` 进行 YAML 配置加载。HTTP/WebSocket 服务器直接使用 **Hertz**（`github.com/cloudwego/hertz`），未使用 go-zero 的 REST 封装或 handler 模式。
- **原因:** Hertz 本身已提供完整的高性能 HTTP/WebSocket 能力，叠加 go-zero REST 层增加不必要的抽象。

### 3. 日志与指标选型
- **PRD 原设计 (US-018):** 建议 `go-zero/core/logx` 和 go-zero 内置 Prometheus 指标。
- **实际实现:**
  - 日志使用 `go.uber.org/zap`，通过 `internal/logger` 包封装，支持 `trace_id` 和 `device_id` 注入。
  - 指标使用 `prometheus/client_golang` 直接注册（`promauto`），暴露 `/metrics` 端点时通过 `expfmt` 直接编码（因 Hertz 的 `RequestContext` 不兼容 `http.ResponseWriter`，无法使用 `promhttp`）。

### 4. 同步包上传格式确定为 `application/octet-stream`
- **PRD 原设计 (US-014):** 请求体格式标注为"`multipart/form-data` 或直接 `application/octet-stream`（需确定）"。
- **实际实现:** 同步包上传 (`POST /v1/sync/bundle`) 使用 `application/octet-stream`，请求体为原始加密字节流。`multipart/form-data` 仅用于移动端素材上传 (`POST /v1/media/upload`)。

### 5. WebSocket 心跳参数调整
- **PRD 原设计 (US-015):** "服务端每 30 秒发送 `ping`，客户端需在 **10 秒内**回复 `pong`，否则断开"。
- **实际实现:** 服务端每 30 秒发送 `{"type":"ping"}`，WebSocket 读取超时 (`SetReadDeadline`) 设为 **40 秒**。这意味着客户端有最多 40 秒窗口回复 `{"type":"pong"}`。
- **原因:** 10 秒在弱网环境下过于严格，40 秒与 ping 间隔配合可容忍一次丢包。

### 6. 错误码为字符串而非数字
- **PRD 原设计 (Design Considerations):** "对外返回统一的 JSON 错误体：`{ "code": 10001, "message": "..." }`"
- **实际实现:** 错误码为语义化字符串，如 `"INVALID_REQUEST"`、`"AUTH_INVALID"`、`"RATE_LIMITED"`。
- **原因:** 字符串错误码在日志和客户端处理中更具可读性，无需维护数字码表。

### 7. `last_seen_at` 字段更新（已修复）
- **PRD 原设计 (US-011):** `devices` 表包含 `last_seen_at` 字段。
- **修复:** `AuthMiddleware` 在 JWT 验证成功后，通过**异步 goroutine**调用 `deviceStore.UpdateLastSeen(ctx, deviceID)` 更新该字段，确保认证延迟不受写操作影响。
- **文件:** `internal/middleware/auth.go`（异步更新逻辑）、`internal/model/device.go`（`UpdateLastSeen` 方法）。

### 8. JWT `jti` 黑名单检查与撤销端点（已修复）
- **PRD 原设计 (US-013):** "检查 `jti` 未重放（可选 Redis 缓存 `jti` 黑名单）"。
- **修复:**
  - `AuthMiddleware` 在验证成功后，将 `jti` 和 `exp` 存入 Hertz 请求上下文，供下游处理器使用。
  - 新增 `POST /v1/auth/revoke` 端点（`internal/handler/auth.go`），设备可主动撤销当前 JWT：
    - 从上下文读取 `jti`。
    - 根据 `exp` 计算令牌剩余生命周期，在 Redis 中写入 `jwt:blacklist:{jti}` 并设置对应 TTL。
    - 返回 `204 No Content`。
  - 被撤销的令牌再次使用时，`AuthMiddleware` 检测到黑名单条目，返回 `401 AUTH_REPLAYED`。
- **文件:** `internal/middleware/auth.go`（`jti`/`exp` 上下文存储）、`internal/handler/auth.go`（`AuthHandler.Revoke`）、`cmd/spine/main.go`（路由注册）。

### 9. 未实现 WebSocket 单 IP 连接数限制
- **PRD 原设计 (Design Considerations):** 提到 "WebSocket 单 IP 最大连接数限制"。
- **实际实现:** 未实现。当前仅通过 JWT 认证确保连接合法性。
- **原因:** Phase 3 为单用户一对一配对场景，单个 IP 的恶意多连接风险较低。

### 10. 无 `internal/logic/` 层
- **PRD 原设计 (Design Considerations):** 目录结构包含 `internal/logic/` 业务逻辑层。
- **实际实现:** 业务逻辑直接位于 `internal/handler/` 和 `internal/model/` 中，未引入单独的 `logic/` 层。
- **原因:** 当前业务复杂度较低，handler + model 两层已足够，避免过度分层。

## Open Questions

1. **Bundle 存储上限:** 如果用户长期不打开桌面端，移动端持续上传，PostgreSQL 是否会无限增长？是否需要设置单用户 Bundle 数量上限或总存储上限？
2. **前向保密 (Forward Secrecy):** 当前设计使用固定的 `sync_key`。如果某台设备的私钥在未来被泄露，历史同步包是否会被解密？是否需要引入每 Bundle 使用独立密钥的 Double Ratchet 机制？
3. **多端扩展:** Phase 3 限定一对一配对。如果未来需要一台桌面 + 多台移动端（或反之），当前的 `paired_device_id` 一对一关系是否需要重构为设备组 (Device Group) 模型？
4. **移动端离线队列:** 如果 Spine 不可达，移动端是否需要在本地缓存待上传的 Bundle？缓存大小上限和清理策略如何设计？
5. **NAT 与自托管可访问性:** 自托管用户的家用路由器通常无公网 IP。是否需要集成 Tailscale/Headscale 或提供反向隧道（如 FRP）作为 Spine 的可选部署模式？
