#!/usr/bin/env python3
"""Mutation tests proving the CI contract fails closed."""

from pathlib import Path
import importlib.util

spec = importlib.util.spec_from_file_location("validator", Path(__file__).with_name("validate-ci-contract.py"))
assert spec and spec.loader
validator = importlib.util.module_from_spec(spec)
spec.loader.exec_module(validator)

workflow = Path(".gitea/workflows/ci.yml").read_text()
assert not validator.validate(workflow), validator.validate(workflow)

mutations = {
    "run id": ("${GITHUB_RUN_ID:?", "${GITHUB_RUN_ID:-}"),
    "run attempt": ("${GITHUB_RUN_ATTEMPT:?", "${GITHUB_RUN_ATTEMPT:-}"),
    "job identity": ("${GITHUB_JOB:?", "${GITHUB_JOB:-}"),
    "workspace identity": ("${GITHUB_WORKSPACE:?", "${GITHUB_WORKSPACE:-}"),
    "source sibling": ("run_root/source", "run_root/checkout"),
    "tool pin": ("CARGO_AUDIT_VERSION: '0.22.2'", "CARGO_AUDIT_VERSION: latest"),
    "tool install": ("cargo install cargo-audit --version", "cargo install cargo-audit --ignored-version"),
    "tool assertion": ("$audit_bin --version", "$audit_bin audit --version"),
    "crypto-bigint exception": ("--ignore RUSTSEC-2026-0387", "--ignore RUSTSEC-2026-0388"),
    "crypto-bigint boundary gate": (
        "scripts/check-crypto-bigint-boundary.py",
        "scripts/check-crypto-bigint-boundary-skipped.py",
    ),
    "crypto-bigint boundary mutation test": (
        "scripts/test-crypto-bigint-boundary.py",
        "scripts/test-crypto-bigint-boundary-skipped.py",
    ),
    "crypto-bigint reverse graph": (
        "cargo tree --workspace --all-features --target all --locked --edges normal,build,dev",
        "cargo-tree-crypto-bigint-boundary-removed",
    ),
    "crypto-bigint expanded paths": (
        "-i crypto-bigint --prefix depth --no-dedupe",
        "cargo-tree-crypto-bigint-boundary-removed",
    ),
    "crypto-bigint rationale": (
        "never validates provider JWTs locally",
        "validates provider JWTs locally",
    ),
    "actions credential": ("CI_GITEA_ACTIONS_TOKEN", "GITEA_TOKEN"),
    "release credential": ("CI_GITEA_RELEASE_TOKEN", "GITEA_TOKEN"),
    "canonical workflow identity": (
        "EXPECTED_MAIN_WORKFLOW_PATH: 'ci.yml@refs/heads/main'",
        "EXPECTED_MAIN_WORKFLOW_PATH: '.gitea/workflows/ci.yml'",
    ),
    "user agent": ("w-release-exact-main/1", "Python-urllib/3"),
    "main branch": ("head_branch') == 'main'", "head_branch') == branch"),
    "main success": ("conclusion') == 'success'", "conclusion') is not None"),
    "static gate": ("dynamically linked", "dynamic-link-check-removed"),
    "runner wrapper isolation": (
        "env -u RUSTC_WRAPPER -u RUSTC_WORKSPACE_WRAPPER -u CARGO_BUILD_RUSTC_WRAPPER",
        "cargo-wrapper-environment-preserved",
    ),
    "aws-lc gate": ("aws-lc-(rs|sys)", "aws-lc-check-removed"),
    "S3 feature proof": ("features = [\"presign\", \"s3\"]", "features omitted"),
    "S3 code proof": ("S3Storage::new", "S3 constructor omitted"),
    "fmt gate": ("cargo fmt --all --check", "cargo fmt --all"),
    "clippy deny": (
        "clippy --workspace --all-targets --all-features --locked -- -D warnings",
        "clippy --workspace --all-targets --all-features --locked",
    ),
    "security hygiene": ("scripts/check-security-hygiene.py", "scripts/check-security-hygiene-skipped.py"),
    "security hygiene mutations": (
        "scripts/test-security-hygiene.py",
        "scripts/test-security-hygiene-skipped.py",
    ),
}

for name, (old, new) in mutations.items():
    assert old in workflow, f"invalid mutation fixture: {name}"
    changed = workflow.replace(old, new)
    assert validator.validate(changed), f"validator accepted mutation: {name}"

print(f"validated {len(mutations)} fail-closed CI mutations")
