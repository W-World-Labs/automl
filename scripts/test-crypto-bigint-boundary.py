#!/usr/bin/env python3
"""Mutation checks for the narrow crypto-bigint exception boundary."""

import importlib.util
from pathlib import Path


path = Path(__file__).with_name("check-crypto-bigint-boundary.py")
spec = importlib.util.spec_from_file_location("boundary", path)
assert spec and spec.loader
boundary = importlib.util.module_from_spec(spec)
spec.loader.exec_module(boundary)

SAFE_TREE = """0crypto-bigint v0.5.5
1elliptic-curve v0.13.8
2ecdsa v0.16.9
3p256 v0.13.2
4jsonwebtoken v11.0.0
5w-id-sdk v1.3.16 (registry `gitea`)
6automl-server v2.0.0-alpha.5 (/path/to/automl-server)
6w-id-axum v0.2.7 (registry `gitea`)
7automl-server v2.0.0-alpha.5 (/path/to/automl-server)
6automl-cli v2.0.0-alpha.5 (/path/to/automl-cli)
3p384 v0.13.1
4jsonwebtoken v11.0.0
5w-id-sdk v1.3.16 (registry `gitea`)
6automl-server v2.0.0-alpha.5 (/path/to/automl-server)
6w-id-axum v0.2.7 (registry `gitea`)
7automl-server v2.0.0-alpha.5 (/path/to/automl-server)
6automl-cli v2.0.0-alpha.5 (/path/to/automl-cli)
2p256 v0.13.2
3jsonwebtoken v11.0.0
4w-id-sdk v1.3.16 (registry `gitea`)
5automl-server v2.0.0-alpha.5 (/path/to/automl-server)
5w-id-axum v0.2.7 (registry `gitea`)
6automl-server v2.0.0-alpha.5 (/path/to/automl-server)
5automl-cli v2.0.0-alpha.5 (/path/to/automl-cli)
2p384 v0.13.1
3jsonwebtoken v11.0.0
4w-id-sdk v1.3.16 (registry `gitea`)
5automl-server v2.0.0-alpha.5 (/path/to/automl-server)
5w-id-axum v0.2.7 (registry `gitea`)
6automl-server v2.0.0-alpha.5 (/path/to/automl-server)
5automl-cli v2.0.0-alpha.5 (/path/to/automl-cli)
2primeorder v0.13.6
3p256 v0.13.2
4jsonwebtoken v11.0.0
5w-id-sdk v1.3.16 (registry `gitea`)
6automl-server v2.0.0-alpha.5 (/path/to/automl-server)
6w-id-axum v0.2.7 (registry `gitea`)
7automl-server v2.0.0-alpha.5 (/path/to/automl-server)
6automl-cli v2.0.0-alpha.5 (/path/to/automl-cli)
3p384 v0.13.1
4jsonwebtoken v11.0.0
5w-id-sdk v1.3.16 (registry `gitea`)
6automl-server v2.0.0-alpha.5 (/path/to/automl-server)
6w-id-axum v0.2.7 (registry `gitea`)
7automl-server v2.0.0-alpha.5 (/path/to/automl-server)
6automl-cli v2.0.0-alpha.5 (/path/to/automl-cli)
"""

assert boundary.validate(SAFE_TREE), "legal duplicated SDK paths must pass"
direct_edge = SAFE_TREE + "1automl-server v2.0.0-alpha.5 (/path/to/automl-server)\n"
assert {node for tree_path in boundary.paths(SAFE_TREE) for node in tree_path} == {
    node for tree_path in boundary.paths(direct_edge) for node in tree_path
}, "direct-edge mutation must preserve the package/version set"
assert not boundary.validate(direct_edge), "direct workspace dependency must fail even with unchanged nodes"
assert not boundary.validate(SAFE_TREE.replace("w-id-sdk v1.3.16", "w-id-sdk v1.3.17"))
assert not boundary.validate(SAFE_TREE.replace("automl-cli v2.0.0-alpha.5", "other-project v2.0.0-alpha.5"))
assert not boundary.validate("")

print("validated crypto-bigint exception path and mutation cases")
