"""
Auto.ml GENESIS-001 — Comprehensive Data Exploration Experiment
================================================================
Demonstrates an end-to-end EDA pipeline with automated insight
generation. Template for all journals — every paper needs good EDA.

This experiment:
  1. Loads a multi-dimensional dataset (California Housing)
  2. Performs comprehensive statistical profiling
  3. Detects outliers (IQR method)
  4. Analyzes feature correlations and multicollinearity (VIF)
  5. Generates 4 publication-quality diagnostic figures
  6. Produces an automated "EDA report" JSON

No custom models — purely descriptive/diagnostic analysis.

Usage:
  uv run python experiments/exp_data_exploration.py
"""

import json
from pathlib import Path

import numpy as np
import pandas as pd
import matplotlib
matplotlib.use("Agg")
import matplotlib.pyplot as plt
from sklearn.datasets import fetch_california_housing

# ── Configuration ──────────────────────────────────────────────
SEED = 42
OUTPUT_DIR = Path("outputs/data_exploration")
FIGURE_DIR = OUTPUT_DIR / "figures"
DATA_DIR = Path("data")
np.random.seed(SEED)


def setup():
    OUTPUT_DIR.mkdir(parents=True, exist_ok=True)
    FIGURE_DIR.mkdir(exist_ok=True)
    DATA_DIR.mkdir(exist_ok=True)


def load_data() -> pd.DataFrame:
    """Load California Housing dataset."""
    print("\n▶ Loading Dataset")
    cache_path = DATA_DIR / "california_housing.csv"

    if cache_path.exists():
        print(f"  📦 Cached: {cache_path}")
        df = pd.read_csv(cache_path)
    else:
        print("  ⬇️  Loading California Housing...")
        housing = fetch_california_housing()
        df = pd.DataFrame(data=housing.data, columns=housing.feature_names)
        df["MedHouseVal"] = housing.target
        df.to_csv(cache_path, index=False)
        print(f"  💾 Saved → {cache_path}")

    print(f"  📊 Shape: {df.shape[0]:,} × {df.shape[1]}")
    return df


def statistical_profile(df: pd.DataFrame) -> dict:
    """Comprehensive statistical profiling of all features."""
    print("\n▶ Statistical Profiling")

    profile = {}
    for col in df.columns:
        data = df[col].dropna()
        q1 = float(data.quantile(0.25))
        q3 = float(data.quantile(0.75))
        iqr = q3 - q1

        profile[col] = {
            "count": int(len(data)),
            "mean": round(float(data.mean()), 4),
            "std": round(float(data.std()), 4),
            "min": round(float(data.min()), 4),
            "q25": round(q1, 4),
            "median": round(float(data.median()), 4),
            "q75": round(q3, 4),
            "max": round(float(data.max()), 4),
            "skewness": round(float(data.skew()), 4),
            "kurtosis": round(float(data.kurtosis()), 4),
            "iqr": round(iqr, 4),
            "missing": int(df[col].isna().sum()),
        }
        print(f"  {col:20s} μ={profile[col]['mean']:>10.4f}  σ={profile[col]['std']:>8.4f}  "
              f"skew={profile[col]['skewness']:>6.2f}")

    # Save full profile
    pd.DataFrame(profile).T.to_csv(OUTPUT_DIR / "statistical_profile.csv")
    return profile


def detect_outliers(df: pd.DataFrame) -> dict:
    """Outlier detection using IQR method."""
    print("\n▶ Outlier Detection (IQR Method)")

    outlier_report = {}
    for col in df.select_dtypes(include=[np.number]).columns:
        q1 = df[col].quantile(0.25)
        q3 = df[col].quantile(0.75)
        iqr = q3 - q1
        lower = q1 - 1.5 * iqr
        upper = q3 + 1.5 * iqr
        n_outliers = int(((df[col] < lower) | (df[col] > upper)).sum())
        pct = round(n_outliers / len(df) * 100, 2)
        outlier_report[col] = {
            "n_outliers": n_outliers,
            "pct_outliers": pct,
            "lower_bound": round(float(lower), 4),
            "upper_bound": round(float(upper), 4),
        }
        if pct > 1:
            print(f"  ⚠️ {col}: {n_outliers} outliers ({pct}%)")
        else:
            print(f"  ✅ {col}: {n_outliers} outliers ({pct}%)")

    return outlier_report


