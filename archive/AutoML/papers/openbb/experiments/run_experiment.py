"""
Auto.ml GENESIS-001 — Demonstration Experiment
================================================
A minimal but complete ML pipeline demonstrating the Auto.ml
Zero-Bootstrap standard. This script:

  1. Downloads the Iris dataset (or loads from cache)
  2. Performs exploratory data analysis (EDA)
  3. Trains a classifier with cross-validation
  4. Generates publication-quality figures
  5. Saves metrics for automated verification

This serves as a TEMPLATE for agents writing real papers.
"""

import json
import hashlib
from pathlib import Path

import numpy as np
import pandas as pd
import matplotlib
matplotlib.use("Agg")  # headless rendering
import matplotlib.pyplot as plt
from sklearn.datasets import load_iris
from sklearn.model_selection import cross_val_score, StratifiedKFold
from sklearn.ensemble import RandomForestClassifier
from sklearn.preprocessing import StandardScaler
from sklearn.metrics import classification_report, confusion_matrix

# ── Configuration ──────────────────────────────────────────────
SEED = 42
OUTPUT_DIR = Path("outputs")
FIGURE_DIR = OUTPUT_DIR / "figures"
DATA_DIR = Path("data")

np.random.seed(SEED)


def setup_dirs():
    """Create output directories."""
    OUTPUT_DIR.mkdir(exist_ok=True)
    FIGURE_DIR.mkdir(exist_ok=True)
    DATA_DIR.mkdir(exist_ok=True)
    print("✅ Directories created")


def load_data() -> pd.DataFrame:
    """
    Step 1: Load and cache the Iris dataset.
    In a real paper, this would download from a URL with SHA256 verification.
    """
    cache_path = DATA_DIR / "iris.csv"

    if cache_path.exists():
        print(f"  📦 Loading cached data from {cache_path}")
        df = pd.read_csv(cache_path)
    else:
        print("  ⬇️  Downloading Iris dataset...")
        iris = load_iris()
        df = pd.DataFrame(
            data=np.c_[iris.data, iris.target],
            columns=[*iris.feature_names, "target"],
        )
        df["species"] = df["target"].map(
            {0: "setosa", 1: "versicolor", 2: "virginica"}
        )
        df.to_csv(cache_path, index=False)
        print(f"  💾 Saved to {cache_path}")

    # Compute and log SHA256 for reproducibility
    sha = hashlib.sha256(cache_path.read_bytes()).hexdigest()
    print(f"  🔒 SHA256: {sha[:16]}...")
    print(f"  📊 Shape: {df.shape[0]} samples × {df.shape[1]} columns")

    return df


def run_eda(df: pd.DataFrame) -> dict:
    """
    Step 2: Exploratory Data Analysis.
    Generates summary statistics and correlation analysis.
    """
    print("\n▶ Step 2: Exploratory Data Analysis")

    feature_cols = [c for c in df.columns if c not in ("target", "species")]

    # Summary statistics
    stats = df[feature_cols].describe()
    stats_path = OUTPUT_DIR / "summary_statistics.csv"
    stats.to_csv(stats_path)
    print(f"  📊 Summary stats → {stats_path}")

    # Correlation matrix
    corr = df[feature_cols].corr()
    corr_path = OUTPUT_DIR / "correlation_matrix.csv"
    corr.to_csv(corr_path)
    print(f"  📊 Correlation matrix → {corr_path}")

    # Class distribution
    class_dist = df["species"].value_counts().to_dict()
    print(f"  📊 Class distribution: {class_dist}")

    # ── Figure 1: Feature distributions ──
    fig, axes = plt.subplots(2, 2, figsize=(10, 8))
    fig.suptitle("Feature Distributions by Species", fontsize=14, fontweight="bold")
    colors = {"setosa": "#2ecc71", "versicolor": "#3498db", "virginica": "#e74c3c"}

    for ax, col in zip(axes.flat, feature_cols):
        for species, color in colors.items():
            subset = df[df["species"] == species]
            ax.hist(subset[col], bins=15, alpha=0.6, label=species, color=color)
        ax.set_xlabel(col, fontsize=9)
        ax.set_ylabel("Count")
        ax.legend(fontsize=8)

    plt.tight_layout()
    fig1_path = FIGURE_DIR / "fig1_feature_distributions.png"
    fig.savefig(fig1_path, dpi=150, bbox_inches="tight")
    plt.close(fig)
    print(f"  🎨 Figure 1 → {fig1_path}")

    # ── Figure 2: Correlation heatmap ──
    fig, ax = plt.subplots(figsize=(8, 6))
    im = ax.imshow(corr.values, cmap="RdBu_r", vmin=-1, vmax=1)
    ax.set_xticks(range(len(feature_cols)))
    ax.set_yticks(range(len(feature_cols)))
    ax.set_xticklabels([c.replace(" (cm)", "") for c in feature_cols], rotation=45, ha="right", fontsize=9)
    ax.set_yticklabels([c.replace(" (cm)", "") for c in feature_cols], fontsize=9)
    # Annotate cells
    for i in range(len(feature_cols)):
        for j in range(len(feature_cols)):
            ax.text(j, i, f"{corr.values[i, j]:.2f}", ha="center", va="center", fontsize=10,
                    color="white" if abs(corr.values[i, j]) > 0.5 else "black")
    fig.colorbar(im, ax=ax, label="Pearson r")
    ax.set_title("Feature Correlation Matrix", fontsize=14, fontweight="bold")
    plt.tight_layout()
    fig2_path = FIGURE_DIR / "fig2_correlation_heatmap.png"
    fig.savefig(fig2_path, dpi=150, bbox_inches="tight")
    plt.close(fig)
    print(f"  🎨 Figure 2 → {fig2_path}")

    return {
        "n_samples": int(df.shape[0]),
        "n_features": len(feature_cols),
        "class_distribution": class_dist,
    }


