# 系统状态机（权威定义）

本文是全系统状态机的**唯一权威**。每张转移表都有三层保障，三层必须一致：

1. **domain 层 `can_transition`**（非法转移 → 409 `CONFLICT`），单测逐格断言本表；
2. **DB CHECK 约束**（非法值根本写不进去，见 `migrations/007_state_machines.sql`）；
3. 验收套件端到端走一遍合法链路（`docs/ACCEPTANCE.md`）。

约定：`◉` 初始态，`■` 终态。终态永不迁出；所有未列出的转移一律非法。

## 1. `paper.review_status` — 评审生命周期

```
◉ draft ──submit──► submitted ──advance──► under_review ──decide──► ■ accepted
              ▲          │                        │
              └─withdraw─┘                        └────decide────► ■ rejected
```

| from \ to | draft | submitted | under_review | accepted | rejected |
|---|---|---|---|---|---|
| draft | – | ✓ 作者提交 | ✗ | ✗ | ✗ |
| submitted | ✓ 撤回 | – | ✓ 管理员推进 | ✗ | ✗ |
| under_review | ✗ | ✗ | – | ✓ human gate | ✓ human gate |
| accepted ■ | ✗ | ✗ | ✗ | – | ✗ |
| rejected ■ | ✗ | ✗ | ✗ | ✗ | – |

副作用：进入 `accepted/rejected` 的同一事务写入 `paper.decided` outbox 事件（回滚则事件不存在）。
实现：`domain::papers::ReviewStatus::can_transition`。

## 2. `paper.code_status` — 代码托管生命周期

```
◉ unknown ■                    （外部 repo 指针：创建即终态，webhook 永不触及）
◉ repo_pending ──建仓成功──► repo_ready ──首个 push──► ■ ready
       └────────push 先于建仓状态翻转───────────────────► ■ ready
```

| from \ to | unknown | repo_pending | repo_ready | ready |
|---|---|---|---|---|
| unknown ■ | – | ✗ | ✗ | ✗ |
| repo_pending | ✗ | – | ✓ 建仓完成 | ✓ push 抢先 |
| repo_ready | ✗ | ✗ | – | ✓ push webhook |
| ready ■ | ✗ | ✗ | ✗ | – |

- 初始态由创建路径决定：带外部 `repo_url` → `unknown`；平台托管（省略 repo_url）→ `repo_pending`。
- `PATCH /papers/{id}` 的 `code_status` 只允许合法转移（平台管理字段，用户不能乱写）。
- webhook 的 UPDATE 带 `code_status IN ('repo_pending','repo_ready','ready')` 守卫，`unknown` 论文在 SQL 层就不可达。
- 补偿：`repo_pending` 由 `w-task` 周期扫描重试建仓（`GITEA_SWEEP_SECS`，默认 60s）。
实现：`domain::papers::CodeStatus::can_transition`。

## 3. reproduce run — 复现证据生命周期

```
◉ registered ──证据完整性通过(exit 0)──► ■ accepted
      │──────证据完整性失败/exit≠0─────► ■ rejected
      │──────超时无证据(sweep)─────────► ■ stale
      └──────(保留：异步校验中间态)────► evidence_submitted ──► ■ accepted/rejected
```

| from \ to | registered | evidence_submitted | accepted | rejected | stale |
|---|---|---|---|---|---|
| registered | – | ✓ 保留 | ✓ | ✓ | ✓ 超时 sweep |
| evidence_submitted | ✗ | – | ✓ | ✓ | ✗ |
| accepted ■ | ✗ | ✗ | – | ✗ | ✗ |
| rejected ■ | ✗ | ✗ | ✗ | – | ✗ |
| stale ■ | ✗ | ✗ | ✗ | ✗ | – |

- 证据提交只被 `registered` 态接受（重复提交 409）；提交者必须是登记 principal。
- `stale`：`RUN_STALE_HOURS`（默认 72h）内无证据，由每小时 sweep 标记。
- 聚合（verified-by-N）只统计终态 `accepted` 且签名有效的 run。
- `paper.commit_sha` 为空（平台托管未 pin）时拒绝登记 run（409）。
实现：`domain::reproduce::RunStatus::can_transition` + `mark_stale_runs`。

## 4. webhook delivery — 出站投递生命周期

```
◉ pending ──2xx──► ■ delivered
     │ 失败: attempt++, 退避 1m/5m/30m/2h/6h
     └──退避用尽──► ■ dead
```

| from \ to | pending | delivered | dead |
|---|---|---|---|
| pending | –（重试自环） | ✓ 2xx | ✓ 第 5 次失败 |
| delivered ■ | ✗ | – | ✗ |
| dead ■ | ✗ | ✗ | – |