def correlation_analysis(df: pd.DataFrame) -> dict:
    """Compute correlation matrix and VIF for multicollinearity."""
    print("\n▶ Correlation & Multicollinearity Analysis")

    corr = df.corr()
    corr.to_csv(OUTPUT_DIR / "correlation_matrix.csv")

    # Find highly correlated pairs
    high_corr = []
    cols = corr.columns.tolist()
    for i, c1 in enumerate(cols):
        for c2 in cols[i+1:]:
            r = float(corr.loc[c1, c2])
            if abs(r) > 0.7:
                high_corr.append({"feature_1": c1, "feature_2": c2, "r": round(r, 4)})
                print(f"  🔗 {c1} ↔ {c2}: r = {r:.4f}")

    # Variance Inflation Factor (simplified — no statsmodels needed)
    feature_cols = [c for c in df.columns if c != "MedHouseVal"]
    X = df[feature_cols].values
    vif_results = {}
    for i, col in enumerate(feature_cols):
        others = np.delete(X, i, axis=1)
        # R² from regressing col on all others
        try:
            from numpy.linalg import lstsq
            coeffs = lstsq(others, X[:, i], rcond=None)[0]
            y_pred = others @ coeffs
            ss_res = np.sum((X[:, i] - y_pred) ** 2)
            ss_tot = np.sum((X[:, i] - X[:, i].mean()) ** 2)
            r_sq = 1 - ss_res / ss_tot if ss_tot > 0 else 0
            vif = 1 / (1 - r_sq) if r_sq < 1 else float("inf")
        except Exception:
            vif = float("nan")
        vif_results[col] = round(float(vif), 2)
        flag = "⚠️" if vif > 5 else "✅"
        print(f"  {flag} VIF({col}): {vif:.2f}")

    return {
        "high_correlations": high_corr,
        "vif": vif_results,
        "max_vif": max(vif_results.values()),
    }


def plot_diagnostics(df: pd.DataFrame, outlier_report: dict):
    """Generate diagnostic figures."""
    print("\n▶ Generating Figures")

    feature_cols = [c for c in df.columns if c != "MedHouseVal"]

    # Figure 1: Distribution grid (first 8 features)
    fig, axes = plt.subplots(2, 4, figsize=(16, 8))
    fig.suptitle("Feature Distributions", fontsize=14, fontweight="bold")
    for ax, col in zip(axes.flat, df.columns[:8]):
        ax.hist(df[col], bins=40, alpha=0.7, color="#3498db", edgecolor="black", linewidth=0.3)
        ax.set_title(col, fontsize=10, fontweight="bold")
        ax.set_ylabel("Count", fontsize=8)
        skew = df[col].skew()
        ax.annotate(f"skew={skew:.2f}", xy=(0.7, 0.85), xycoords="axes fraction", fontsize=8,
                    bbox=dict(boxstyle="round", facecolor="wheat", alpha=0.5))
    plt.tight_layout()
    fig.savefig(FIGURE_DIR / "fig1_distributions.png", dpi=150, bbox_inches="tight")
    plt.close(fig)
    print("  🎨 Figure 1 → distributions")

    # Figure 2: Correlation heatmap
    corr = df[feature_cols].corr()
    fig, ax = plt.subplots(figsize=(10, 8))
    im = ax.imshow(corr.values, cmap="RdBu_r", vmin=-1, vmax=1)
    ax.set_xticks(range(len(feature_cols)))
    ax.set_yticks(range(len(feature_cols)))
    short_names = [c[:10] for c in feature_cols]
    ax.set_xticklabels(short_names, rotation=45, ha="right", fontsize=9)
    ax.set_yticklabels(short_names, fontsize=9)
    for i in range(len(feature_cols)):
        for j in range(len(feature_cols)):
            v = corr.values[i, j]
            ax.text(j, i, f"{v:.2f}", ha="center", va="center", fontsize=8,
                    color="white" if abs(v) > 0.5 else "black")
    fig.colorbar(im, ax=ax, label="Pearson r")
    ax.set_title("Feature Correlation Matrix", fontsize=14, fontweight="bold")
    plt.tight_layout()
    fig.savefig(FIGURE_DIR / "fig2_correlation_heatmap.png", dpi=150, bbox_inches="tight")
    plt.close(fig)
    print("  🎨 Figure 2 → correlation heatmap")

    # Figure 3: Outlier summary
    fig, ax = plt.subplots(figsize=(10, 5))
    cols = list(outlier_report.keys())
    pcts = [outlier_report[c]["pct_outliers"] for c in cols]
    colors = ["#e74c3c" if p > 5 else "#f39c12" if p > 1 else "#2ecc71" for p in pcts]
    ax.barh(cols, pcts, color=colors, edgecolor="black", linewidth=0.5)
    ax.set_xlabel("Outlier Percentage (%)", fontsize=12)
    ax.set_title("Outlier Rate by Feature (IQR Method)", fontsize=14, fontweight="bold")
    ax.axvline(x=5, color="red", linestyle="--", alpha=0.5, label="5% threshold")
    ax.legend()
    plt.tight_layout()
    fig.savefig(FIGURE_DIR / "fig3_outlier_rates.png", dpi=150, bbox_inches="tight")
    plt.close(fig)
    print("  🎨 Figure 3 → outlier rates")

    # Figure 4: Target vs top-2 correlated features
    target_corr = df.corr()["MedHouseVal"].drop("MedHouseVal").abs().sort_values(ascending=False)
    top2 = target_corr.index[:2].tolist()
    fig, axes = plt.subplots(1, 2, figsize=(14, 5))
    fig.suptitle("Target Variable Relationships", fontsize=14, fontweight="bold")
    for ax, feat in zip(axes, top2):
        ax.scatter(df[feat], df["MedHouseVal"], alpha=0.1, s=5, color="#3498db")
        ax.set_xlabel(feat, fontsize=11)
        ax.set_ylabel("MedHouseVal", fontsize=11)
        r = float(df[feat].corr(df["MedHouseVal"]))
        ax.set_title(f"{feat} vs Target (r={r:.3f})", fontsize=12, fontweight="bold")
    plt.tight_layout()
    fig.savefig(FIGURE_DIR / "fig4_target_relationships.png", dpi=150, bbox_inches="tight")
    plt.close(fig)
    print("  🎨 Figure 4 → target relationships")