def train_and_evaluate(df: pd.DataFrame) -> dict:
    """
    Step 3: Train a Random Forest classifier with cross-validation.
    Returns metrics for automated verification.
    """
    print("\n▶ Step 3: Model Training & Cross-Validation")

    feature_cols = [c for c in df.columns if c not in ("target", "species")]
    X = df[feature_cols].values
    y = df["target"].values.astype(int)

    # Standardize features
    scaler = StandardScaler()
    X_scaled = scaler.fit_transform(X)

    # Train with stratified 5-fold CV
    model = RandomForestClassifier(
        n_estimators=100,
        max_depth=5,
        random_state=SEED,
    )

    cv = StratifiedKFold(n_splits=5, shuffle=True, random_state=SEED)
    scores = cross_val_score(model, X_scaled, y, cv=cv, scoring="accuracy")

    print(f"  🎯 CV Accuracy: {scores.mean():.4f} ± {scores.std():.4f}")
    print(f"  📊 Fold scores: {[f'{s:.4f}' for s in scores]}")

    # Final model on full data for confusion matrix
    model.fit(X_scaled, y)
    y_pred = model.predict(X_scaled)
    cm = confusion_matrix(y, y_pred)
    report = classification_report(y, y_pred, target_names=["setosa", "versicolor", "virginica"], output_dict=True)

    # ── Figure 3: Confusion Matrix ──
    fig, ax = plt.subplots(figsize=(7, 6))
    im = ax.imshow(cm, cmap="Blues")
    species = ["Setosa", "Versicolor", "Virginica"]
    ax.set_xticks(range(3))
    ax.set_yticks(range(3))
    ax.set_xticklabels(species, fontsize=11)
    ax.set_yticklabels(species, fontsize=11)
    ax.set_xlabel("Predicted", fontsize=12)
    ax.set_ylabel("Actual", fontsize=12)
    for i in range(3):
        for j in range(3):
            ax.text(j, i, str(cm[i, j]), ha="center", va="center",
                    fontsize=16, fontweight="bold",
                    color="white" if cm[i, j] > cm.max() / 2 else "black")
    ax.set_title("Confusion Matrix (Full Training Set)", fontsize=14, fontweight="bold")
    fig.colorbar(im, ax=ax)
    plt.tight_layout()
    fig3_path = FIGURE_DIR / "fig3_confusion_matrix.png"
    fig.savefig(fig3_path, dpi=150, bbox_inches="tight")
    plt.close(fig)
    print(f"  🎨 Figure 3 → {fig3_path}")

    # ── Figure 4: Feature Importance ──
    importances = model.feature_importances_
    indices = np.argsort(importances)[::-1]
    fig, ax = plt.subplots(figsize=(8, 5))
    bars = ax.bar(range(len(feature_cols)), importances[indices],
                  color=["#3498db", "#2ecc71", "#e74c3c", "#f39c12"])
    ax.set_xticks(range(len(feature_cols)))
    ax.set_xticklabels([feature_cols[i].replace(" (cm)", "") for i in indices],
                       rotation=30, ha="right", fontsize=10)
    ax.set_ylabel("Importance (Gini)", fontsize=12)
    ax.set_title("Random Forest Feature Importance", fontsize=14, fontweight="bold")
    # Add value labels
    for bar, imp in zip(bars, importances[indices]):
        ax.text(bar.get_x() + bar.get_width() / 2, bar.get_height() + 0.01,
                f"{imp:.3f}", ha="center", fontsize=10)
    plt.tight_layout()
    fig4_path = FIGURE_DIR / "fig4_feature_importance.png"
    fig.savefig(fig4_path, dpi=150, bbox_inches="tight")
    plt.close(fig)
    print(f"  🎨 Figure 4 → {fig4_path}")

    metrics = {
        "cv_accuracy_mean": round(float(scores.mean()), 4),
        "cv_accuracy_std": round(float(scores.std()), 4),
        "cv_fold_scores": [round(float(s), 4) for s in scores],
        "train_accuracy": round(float(report["accuracy"]), 4),
        "per_class_f1": {
            name: round(report[name]["f1-score"], 4)
            for name in ["setosa", "versicolor", "virginica"]
        },
    }

    return metrics


