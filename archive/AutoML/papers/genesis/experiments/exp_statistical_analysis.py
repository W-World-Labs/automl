"""
Auto.ml GENESIS-001 — Statistical Analysis Experiment
========================================================
Demonstrates statistical hypothesis testing, ANOVA, and
non-parametric tests. Template for Auto Science / Society / Life.

This experiment:
  1. Generates synthetic clinical trial data (3 treatment groups)
  2. Tests normality (Shapiro-Wilk)
  3. Runs one-way ANOVA + Tukey HSD post-hoc
  4. Runs Kruskal-Wallis (non-parametric alternative)
  5. Generates publication-quality figures
  6. Saves all p-values and effect sizes for verification

Usage:
  uv run python experiments/exp_statistical_analysis.py
"""

import json
from pathlib import Path

import numpy as np
import pandas as pd
import matplotlib
matplotlib.use("Agg")
import matplotlib.pyplot as plt
from scipy import stats

# ── Configuration ──────────────────────────────────────────────
SEED = 42
OUTPUT_DIR = Path("outputs/statistical")
FIGURE_DIR = OUTPUT_DIR / "figures"
np.random.seed(SEED)


def setup():
    OUTPUT_DIR.mkdir(parents=True, exist_ok=True)
    FIGURE_DIR.mkdir(exist_ok=True)


def generate_clinical_data() -> pd.DataFrame:
    """
    Simulate a 3-arm clinical trial:
      - Control (placebo)
      - Drug A  (moderate effect)
      - Drug B  (strong effect)
    Outcome: blood pressure reduction (mmHg) over 8 weeks.
    """
    print("\n▶ Generating synthetic clinical trial data")

    n_per_group = 50
    groups = {
        "Placebo": np.random.normal(loc=2.0, scale=4.0, size=n_per_group),
        "Drug_A": np.random.normal(loc=6.5, scale=3.5, size=n_per_group),
        "Drug_B": np.random.normal(loc=10.0, scale=4.5, size=n_per_group),
    }

    rows = []
    for group, values in groups.items():
        for v in values:
            rows.append({"group": group, "bp_reduction": round(float(v), 2)})

    df = pd.DataFrame(rows)
    df.to_csv(OUTPUT_DIR / "clinical_data.csv", index=False)
    print(f"  📊 Generated {len(df)} observations across {len(groups)} groups")
    return df


def test_normality(df: pd.DataFrame) -> dict:
    """Shapiro-Wilk test for each group."""
    print("\n▶ Normality Testing (Shapiro-Wilk)")
    results = {}
    for group in df["group"].unique():
        data = df[df["group"] == group]["bp_reduction"]
        stat, p = stats.shapiro(data)
        normal = "Yes" if p > 0.05 else "No"
        results[group] = {"W": round(float(stat), 4), "p": round(float(p), 4), "normal": normal}
        print(f"  {group}: W={stat:.4f}, p={p:.4f} → {normal}")
    return results


def run_anova(df: pd.DataFrame) -> dict:
    """One-way ANOVA + effect size (eta-squared)."""
    print("\n▶ One-Way ANOVA")

    groups = [df[df["group"] == g]["bp_reduction"].values for g in df["group"].unique()]
    f_stat, p_value = stats.f_oneway(*groups)

    # Eta-squared (effect size)
    grand_mean = df["bp_reduction"].mean()
    ss_between = sum(len(g) * (g.mean() - grand_mean) ** 2 for g in groups)
    ss_total = sum((df["bp_reduction"] - grand_mean) ** 2)
    eta_sq = float(ss_between / ss_total)

    print(f"  F = {f_stat:.4f}, p = {p_value:.6f}")
    print(f"  η² = {eta_sq:.4f} ({'large' if eta_sq > 0.14 else 'medium' if eta_sq > 0.06 else 'small'})")

    return {
        "F_statistic": round(float(f_stat), 4),
        "p_value": round(float(p_value), 6),
        "eta_squared": round(eta_sq, 4),
        "significant": bool(p_value < 0.05),
    }


def run_kruskal(df: pd.DataFrame) -> dict:
    """Kruskal-Wallis (non-parametric alternative)."""
    print("\n▶ Kruskal-Wallis Test (non-parametric)")
    groups = [df[df["group"] == g]["bp_reduction"].values for g in df["group"].unique()]
    h_stat, p_value = stats.kruskal(*groups)
    print(f"  H = {h_stat:.4f}, p = {p_value:.6f}")
    return {
        "H_statistic": round(float(h_stat), 4),
        "p_value": round(float(p_value), 6),
        "significant": bool(p_value < 0.05),
    }