def main():
    print("═" * 60)
    print("  GENESIS — Comprehensive Data Exploration")
    print("═" * 60)

    setup()
    df = load_data()
    profile = statistical_profile(df)
    outliers = detect_outliers(df)
    corr_analysis = correlation_analysis(df)
    plot_diagnostics(df, outliers)

    # Save results
    results = {
        "experiment": "data_exploration",
        "seed": SEED,
        "dataset": "California Housing",
        "n_samples": len(df),
        "n_features": len(df.columns) - 1,
        "statistical_profile": profile,
        "outlier_report": outliers,
        "correlation_analysis": {
            "high_correlations": corr_analysis["high_correlations"],
            "vif": corr_analysis["vif"],
        },
        "metrics": {
            "max_vif": corr_analysis["max_vif"],
            "n_high_correlations": len(corr_analysis["high_correlations"]),
            "total_outlier_pct": round(
                sum(v["pct_outliers"] for v in outliers.values()) / len(outliers), 2
            ),
        },
    }

    with open(OUTPUT_DIR / "results.json", "w") as f:
        json.dump(results, f, indent=2)
    print(f"\n  💾 Results → {OUTPUT_DIR / 'results.json'}")

    # Verify
    print("\n▶ Self-Verification")
    expected = [
        OUTPUT_DIR / "results.json",
        OUTPUT_DIR / "statistical_profile.csv",
        OUTPUT_DIR / "correlation_matrix.csv",
        FIGURE_DIR / "fig1_distributions.png",
        FIGURE_DIR / "fig2_correlation_heatmap.png",
        FIGURE_DIR / "fig3_outlier_rates.png",
        FIGURE_DIR / "fig4_target_relationships.png",
    ]
    ok = True
    for p in expected:
        if p.exists():
            print(f"  ✅ {p}")
        else:
            print(f"  ❌ {p} MISSING")
            ok = False

    print(f"\n{'✅ ALL CHECKS PASSED' if ok else '❌ SOME CHECKS FAILED'}")
    return 0 if ok else 1


if __name__ == "__main__":
    exit(main())
