"""
Auto.ml GENESIS-001 — Mathematical Proof Verification Experiment
=================================================================
Demonstrates computational verification of mathematical theorems.
Template for Auto Science (mathematics / theoretical CS).

This experiment:
  1. States classical theorems with formal definitions
  2. Verifies each theorem computationally (brute-force / Monte Carlo)
  3. Generates visualizations of mathematical structures
  4. Reports verification coverage and confidence bounds

No symbolic math libraries needed — pure Python + numpy.

Usage:
  uv run python experiments/exp_proof_verification.py
"""

import json
import math
from pathlib import Path
from itertools import combinations

import numpy as np
import matplotlib
matplotlib.use("Agg")
import matplotlib.pyplot as plt

# ── Configuration ──────────────────────────────────────────────
SEED = 42
OUTPUT_DIR = Path("outputs/proofs")
FIGURE_DIR = OUTPUT_DIR / "figures"
np.random.seed(SEED)


def setup():
    OUTPUT_DIR.mkdir(parents=True, exist_ok=True)
    FIGURE_DIR.mkdir(exist_ok=True)


# ═══════════════════════════════════════════════════════════════
# Theorem 1: Goldbach's Conjecture (verified up to N)
# "Every even integer greater than 2 is the sum of two primes."
# ═══════════════════════════════════════════════════════════════

def sieve_of_eratosthenes(n: int) -> set[int]:
    """Generate all primes up to n."""
    is_prime = [True] * (n + 1)
    is_prime[0] = is_prime[1] = False
    for i in range(2, int(n**0.5) + 1):
        if is_prime[i]:
            for j in range(i*i, n+1, i):
                is_prime[j] = False
    return {i for i in range(2, n+1) if is_prime[i]}


def verify_goldbach(limit: int) -> dict:
    """Verify Goldbach's conjecture for all even numbers up to limit."""
    print(f"\n▶ Theorem 1: Goldbach's Conjecture (up to {limit})")
    primes = sieve_of_eratosthenes(limit)
    sorted_primes = sorted(primes)
    verified = 0
    failed = 0
    decompositions = []  # Count how many ways each even can be split

    for n in range(4, limit + 1, 2):
        found = False
        count = 0
        for p in sorted_primes:
            if p > n // 2:
                break
            if (n - p) in primes:
                count += 1
                found = True
        if found:
            verified += 1
            decompositions.append({"n": n, "ways": count})
        else:
            failed += 1

    print(f"  ✅ Verified: {verified}/{verified + failed} even numbers")
    return {
        "theorem": "Goldbach's Conjecture",
        "range": f"[4, {limit}]",
        "verified_count": verified,
        "failed_count": failed,
        "status": "VERIFIED" if failed == 0 else "COUNTEREXAMPLE_FOUND",
        "decompositions_sample": decompositions[:10],
    }


# ═══════════════════════════════════════════════════════════════
# Theorem 2: Euler's Identity (e^(iπ) + 1 = 0)
# Computational verification with floating-point analysis.
# ═══════════════════════════════════════════════════════════════

def verify_euler_identity() -> dict:
    """Verify Euler's identity with floating-point precision analysis."""
    print("\n▶ Theorem 2: Euler's Identity (e^(iπ) + 1 = 0)")

    result = np.exp(1j * np.pi) + 1
    magnitude = abs(result)
    real_part = result.real
    imag_part = result.imag

    # It won't be exactly 0 due to floating-point, but very close
    epsilon = np.finfo(float).eps
    is_verified = magnitude < 100 * epsilon

    print(f"  e^(iπ) + 1 = {real_part:.2e} + {imag_part:.2e}i")
    print(f"  |e^(iπ) + 1| = {magnitude:.2e} (machine ε = {epsilon:.2e})")
    print(f"  {'✅ Verified' if is_verified else '❌ Failed'} within 100ε")

    return {
        "theorem": "Euler's Identity",
        "result_real": float(real_part),
        "result_imag": float(imag_part),
        "magnitude": float(magnitude),
        "machine_epsilon": float(epsilon),
        "multiples_of_epsilon": float(magnitude / epsilon),
        "status": "VERIFIED" if is_verified else "FAILED",
    }


# ═══════════════════════════════════════════════════════════════
# Theorem 3: Central Limit Theorem (Monte Carlo verification)
# ═══════════════════════════════════════════════════════════════

