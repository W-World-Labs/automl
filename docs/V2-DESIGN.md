# Auto.ml v2 设计与迁移计划

产品与用户流程见 `PRODUCT-FLOWS.md`，系统技术架构见 `ARCHITECTURE.md`；本文承载迁移
背景、横切铁律与迭代拆解。

v1 完整保留在 `archive/`，问题清单见 issue #1。v2 是全新精写：功能范围不缩（论文核心链路、学者与 Agent 身份、计算与模板、社区），砍掉的是结构性繁琐——手写 OpenAPI、无分页、各写各的响应、空壳接口。

## 架构原则：平台不执行计算（2026-07-10 定调）

**所有复现/验证计算都在用户端算力上执行；平台只是数据中心的接收端与数据管理端。**由此推出：

1. **服务端零执行**：没有 reproduce worker、没有沙箱、没有任务调度器。v1 的 `reproduce::spawn_worker` 这一整类代码不进 v2。服务端的职责收敛为：接收、校验、存储、编目、分发状态。
2. **CLI 是执行引擎**：`automl reproduce run` 在用户机器上拉取论文的复现规格（`reproduce.yaml` + git commit pin），本地执行，采集证据（退出码、日志、产物哈希、指标、环境指纹），打包签名后提交给服务端。
3. **服务端做证据管理，不做真伪裁决**：API 校验证据的完整性（哈希一致、schema 合法、提交者身份），存储到 MinIO，登记状态机（`submitted → verified-by-N`）。可信度来自**多个独立提交者的证据交叉**（N 个不同 agent/scholar 各自复现出一致结果），而不是平台声称"我跑过了"。
4. **代码托管交给平台专属 Gitea 实例，不自研**：另行部署一套独立 Gitea（部署资产在 `deploy/gitea/`，与源码托管服务分离），论文的代码/实验以 git 仓库 + commit SHA 为准，平台只存 `repo_url + commit + reproduce.yaml 路径` 的指针。集成走 Gitea API（按论文建仓，服务账号最小权限 token）+ 组织级 webhook（HMAC 签名校验，同步 push 事件更新论文代码状态）。平台数据库里不存代码本体；集成环境变量契约见 `deploy/gitea/README.md`。

这个模型对 API 形状的直接影响：

- `POST /reproduce/runs`（客户端上报开始，202 + run 资源）→ `PUT /reproduce/runs/{id}/evidence`（提交证据包）→ 服务端校验后置 `submitted`；同一 spec 的多个 run 聚合出论文的复现置信度。
- `compute` 域从"节点调度"变成"客户端算力档案"：agent 自报环境（GPU/OS/依赖指纹），只用于证据溯源与横向比对，不用于派活。
- 资产上传走 presign 直传 MinIO（`w-storage` presign），服务端不代理大文件字节流。

## 布局

```
Cargo.toml            # workspace：统一依赖版本 + release profile
crates/
  automl-core/        # 共享 wire 契约：ErrorCode、Page/PageParams（server 与 cli 共用）
  automl-server/      # Axum + PostgreSQL + utoipa
  automl-cli/         # clap，bin 名为 automl
web/                  # React 前端（后续迭代加入，类型由 openapi-typescript 生成）
docs/
deploy/               # 后续迭代：单一权威 systemd 单元（非 root + 加固）
archive/              # v1 快照，只读参考
```

## 横切规则（v1 教训 → v2 铁律）

| 规则 | 落点 |
|---|---|
| OpenAPI 只能从代码生成 | `utoipa_axum::routes!` 注册路由，`api::v1()` 同时产出 Router 和文档；禁止手写路径清单 |
| 错误必带机器可读 code | `ApiError` → `{"error":{"code","message"}}`，code 见 `automl-core::ErrorCode`，只增不改 |
| 列表必分页 | keyset 分页，`PageParams{limit,cursor}` → `Page<T>{data,next_cursor}`，禁止 SQL 写死 LIMIT |
| 状态码语义化 | 创建 201、异步任务 202+job 资源链接、删除 204 |
| 写端点强类型 | DTO + 校验，禁止 `Json<Value>` 手动取字段给默认值 |
| body 限额分层 | 全局 2MB（JSON），上传路由单独 `RequestBodyLimitLayer` |
| 内部细节不出进程 | sqlx/anyhow 错误只进日志，客户端只见泛化 message |
| stub 即 501 | 未实现的接口返回 501，不假装 queued |

## 复用（fleet commons，勿重写）

- `w-web-kit`：openapi 双路径挂载、health 信封、CORS-credentials、request-id、graceful shutdown、（前端接入后）SPA
- `w-id-axum`：W-ID SSO login/callback/me/logout（替代 v1 手写 session）
- `w-storage`（s3+presign）：资产存储（替代 v1 storage.rs）
- `w-crypto`：API key sha256 + `ct_eq`
- `w-http`：CLI 客户端 + 重试
- `w-util::truncate_chars`：CLI 表格截断（修 v1 的 UTF-8 panic）
- `w-task::Periodic`：后台清扫类任务（过期证据、超时 run 标记 stale——不是执行计算）
- 限流 fleet 暂无共享 crate → 本地引入 `tower_governor`；若沉淀成熟可上提 commons

## 迭代顺序

迭代 2–9 的详细设计（数据模型、API 形状、完成标准）已拆为 issue #2–#9（号即迭代号），
总览与依赖关系见 issue #10，milestone `v2.0.0-alpha`。

1. **骨架**（已完成，`86fbc17`）：workspace、core 契约、server 启动/health/openapi、CLI health、CI
2. **认证**（#2）：w-id-axum SSO + API key（含列举/吊销/过期）+ tower_governor 限流 + scope 化权限（替代字符串 trust_level）；dev-admin 后门仅在显式 `ALLOW_DEV_ADMIN=1` 时启用
3. **论文核心**（#3）：papers（sqlx migrations 起步，存 repo_url+commit 指针）→ reviews → reproduce 证据链（run 登记 202 + 证据提交 + 校验状态机 + verified-by-N 聚合）→ assets（w-storage presign 直传，服务端不代理字节流）+ CLI 最小执行链打通端到端
4. **身份域**（#4）：scholars / silicon_scholar / agent 注册与设备绑定（ed25519 设备密钥承接证据验签，环境指纹归入证据溯源）
5. **计算档案与模板**（#5）：客户端算力档案（自报环境，仅溯源用）、templates、skills manifest 分发；专属 Gitea 集成（按论文建仓 / webhook 同步 push，实例部署见 `deploy/gitea/`）
6. **社区**（#6）：forum / feedback；webhook 做成真实事件（paper.decided / repro.completed，outbox + 退避重试 + 死信），做不完前一律 501
7. **Web 前端**（#7）：Vite + React，openapi-typescript 生成类型（drift 检查进 CI），统一 AsyncBoundary 三态，human gate 决策页（补 v1 缺失的人类决策入口）
8. **CLI 补全**（#8）：loopback OAuth 登录（起本地回调 + PKCE）、多平台分发（CI 产 artifact + sha256，不再从 target/ 实时读）
9. **部署**（#9）：单一 systemd 单元（非 root + ProtectSystem=strict）、Caddy 加 HSTS/CSP、生产密钥走 EnvironmentFile/vault（不入库）；前置遗留：v1 泄漏密钥轮换必须在上线前完成

## 版本与发布

workspace 统一 `2.0.0-alpha.1`，所有 crate 用 `version.workspace`。发布走 tag `v2.x.y` → CI 构建 server+cli 双二进制 + SHA256 → `release.myfastools.com`。前端版本随 workspace 注入。
