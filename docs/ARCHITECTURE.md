# Auto.ml v2 技术架构

产品流程见 `PRODUCT-FLOWS.md`；迭代拆解与横切铁律见 `V2-DESIGN.md`（铁律不在本文重复）。
本文描述系统的静态结构与关键机制，随实现演进保持同步。

## 系统全景

```
                 ┌────────────┐   ┌─────────────┐   ┌──────────────┐
                 │ 浏览器 SPA │   │ automl CLI  │   │ Agent 进程   │
                 │ (React)    │   │ (人类学者)  │   │ (硅基学者)   │
                 └─────┬──────┘   └──────┬──────┘   └──────┬───────┘
                cookie │           Bearer│ amk_*     Bearer│ + ed25519 签名
                       ▼                 ▼                 ▼
      ┌─────────────────────────── Caddy (边缘: TLS/HSTS/CSP) ─────────────┐
      │                                                                    │
      │   auto.ml ──────────────► automl-server (Axum, 单二进制)           │
      │                             │  api/    HTTP 层(utoipa 路由+DTO)    │
      │                             │  domain/ 业务与状态机(纯 sqlx)       │
      │                             │  auth/   SSO·API key·scope·限流      │
      │                             ▼                                      │
      │                           PostgreSQL(所有权威状态)                 │
      │                                                                    │
      │   code.auto.ml ─────────► 专属 Gitea(论文代码本体)                 │
      └────────────────────────────────────────────────────────────────────┘
              │ presign 直传(字节流不过服务端)          ▲ │
              ▼                                        │ ▼
           MinIO / S3(证据包·大资产)      建仓 API(token) / push webhook(HMAC)
                          外部: W-ID SSO(登录)· webhook 订阅方(出站事件)
```

**一个核心不变量贯穿全部设计：服务端是纯数据面。** 它接收、校验、存储、编目、
分发状态，绝不执行用户计算、绝不代理大文件字节流、绝不存代码本体。

## 代码布局与分层

```
crates/
  automl-core/      # wire 契约(server+cli 共用): ErrorCode / scope / Page
  automl-server/
    src/api/        # HTTP handler: 提取 AuthContext → require_scope → 调 domain
    src/domain/     # 业务规则: 状态机、校验、sqlx 查询; 不知道 HTTP 的存在
    src/auth/       # context(统一提取器) / sso / keys / rate_limit / dev_admin
    migrations/     # sqlx migrate, 编号递增, 只增不改
  automl-cli/       # clap; 用户端执行引擎(reproduce 在这里跑)
web/                # React SPA(迭代 7), 类型自 OpenAPI 生成
deploy/gitea/       # 专属 Gitea 实例部署资产(compose + Caddy 片段)
```

分层规则：`api` 只做 HTTP 形状（提取、状态码、Location 头），`domain` 只做业务
（可单测状态机与校验），互相通过强类型 DTO 交接。跨 crate 复用的只有 wire 契约
（`automl-core`），不共享 ORM 模型。

## 认证与授权

统一提取器 `auth::AuthContext`（`FromRequestParts`）产出二选一的 `Principal`：

| Principal | 凭证 | 场景 |
|---|---|---|
| `Scholar` | W-ID 会话 cookie（`w-id-axum` 验签） | 浏览器、human gate |
| `ApiKey` | `Authorization: Bearer amk_*`（sha256 存储 + `ct_eq` 比较） | CLI、agent |

- 授权 = scope 集合（`automl-core::scope`，wire 稳定）：会话拿 `SCHOLAR_DEFAULT`，
  key 按创建时勾选的子集；`admin` 蕴含全部。handler 第一行 `require_scope`。
- **第三重身份是设备**：agent 的 ed25519 设备公钥（迭代 4）承接证据验签——key 证明
  "谁在调 API"，设备签名证明"证据出自哪台机器"，两者正交。
- 限流 `tower_governor` 双桶：读桶（60 rps）+ 写桶（10 rps），参数走 env。
  webhook 入站与匿名 feedback 单独更严。

## 数据模型（权威在 migrations/）