def save_results(eda_stats: dict, metrics: dict):
    """
    Step 4: Save all results as JSON for automated verification.
    The Reproducer agent checks these against reproduce.yaml tolerances.
    """
    print("\n▶ Step 4: Saving Results")

    results = {
        "paper_id": "GENESIS-001",
        "experiment": "iris_classification_demo",
        "seed": SEED,
        "eda": eda_stats,
        "model": {
            "type": "RandomForestClassifier",
            "n_estimators": 100,
            "max_depth": 5,
        },
        "metrics": metrics,
    }

    results_path = OUTPUT_DIR / "results.json"
    with open(results_path, "w") as f:
        json.dump(results, f, indent=2)
    print(f"  💾 Results → {results_path}")

    return results


def verify_outputs():
    """
    Step 5: Self-verification — check all expected outputs exist.
    """
    print("\n▶ Step 5: Output Verification")

    expected_files = [
        OUTPUT_DIR / "results.json",
        OUTPUT_DIR / "summary_statistics.csv",
        OUTPUT_DIR / "correlation_matrix.csv",
        FIGURE_DIR / "fig1_feature_distributions.png",
        FIGURE_DIR / "fig2_correlation_heatmap.png",
        FIGURE_DIR / "fig3_confusion_matrix.png",
        FIGURE_DIR / "fig4_feature_importance.png",
    ]

    all_ok = True
    for path in expected_files:
        if path.exists() and path.stat().st_size > 0:
            print(f"  ✅ {path.relative_to('.')} ({path.stat().st_size:,}B)")
        else:
            print(f"  ❌ {path.relative_to('.')} MISSING")
            all_ok = False

    # Verify metrics meet minimum thresholds
    results = json.loads((OUTPUT_DIR / "results.json").read_text())
    acc = results["metrics"]["cv_accuracy_mean"]
    print(f"\n  🎯 CV Accuracy: {acc:.4f} (threshold: ≥0.90)")
    if acc >= 0.90:
        print("  ✅ Accuracy check PASSED")
    else:
        print("  ❌ Accuracy check FAILED")
        all_ok = False

    return all_ok


def main():
    print("═" * 60)
    print("  Auto.ml GENESIS-001 — Demonstration Experiment")
    print("═" * 60)

    setup_dirs()

    print("\n▶ Step 1: Loading Dataset")
    df = load_data()

    eda_stats = run_eda(df)

    metrics = train_and_evaluate(df)

    results = save_results(eda_stats, metrics)

    ok = verify_outputs()

    print("\n" + "═" * 60)
    if ok:
        print("  ✅ ALL CHECKS PASSED — Experiment reproducible")
    else:
        print("  ❌ SOME CHECKS FAILED — See above for details")
    print("═" * 60)

    return 0 if ok else 1


if __name__ == "__main__":
    exit(main())