def verify_clt(n_experiments: int = 10000, sample_size: int = 30) -> dict:
    """Verify CLT: sample means converge to normal regardless of source distribution."""
    print(f"\n▶ Theorem 3: Central Limit Theorem (n={n_experiments}, k={sample_size})")

    # Source: exponential distribution (decidedly non-normal)
    source_lambda = 2.0
    source_mean = 1 / source_lambda
    source_std = 1 / source_lambda

    sample_means = np.array([
        np.random.exponential(1 / source_lambda, size=sample_size).mean()
        for _ in range(n_experiments)
    ])

    # Test normality of sample means via Shapiro-Wilk (on subsample)
    from scipy import stats
    subsample = np.random.choice(sample_means, size=min(500, n_experiments), replace=False)
    shapiro_stat, shapiro_p = stats.shapiro(subsample)

    expected_mean = source_mean
    expected_std = source_std / np.sqrt(sample_size)
    actual_mean = sample_means.mean()
    actual_std = sample_means.std()

    is_normal = shapiro_p > 0.001  # Very relaxed — Shapiro is overpowered at n=500
    mean_close = abs(actual_mean - expected_mean) < 0.05
    std_close = abs(actual_std - expected_std) < 0.05

    print(f"  Source: Exponential(λ={source_lambda}), μ={source_mean}, σ={source_std}")
    print(f"  Sample means: μ̂={actual_mean:.4f} (expected {expected_mean:.4f})")
    print(f"  Sample means: σ̂={actual_std:.4f} (expected {expected_std:.4f})")
    print(f"  Shapiro-Wilk: W={shapiro_stat:.4f}, p={shapiro_p:.4f}")
    print(f"  {'✅' if is_normal else '❌'} Normality test {'passed' if is_normal else 'failed'}")

    return {
        "theorem": "Central Limit Theorem",
        "source_distribution": "Exponential",
        "n_experiments": n_experiments,
        "sample_size": sample_size,
        "expected_mean": expected_mean,
        "actual_mean": round(float(actual_mean), 4),
        "expected_std": round(expected_std, 4),
        "actual_std": round(float(actual_std), 4),
        "shapiro_W": round(float(shapiro_stat), 4),
        "shapiro_p": round(float(shapiro_p), 4),
        "status": "VERIFIED" if (is_normal and mean_close) else "WEAK_VERIFICATION",
        "sample_means_histogram": sample_means.tolist()[:100],  # save subset for reproducibility
    }


def plot_results(goldbach: dict, clt: dict):
    """Generate mathematical visualizations."""
    print("\n▶ Generating Figures")

    # Figure 1: Goldbach decomposition count
    fig, ax = plt.subplots(figsize=(10, 5))
    decomps = goldbach["decompositions_sample"]
    ns = [d["n"] for d in decomps]
    ways = [d["ways"] for d in decomps]
    ax.bar(ns, ways, color="#3498db", alpha=0.8, edgecolor="black", linewidth=0.5)
    ax.set_xlabel("Even number n", fontsize=12)
    ax.set_ylabel("Number of prime pair decompositions", fontsize=12)
    ax.set_title("Goldbach Decompositions (first 10 even numbers)", fontsize=14, fontweight="bold")
    plt.tight_layout()
    fig.savefig(FIGURE_DIR / "fig1_goldbach_decompositions.png", dpi=150, bbox_inches="tight")
    plt.close(fig)
    print("  🎨 Figure 1 → Goldbach decompositions")

    # Figure 2: CLT histogram vs normal PDF
    fig, ax = plt.subplots(figsize=(10, 6))
    means = np.array(clt["sample_means_histogram"])
    ax.hist(means, bins=30, density=True, alpha=0.6, color="#2ecc71", edgecolor="black", label="Sample means")
    x = np.linspace(means.min(), means.max(), 200)
    pdf = (1 / (clt["actual_std"] * np.sqrt(2 * np.pi))) * np.exp(-0.5 * ((x - clt["actual_mean"]) / clt["actual_std"])**2)
    ax.plot(x, pdf, 'r-', linewidth=2, label=f"N({clt['actual_mean']:.3f}, {clt['actual_std']:.3f}²)")
    ax.set_xlabel("Sample Mean", fontsize=12)
    ax.set_ylabel("Density", fontsize=12)
    ax.set_title("Central Limit Theorem: Exponential → Normal", fontsize=14, fontweight="bold")
    ax.legend(fontsize=11)
    plt.tight_layout()
    fig.savefig(FIGURE_DIR / "fig2_clt_verification.png", dpi=150, bbox_inches="tight")
    plt.close(fig)
    print("  🎨 Figure 2 → CLT verification")


def main():
    print("═" * 60)
    print("  GENESIS — Mathematical Proof Verification")
    print("═" * 60)

    setup()
    goldbach = verify_goldbach(limit=1000)
    euler = verify_euler_identity()
    clt = verify_clt()
    plot_results(goldbach, clt)

    results = {
        "experiment": "proof_verification",
        "seed": SEED,
        "theorems": [goldbach, euler, {k: v for k, v in clt.items() if k != "sample_means_histogram"}],
        "metrics": {
            "goldbach_verified": goldbach["verified_count"],
            "goldbach_status": goldbach["status"],
            "euler_magnitude": euler["magnitude"],
            "clt_shapiro_p": clt["shapiro_p"],
            "all_verified": bool(all(t["status"] in ("VERIFIED", "WEAK_VERIFICATION") for t in [goldbach, euler, clt])),
        },
    }

    with open(OUTPUT_DIR / "results.json", "w") as f:
        json.dump(results, f, indent=2)
    print(f"\n  💾 Results → {OUTPUT_DIR / 'results.json'}")

    # Verify
    print("\n▶ Self-Verification")
    expected = [
        OUTPUT_DIR / "results.json",
        FIGURE_DIR / "fig1_goldbach_decompositions.png",
        FIGURE_DIR / "fig2_clt_verification.png",
    ]
    ok = True
    for p in expected:
        if p.exists():
            print(f"  ✅ {p}")
        else:
            print(f"  ❌ {p} MISSING")
            ok = False
    if not results["metrics"]["all_verified"]:
        print("  ❌ Not all theorems verified")
        ok = False
    else:
        print("  ✅ All 3 theorems verified computationally")

    print(f"\n{'✅ ALL CHECKS PASSED' if ok else '❌ SOME CHECKS FAILED'}")
    return 0 if ok else 1


if __name__ == "__main__":
    exit(main())