- 事件源头是事务性 outbox；fan-out 只投给 `active` 订阅。
- `event_id` 跨投递稳定，接收端幂等去重；签名 `X-Automl-Signature: sha256=HMAC(secret, body)`。
实现：`domain::events::delivery_can_transition` + worker SQL 守卫（`WHERE status='pending'`）。

## 5. feedback — 反馈 triage

```
◉ new ──► triaged ──► ■ closed
   │                     ▲ │
   └──────fast close─────┘ └─reopen─► triaged
```

| from \ to | new | triaged | closed |
|---|---|---|---|
| new | – | ✓ | ✓ 快速关闭 |
| triaged | ✗ | – | ✓ |
| closed | ✗ | ✓ 重开 | – |

实现：`domain::feedback::can_transition`。

## 6. CLI 授权码（一次性）

```
◉ issued ──5min 内 + verifier 匹配──► ■ used（换发 API key）
     └──超时/验证失败──► ■ expired/burned
```

- code 只存 sha256；claim 是原子 UPDATE（`used_at IS NULL AND expires_at > now()`），
  并发重放必然只有一个赢家；verifier 校验失败也已烧码（防离线爆破）。
实现：`auth::cli_auth::exchange`。

## 7. API key / 设备密钥（吊销型，非流转型）

- API key：`active → revoked`（软删，`revoked_at`），终态；过期（`expires_at`）等效终态。
- agent device：`active → revoked`（`revoked_at`），吊销后验签一律 422 `SIGNATURE_INVALID`。

## 8. Research project lifecycle

This section is authoritative for ResearchProject status transitions and the
transactional ACL/lifecycle guarantees. The design overview in
`RESEARCH-HUB-DESIGN.md` must stay consistent with this table.

```
◉ draft ──activate──► active ──archive──► archived
  │                    │                    │
  └────────delete──────┴──────delete────────┴──► ■ deleted
```

| from \ to | draft | active | archived | deleted |
|---|---|---|---|---|
| draft | – | ✓ activate | ✗ | ✓ delete |
| active | ✗ | – | ✓ archive | ✓ delete |
| archived | ✗ | ✗ | – | ✓ delete |
| deleted ■ | ✗ | ✗ | ✗ | – |

- Deleted is a terminal tombstone: no restore, unarchive, or transition back to
  any live state. Draft, active, and archived projects can be soft-deleted.
- Archived projects remain live. Metadata edits and membership governance are
  permitted while archived; status cannot be changed back to active.
- Serialize project mutations on the project row/lock. In the same transaction,
  re-read project liveness and caller authorization against current rows, apply
  only requested fields, and write the matching audit event. A stale title-only
  PATCH must neither overwrite omitted governance fields nor make a deleted
  project live. The committed event must describe the committed mutation.
- Status/visibility changes require `projects:write` and `projects:admin` plus
  the live Owner role; ordinary metadata PATCH requires write plus Owner.
  Membership governance requires admin plus Owner; global admin never bypasses
  project membership.
- Membership governance uses the same per-project serialization. Recheck caller
  and target membership under that protection and preserve at least one human
  Owner after every committed change. Concurrent removals that would leave no
  human Owner cannot both commit.
- Creation starts with its creator as human Owner in the same transaction.
  Member roles and visibility do not grant membership to nonmembers. `lab` is
  an inert pointer in P0. An authenticated nonmember with `projects:read` may
  fetch an individual `public_meta` project through its allowlisted projection;
  anonymous access is denied and nonmember projects are excluded from project
  lists. Papers, members, and audit stay member-only.
- Project audit is append-only under ordinary application operations and is
  written atomically with the mutation. P0 records the accountable scholar and
  actor kind, not API-key ID or agent identity; metadata audit records changed
  field names, not a reconstructible before/after image. The DB trigger is not
  a defense against privileged database operators. A soft-deleted project's
  audit remains stored but project read routes, including audit, return 404.
- No 90-day purge is implemented. Physical retention/purge policy is deferred;
  any future purge must be designed compatibly with audit retention.

P0 project authorization is scholar-delegated: Scholar sessions and API keys
acting for an accountable scholar are checked against that scholar's live
project membership and scopes. Software may use a shared scholar-delegated key;
the request is authorized as that scholar and does not establish a verified
agent identity. Agent enrollment/project membership are deferred (enrollment
returns 501 where registered); no agent ACL or isolation guarantee is claimed.
This does not alter existing v2 agent/device signed-reproduction behavior.
Human CLI governance such as `automl project member-add` remains available to an
authorized scholar-delegated API key.
