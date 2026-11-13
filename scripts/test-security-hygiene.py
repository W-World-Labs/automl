#!/usr/bin/env python3
"""Mutation tests proving repository security policy fails closed."""

from __future__ import annotations

import importlib.util
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
CHECKER_PATH = Path(__file__).with_name("check-security-hygiene.py")

spec = importlib.util.spec_from_file_location("security_hygiene", CHECKER_PATH)
assert spec and spec.loader
checker = importlib.util.module_from_spec(spec)
spec.loader.exec_module(checker)

base_files = checker.tracked_files()


def read_base(path: str) -> str:
    return (ROOT / path).read_text()


baseline = checker.validate(base_files, read_base)
assert not baseline, baseline


def mutate_text(path: str, old: str, new: str) -> dict[str, str]:
    text = read_base(path)
    assert old in text, f"invalid mutation fixture for {path}: {old}"
    return {path: text.replace(old, new, 1)}


def run_mutation(
    *,
    overlay: dict[str, str] | None = None,
    add: tuple[str, ...] = (),
    remove: tuple[str, ...] = (),
) -> list[str]:
    changed = overlay or {}
    files = sorted((set(base_files) | set(add)) - set(remove))

    def read(path: str) -> str:
        if path in changed:
            return changed[path]
        return read_base(path)

    return checker.validate(files, read)


def expect_failure(label: str, errors: list[str], fragment: str) -> None:
    assert errors, f"checker accepted mutation: {label}"
    assert any(fragment in error for error in errors), (label, fragment, errors)


