"""
MLP Baseline for WiFi CSI HAR.
Flattens the CSI matrix and classifies with fully-connected layers.
"""

import torch
import torch.nn as nn


class MLP(nn.Module):
    def __init__(self, input_size: int, n_classes: int, hidden_dim: int = 128, dropout: float = 0.3):
        super().__init__()
        self.flatten = nn.Flatten()
        self.net = nn.Sequential(
            nn.Linear(input_size, hidden_dim),
            nn.ReLU(),
            nn.Dropout(dropout),
            nn.Linear(hidden_dim, hidden_dim // 2),
            nn.ReLU(),
            nn.Dropout(dropout),
            nn.Linear(hidden_dim // 2, n_classes),
        )

    def forward(self, x: torch.Tensor) -> torch.Tensor:
        # x: [B, T, S]
        x = self.flatten(x)  # [B, T*S]
        return self.net(x)
