# Source and third-party notices

W-World-Labs newly authorizes its previously unlicensed application source under
MIT for this distribution, retaining existing author notices. The root LICENSE
does not relicense material that already has other terms or grant rights to
external data, model weights, hosted services or third-party papers.

## Original research declarations

The original project metadata names W Intelligence Lab / Auto.ml as author.
Preserve that attribution and existing file headers when copying these sources.

- `archive/AutoML/papers/genesis/` and `wifi-csi-har/` declare CC-BY-4.0 in their
  original `pyproject.toml`. The original paper class explicitly says it is an
  original design, copyright 2026 W Intelligence Lab / Auto.ml, released under
  CC-BY-4.0. `papers/template/` and copies of that class retain those terms.
  The full license is in `licenses/CC-BY-4.0.txt`.
- `archive/AutoML/papers/openbb/` declares Apache-2.0. Its original experiment
  callers remain under those terms; `automl-paper.cls` keeps its explicit
  CC-BY-4.0 notice. The Apache text is in `licenses/Apache-2.0.txt`.
- Bibliography and reference manifests identify external works; those citations
  do not grant redistribution rights to the cited works. Referenced third-party
  paper/manuscript files, datasets, weights and unreviewed generated media are not bundled.
- OpenBB implementation/configuration and the missing original Wi-Fi data loader
  are not present in this source snapshot. No external engine or protected plugin
  implementation was copied to fill those gaps.

## Library and asset boundaries

Rust components are public source dependencies, not vendored implementations.
The exact reviewed W-World-Labs source commits are in Cargo manifests. Cargo
generates registry checksums in CI; a source publication is distinct from crates.io
publication or consumer installation. npm/uv lock declarations describe separately
obtained packages; their licenses do not become the repository's MIT license.

The original archived Vite scaffold logo is separately attributed to Vite and
its contributors under the retained MIT text in `licenses/vite-mit.txt`.
Auto.ml branding SVGs reference remotely obtained Outfit/Inter fonts; no font
binary is included. Retain the font providers' applicable OFL terms when producing
an artifact that actually embeds font software. Names and logos do not imply
endorsement by an upstream project.

Public source CI publishes only the reviewed source and its evidence. Native
libraries, compiled product redistributions, research datasets and models need an
artifact-specific rights review when separately distributed.
