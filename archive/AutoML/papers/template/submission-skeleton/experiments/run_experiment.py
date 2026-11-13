"""
Auto Submission Template — Example Experiment
==============================================
A complete, working ML experiment that demonstrates the Auto
submission standard. Replace this with your own experiment code.

This example:
  1. Loads the Wine dataset (bundled with scikit-learn)
  2. Performs exploratory data analysis
  3. Trains a Random Forest with 5-fold cross-validation
  4. Generates publication-quality figures
  5. Saves metrics JSON for automated verification

Usage:
  uv run python experiments/run_experiment.py
"""

import json
import hashlib
from pathlib import Path

import numpy as np
import pandas as pd
import matplotlib
matplotlib.use("Agg")  # headless — no display server needed
import matplotlib.pyplot as plt
from sklearn.datasets import load_wine
from sklearn.model_selection import cross_val_score, StratifiedKFold
from sklearn.ensemble import RandomForestClassifier
from sklearn.preprocessing import StandardScaler
from sklearn.metrics import classification_report, confusion_matrix

# ── Configuration ──────────────────────────────────────────────────
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
    print("✅ Directories ready")


def load_data() -> pd.DataFrame:
    """
    Load the Wine dataset. In a real paper, this would download
    from an external source with SHA256 verification.
    """
    cache_path = DATA_DIR / "wine.csv"

    if cache_path.exists():
        print(f"  📦 Cached: {cache_path}")
        df = pd.read_csv(cache_path)
    else:
        print("  ⬇️  Loading Wine dataset...")
        wine = load_wine()
        df = pd.DataFrame(data=wine.data, columns=wine.feature_names)
        df["target"] = wine.target
        df["class_name"] = df["target"].map(
            {0: "class_0", 1: "class_1", 2: "class_2"}
        )
        df.to_csv(cache_path, index=False)
        print(f"  💾 Saved → {cache_path}")

    sha = hashlib.sha256(cache_path.read_bytes()).hexdigest()
    print(f"  🔒 SHA256: {sha[:16]}...")
    print(f"  📊 Shape: {df.shape[0]} × {df.shape[1]}")
    return df


def run_eda(df: pd.DataFrame) -> dict:
    """Generate EDA figures and statistics."""
    print("\n▶ Exploratory Data Analysis")

    feature_cols = [c for c in df.columns if c not in ("target", "class_name")]

    # Summary stats
    stats = df[feature_cols].describe()
    stats.to_csv(OUTPUT_DIR / "summary_statistics.csv")
    print(f"  📊 Summary stats saved")

    # Correlation
    corr = df[feature_cols].corr()
    corr.to_csv(OUTPUT_DIR / "correlation_matrix.csv")

    # Class distribution
    class_dist = df["class_name"].value_counts().to_dict()
    print(f"  📊 Classes: {class_dist}")

    # ── Figure 1: Top-4 feature distributions ──
    top4 = ["alcohol", "malic_acid", "flavanoids", "color_intensity"]
    fig, axes = plt.subplots(2, 2, figsize=(10, 8))
    fig.suptitle("Feature Distributions by Wine Class", fontsize=14, fontweight="bold")
    colors = {"class_0": "#2ecc71", "class_1": "#3498db", "class_2": "#e74c3c"}
    for ax, col in zip(axes.flat, top4):
        for cls, color in colors.items():
            subset = df[df["class_name"] == cls]
            ax.hist(subset[col], bins=15, alpha=0.6, label=cls, color=color)
        ax.set_xlabel(col, fontsize=9)
        ax.set_ylabel("Count")
        ax.legend(fontsize=7)
    plt.tight_layout()
    fig.savefig(FIGURE_DIR / "fig1_feature_distributions.png", dpi=150, bbox_inches="tight")
    plt.close(fig)
    print("  🎨 Figure 1 → feature distributions")

    # ── Figure 2: Correlation heatmap (top-6 features) ──
    top6 = top4 + ["od280/od315_of_diluted_wines", "proline"]
    sub_corr = df[top6].corr()
    fig, ax = plt.subplots(figsize=(8, 6))
    im = ax.imshow(sub_corr.values, cmap="RdBu_r", vmin=-1, vmax=1)
    ax.set_xticks(range(len(top6)))
    ax.set_yticks(range(len(top6)))
    short = [c[:12] for c in top6]
    ax.set_xticklabels(short, rotation=45, ha="right", fontsize=9)
    ax.set_yticklabels(short, fontsize=9)
    for i in range(len(top6)):
        for j in range(len(top6)):
            v = sub_corr.values[i, j]
            ax.text(j, i, f"{v:.2f}", ha="center", va="center", fontsize=9,
                    color="white" if abs(v) > 0.5 else "black")
    fig.colorbar(im, ax=ax, label="Pearson r")
    ax.set_title("Feature Correlation Matrix", fontsize=14, fontweight="bold")
    plt.tight_layout()
    fig.savefig(FIGURE_DIR / "fig2_correlation_heatmap.png", dpi=150, bbox_inches="tight")
    plt.close(fig)
    print("  🎨 Figure 2 → correlation heatmap")

    return {
        "n_samples": int(df.shape[0]),
        "n_features": len(feature_cols),
        "class_distribution": class_dist,
    }


