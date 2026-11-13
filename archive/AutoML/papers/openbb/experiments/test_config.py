#!/usr/bin/env python3
"""
OpenBB Config Pipeline Validation
==================================

Validates the auto-adaptive configuration system:
1. Load all YAML configurations
2. Apply tier overrides  
3. Test reflection-based field mapping
4. Verify base inheritance chain
5. Build OpenBBConfig objects

Output: outputs/config_validation.json
"""

import sys
import os
import json
import time

# Add OpenBB source to path
OPENBB_ROOT = os.environ.get("OPENBB_ROOT", os.path.join(os.path.dirname(__file__), "..", "..", "..", "minimind", "openbb"))
sys.path.insert(0, os.path.abspath(OPENBB_ROOT))

CONFIGS_TO_TEST = [
    ("cfg/openbb-text.yaml", "nano"),
    ("cfg/openbb-text.yaml", "mini"),
    ("cfg/openbb-text.yaml", "4b"),
    ("cfg/openbb-vl.yaml", None),
    ("cfg/openbb-vl-v2-dense.yaml", None),
    ("cfg/openbb-vl-v2-moe.yaml", None),
    ("cfg/openbb-omni.yaml", None),
    ("cfg/openbb-audio.yaml", None),
]


def main():
    os.makedirs("outputs", exist_ok=True)
    
    from cfg.parser import load_config, build_openbb_config_from_yaml

    results = {
        "configs_parsed": 0,
        "configs_built": 0,
        "details": [],
        "errors": [],
    }

    for config_path, tier in CONFIGS_TO_TEST:
        name = f"{os.path.basename(config_path)}:{tier or 'default'}"
        t0 = time.time()
        try:
            cfg = load_config(config_path, tier=tier)
            results["configs_parsed"] += 1

            openbb_cfg = build_openbb_config_from_yaml(cfg)
            results["configs_built"] += 1

            detail = {
                "config": config_path,
                "tier": tier,
                "hidden_size": openbb_cfg.hidden_size,
                "num_layers": openbb_cfg.num_hidden_layers,
                "num_heads": openbb_cfg.num_attention_heads,
                "vlm_enabled": cfg.get("vlm", {}).get("enabled", False),
                "audio_enabled": cfg.get("audio", {}).get("enabled", False),
                "parse_time_ms": round((time.time() - t0) * 1000, 1),
                "status": "OK",
            }
            results["details"].append(detail)
            print(f"  ✅ {name}: hidden={openbb_cfg.hidden_size}, "
                  f"layers={openbb_cfg.num_hidden_layers}")

        except Exception as e:
            results["errors"].append({"config": name, "error": str(e)})
            print(f"  ❌ {name}: {e}")

    # Summary
    print(f"\n{'='*60}")
    print(f"  Parsed: {results['configs_parsed']}/{len(CONFIGS_TO_TEST)}")
    print(f"  Built:  {results['configs_built']}/{len(CONFIGS_TO_TEST)}")
    print(f"  Errors: {len(results['errors'])}")
    print(f"{'='*60}")

    with open("outputs/config_validation.json", "w") as f:
        json.dump(results, f, indent=2)

    # Exit with error if any failed
    if results["configs_built"] < len(CONFIGS_TO_TEST):
        sys.exit(1)


if __name__ == "__main__":
    main()
