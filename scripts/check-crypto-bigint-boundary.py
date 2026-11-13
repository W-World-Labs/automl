#!/usr/bin/env python3
"""Keep the narrowly reviewed RustSec crypto-bigint exception on one path."""

from pathlib import Path
import re
import sys


EXPECTED = {
    ("automl-cli", "2.0.0-alpha.5"),
    ("automl-server", "2.0.0-alpha.5"),
    ("crypto-bigint", "0.5.5"),
    ("ecdsa", "0.16.9"),
    ("elliptic-curve", "0.13.8"),
    ("jsonwebtoken", "11.0.0"),
    ("p256", "0.13.2"),
    ("p384", "0.13.1"),
    ("primeorder", "0.13.6"),
    ("w-id-axum", "0.2.7"),
    ("w-id-sdk", "1.3.16"),
}
SDK = ("w-id-sdk", "1.3.16")
WORKSPACE_CONSUMERS = {("automl-cli", "2.0.0-alpha.5"), ("automl-server", "2.0.0-alpha.5")}


def paths(tree: str) -> list[tuple[tuple[str, str], ...]]:
    """Parse cargo tree's --prefix depth output into expanded root-to-node paths."""
    stack: list[tuple[str, str]] = []
    result = []
    for line in tree.splitlines():
        match = re.fullmatch(r"(\d+)([A-Za-z0-9_-]+) v([^ ]+)(?: .*)?", line.strip())
        if not match:
            return []
        depth, name, version = int(match.group(1)), match.group(2), match.group(3)
        if depth > len(stack):
            return []
        stack = stack[:depth]
        stack.append((name, version))
        result.append(tuple(stack))
    return result


def validate(tree: str) -> bool:
    nodes = paths(tree)
    if not nodes or {node for path in nodes for node in path} != EXPECTED:
        return False
    if nodes[0] != (("crypto-bigint", "0.5.5"),):
        return False

    leaves = [
        path
        for index, path in enumerate(nodes)
        if index + 1 == len(nodes) or len(nodes[index + 1]) <= len(path)
    ]
    if not leaves:
        return False
    for path in leaves:
        if path[-1] not in WORKSPACE_CONSUMERS or SDK not in path[:-1]:
            return False
    return {path[-1] for path in leaves} == WORKSPACE_CONSUMERS


def main() -> int:
    path = Path(sys.argv[1]) if len(sys.argv) > 1 else None
    tree = path.read_text() if path else sys.stdin.read()
    if not validate(tree):
        print(
            "crypto-bigint exception boundary changed; every workspace path "
            "must pass through the pinned W-ID SDK (RUSTSEC-2026-0387)",
            file=sys.stderr,
        )
        return 1
    print("all crypto-bigint dependency paths pass through the pinned W-ID SDK")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
