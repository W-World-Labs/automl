# Auto.ml

Auto.ml is an automated research and reproducible-experiment platform. This source
contains the current Rust API/data plane, React web application, local `automl`
CLI, migrations and existing application tests. It also retains the original
research experiment and model source under `archive/AutoML/papers/`.

The server stores and validates papers, reviews, project membership, experiments,
code pointers and signed reproduction evidence. Reproduction compute runs only
through explicit client CLI commands on the user's machine. The server is not a
training worker, execution sandbox, model installer or repository-code store.

## Source layout

- `crates/automl-core`: shared wire contract, error codes and pagination.
- `crates/automl-server`: authenticated Axum/PostgreSQL API, project ACLs,
  research state machines, storage, integration adapters and server tests.
- `crates/automl-cli`: login, project/paper operations, local reproduction and
  signed evidence submission, with its original end-to-end acceptance journey.
- `web`: current React/Vite application, generated OpenAPI types and tests.
- `archive/AutoML/papers`: original scientific examples, five Wi-Fi CSI model
  implementations and the real cross-validation/optimizer/evaluation code.

Private history, production configuration, operator CI/deployment assets,
unreviewed manuscripts and generated paper/media artifacts, runtime data and installed dependency
trees are excluded. Read [source-provenance.json](source-provenance.json) for the
scope and [THIRD-PARTY-NOTICES.md](THIRD-PARTY-NOTICES.md) for license boundaries.

## Configure your own instance

Use Rust 1.96.0, Node.js 22.16.0 and PostgreSQL 18 as selected by public CI.
Copy `.env.example` into your private runtime configuration and supply your own
database, storage, W-ID client and secrets. Register your exact callback with the
intended W-ID instance. Missing SSO configuration leaves its routes unavailable;
it does not grant anonymous application access. The development-admin route
remains disabled by default and unavailable in production.

The current application uses immutable public W component source pins; it needs
no private package registry. Preserve the W-ID SDK/adapter pair and the cookie API
version when upgrading. Generate and inspect Cargo locks and the archived frontend npm lock through
public CI, then retain them in source before claiming a validated distribution.

For an independently configured development environment:

```sh
cd web
npm ci
npm run build
cd ..
cargo run -p automl-server
cargo run -p automl-cli -- --help
```

Apply migrations only to a database you own. Configure storage and integration
origins explicitly. This source grants no access to hosted services and performs
no production installation or activation.

## Research source and external inputs

The Wi-Fi example keeps MLP, 1D-CNN, LSTM, CNN-GRU and CSIFormer implementations,
along with seeded stratified K-fold splitting, Adam training, loss/backpropagation
and classification metrics. Genesis examples retain their original analysis and
simulation code. OpenBB examples retain their original configuration and pretrain
callers and assertions.

Two original input gaps are explicit: the Wi-Fi runner imports
`experiments.data.loader`, which is absent from the reviewed source checkout;
OpenBB callers require separately supplied OpenBB implementation/configuration
through `OPENBB_ROOT`. No substitute loader, external implementation, dataset or
model weight is bundled. These examples are source for review, not a claim of
complete training reproducibility, measured accuracy or independently reproduced
paper results. Read each experiment's original license and obtain external input
rights separately before running it. Existing dataset-license labels are historical
descriptions, not redistribution evidence or a new data license grant.

## Validation

Public CI scans source and reachable history, resolves public Cargo dependencies,
runs the existing current-application Rust/frontend checks and packages committed
source with its manifest, notices and checksums. It uses synthetic PostgreSQL and
loopback test services, not production credentials or providers. No paper training,
model download, dataset download or hardware performance test is performed.

The old private-runner/deployment contract scripts are preserved as historical
source, but their operator-specific environment is outside the public validation
scope. Current product unit/integration/frontend and CLI acceptance assertions
remain intact. Actual results belong to the exact source commit and CI run.

## Licenses

Application code owned by W-World-Labs/Auto.ml is MIT, with existing authors and
notices retained. The research subprojects' declared CC-BY-4.0 and Apache-2.0 terms
remain in force; the root MIT text does not replace them. Dependencies, external
papers, datasets, model weights, fonts and service access retain separate terms.
