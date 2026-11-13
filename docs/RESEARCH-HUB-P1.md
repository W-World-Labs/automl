# Research Hub P1: project fuel shelf

P1 implements design v1.1 §12.7 on top of P0. The shelf stores external
identifiers, notes, tags, and optional metadata caches; it contains no PDF bytes
or research execution state.

## API and CLI

`POST /api/v1/research-projects/{id}/papers` accepts exactly one nonempty
primary identifier: `doi`, `arxiv_id`, `w_papers_work_id`, `openalex_id`, or
`daas_document_id`. Optional fields are `fuel_role`, `notes`, and `tags`.
Fuel roles are `seed` (default), `related`, `baseline`, `cited_by`, and `reading`.
Creation requires `projects:write` and project Member or Owner membership.

```sh
automl project paper-add PROJECT_UUID --doi https://doi.org/10.1234/example \
  --fuel-role baseline --notes 'Comparison method' --tags vision,baseline
automl project paper-add PROJECT_UUID --arxiv-id 2401.01234
automl project paper-add PROJECT_UUID --w-papers-work-id WORK_ID
```

Creation returns 201 and the shelf entry. Invalid identifiers or request
shapes return 400. A duplicate normalized DOI in the same project's live shelf
returns 409. DOI normalization removes common DOI URL / `doi:` prefixes, trims
surrounding whitespace, and lowercases the identifier. Another project can
contain the same DOI. The entry and its `paper.added` audit event commit together.

`GET /api/v1/research-projects/{id}/papers?limit=20&cursor=...` returns
`{data, next_cursor}` with descending `(created_at, id)` keyset pagination.
Treat the cursor as opaque and URL-encode it when constructing requests.
Reading requires `projects:read` and project Viewer, Member, or Owner membership,
including for `public_meta` projects. Nonmembers receive 403; deleted projects
receive 404.

Cache fields are `cached_title`, `cached_year`, and `cached_authors`. Stub mode
leaves title/year null and authors empty; clients can display the supplied
identifier. Enrichment can discover additional external pointers, so the
exactly-one rule applies to the request, not to persisted metadata.
Migration `009_project_papers.sql` reserves `deleted_at` and a nullable weak
`paper_id` link to v2 papers. P1 exposes only add/list operations.

## Optional metadata sources

Configuration is read once at server startup:

| Configuration | Behavior |
| --- | --- |
| No enrichment variables | `StubEnricher`; no upstream dependency |
| `OPENALEX_MODE=public` | Optional public OpenAlex DOI/work lookup |
| `DAAS_OPENALEX_BASE` | OpenAlex-compatible gateway; takes precedence over public mode |
| `WPAPERS_BASE` | Optional W Papers work/arXiv title cache lookup |

Provider failures degrade to the original pointer. Requests have bounded time
and response size. AutoML never fetches arXiv.org. The arXiv path stays local in
stub mode or uses W Papers when configured.

The proposed W Papers endpoints are `GET /api/works/{work_id}` and
`GET /api/works/arxiv/{arxiv_id}` under `WPAPERS_BASE`. Gateway compatibility
and W Papers export remain external integration work, tracked in
[W Papers integration](https://github.com/W-World-Labs/w-papers), linked to
[AutoML research hub](https://github.com/W-World-Labs/automl).
They are not prerequisites for the local stub flow.

## Verification

The focused `project_papers_integration` suite uses real PostgreSQL and an
explicit stub provider. Provider unit tests use local HTTP fixtures. Run the
focused suite with `DATABASE_URL` set to a disposable PostgreSQL database:

```sh
cargo test -p automl-server --test project_papers_integration
```

The workspace acceptance test additionally exercises `project paper-add`
through the real CLI and server binaries. API schemas are generated from
Rust handlers into `web/openapi.json` and `web/src/api/schema.d.ts`.
