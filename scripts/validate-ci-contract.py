#!/usr/bin/env python3
"""Fail-closed checks for AutoML's formal CI and release contract."""

from pathlib import Path
import re
import sys


def validate(text: str) -> list[str]:
    errors: list[str] = []

    def require(fragment: str, message: str) -> None:
        if fragment not in text:
            errors.append(message)

    for identity in ("GITHUB_REPOSITORY", "GITHUB_RUN_ID", "GITHUB_RUN_ATTEMPT", "GITHUB_JOB", "GITHUB_WORKSPACE"):
        if text.count(f'${{{identity}:?') < 3:
            errors.append(f"all formal jobs must require {identity}")
    require('${GITHUB_WORKSPACE}/.ci-runs/${repo_slug}/${GITHUB_RUN_ID}-${GITHUB_RUN_ATTEMPT}-${job_slug}', "run root must stay checkout-compatible and bind repo, run, attempt, and job")
    for sibling in ("source", "tools", "artifacts", "tmp"):
        require(f'run_root/{sibling}', f"missing isolated {sibling}/ sibling")
    require("CARGO_AUDIT_VERSION: '0.22.2'", "cargo-audit version must be exact and pinned")
    require('cargo install cargo-audit --version "$CARGO_AUDIT_VERSION"', "cargo-audit must be installed run-locally")
    require('test "$($audit_bin --version)" = "cargo-audit $CARGO_AUDIT_VERSION"', "cargo-audit version must be asserted")
    require("issues/19", "the no-fix RSA advisory exception needs an owned tracker")
    require("--ignore RUSTSEC-2023-0071", "the exact no-fix RSA advisory exception must remain explicit")
    require("--ignore RUSTSEC-2026-0387", "the narrowly reviewed crypto-bigint advisory exception must remain explicit")
    require(
        "scripts/check-crypto-bigint-boundary.py",
        "the crypto-bigint advisory exception must fail closed if its dependency boundary changes",
    )
    require(
        "scripts/test-crypto-bigint-boundary.py",
        "the crypto-bigint boundary gate must have mutation coverage",
    )
    require(
        "cargo tree --workspace --all-features --target all --locked --edges normal,build,dev",
        "the crypto-bigint exception must inspect the locked all-target/all-feature reverse graph",
    )
    require(
        "-i crypto-bigint --prefix depth --no-dedupe",
        "the crypto-bigint exception must inspect every expanded dependency path",
    )
    require("never validates provider JWTs locally", "the crypto-bigint exception needs its reviewed HS256 boundary")
    require("CI_GITEA_ACTIONS_TOKEN", "dedicated Actions-read credential is required")
    require("CI_GITEA_RELEASE_TOKEN", "dedicated release credential is required")
    require("CI_GIT_CLONE_TOKEN", "dedicated clone credential is required")
    require("CARGO_REGISTRIES_GITEA_TOKEN", "dedicated Cargo credential is required")
    require(
        "EXPECTED_MAIN_WORKFLOW_PATH: 'ci.yml@refs/heads/main'",
        "exact-main gate must match Gitea's canonical workflow identity",
    )
    require("User-Agent': 'w-release-exact-main/1", "exact-main query needs stable User-Agent")
    for predicate in ("head_sha", "head_branch') == 'main'", "event') == 'push'", "conclusion') == 'success'"):
        require(predicate, f"exact-main gate missing predicate: {predicate}")
    require('! file "$bin" | grep -F \'dynamically linked\'', "release binaries need static gate")
    wrapper_free_build = (
        "env -u RUSTC_WRAPPER -u RUSTC_WORKSPACE_WRAPPER "
        "-u CARGO_BUILD_RUSTC_WRAPPER \\\n"
        "            cargo build --release --locked --target x86_64-unknown-linux-musl"
    )
    require(
        wrapper_free_build,
        "formal build must remove inherited Rust wrappers so deep run-unique paths cannot overflow sccache's Unix socket",
    )
    require("aws-lc-(rs|sys)", "all-features graph must reject AWS-LC")
    require("features = [\"presign\", \"s3\"]", "Smithy exception needs feature proof")
    require("S3Storage::new", "Smithy exception needs S3 code proof")
    require(".presign_put(", "Smithy exception needs presign code proof")
    require("cargo fmt --all --check", "CI must enforce rustfmt")
    require("clippy --workspace --all-targets --all-features --locked -- -D warnings", "CI must fail clippy on warnings")
    require("scripts/check-security-hygiene.py", "CI must run issue #1 security hygiene gate")
    require("scripts/test-security-hygiene.py", "CI must mutation-test the security hygiene gate")
    if re.search(r"token:\s*\$\{\{ secrets\.(?:CI_GITEA_ACTIONS_TOKEN|CI_GITEA_RELEASE_TOKEN)", text):
        errors.append("Actions/release credentials must not be used for checkout")
    return errors


def main() -> int:
    path = Path(sys.argv[1] if len(sys.argv) > 1 else ".gitea/workflows/ci.yml")
    errors = validate(path.read_text())
    for error in errors:
        print(f"error: {error}", file=sys.stderr)
    return int(bool(errors))


if __name__ == "__main__":
    raise SystemExit(main())
