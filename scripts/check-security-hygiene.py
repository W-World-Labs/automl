#!/usr/bin/env python3
"""Fail-closed repository and production-unit hygiene checks.

The checker deliberately reasons from tracked path names and approved config
structure. It never prints file contents or secret-like values.
"""

from __future__ import annotations

from collections.abc import Callable
from pathlib import Path, PurePosixPath
import re
import subprocess

ROOT = Path(__file__).resolve().parents[1]
CANONICAL_UNIT = "deploy/systemd/w-automl.service"
CANONICAL_ENV_FILE = "/etc/w/w-automl.service.env"
INCIDENT_RUNBOOK = "docs/SECRET-INCIDENT-RUNBOOK.md"

ReadText = Callable[[str], str]
SENSITIVE_ASSIGNMENT = re.compile(
    r"(?m)^\s*(?:export\s+)?"
    r"([A-Z][A-Z0-9_]*(?:SECRET|PASSWORD|TOKEN|API_KEY|ACCESS_KEY)|"
    r"ADMIN_KEYS|DATABASE_URL)\s*=\s*([^#\r\n]*)$"
)
SAFE_EXAMPLE_MARKERS = (
    "${",
    "$",
    "<",
    "change-me",
    "changeme",
    "dev",
    "dummy",
    "example",
    "local",
    "placeholder",
    "replace",
    "test",
    "your_",
)


def tracked_files() -> list[str]:
    """Return files relevant to a local check, including not-yet-staged files."""

    out = subprocess.check_output(
        ["git", "ls-files", "--cached", "--others", "--exclude-standard"],
        cwd=ROOT,
        text=True,
    )
    return sorted(
        {
            line
            for line in out.splitlines()
            if line and (ROOT / line).is_file()
        }
    )


def is_secretish_path(path: str) -> bool:
    """Classify paths that must never be committed, without opening them."""

    name = PurePosixPath(path).name.lower()
    env_like = (
        name == ".env"
        or name.startswith(".env.")
        or name.endswith(".env")
        or ".env." in name
    )
    if env_like:
        return not name.endswith(".example")

    if PurePosixPath(name).suffix in {
        ".key",
        ".pem",
        ".p12",
        ".pfx",
        ".jks",
        ".keystore",
    }:
        return True

    if name in {
        ".netrc",
        "credentials",
        "credentials.json",
        "id_dsa",
        "id_ed25519",
        "id_ecdsa",
        "id_rsa",
        "service-account.json",
        "service_account.json",
    }:
        return True

    return bool(re.fullmatch(r"service[-_]account(?:\.[a-z0-9_-]+)?\.json", name))


