# v2.0.0-alpha 验收报告

> 历史报告：下列结果记录原开发环境，不代表当前公开提交已通过验收。
> 公开提交的实际结果以对应 GitHub Actions run 和源码工件为准。

验收执行：2026-07-10，全部自动化。状态机权威见 `STATE-MACHINES.md`，旅程定义见
`PRODUCT-FLOWS.md`。

## 如何复跑

```bash
# 需要真 Postgres（automl_v2 库）与 git；套件自起真 server 进程 + mock Gitea + 本地 webhook 接收端
DATABASE_URL=postgres://automl:automl_dev_password@127.0.0.1:55439/automl_v2 \
  cargo test --workspace            # 全部集成/单元套件（13 个测试目标）
DATABASE_URL=… cargo test -p automl-cli --test acceptance   # 端到端验收（真 CLI 二进制）
cd web && npm ci && npm run lint && npm test && npm run build  # 前端门禁
```

## 端到端验收（`crates/automl-cli/tests/acceptance.rs`）

单个 `full_product_acceptance` 用**真实进程**走完全部旅程：真 `automl-server`
二进制（1s worker tick）、真 `automl` CLI 二进制、mock Gitea、本地签名校验
webhook 接收端、真实本地 git 仓库 fixture。

| # | 验收点 | 结果 |
|---|---|---|
| J1.1 | 平台托管论文创建 → `repo_pending`，mock Gitea 收到建仓调用，补偿循环翻转 `repo_ready` | ✅ |
| J1.2 | HMAC 签名 push webhook → `ready` + `latest_commit` 同步；pin 的 `commit_sha` 不被触碰 | ✅ |
| J1.3 | 作者 pin commit → submit；**作者无法推进/决策（403）** —— human gate 仅 admin | ✅ |
| J1.4 | admin 推进 `under_review` → 评审落库 → 决策 `accepted` | ✅ |
| J1.5 | `accepted` 终态：任何迁出 409（状态机） | ✅ |
| J1.6 | `paper.decided` 事件投递到订阅方；**接收端用自己的 secret 验签通过**；带 `event_id` | ✅ |
| J5.1 | 真 CLI `automl login`：loopback 随机端口 + PKCE；测试扮演浏览器带 W-ID 会话走 `/cli/auth/start` → 302 到 loopback → CLI 换发 key 退出 0 | ✅ |
| J5.2 | `credentials.toml` 存在且 **0600** | ✅ |
| J2.1 | 学者建 agent → 真 CLI `device register`（ed25519，私钥 0600，元数据落盘） | ✅ |
| J2.2 | 真 CLI `reproduce run`：clone 本地 git fixture → checkout pin → 执行 `reproduce.sh` → 指标提取（accuracy=0.93）→ **设备私钥签名证据** → 提交 `accepted`，服务端 `signature_valid=true` | ✅ |
| J3.1 | `GET /papers/{id}/reproduction`：runs_total=1，**verified_by_n=1**（仅签名有效的 accepted 计入） | ✅ |
| J2.3 | `repro.completed` 事件投递到订阅方 | ✅ |
| J2.4 | CLI `papers list`/`runs list` `--json` 稳定 wire 形状；**含 CJK 标题的表格渲染不 panic** | ✅ |
| J4.1 | 论坛：建 thread（含首帖 markdown）→ 回帖 | ✅ |
| J4.2 | 匿名 feedback → admin triage `new → triaged`；**回退 `new` 409（状态机）** | ✅ |
| J4.3 | 通知中心如实 **501** | ✅ |
| J5.3 | `automl logout`：凭据删除、key 尽力吊销 | ✅ |

## 状态机验收（`STATE-MACHINES.md` ↔ 实现一致性）

| 机器 | 转移表单测 | DB CHECK | 集成/验收覆盖 |
|---|---|---|---|
| review_status | `papers::tests::state_machine_allows_happy_path_only` | 001 | J1.3–J1.5 + admin 门 `papers_crud_and_illegal_status_transition` |
| code_status | `papers::tests::code_status_machine` | 007 | 迭代5套件：合法链 + 外部指针 webhook 不可达 |
| run status | `reproduce::tests::run_status_machine` | 007（含 stale） | J2.2 + 重复提交 409（迭代3套件）；stale sweep 单元路径 |
| webhook delivery | `events::tests::delivery_states_are_terminal` | 005 | 迭代6套件：重试→恢复 / 重试→死信 |
| feedback triage | `feedback::tests::triage_transitions` | 005 | J4.2 |
| CLI auth code | （SQL 原子 claim） | 006 | 迭代8套件：单次防重放 + 错 verifier 烧码 |

## 门禁记录（本次验收运行）

| 门禁 | 结果 |
|---|---|
| `cargo test --workspace`（13 目标，真 Postgres） | ✅ 全绿 |
| `cargo clippy --workspace --all-targets -- -D warnings` | ✅ 0 告警 |
| `cargo fmt --check` | ✅ |
| `cargo test -p automl-cli --test acceptance` | ✅ 1 passed (10.2s) |
| web `npm run lint` / `npm test` / `npm run build` | ✅ |
| OpenAPI 类型 drift（重新生成 diff 为空） | ✅ |

## 已知边界（如实记录，不算通过项）

- **需真实外部环境的两项未纳入自动验收**：真 W-ID 生产实例的浏览器登录
  （验收用同 JWT 契约的伪造会话覆盖协议路径）；tag 构建三平台产物
  （CI runner 当前仅 linux-x86_64，`/cli/install` 只列真实存在的平台）。
- 专属 Gitea 用 mock 验收 API 契约（建仓幂等 201/409、webhook 签名）；
  真实例部署属迭代 9 上线清单。
- `self-update` 逻辑已实现（sha256 校验 + 原子替换），需首个正式 tag 产物后方可实测。
