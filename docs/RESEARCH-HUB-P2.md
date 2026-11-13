# Research Hub P2: DaaS binding and project assets

P2 implements the DaaS attach surface from design v1.1 §12.7 on top of P0/P1:
a one-to-one `project_binding` between a research project and a W-DaaS
workspace, `project_asset` pointers for hub outputs, a PDF→DocumentAsset
intake, and the project-scoped hybrid search entry. Everything works against
`StubDaaS` — no live W-DaaS deployment is required for dev or CI.

## Workspace binding

`PUT /api/v1/research-projects/{id}/daas/binding` with
`{"workspace_id": "..."}` binds (or rebinds) the project. It requires
`projects:admin` and project Owner. The response carries the binding row plus
`provider_name` (`stub` or `daas`) and `verified` — true only when a
configured upstream confirmed the workspace. Binding to a new workspace
retires the previous row; `research_projects.w_daas_workspace_id` is synced in
the same transaction, so the project DTO and the binding log tell one story.

`GET /api/v1/research-projects/{id}/daas/binding` returns the active row
(Viewer+) or 404. `DELETE` retires it (Owner) and clears the project pointer;
unbinding when nothing is bound is an honest 404. A workspace id supplied at
project create time becomes an unverified binding row automatically.

```sh
automl project bind-daas PROJECT_UUID --workspace-id ws-lab-1
automl project binding PROJECT_UUID
automl project unbind-daas PROJECT_UUID
```

## Project assets

`project_asset` rows register hub outputs: `note` and `search_snapshot` are
content kinds whose payload may live in `metadata` alone; `kg_slice`,
`research_draft`, `experiment_spec`, `sim_config`, `result_bundle`, `figure`,
and `notebook_ptr` are pointer kinds that must carry `uri` or
`daas_artifact_id` (400 otherwise).

`POST /api/v1/research-projects/{id}/assets` (Member+) creates,
`GET /assets` (Viewer+) keyset-paginates with an optional `kind` filter,
`GET /assets/{asset_id}` reads one, `DELETE /assets/{asset_id}` soft-deletes
(Member+). Unknown kinds, non-object metadata, control characters, and
metadata over 64 KiB are 400.

```sh
automl project asset-add PROJECT_UUID --kind note --title 'Kickoff' \
  --metadata '{"body":"first notes"}'
automl project asset-add PROJECT_UUID --kind kg_slice --daas-artifact-id art-9
automl project assets PROJECT_UUID [--kind note] [--all]
automl project asset-get PROJECT_UUID ASSET_UUID
automl project asset-rm PROJECT_UUID ASSET_UUID
```

## PDF intake

`POST /api/v1/research-projects/{id}/papers/from-pdf` registers a PDF as a
DaaS DocumentAsset inside the bound workspace and puts the returned
`daas_document_id` on the fuel shelf (Member+). An unbound project gets 409
when the upstream call would be needed; passing `daas_document_id` directly
records the pointer without any upstream call. A live upstream failure is an
honest 503 — never a fabricated id.

```sh
automl project import-pdf PROJECT_UUID --title paper.pdf [--uri s3://...]
automl project import-pdf PROJECT_UUID --daas-document-id doc-existing
```

## Hybrid search entry

`POST /api/v1/research-projects/{id}/search` with `{"query": "...", "limit": N,
"persist": false}` searches the bound workspace. Viewer+ may search; an
unbound project is 409. `persist: true` additionally requires `projects:write`
and Member, and records a `search_snapshot` asset holding query, provider,
and hits. The stub provider has no corpus and honestly returns an empty hit
page.

```sh
automl project search PROJECT_UUID --query 'attention' [--persist] [--limit 10]
```

## Provider selection

| Configuration | Behavior |
| --- | --- |
| No `DAAS_*` variables | `StubDaaS`: unverified binds, `stub-doc-*` ids, empty search pages |
| `DAAS_API_BASE` (± `DAAS_API_TOKEN`) | Bounded HTTP adapter (4 s, 512 KiB cap, no redirects) |

The live adapter assumes `GET {base}/api/workspaces/{id}`,
`POST {base}/api/workspaces/{id}/documents`, and
`GET {base}/api/workspaces/{id}/search?q=&limit=` — a best-effort contract
pending the cross-repository W-DaaS follow-up (tracked from
[AutoML research hub](https://github.com/W-World-Labs/automl)).
Workspace lookups never fail the bind: outages degrade to `verified: false`.
Document import and hybrid search surface upstream failure as 503.

## Verification

Migration `010_project_daas.sql` creates `project_bindings` (one active row
per project enforced by a partial unique index) and `project_assets` (kind
CHECK aligned with design §12.3, pointer/content rule, soft delete). The
focused `project_daas_integration` suite needs only a disposable PostgreSQL:

```sh
cargo test -p automl-server --test project_daas_integration
```

It covers the binding lifecycle and audit rows, Owner/scope ACL on bind and
unbind, asset kind validation, keyset pagination and kind filters,
soft-delete reads, the unbound 409 paths, stub search and snapshot
persistence, and the stub PDF intake.