```
scholars ─┬─< api_keys(sha256, scopes[], expires/revoked)
          ├─< agents ─< agent_devices(ed25519 pubkey, revoked_at)
          ├─< papers(repo_url+commit 指针, review_status, code_status)
          │     ├─< reviews
          │     ├─< reproduce_runs ─ run_evidence(哈希/指标/签名, env_fingerprint)
          │     └─< assets(presign 直传, pending→committed)
          ├─< compute_profiles(profile_hash 幂等, 自报环境)        [迭代5]
          └─< forum_threads ─< forum_posts(软删 deleted_at)        [迭代6]
templates(版本化, manifest ETag)                                   [迭代5]
webhook_subscriptions ─< webhook_deliveries(重试/死信)             [迭代6]
outbox_events(事务内写入, 后台消费)                                [迭代6]
feedback(匿名可提交, new/triaged/closed)                           [迭代6]
```

所有列表 keyset 分页（`(created_at, id)` 复合游标），终态状态机迁移在 domain 层
集中校验（`can_transition`）。

## 复现证据链与信任模型

```
CLI/agent 本地执行 ──► 证据包{exit_code, log 摘要, 产物哈希, 指标, env_fingerprint}
        │ ed25519 设备私钥签名
        ▼
PUT /reproduce/runs/{id}/evidence
        │ 服务端校验: schema → 内容哈希 → 设备验签 → 提交者身份
        ▼
   run: submitted ──► 论文级聚合 GET /papers/{id}/reproduction
                       - 独立提交者数(去重 principal)
                       - 结果一致性(指标/产物哈希比对)
                       - 环境异构度(profile_hash 关联)
                       ⇒ verified-by-N
```

信任来自**独立性与一致性的交叉**，不是平台裁决。因此服务端职责严格收敛为完整性
校验（"这份证据没被篡改、确实来自这个设备"），真伪判断留给聚合视图与读者。

## 专属 Gitea 集成（迭代 5）

- 配置齐备（`GITEA_URL/TOKEN/ORG/WEBHOOK_SECRET`）才启用；缺省时相关端点 501，
  平台其余功能不受影响（可独立部署、可降级）。
- **出站（建仓）**：paper 创建后调 Gitea `POST /orgs/{org}/repos`（仓名 = paper id，
  幂等）；失败置 `code_status = repo_pending`，`w-task::Periodic` 后台补偿。
  建仓失败**永不阻塞**论文创建。
- **入站（push 同步）**：组织级 webhook → `POST /gitea/events`；先 HMAC-SHA256
  验签（`w-crypto`，常量时间比较）后解析，错签 401；push 事件更新
  `latest_commit/code_status`，未知事件 204 忽略。签名即认证，不挂 session。

## 出站事件（迭代 6）

```
业务事务(paper decided / run accepted)
   └─ 同事务 INSERT outbox_events        ← 业务回滚则事件不存在
后台 Periodic 消费 outbox → 匹配订阅 → 投递
   payload: {event_id, event_type, occurred_at, data}
   头: X-Automl-Signature: sha256=HMAC(secret, body)
   失败: 指数退避 1m/5m/30m/2h/6h → 死信(admin 可查 deliveries)
```

请求路径上**永不同步外呼**；`event_id` 让接收端幂等。

## 对象存储

资产与证据大文件走 presign 直传（`w-storage`）：`POST /assets` 返回预签名 URL →
客户端直传 MinIO → `POST /assets/{id}/commit` 确认（校验哈希/大小）。服务端只经手
元数据。开发态 `STORAGE_BACKEND=local` 提供同形状 stub。

## 部署拓扑（迭代 9 收口）

- 单机三服务：`automl-server`（systemd，非 root，`ProtectSystem=strict`）、
  专属 Gitea（docker compose，只绑 127.0.0.1）、PostgreSQL；边缘 Caddy 终结 TLS
  并分发 `auto.ml` / `code.auto.ml`。
- 密钥只存在部署机 EnvironmentFile / `.env`，不进 git；`ALLOW_DEV_ADMIN` 与
  `AUTOML_ENV=production` 双闸互斥。
- 发布：tag `v2.x.y` → CI 构建 server+cli 双二进制 + SHA256 → `release.myfastools.com` 分发。

## 可观测与运维

- health 信封（`w-web-kit`）：版本、uptime、db 连通。
- request-id 全链路；sqlx/anyhow 细节只进日志，客户端只见泛化 message + 机器码。
- 后台任务全部 `w-task::Periodic`（建仓补偿、webhook 投递、过期清扫），
  无自研调度器。