def train_and_evaluate(df: pd.DataFrame) -> dict:
    """Train Random Forest with cross-validation."""
    print("\n▶ Model Training & Cross-Validation")

    feature_cols = [c for c in df.columns if c not in ("target", "class_name")]
    X = df[feature_cols].values
    y = df["target"].values.astype(int)

    scaler = StandardScaler()
    X_scaled = scaler.fit_transform(X)

    model = RandomForestClassifier(
        n_estimators=100,
        max_depth=8,
        random_state=SEED,
    )

    cv = StratifiedKFold(n_splits=5, shuffle=True, random_state=SEED)
    scores = cross_val_score(model, X_scaled, y, cv=cv, scoring="accuracy")

    print(f"  🎯 CV Accuracy: {scores.mean():.4f} ± {scores.std():.4f}")

    # Final model for confusion matrix
    model.fit(X_scaled, y)
    y_pred = model.predict(X_scaled)
    cm = confusion_matrix(y, y_pred)
    report = classification_report(
        y, y_pred, target_names=["class_0", "class_1", "class_2"], output_dict=True
    )

    # ── Figure 3: Confusion Matrix ──
    fig, ax = plt.subplots(figsize=(7, 6))
    im = ax.imshow(cm, cmap="Blues")
    classes = ["Class 0", "Class 1", "Class 2"]
    ax.set_xticks(range(3))
    ax.set_yticks(range(3))
    ax.set_xticklabels(classes, fontsize=11)
    ax.set_yticklabels(classes, fontsize=11)
    ax.set_xlabel("Predicted", fontsize=12)
    ax.set_ylabel("Actual", fontsize=12)
    for i in range(3):
        for j in range(3):
            ax.text(j, i, str(cm[i, j]), ha="center", va="center", fontsize=16,
                    fontweight="bold", color="white" if cm[i, j] > cm.max() / 2 else "black")
    ax.set_title("Confusion Matrix", fontsize=14, fontweight="bold")
    fig.colorbar(im, ax=ax)
    plt.tight_layout()
    fig.savefig(FIGURE_DIR / "fig3_confusion_matrix.png", dpi=150, bbox_inches="tight")
    plt.close(fig)
    print("  🎨 Figure 3 → confusion matrix")

    # ── Figure 4: Feature Importance ──
    importances = model.feature_importances_
    indices = np.argsort(importances)[::-1][:8]  # Top 8
    fig, ax = plt.subplots(figsize=(8, 5))
    names = [feature_cols[i][:15] for i in indices]
    bars = ax.barh(range(len(indices)), importances[indices], color="#3498db")
    ax.set_yticks(range(len(indices)))
    ax.set_yticklabels(names, fontsize=9)
    ax.set_xlabel("Importance (Gini)", fontsize=12)
    ax.set_title("Top-8 Feature Importance", fontsize=14, fontweight="bold")
    ax.invert_yaxis()
    plt.tight_layout()
    fig.savefig(FIGURE_DIR / "fig4_feature_importance.png", dpi=150, bbox_inches="tight")
    plt.close(fig)
    print("  🎨 Figure 4 → feature importance")

    return {
        "cv_accuracy_mean": round(float(scores.mean()), 4),
        "cv_accuracy_std": round(float(scores.std()), 4),
        "cv_fold_scores": [round(float(s), 4) for s in scores],
        "train_accuracy": round(float(report["accuracy"]), 4),
        "per_class_f1": {
            name: round(report[name]["f1-score"], 4)
            for name in ["class_0", "class_1", "class_2"]
        },
    }


def save_results(eda_stats: dict, metrics: dict):
    """Save results JSON for automated verification."""
    print("\n▶ Saving Results")

    results = {
        "paper_id": "TEMPLATE-001",
        "experiment": "wine_classification_demo",
        "seed": SEED,
        "eda": eda_stats,
        "model": {
            "type": "RandomForestClassifier",
            "n_estimators": 100,
            "max_depth": 8,
        },
        "metrics": metrics,
    }

    results_path = OUTPUT_DIR / "results.json"
    with open(results_path, "w") as f:
        json.dump(results, f, indent=2)
    print(f"  💾 → {results_path}")
    return results


def verify_outputs() -> bool:
    """Self-check: verify all expected files exist and metrics pass."""
    print("\n▶ Self-Verification")

    expected = [
        OUTPUT_DIR / "results.json",
        OUTPUT_DIR / "summary_statistics.csv",
        OUTPUT_DIR / "correlation_matrix.csv",
        FIGURE_DIR / "fig1_feature_distributions.png",
        FIGURE_DIR / "fig2_correlation_heatmap.png",
        FIGURE_DIR / "fig3_confusion_matrix.png",
        FIGURE_DIR / "fig4_feature_importance.png",
    ]

    ok = True
    for p in expected:
        if p.exists() and p.stat().st_size > 0:
            print(f"  ✅ {p.relative_to('.')} ({p.stat().st_size:,}B)")
        else:
            print(f"  ❌ {p.relative_to('.')} MISSING")
            ok = False

    # Check metric threshold
    results = json.loads((OUTPUT_DIR / "results.json").read_text())
    acc = results["metrics"]["cv_accuracy_mean"]
    print(f"\n  🎯 CV Accuracy: {acc:.4f} (threshold: ≥0.90)")
    if acc >= 0.90:
        print("  ✅ Accuracy PASSED")
    else:
        print("  ❌ Accuracy FAILED")
        ok = False

    return ok


def main():
    print("═" * 60)
    print("  Auto Template — Wine Classification Experiment")
    print("═" * 60)

    setup_dirs()

    print("\n▶ Loading Dataset")
    df = load_data()

    eda_stats = run_eda(df)
    metrics = train_and_evaluate(df)
    save_results(eda_stats, metrics)
    ok = verify_outputs()

    print("\n" + "═" * 60)
    if ok:
        print("  ✅ ALL CHECKS PASSED — Ready for submission")
    else:
        print("  ❌ SOME CHECKS FAILED — Fix before submitting")
    print("═" * 60)

    return 0 if ok else 1


if __name__ == "__main__":
    exit(main())
