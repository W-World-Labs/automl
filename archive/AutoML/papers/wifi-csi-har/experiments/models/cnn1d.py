"""
1D-CNN for WiFi CSI HAR.
Applies temporal convolutions across time steps to extract spatial features.
"""

import torch
import torch.nn as nn


class CNN1D(nn.Module):
    def __init__(self, n_subcarriers: int, n_classes: int, dropout: float = 0.3):
        super().__init__()
        self.features = nn.Sequential(
            # Block 1
            nn.Conv1d(n_subcarriers, 64, kernel_size=5, padding=2),
            nn.BatchNorm1d(64),
            nn.ReLU(),
            nn.MaxPool1d(2),
            # Block 2
            nn.Conv1d(64, 128, kernel_size=3, padding=1),
            nn.BatchNorm1d(128),
            nn.ReLU(),
            nn.MaxPool1d(2),
            # Block 3
            nn.Conv1d(128, 64, kernel_size=3, padding=1),
            nn.BatchNorm1d(64),
            nn.ReLU(),
            nn.AdaptiveAvgPool1d(1),
        )
        self.classifier = nn.Sequential(
            nn.Dropout(dropout),
            nn.Linear(64, n_classes),
        )

    def forward(self, x: torch.Tensor) -> torch.Tensor:
        # x: [B, T, S] → transpose to [B, S, T] for Conv1d
        x = x.transpose(1, 2)
        x = self.features(x)  # [B, 64, 1]
        x = x.squeeze(-1)     # [B, 64]
        return self.classifier(x)