def validate(files: list[str], read_text: ReadText) -> list[str]:
    errors: list[str] = []
    file_set = set(files)

    def read_required(path: str) -> str:
        if path not in file_set:
            errors.append(f"required tracked file missing: {path}")
            return ""
        try:
            return read_text(path)
        except (OSError, UnicodeError):
            errors.append(f"required text file unreadable: {path}")
            return ""

    # 1) Forced adds must not bypass .gitignore. Examples may contain names and
    # placeholders, but live env/key/credential paths are rejected by filename.
    for path in files:
        if is_secretish_path(path):
            errors.append(f"tracked secret-like file: {path}")

    gitignore = read_required(".gitignore")
    ignored_secret_paths = (
        "*.key",
        "*.pem",
        "*.p12",
        "*.pfx",
        "*.jks",
        "*.keystore",
        "**/.netrc",
        "**/credentials",
        "**/credentials.json",
        "**/service-account*.json",
        "**/service_account*.json",
        "**/id_dsa",
        "**/id_ecdsa",
        "**/id_ed25519",
        "**/id_rsa",
    )
    for pattern in ("**/*.env", "*.env.*", *ignored_secret_paths):
        if pattern not in gitignore.splitlines():
            errors.append(f".gitignore must reject secret-like pattern: {pattern}")
    for line in gitignore.splitlines():
        stripped = line.strip()
        if stripped.startswith("!") and "env.hk" in stripped.lower():
            errors.append(".gitignore must not allow an env.hk credential exception")

    # Content checks are deliberately shape-based and report names/paths only.
    # They catch secrets hidden in allowed examples or docs without echoing a
    # candidate value back into CI output.
    for path in files:
        if is_secretish_path(path) or path.endswith(
            (".lock", ".png", ".jpeg", ".jpg", ".pdf")
        ):
            continue
        try:
            text = read_text(path)
        except (OSError, UnicodeError):
            continue
        if re.search(r"-----BEGIN [A-Z0-9 ]*PRIVATE KEY-----", text):
            errors.append(f"tracked text contains private-key material: {path}")
        for match in SENSITIVE_ASSIGNMENT.finditer(text):
            name = match.group(1)
            value = match.group(2).strip().strip("'\"").lower()
            if value and not any(marker in value for marker in SAFE_EXAMPLE_MARKERS):
                errors.append(
                    f"tracked text contains non-placeholder sensitive assignment: {path}:{name}"
                )

    # 2) Exactly one non-archive service template may be runnable.
    units = sorted(path for path in files if path.endswith(".service"))
    active_units = [path for path in units if not path.startswith("archive/")]
    if active_units != [CANONICAL_UNIT]:
        errors.append(
            "the only active systemd template must be "
            f"{CANONICAL_UNIT}; found {active_units}"
        )

    unit = read_required(CANONICAL_UNIT)
    required_directives = (
        "User=automl",
        "Group=automl",
        "WorkingDirectory=/opt/automl/apps/automl-v2",
        f"EnvironmentFile={CANONICAL_ENV_FILE}",
        "ExecStart=/opt/automl/apps/automl-v2/bin/automl-server",
        "UMask=0077",
        "NoNewPrivileges=true",
        "ProtectSystem=strict",
        "ProtectHome=true",
        "PrivateTmp=true",
        "CapabilityBoundingSet=",
        "ReadWritePaths=/opt/automl/data/automl-v2",
    )
    for directive in required_directives:
        if directive not in unit.splitlines():
            errors.append(f"canonical unit missing exact directive: {directive}")
    if re.search(r"(?m)^User=(?:root|nobody)\s*$", unit):
        errors.append("canonical unit must run as the dedicated automl user")
    environment_files = re.findall(r"(?m)^EnvironmentFile=(.+)$", unit)
    if environment_files != [CANONICAL_ENV_FILE]:
        errors.append(
            "canonical unit must have exactly one fail-closed EnvironmentFile at "
            f"{CANONICAL_ENV_FILE}"
        )
    if re.search(r"(?m)^Environment=", unit):
        errors.append("canonical unit must not contain inline Environment= settings")

    for path in units:
        if path == CANONICAL_UNIT:
            continue
        archived = read_required(path)
        if not path.startswith("archive/"):
            errors.append(f"non-canonical active unit is forbidden: {path}")
        if not re.search(r"(?m)^ConditionPathExists=/nonexistent/", archived):
            errors.append(f"archived unit must fail closed with ConditionPathExists: {path}")
        if not re.search(r"(?m)^ExecStart=/bin/false\s*$", archived):
            errors.append(f"archived unit must use ExecStart=/bin/false: {path}")
        if re.search(r"(?m)^(?:Environment|EnvironmentFile)=", archived):
            errors.append(f"archived unit must not load runtime configuration: {path}")
        archived_users = re.findall(r"(?m)^User=(.+)$", archived)
        if archived_users != ["automl"]:
            errors.append(f"archived unit must retain dedicated non-root identity: {path}")
        if "deploy/systemd/automl-server.service" in archived:
            errors.append(f"archived unit points to stale deployment authority: {path}")

    # 3) Live and archived dev-admin paths remain explicitly dual-gated.
    live_cfg = read_required("crates/automl-server/src/config.rs")
    if "ALLOW_DEV_ADMIN" not in live_cfg or "dev_admin_enabled" not in live_cfg:
        errors.append("live server must dual-gate dev-admin via ALLOW_DEV_ADMIN")
    live_compact = re.sub(r"\s+", "", live_cfg)
    if "self.allow_dev_admin&&!self.production" not in live_compact:
        errors.append("live dev_admin_enabled must require allow_dev_admin && !production")

    archive_mw_path = "archive/AutoML/automl-server/src/auth/middleware.rs"
    if archive_mw_path in file_set:
        archive_mw = read_required(archive_mw_path)
        if "admin_keys.is_empty() && !state.config.is_production()" in archive_mw:
            errors.append("archive middleware still permits implicit dev-admin elevation")
        if "dev_admin_elevation_enabled" not in archive_mw:
            errors.append("archive middleware must call dev_admin_elevation_enabled()")

    archive_cfg_path = "archive/AutoML/automl-server/src/config.rs"
    if archive_cfg_path in file_set:
        archive_cfg = read_required(archive_cfg_path)
        if "ALLOW_DEV_ADMIN" not in archive_cfg or "dev_admin_elevation_enabled" not in archive_cfg:
            errors.append("archive config must implement the explicit dev-admin dual gate")

    # 4) CI must enforce this checker and its fail-closed mutation suite.
    ci = read_required(".gitea/workflows/ci.yml")
    for fragment, message in (
        ("cargo fmt --all --check", "CI must run cargo fmt --check"),
        ("clippy --workspace --all-targets --all-features --locked -- -D warnings", "CI must fail clippy on warnings"),
        ("python3 scripts/check-security-hygiene.py", "CI must run the security hygiene checker"),
        ("python3 scripts/test-security-hygiene.py", "CI must run security hygiene mutation tests"),
    ):
        if fragment not in ci:
            errors.append(message)

    # 5) Deployment documentation must point to the same unit and config
    # authority. The historical public client identifier is scrubbed from HEAD.
    root_docs = read_required("README.md")
    deploy_docs = read_required("deploy/README.md")
    for fragment in (CANONICAL_UNIT, CANONICAL_ENV_FILE, "w-automl.service"):
        if fragment not in deploy_docs:
            errors.append(f"deployment docs missing canonical contract: {fragment}")
    for stale in (
        "deploy/systemd/automl-server.service",
        "/etc/systemd/system/automl-server.service",
        "/opt/automl/data/automl-v2/runtime.env",
    ):
        if stale in deploy_docs:
            errors.append(f"deployment docs still advertise stale authority: {stale}")

    for fragment in (CANONICAL_UNIT, INCIDENT_RUNBOOK, "W-Radar"):
        if fragment not in root_docs:
            errors.append(f"root README missing authority/incident contract: {fragment}")
    for stale in (
        "/etc/systemd/system/automl-server.service",
        "/opt/automl/data/automl-v2/runtime.env",
    ):
        if stale in root_docs:
            errors.append(f"root README still advertises stale authority: {stale}")

    archive_docs = read_required("archive/AutoML/deploy/README.md")
    if "HISTORICAL / DO NOT DEPLOY" not in archive_docs:
        errors.append("archive deployment docs must be marked historical/non-deployable")
    for forbidden in (
        "systemctl enable --now automl-server.service",
        "deploy/systemd/automl-server.service",
        "AutoML/.env.prod` or",
        "/opt/automl/data/automl/runtime.env",
    ):
        if forbidden in archive_docs:
            errors.append(f"archive deployment docs contain active/stale instruction: {forbidden}")

    incident_runbook = re.sub(r"\s+", " ", read_required(INCIDENT_RUNBOOK))
    for fragment in (
        "Rotation, revocation, history rewriting",
        "AutoML/.env.prod",
        "W_ID_CLIENT_SECRET",
        "POSTGRES_PASSWORD",
        "were **not** performed",
    ):
        if fragment not in incident_runbook:
            errors.append(f"secret incident runbook missing required boundary: {fragment}")

    historical_client_id = "bf909c842cb5866e" + "19ecd566"
    for path in files:
        if is_secretish_path(path):
            continue
        if path.endswith((".rs", ".toml", ".lock", ".png", ".jpeg", ".jpg", ".pdf")):
            continue
        if path.endswith("check-security-hygiene.py"):
            continue
        try:
            text = read_text(path)
        except (OSError, UnicodeError):
            continue
        if historical_client_id in text:
            errors.append(f"tracked file still contains historical W-ID client id: {path}")

    return errors


def main() -> int:
    files = tracked_files()
    errors = validate(files, lambda path: (ROOT / path).read_text())
    for error in errors:
        print(f"error: {error}")
    if errors:
        return 1
    print("security hygiene OK")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
