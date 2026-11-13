"""
Evaluation metrics for WiFi CSI HAR experiments.
"""

import time
import numpy as np
from typing import Optional


def compute_metrics(y_true: np.ndarray, y_pred: np.ndarray, n_classes: int) -> dict:
    """
    Compute classification metrics.

    Returns:
        dict with accuracy, f1_macro, per_class_f1, confusion_matrix
    """
    from sklearn.metrics import (
        accuracy_score,
        f1_score,
        confusion_matrix,
        classification_report,
    )

    acc = accuracy_score(y_true, y_pred)
    f1 = f1_score(y_true, y_pred, average="macro", zero_division=0)
    cm = confusion_matrix(y_true, y_pred, labels=list(range(n_classes)))
    per_class_f1 = f1_score(y_true, y_pred, average=None, zero_division=0, labels=list(range(n_classes)))

    return {
        "accuracy": float(acc),
        "f1_macro": float(f1),
        "per_class_f1": per_class_f1.tolist(),
        "confusion_matrix": cm.tolist(),
    }


def measure_inference_time(model, sample_input, n_runs: int = 100, device: str = "cpu") -> float:
    """
    Measure average inference time in milliseconds.
    """
    import torch

    model.eval()
    model = model.to(device)
    x = sample_input.to(device)

    # Warmup
    with torch.no_grad():
        for _ in range(10):
            model(x)

    # Measure
    if device == "cuda":
        torch.cuda.synchronize()

    start = time.perf_counter()
    with torch.no_grad():
        for _ in range(n_runs):
            model(x)

    if device == "cuda":
        torch.cuda.synchronize()

    elapsed = (time.perf_counter() - start) / n_runs * 1000  # ms
    return round(elapsed, 3)


def count_parameters(model) -> int:
    """Count total trainable parameters."""
    return sum(p.numel() for p in model.parameters() if p.requires_grad)


def cross_validate(
    model_class,
    model_kwargs: dict,
    X: np.ndarray,
    y: np.ndarray,
    n_folds: int = 5,
    n_epochs: int = 50,
    batch_size: int = 32,
    lr: float = 1e-3,
    device: str = "cpu",
    seed: int = 42,
    verbose: bool = True,
) -> dict:
    """
    K-fold cross-validation for PyTorch models.

    Args:
        model_class: Model class to instantiate
        model_kwargs: Keyword arguments for model constructor
        X: Input data [N, T, S]
        y: Labels [N]
        n_folds: Number of CV folds
        n_epochs: Training epochs per fold
        batch_size: Batch size
        lr: Learning rate
        device: 'cpu' or 'cuda'
        seed: Random seed

    Returns:
        Dictionary with per-fold and mean metrics.
    """
    import torch
    from torch.utils.data import DataLoader, TensorDataset
    from sklearn.model_selection import StratifiedKFold

    np.random.seed(seed)
    torch.manual_seed(seed)

    n_classes = len(np.unique(y))
    skf = StratifiedKFold(n_splits=n_folds, shuffle=True, random_state=seed)

    fold_results = []

    for fold_i, (train_idx, test_idx) in enumerate(skf.split(X, y)):
        if verbose:
            print(f"    Fold {fold_i + 1}/{n_folds}", end=" ", flush=True)

        # Data
        X_train = torch.FloatTensor(X[train_idx])
        y_train = torch.LongTensor(y[train_idx])
        X_test = torch.FloatTensor(X[test_idx])
        y_test = torch.LongTensor(y[test_idx])

        train_loader = DataLoader(
            TensorDataset(X_train, y_train),
            batch_size=batch_size, shuffle=True, drop_last=False
        )

        # Model
        model = model_class(**model_kwargs).to(device)
        optimizer = torch.optim.Adam(model.parameters(), lr=lr)
        criterion = torch.nn.CrossEntropyLoss()

        # Train
        model.train()
        for epoch in range(n_epochs):
            for xb, yb in train_loader:
                xb, yb = xb.to(device), yb.to(device)
                optimizer.zero_grad()
                out = model(xb)
                loss = criterion(out, yb)
                loss.backward()
                optimizer.step()

        # Evaluate
        model.eval()
        with torch.no_grad():
            preds = model(X_test.to(device)).argmax(dim=1).cpu().numpy()

        metrics = compute_metrics(y_test.numpy(), preds, n_classes)
        fold_results.append(metrics)

        if verbose:
            print(f"→ Acc: {metrics['accuracy']:.4f}  F1: {metrics['f1_macro']:.4f}")

    # Aggregate
    mean_acc = np.mean([r["accuracy"] for r in fold_results])
    std_acc = np.std([r["accuracy"] for r in fold_results])
    mean_f1 = np.mean([r["f1_macro"] for r in fold_results])
    std_f1 = np.std([r["f1_macro"] for r in fold_results])

    return {
        "folds": fold_results,
        "mean_accuracy": round(float(mean_acc), 4),
        "std_accuracy": round(float(std_acc), 4),
        "mean_f1": round(float(mean_f1), 4),
        "std_f1": round(float(std_f1), 4),
        "n_folds": n_folds,
    }