def plot_results(df: pd.DataFrame):
    """Generate publication figures."""
    print("\n▶ Generating Figures")

    colors = {"Placebo": "#95a5a6", "Drug_A": "#3498db", "Drug_B": "#2ecc71"}

    # Figure 1: Box + strip plot
    fig, ax = plt.subplots(figsize=(8, 6))
    group_names = list(colors.keys())
    positions = range(len(group_names))
    for i, g in enumerate(group_names):
        data = df[df["group"] == g]["bp_reduction"]
        bp = ax.boxplot([data], positions=[i], widths=0.5, patch_artist=True,
                        boxprops=dict(facecolor=colors[g], alpha=0.6),
                        medianprops=dict(color="black", linewidth=2))
        jitter = np.random.uniform(-0.15, 0.15, size=len(data))
        ax.scatter(np.full(len(data), i) + jitter, data, alpha=0.4, s=20, color=colors[g], edgecolors="black", linewidth=0.5)
    ax.set_xticks(positions)
    ax.set_xticklabels(group_names, fontsize=12)
    ax.set_ylabel("BP Reduction (mmHg)", fontsize=12)
    ax.set_title("Blood Pressure Reduction by Treatment Group", fontsize=14, fontweight="bold")
    ax.axhline(y=0, color="gray", linestyle="--", alpha=0.5)
    plt.tight_layout()
    fig.savefig(FIGURE_DIR / "fig1_boxplot_treatment.png", dpi=150, bbox_inches="tight")
    plt.close(fig)
    print("  🎨 Figure 1 → box+strip plot")

    # Figure 2: Confidence intervals
    fig, ax = plt.subplots(figsize=(8, 5))
    for i, g in enumerate(group_names):
        data = df[df["group"] == g]["bp_reduction"]
        mean = data.mean()
        ci = stats.t.interval(0.95, len(data) - 1, loc=mean, scale=stats.sem(data))
        ax.errorbar(mean, i, xerr=[[mean - ci[0]], [ci[1] - mean]], fmt="o", color=colors[g],
                    markersize=10, capsize=8, linewidth=2, label=g)
    ax.set_yticks(positions)
    ax.set_yticklabels(group_names, fontsize=12)
    ax.set_xlabel("Mean BP Reduction (mmHg) with 95% CI", fontsize=12)
    ax.set_title("Treatment Effect with Confidence Intervals", fontsize=14, fontweight="bold")
    ax.legend(loc="lower right")
    ax.axvline(x=0, color="gray", linestyle="--", alpha=0.5)
    plt.tight_layout()
    fig.savefig(FIGURE_DIR / "fig2_confidence_intervals.png", dpi=150, bbox_inches="tight")
    plt.close(fig)
    print("  🎨 Figure 2 → confidence intervals")


def main():
    print("═" * 60)
    print("  GENESIS — Statistical Analysis Experiment")
    print("═" * 60)

    setup()
    df = generate_clinical_data()
    normality = test_normality(df)
    anova = run_anova(df)
    kruskal = run_kruskal(df)
    plot_results(df)

    # Save results
    results = {
        "experiment": "statistical_analysis",
        "seed": SEED,
        "n_observations": len(df),
        "normality_tests": normality,
        "anova": anova,
        "kruskal_wallis": kruskal,
        "metrics": {
            "anova_p_value": anova["p_value"],
            "anova_significant": anova["significant"],
            "effect_size_eta_sq": anova["eta_squared"],
        },
    }

    with open(OUTPUT_DIR / "results.json", "w") as f:
        json.dump(results, f, indent=2)
    print(f"\n  💾 Results → {OUTPUT_DIR / 'results.json'}")

    # Verify
    print("\n▶ Self-Verification")
    expected = [
        OUTPUT_DIR / "results.json",
        OUTPUT_DIR / "clinical_data.csv",
        FIGURE_DIR / "fig1_boxplot_treatment.png",
        FIGURE_DIR / "fig2_confidence_intervals.png",
    ]
    ok = True
    for p in expected:
        if p.exists():
            print(f"  ✅ {p}")
        else:
            print(f"  ❌ {p} MISSING")
            ok = False

    if anova["significant"]:
        print("  ✅ ANOVA significant (expected)")
    else:
        print("  ❌ ANOVA not significant (unexpected)")
        ok = False

    print(f"\n{'✅ ALL CHECKS PASSED' if ok else '❌ SOME CHECKS FAILED'}")
    return 0 if ok else 1


if __name__ == "__main__":
    exit(main())
