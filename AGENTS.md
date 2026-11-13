# Auto.ml public source

Keep changes small and preserve the real research platform, local reproduction
CLI, authentication, project ACLs, state machines, signatures and existing product
assertions. Do not replace implementations with stubs or anonymous fallbacks.

The current application is the root Cargo workspace in `crates/` and React app
in `web/`. `automl-core` owns shared wire contracts; the server is a data plane;
explicit client-side CLI commands own reproduction compute. Preserve these roles.
Do not execute submitted repositories, train models or fetch datasets on the server.

Use immutable public source dependencies. The W-ID SDK and axum adapter must share
their reviewed SDK contract; the adapter's axum-extra cookie type is part of its
API. Keep rustls on ring, the real S3/presign path and optional Radar integration.
Do not introduce private registries, vendor unpublished implementations or expose
provider secrets, user documents, client credentials or operator inventories.

`archive/AutoML/papers/` preserves original research model, training, experiment,
template and reproduction source. Its declared CC-BY-4.0 / Apache-2.0 terms are
separate from the application's MIT license. Read `THIRD-PARTY-NOTICES.md` and
`source-provenance.json`. Missing original data loaders and separately supplied
OpenBB code are recorded limitations; do not invent substitutes or claim results.
Model weights, datasets, captured documents and generated outputs need their own
rights and explicit user action. Source CI performs no research training.

Use standard public GitHub-hosted CI for product dependency installation, builds,
tests and packaging. Local work is limited to source edits and metadata/security
review. Keep existing tests; add tests only for a particularly necessary change.
Do not operate production or install consumers as part of a source release.

Read the modern-web-guidance skill before frontend work. Retain the current React,
Vite, DOMPurify, generated OpenAPI types, async states and browser bootstrap gates.
Do not hand-write generated API definitions or weaken the backend's scope checks.

The source artifact packages committed files and license notices. Native lockfile
generation happens in public CI; generated locks must be inspected and committed
before full validation and source packaging can pass. The old private-runner and
operator-deployment contract scripts remain historical source, not public service
activation evidence. Never import private Git history or operational configuration.