cases: list[tuple[str, list[str], str]] = [
    (
        "forced tracked runtime env",
        run_mutation(overlay={"runtime.env": ""}, add=("runtime.env",)),
        "tracked secret-like file",
    ),
    (
        "forced tracked regional env",
        run_mutation(overlay={"deploy/.env.hk-prod": ""}, add=("deploy/.env.hk-prod",)),
        "tracked secret-like file",
    ),
    (
        "forced tracked private key",
        run_mutation(overlay={"deploy/private.key": ""}, add=("deploy/private.key",)),
        "tracked secret-like file",
    ),
    (
        "forced tracked Java keystore",
        run_mutation(overlay={"deploy/runtime.jks": ""}, add=("deploy/runtime.jks",)),
        "tracked secret-like file",
    ),
    (
        "forced tracked service account",
        run_mutation(
            overlay={"deploy/service-account.prod.json": ""},
            add=("deploy/service-account.prod.json",),
        ),
        "tracked secret-like file",
    ),
    (
        "env ignore exception",
        run_mutation(
            overlay=mutate_text(".gitignore", "!w.project.tsv", "!.env.hk-prod\n!w.project.tsv")
        ),
        "env.hk credential exception",
    ),
    (
        "missing keystore ignore",
        run_mutation(overlay=mutate_text(".gitignore", "*.jks", "*.jks-removed")),
        ".gitignore must reject secret-like pattern",
    ),
    (
        "secret hidden in allowed example",
        run_mutation(
            overlay={
                ".env.example": "JWT_" + "SECRET=" + "0123456789abcdef" * 4 + "\n"
            }
        ),
        "non-placeholder sensitive assignment",
    ),
    (
        "private key hidden in documentation",
        run_mutation(
            overlay={
                "README.md": "-----BEGIN " + "PRIVATE KEY-----\nredacted\n"
            }
        ),
        "tracked text contains private-key material",
    ),
    (
        "missing canonical unit",
        run_mutation(remove=(checker.CANONICAL_UNIT,)),
        "only active systemd template",
    ),
    (
        "second active unit",
        run_mutation(
            overlay={"deploy/systemd/automl-server.service": "[Service]\nExecStart=/bin/false\n"},
            add=("deploy/systemd/automl-server.service",),
        ),
        "only active systemd template",
    ),
    (
        "root canonical unit",
        run_mutation(
            overlay=mutate_text(checker.CANONICAL_UNIT, "User=automl", "User=root")
        ),
        "dedicated automl user",
    ),
    (
        "stale environment authority",
        run_mutation(
            overlay=mutate_text(
                checker.CANONICAL_UNIT,
                f"EnvironmentFile={checker.CANONICAL_ENV_FILE}",
                "EnvironmentFile=/opt/automl/data/automl-v2/runtime.env",
            )
        ),
        "exactly one fail-closed EnvironmentFile",
    ),
    (
        "inline environment",
        run_mutation(
            overlay=mutate_text(
                checker.CANONICAL_UNIT,
                "Restart=on-failure",
                "Environment=EXAMPLE_SETTING=example\nRestart=on-failure",
            )
        ),
        "must not contain inline Environment",
    ),
    (
        "missing restrictive umask",
        run_mutation(overlay=mutate_text(checker.CANONICAL_UNIT, "UMask=0077", "UMask=0022")),
        "UMask=0077",
    ),
    (
        "runnable archive",
        run_mutation(
            overlay=mutate_text(
                "archive/AutoML/deploy/systemd/automl-server.service",
                "ConditionPathExists=/nonexistent/retired-v1-automl-systemd-unit\n",
                "",
            )
        ),
        "archived unit must fail closed",
    ),
    (
        "archive loads config",
        run_mutation(
            overlay=mutate_text(
                "archive/AutoML/deploy/systemd/automl-server.service",
                "ExecStart=/bin/false",
                "EnvironmentFile=/etc/example.env\nExecStart=/bin/false",
            )
        ),
        "archived unit must not load runtime configuration",
    ),
    (
        "archive switches back to root",
        run_mutation(
            overlay=mutate_text(
                "archive/AutoML/deploy/188/automl-server.service",
                "User=automl",
                "User=root",
            )
        ),
        "archived unit must retain dedicated non-root identity",
    ),
    (
        "archive points to stale unit",
        run_mutation(
            overlay=mutate_text(
                "archive/AutoML/deploy/188/automl-server.service",
                "deploy/systemd/w-automl.service",
                "deploy/systemd/automl-server.service",
            )
        ),
        "archived unit points to stale deployment authority",
    ),
    (
        "CI skips security checker",
        run_mutation(
            overlay=mutate_text(
                ".gitea/workflows/ci.yml",
                "python3 scripts/check-security-hygiene.py",
                "python3 scripts/check-security-hygiene-skipped.py",
            )
        ),
        "CI must run the security hygiene checker",
    ),
    (
        "CI skips mutation suite",
        run_mutation(
            overlay=mutate_text(
                ".gitea/workflows/ci.yml",
                "python3 scripts/test-security-hygiene.py",
                "python3 scripts/test-security-hygiene-skipped.py",
            )
        ),
        "CI must run security hygiene mutation tests",
    ),
    (
        "docs advertise stale env projection",
        run_mutation(
            overlay=mutate_text(
                "deploy/README.md",
                checker.CANONICAL_ENV_FILE,
                "/opt/automl/data/automl-v2/runtime.env",
            )
        ),
        "deployment docs still advertise stale authority",
    ),
    (
        "root docs advertise stale active unit",
        run_mutation(
            overlay=mutate_text(
                "README.md",
                checker.CANONICAL_UNIT,
                "/etc/systemd/system/automl-server.service",
            )
        ),
        "root README still advertises stale authority",
    ),
    (
        "archive docs become deployable",
        run_mutation(
            overlay=mutate_text(
                "archive/AutoML/deploy/README.md",
                "HISTORICAL / DO NOT DEPLOY",
                "legacy deployment guide",
            )
        ),
        "archive deployment docs must be marked historical",
    ),
    (
        "missing incident runbook",
        run_mutation(remove=(checker.INCIDENT_RUNBOOK,)),
        "required tracked file missing",
    ),
    (
        "runbook claims external action was performed",
        run_mutation(
            overlay=mutate_text(
                checker.INCIDENT_RUNBOOK,
                "were **not** performed",
                "were performed",
            )
        ),
        "secret incident runbook missing required boundary",
    ),
]

for label, errors, fragment in cases:
    expect_failure(label, errors, fragment)

print(f"validated {len(cases)} fail-closed security hygiene mutations")
