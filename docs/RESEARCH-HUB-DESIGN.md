# Research Hub 设计：以项目为中心的课题组 Auto Research

> Status: **Design v1.1 — project-centric refinements locked (NEVER-BLOCK-DEV defaults)**（W-AI 2026-09-26）  
> Repo: [W-World-Labs/automl](https://github.com/W-World-Labs/automl)  
> Related: `docs/V2-DESIGN.md` · `docs/ARCHITECTURE.md` · `docs/PRODUCT-FLOWS.md`  
> Integrations: W Papers · a separately configured W-DaaS provider · OpenAlex

---

## 1. Product one-liner

**auto.ml = Research OS for 课题组。**  
工作单元不是单篇论文，而是 **`ResearchProject`（研究项目）**：一组人、一批文献燃料、若干分析/检索资产、多次 auto-research 运行，以及可交叉验证的复现证据。

v2 已定调的证据面（CLI `reproduce.yaml` + `verified-by-N`、平台不执行计算）完整保留，并升维到「项目」语境下编排。

---

## 2. Roles（与 W-ID / W-DaaS 对齐）

| Role | 谁 | 能力概要 | 身份 |
|---|---|---|---|
| **Owner / 导师** | 课题组负责人 | 建项目、成员与 ACL、human gate、绑定 W-DaaS workspace | W-ID SSO |
| **Member / 学生** | 课题组成员 | 导入燃料、阅读/标注、发起 research run、提交复现证据 | W-ID SSO |
| **Viewer** | 只读协作者 / 外部评审 | 浏览项目书架、资产、聚合状态 | W-ID SSO（只读 scope） |
| **Agent（未来阶段）** | 自动化执行身份 | P0 不登记、不授权；未来绑定契约另行定义 | P0 不支持 agent 请求身份 |

**Workspace 对齐：**  
ResearchProject ↔ W-DaaS `project` / `workspace_id` 一对一或显式绑定（`project_binding`）。P0 only stores an unvalidated workspace pointer and makes no downstream call or access grant. Before P2 use, verify workspace authority and enforce project isolation in the downstream product.

Future agent work must bind an agent to an accountable human and retain signed evidence provenance. This is not a P0 authorization guarantee.

---

## 3. Four layers

```
┌─────────────────────────────────────────────────────────────┐
│  Layer 1  auto.ml Research Hub                              │
│  入口 · 项目编排 · ACL · research_run 状态 · human gate     │
│  （本仓库 20-01-automl）                                    │
└───────────────┬───────────────────────────┬─────────────────┘
                │ fuel / shelf              │ parse / KG / search / audit
┌───────────────▼──────────┐   ┌────────────▼──────────────────────────┐
│  Layer 2  W Papers       │   │  Layer 4  W-DaaS                      │
│  30-29-w-papers          │   │  00-03-data-asset-platform            │
│  capture → work-artifact │   │  data.myfastools.com                  │
│  .v1 → import to shelf   │   │  parse · chunks · KG · hybrid search  │
└───────────────┬──────────┘   │  paper-audit · deep-research          │
                │ DOI / ids    └────────────▲──────────────────────────┘
┌───────────────▼──────────┐                │ OpenAlex mirror / cite graph
│  Layer 3  OpenAlex@DOI   │────────────────┘
│  本地书目镜像 + 自动更新 │  优先复用 W-DaaS academic OpenAlex
│  （勿自建第二套爬虫）    │
└──────────────────────────┘

Evidence plane（贯穿，v2 铁律）:
  平台不做 compute → CLI reproduce.yaml → 证据包签名 → verified-by-N
```

### 3.1 auto.ml Research Hub（本仓）

- 项目 CRUD、成员角色、审计事件。
- 项目书架（`project_paper`）、资产指针（`project_asset`）。
- `research_run` 编排与状态机（发起、挂起、完成、失败、需人工）。
- 复现绑定（`reproduce_binding`）把项目内论文/产物接到 v2 evidence 链。
- **不**解析 PDF、不跑向量检索、不执行训练/推理任务。

### 3.2 W Papers（燃料捕获）

- 用户在浏览器/扩展捕获全文与元数据 → 稳定 `work-artifact.v1`。
- Future Research Hub import can reference a W Papers library / artifact id; P0 has no working fuel write path and never copies corpus bytes into AutoML.
- 边界遵守 W Papers MVP：不自建 arXiv 爬虫、不混用 status-api。

### 3.3 OpenAlex via DOI

- 以 DOI 为主键拉取 OpenAlex Work，落本地/项目级书目镜像，支持字段自动更新。
- **优先复用** W-DaaS 已有 academic OpenAlex（含引文邻域图等能力）；auto.ml 只存 `openalex_id` + 必要缓存字段，避免双源漂移。

### 3.4 W-DaaS（解析与智能）

- PDF/文档 → parse → chunks → embedding → hybrid search。
- KG / claims、paper-audit、deep-research 能力以 DaaS API 为准。
- Research Hub 通过 `workspace_id` / `project_binding` 调用；产物以 `project_asset` 指针登记回 auto.ml。

### 3.5 Evidence plane（保持 v2）

- 服务端零执行；`automl reproduce run` 在用户/Agent 算力上跑。P0 project ACL changes do not alter existing v2 agent/device signed-reproduction behavior.
- `verified-by-N` 来自独立提交者交叉，不是平台背书。
- 项目级只增加「哪些 run 属于本项目、与哪篇 project_paper / research_run 绑定」。

---

## 4. User journey（project-centric）

```
Create project
    → Fuel（DOI + W Papers capture + PDF upload→DaaS）
    → Read / Analyze（书架 · 检索 · KG · audit）
    → Auto Research（编排 Agent / 引用 Ai2 Asta-class OSS 思路与可复用组件）
    → Reproduce + Human gate（CLI 证据 · verified-by-N · 导师决策）
```

1. **Create project**  
   Owner 建 `research_project`；可存一个未验证的 W-DaaS workspace pointer；邀请 Member / Viewer。Agent enrollment is deferred and returns 501 in P0.

2. **Fuel**  
   - 粘贴 DOI → OpenAlex 补全书目 → `project_paper`。  
   - 从 W Papers 私有库 / capture 导入 → 挂 `w_papers_*` id。  
   - 上传 PDF → 经 W-DaaS 登记 DocumentAsset → 回写 `daas_*` id。

3. **Read / Analyze**  
   项目内阅读、笔记、hybrid search、引文邻域、paper-audit；结果进 `project_asset`。

4. **Auto Research**  
   发起 `research_run`：问题陈述、种子文献、工具策略（检索/总结/假设/实验设计）。  
   实现上**引用** Ai2 Asta 一类开源 research agent 的流程与组件（规划–工具–引用），不宣称自研同等能力；输出结构化草稿 + 引用清单，状态需可审计。

5. **Reproduce + Human gate**  
   对可执行结论绑 `reproduce.yaml`；CLI 提交证据；future agent evidence requires a separately specified identity binding. Owner 做 accept/reject/iterate。
   关键事件进 `audit_event`，可外发 webhook（继承 v2 outbox）。

---

## 5. Core objects

| Object | 职责 | 关键外键 / 备注 |
|---|---|---|
| `research_project` | 课题组工作单元；名称、可见性、绑定 workspace | `w_daas_workspace_id` |
| `project_member` | P0 scholar member and role (owner/member/viewer); no agent request authorization | `user_id`; `agent_id` is deferred |
| `project_paper` | 项目书架条目 | `doi`, `openalex_id`, `w_papers_work_id`, `daas_document_id`, 可选 `paper_id`→v2 papers |
| `project_asset` | 解析产物、笔记、检索快照、research 输出指针 | `kind`, `uri`/`daas_artifact_id` |
| `research_run` | 一次 auto-research 运行 | `project_id`, status, input/output refs |
| `reproduce_binding` | 项目语境下的复现规格绑定 | → v2 reproduce run / paper |
| `audit_event` | 谁在何时对项目做了什么 | append-only |

**扩展现有 v2 `papers`（增量，不破坏）：**

- `doi`（可空，唯一约束按规范化 DOI）
- `openalex_id`（可空）
- `project_id`（可空；公开/社区论文可无项目；项目私有论文必填或经 `project_paper` 关联——实现阶段二选一，P0 推荐 **`project_paper` 为权威关联**，`papers.project_id` 作可选冗余加速）

状态机：`research_run` 建议  
`draft → queued → running → awaiting_human → completed | failed | cancelled`（非法迁移 409）。

---

## 6. Phases P0–P4（owners）

| Phase | 目标 | Owner 仓 | 产出 |
|---|---|---|---|
| **P0** | 项目骨架 + ACL + 空书架 | `20-01-automl` | `research_project` / `project_member` API + CLI（无 Web 交付要求）；W-ID scope |
| **P1** | Fuel：DOI→OpenAlex + W Papers import | `20-01-automl` + `30-29-w-papers` | `project_paper`；OpenAlex 客户端优先走 DaaS；W Papers import API 契约 |
| **P2** | DaaS 解析/检索挂接 | `00-03-data-asset-platform` + `20-01-automl` | workspace 绑定、PDF→asset、hybrid search 入口、`project_asset` |
| **P3** | Auto Research run | `20-01-automl`（编排）+ DaaS deep-research/工具 | `research_run`；接入/包装 Asta-class 流程；审计 |
| **P4** | Reproduce 绑定 + Human gate 项目化 | `20-01-automl` | `reproduce_binding`；项目决策页；webhook 事件扩展 |

依赖：P0 不阻塞于 Owner 额外 ack（v1.1 已拍板）；P1 不阻塞于完整 DaaS；P2 可与 P1 后半并行；P3 依赖 P1 书架与 P2 检索至少之一可用；P4 依赖 v2 evidence API 已可用。

---

## 7. Explicit non-goals（继承 v2 + Hub 边界）

- 平台**不执行**用户计算（无 reproduce worker / 沙箱 / 官方复现队列）。
- 平台**不存储**代码本体（只存 Gitea 指针）。
- Research Hub **不自建**全文 corpus、不替代 W Papers capture、不另起一套 OpenAlex 爬虫。
- **不**在 auto.ml 内重写 DaaS 的 parse / KG / hybrid search。
- **不**把 Auto Research 宣称为已对齐 Ai2 Asta 的产品对等物——仅引用其 OSS 思路与可复用组件。
- **不**在本设计阶段改动 GitHub；Issues 落在 fleet Gitea。

---

## 8. Honest gap vs today

| 能力 | 今天 | Hub 目标 |
|---|---|---|
| 工作单元 | 以单篇 `paper` + 社区为主 | `ResearchProject` 为中心 |
| 文献燃料 | 手动建 paper 元数据/指针 | DOI / W Papers / PDF→DaaS 多源汇入书架 |
| OpenAlex | DaaS 侧有学术 OpenAlex 能力；auto.ml 未项目化消费 | Hub 经 DOI 镜像 + 项目书架 |
| 解析与检索 | 在 W-DaaS，与 auto.ml 未打通项目 ACL | workspace 绑定 + `project_asset` |
| Auto Research | 无一等公民 `research_run` | 可审计的编排运行 |
| 复现 | v2 evidence 面设计/实现中，偏单论文 | 项目绑定 + human gate |
| ACL | Scholar sessions / scholar-delegated API-key scopes | P0 scholar project roles（Owner/Member/Viewer）；agent request identity deferred |

即：v2 证据与身份底座可复用；**缺的是项目容器、燃料管道、与 Papers/DaaS 的正式契约**。

---

## 9. Acceptance for this Issue

- [x] 设计文档落盘：`docs/RESEARCH-HUB-DESIGN.md`（及 directives 副本）
- [x] Owner ack / NEVER-BLOCK-DEV 默认拍板：见 §12.8；**P0 已解阻可开工**（项目骨架 + ACL）
- [ ] 本 Issue 作为 epic：下方 phased checklist 用于跟踪；不强制拆 4 个独立 Issue（除非后续仓内约定变更）
- [ ] 实现 PR 需引用本 Issue；跨仓契约变更在 W Papers / W-DaaS 各自开跟进 Issue 并互链

### Epic checklist

- [ ] **P0** 项目骨架 + ACL（`20-01-automl`）
- [ ] **P1** Fuel：DOI/OpenAlex + W Papers import（`20-01-automl` + `30-29-w-papers`）
- [x] **P2** DaaS 解析/检索挂接 — Auto.ml 侧已落地为 stub-attach：`project_binding` + `project_asset` + PDF→DocumentAsset + hybrid search 入口，默认走 `StubDaaS`（`DAAS_API_BASE` 配置后切换 live adapter）；live W-DaaS 端点契约仍待 `00-03-data-asset-platform` 跟进（不阻塞，见 `docs/RESEARCH-HUB-P2.md`）
- [ ] **P3** Auto Research `research_run`（编排 + Asta-class 组件）
- [ ] **P4** Reproduce binding + 项目 Human gate

---

## 10. API / 契约草图（非绑定，future-phase reference）

This sketch is not a claim that these endpoints exist in P0. Implemented P0 paths use `/api/v1`; unimplemented integrations return 501 only where a route is registered, and otherwise remain absent. P0 shelf reads return an empty member-only page; fuel writes are P1.

```
POST   /api/v1/research-projects
GET    /api/v1/research-projects/:id
POST   /api/v1/research-projects/:id/members
POST   /api/v1/research-projects/:id/papers          # P1 fuel write
GET    /api/v1/research-projects/:id/papers          # P0 member-only empty page
POST   /api/v1/research-projects/:id/research-runs   # future phase
GET    /api/v1/research-runs/:id                     # future phase
POST   /api/v1/research-projects/:id/reproduce-bindings # future phase
GET    /api/v1/research-projects/:id/audit
```

错误码、分页、OpenAPI 生成、501 stub 等横切铁律完全继承 `V2-DESIGN.md`。

---

## 11. 文档与变更记录

| Date (Asia/Shanghai) | Author | Note |
|---|---|---|
| 2026-09-26 | W-AI / agent | 首版设计；用户批准后写入 Issue 进入实现规划 |
| 2026-09-26 | W-AI / agent | v1.1：拍板混合 C / W-Compute / Py=amk 执行面；补齐实验·结果附着、arXiv 集成草图、P0/P1 可开工切片 |

---

## 12. v1.1 细化：ResearchProject 为唯一中心（2026-09-26）

> 原则：**NEVER-BLOCK-DEV** — 开放问题给出默认拍板 + stub 契约，P0/P1 不堵在跨仓 infra。
> 证据面 v2 铁律不变：平台零执行、指针不存字节本体、`verified-by-N` 交叉可信。

### 12.1 Project 生命周期与课题组权限

The authoritative project transition table is [STATE-MACHINES.md §8](STATE-MACHINES.md#8-research-project-lifecycle). That table controls status changes, serialization, and audit atomicity.

```
draft ──activate──► active ──archive──► archived
  │                    │                    │
  └──────delete────────┴────delete─────────┴──► deleted (terminal)

archived permits metadata and membership edits while the project remains live;
there is no unarchive/restore transition. Physical purge/retention is deferred.
```

| 字段 / 概念 | P0 | 说明 |
|---|---|---|
| `research_project.id` | ✅ | UUID |
| `slug` / `title` / `summary` | ✅ | slug 在 immutable `created_by` scholar 命名空间内唯一；Owner 变更不改变命名空间 |
| `visibility` | ✅ | `private`（默认）/ `lab` / `public_meta`（仅元数据） |
| `status` | ✅ | `draft\|active\|archived\|deleted` |
| `lab_id` | stub 可空 | Unvalidated pointer only; P0 has no lab-wide access or lab table |
| `w_daas_workspace_id` | 可空 | Unvalidated pointer only; P0 makes no DaaS calls or access grant |
| `created_by` | ✅ | Immutable creator scholar; initial Owner, not necessarily a current Owner |

**成员与 ACL（叠加全局 scope，不以全局替代项目角色）：**

The table includes later-phase capabilities. P0 implements project metadata, scholar membership governance, and member-only empty shelf reads; DaaS, fuel writes, research runs, and human gates are deferred.

| 项目角色 | 能力 |
|---|---|
| Owner | 改项目、成员、visibility、human gate、绑定 DaaS、归档 |
| Member | 读写书架/资产、发起 `research_run`、提交 reproduce 证据 |
| Viewer | 读项目元数据、书架、聚合状态 |
| Agent | P0 project enrollment/membership is unavailable (501 where registered); no agent project principal is established |

P0 project authorization accepts a Scholar session or an API key delegated to
an accountable scholar. A key used by software remains the scholar's credential:
project membership and scopes are checked for that scholar, without asserting
that the request has a verified agent identity. Agent enrollment and agent
project membership are deferred; no agent ACL or isolation guarantee is claimed.
This does not reject existing shared scholar-delegated keys or change v2
agent/device signed-reproduction behavior. Preserve human CLI governance,
including `automl project member-add` with a scholar-delegated key.

新增 wire scope（只增不改）：`projects:read` · `projects:write` · `projects:admin`。
有效权限 = 凭证 scope ∩ scholar 项目角色；读侧由 `project_acl` helper 检查，写侧在 domain 事务持锁后复查角色。
All project mutations serialize per project and re-read liveness, caller role,
and any target membership under that lock. Lifecycle and membership changes
commit with matching audit events. Deleted projects are never made live again.

P0 visibility contract: `private` is members-only. `lab` is an inert reserved
value and remains members-only. `public_meta` permits only this nonmember
projection: `id`, `slug`, `title`, `summary`, `visibility`, `status`,
`created_at`, `updated_at`. The project detail read requires authentication and
`projects:read`; anonymous reads are not allowed, and nonmember projects are not
included in the project list. It excludes `created_by`, `lab_id`, and
`w_daas_workspace_id`. Nonmembers cannot read papers, members, or audit under any
visibility. P0 papers listing is an authenticated, member-only empty page;
there is no working fuel write path.

### 12.2 文献 / Papers 作为「燃料」挂到项目

**权威关联：`project_paper`（书架条目）**；v2 `papers` 表保持社区/公开复现主体，经可选 `paper_id` 弱链，不把 `papers.project_id` 做成强制。

```
ResearchProject
  └── project_paper[]          # 燃料条目（元数据 + 外指针，无 PDF 字节）
        ├── doi / openalex_id / arxiv_id
        ├── w_papers_work_id   # W Papers library / work-artifact
        ├── daas_document_id   # 解析后的 DocumentAsset
        ├── paper_id?          # → v2 papers（若已进复现社区）
        ├── fuel_role          # seed | related | baseline | cited_by | reading
        └── notes / tags
```

**导入路径（P1，均可 stub）：**

1. **DOI** → 规范化 → OpenAlex（优先 DaaS academic mirror；无镜像则 `OPENALEX_MODE=public|stub`）→ 写书架缓存字段。
2. **W Papers** → `w_papers_work_id` / artifact id；**不复制** corpus；遵守 W Papers：不自建 arXiv 爬虫、不走 mixed status-api。
3. **arXiv id** → 经 W Papers `GET /api/works/arxiv/{id}` 或 OpenAlex 解析；Hub **永不**直接爬 arXiv.org。
4. **PDF 上传** → 客户端直传 DaaS / w-storage → 回写 `daas_document_id`（P2；P1 可只收指针）。

### 12.3 实验 / 仿真 / 结果挂接

不在 Hub 内跑仿真引擎。一等公民是**指针 + 状态**：

| Object | kind / 用途 | 字节落点 |
|---|---|---|
| `project_asset` | `note` · `search_snapshot` · `kg_slice` · `research_draft` · `experiment_spec` · `sim_config` · `result_bundle` · `figure` · `notebook_ptr` | DaaS artifact **或** AutoML `assets`→w-storage |
| `research_run` | 一次 auto-research / 实验设计编排 | 输入输出均为 asset / paper 指针 |
| `reproduce_binding` | 把可执行结论绑到 v2 `reproduce_runs` | 证据包 → w-storage（现有链） |

推荐关系：

```
research_run ──produces──► project_asset (draft/spec/results)
     │
     └── optionally──► reproduce_binding ──► reproduce_runs / run_evidence
```

`experiment_spec` 可引用外部仿真仓（Gitea/W-Sim 等）的 `repo_url+commit`，Hub 只登记指针——与 v2「代码不进 DB」一致。

### 12.4 文件存储默认：**混合 C**（已拍板）

| 内容类型 | 落点 | 理由 |
|---|---|---|
| 文献 PDF / 需 parse·KG·hybrid search 的文档 | **W-DaaS DocumentAsset**（元数据 PG + 字节 MinIO/w-storage） | 复用 DaaS 知识面，避免 Hub 重写 parse |
| 复现证据包、大结果包、与 paper 绑定的二进制 | **AutoML assets + w-storage presign**（已有） | 证据链与 verified-by-N 已在此路径 |
| Hub DB | **仅指针**（`daas_*` / `object_key` / `w_papers_*`） | 禁止第二份 corpus |

**不选 A**（纯对象存储无知识面）— 失去 parse/KG。
**不选 B**（全部塞进 DaaS 文档流水线）— 污染证据语义、放大跨产品耦合。

P0 **零依赖存储**：只建表与 API，不接 MinIO/DaaS；P1 书架可无字节。

### 12.5 Compute / 「Py」Agent / 「Deeply Compute」附着点

| 名称 | 决议 | 附着方式 |
|---|---|---|
| **W-Compute**（`00-24-w-compute`） | **算力底层（可选）**；Hub/AutoML **不派活** | `compute_profiles.profile` JSON 可含 `w_compute_instance_id`；仅 provenance |
| 「Deeply Compute」 | 舰队无独立产品 → **别名映射到 W-Compute** | 文档统一用 W-Compute |
| 「Py」Agent | **非独立产品名**；未来可指 Python 侧执行工人 | P0 不授权 agent principal；未来身份绑定与撤销契约另行设计 |
| AutoML `compute` API | 保持 provenance-only | 不新增 scheduler |

P0 project ACL does not enroll or authenticate a distinct agent principal.
Agent membership enrollment returns 501 after the live Owner check. Software
using an API key still acts as that key's scholar; an agent row is not an
authorization or revocation boundary. Future agent binding must verify the
credential-to-agent identity, intersect credential scopes, agent membership and
live human sponsor membership, prohibit agent governance, and revoke access
when either membership is removed. Existing v2 agents/devices and signed
reproduction evidence are unchanged.

### 12.6 arXiv / Auto-Research 集成草图

```
[User/Agent] 提问 / 种子文献
      │
      ▼
research_run (Hub 编排 · 状态机 · 审计)
      │  tools (pluggable)
      ├─ shelf.search / OpenAlex cite-graph（经 DaaS 或 stub）
      ├─ W Papers work-artifact 读（arXiv/DOI）
      ├─ （可选）alphaXiv MCP：搜/读/PDF QA — 仅作 tool provider
      ├─ （可选）DaaS deep-research / paper-audit
      └─ Asta-class planner（Ai2 流程组件：plan→tool→cite）— 默认适配器接口
      │
      ▼
project_asset(research_draft) + 引用清单
      │
      ▼
human gate / reproduce_binding → verified-by-N
```

**P3 默认优先级（已拍板，可后调）：**

1. Hub 内 `research_run` 状态机 + **NoopExecutor stub**（fixtures → 结构化草稿）— 先打通审计与 UI。
2. **Ai2 Asta-class** 适配器（流程与引用格式）；不宣称产品对等。
3. **alphaXiv MCP** 作为可选 literature tool（不是整条编排）。
4. Arbor / AutoResearchClaw 等列为 P3+ 实验 executor。

**硬边界：** Hub 不自建 arXiv 爬虫；不把 Auto Research 营销为「已对齐 Asta」。

### 12.7 具体可开工切片（Devin，不堵 infra）

#### P0 — 项目骨架（仅 `20-01-automl`）

1. Migration `008_research_projects.sql`：`research_projects` · `project_members` · `audit_events`（append-only）。
2. Scopes：`projects:read|write|admin` 写入 `automl-core`（`ALL` / `SCHOLAR_DEFAULT` 增量）。
3. API（utoipa 生成；客户端路径带 `/api`；下列为已实现 P0 路由，未注册的后续路由并不承诺返回 501）：
   - `POST/GET /api/v1/research-projects`
   - `GET/PATCH /api/v1/research-projects/:id`
   - `POST/GET /api/v1/research-projects/:id/members`
   - `DELETE /api/v1/research-projects/:id/members/:member_id`
   - `GET /api/v1/research-projects/:id/audit`
   - `GET /api/v1/research-projects/:id/papers` → members-only **空列表**（Fuel 写入属于 P1）
   - project agent enrollment is not available in P0 (501 where registered)
4. ACL helper：`require_project_role(project_id, min_role)`。
5. CLI：`automl project create|list|get|member-add`。
6. Tests: lifecycle transitions, nonmember denial, keyset pagination, and governance/concurrency gates.
7. **不做：** DaaS 绑定实调、PDF、research_run 执行器、Web 大页（可选一个列表占位）。
8. Audit records the accountable scholar and actor kind, but not API-key ID or agent identity. Metadata events keep changed field names, not before/after snapshots. Retained audit is unreadable through project routes after soft deletion; the append-only trigger is not protection against privileged database operators. No 90-day purge is implemented; retention and any purge compatible with audit retention remain deferred.

#### P1 — Fuel 书架（stubs 允许）

1. Migration：`project_papers`（含 `fuel_role`、外指针列、`(project_id, doi)` 规范化唯一可空）。
2. `POST /v1/research-projects/:id/papers` 接受判别 body：`{doi}` | `{arxiv_id}` | `{w_papers_work_id}` | `{openalex_id}` | `{daas_document_id}`。
3. Enricher 接口 + 三种实现：`StubEnricher`（默认 CI）· `PublicOpenAlexEnricher`（可选）· `DaasOpenAlexEnricher`（`DAAS_OPENALEX_BASE` 有则启用）。
4. W Papers：仅存 id；若 `WPAPERS_BASE` 设置则拉取标题缓存，失败不阻断建条目（降级为 raw id）。
5. CLI：`automl project paper-add --doi ...`。
6. 跨仓：在 `30-29-w-papers` 开跟进 Issue「export-to-research-project 契约」（互链 #30）；**不阻塞**本仓 P1 stub。

#### P2 — DaaS 解析/检索挂接（stub-attach 已落地）

1. Migration `010_project_daas.sql`：`project_bindings`（每项目至多一行 active，partial unique index；rebind 同事务退休旧行并同步 `research_projects.w_daas_workspace_id`）· `project_assets`（kind CHECK 对齐 §12.3；`note`/`search_snapshot` 为内容 kind，其余为指针 kind 须带 `uri` 或 `daas_artifact_id`；soft delete）。
2. `Daas` provider 双实现：`StubDaaS`（默认 CI，bind 一律 `verified:false`、`stub-doc-*` id、空 search 页）· `LiveDaas`（仅 `DAAS_API_BASE` 设置时启用；4 s 超时、512 KiB 上限、禁 redirect；workspace 探活失败降级 `verified:false` 不阻塞绑定）。
3. API（沿用 `require_project_role`）：`PUT/GET/DELETE .../daas/binding`（写=Owner+`projects:admin`，读=Viewer+）；`POST/GET .../assets`、`GET/DELETE .../assets/{id}`（写=Member+`projects:write`，读=Viewer+，keyset + `kind` 过滤）；`POST .../papers/from-pdf`（Member+，未绑定且需上行→409，上行失败→503 诚实失败）；`POST .../search`（Viewer+，`persist:true` 需 `projects:write`+Member 并落 `search_snapshot` asset）。
4. CLI：`automl project bind-daas|unbind-daas|binding`、`asset-add|assets|asset-get|asset-rm`、`import-pdf`、`search`。
5. 跨仓：live W-DaaS workspace/documents/search 端点契约在 `00-03-data-asset-platform` 跟进 Issue（互链 #30）；**不阻塞**本仓 stub。
6. **不做（留给 P3/P4）：** `research_run` / Asta executor、`reproduce_binding`、Human gate 页；live DaaS parse/KG 不作为硬依赖。

#### P3+ 提醒（不在本次实现）

- P3：`research_run` + NoopExecutor → Asta adapter → 可选 alphaXiv tool。
- P4：`reproduce_binding` + 项目 Human gate 页 + webhook 事件。

### 12.8 决议清单（关闭原开放问题）

| 原待拍板 | 决议 | 可逆性 |
|---|---|---|
| 文件策略混合 C | **采纳为默认** | 高（指针层可迁移） |
| W-Compute 仅算力底层 | **确认**；Deeply Compute→别名 | 文档级 |
| 「Py」指代 | **= Python 执行工人**，非独立产品；P0 project ACL does not establish agent request identity | Future contract required |
| P3 首接 | **Noop stub → Asta-class adapter；alphaXiv 作 tool** | 适配器可插拔 |

**NEED_USER（极少）：** 无阻塞项。可选日后确认：lab 组织是否一等公民、对外品牌是否单独叫「Py」。

### 12.9 Code vs docs（诚实快照 2026-09-26 Asia/Shanghai）

| 层 | 状态 |
|---|---|
| Docs / Issue #30 | ✅ 设计 + v1.1 细化 |
| `research_project*` code / migration | Implemented in open PR #32 at reviewed commit `28a3567`; not merged or deployed. Review gates are addressed in a follow-up and remain pending verification. |
| Fuel / research_run / project_asset | Outside P0; `/papers` is an empty member-only page. P1 fuel is tracked separately in open PR #34. |
| v2 papers · reproduce · assets · compute_profiles · agents | ✅ 可复用底座 |
| Design / docs | This document records the P0 identity, visibility, lifecycle, audit, and stub limits; implementation status must be updated with the shipped revision |

