"""
CNN-GRU Hybrid for WiFi CSI HAR.
Combines spatial feature extraction (CNN) with temporal modeling (GRU).
"""

import torch
import torch.nn as nn


class CNNGRU(nn.Module):
    def __init__(
        self,
        n_subcarriers: int,
        n_classes: int,
        cnn_channels: int = 64,
        gru_hidden: int = 128,
        gru_layers: int = 1,
        dropout: float = 0.3,
    ):
        super().__init__()
        # Spatial feature extraction via Conv1d
        self.cnn = nn.Sequential(
            nn.Conv1d(n_subcarriers, cnn_channels, kernel_size=3, padding=1),
            nn.BatchNorm1d(cnn_channels),
            nn.ReLU(),
            nn.Conv1d(cnn_channels, cnn_channels, kernel_size=3, padding=1),
            nn.BatchNorm1d(cnn_channels),
            nn.ReLU(),
        )
        # Temporal modeling
        self.gru = nn.GRU(
            input_size=cnn_channels,
            hidden_size=gru_hidden,
            num_layers=gru_layers,
            batch_first=True,
            dropout=dropout if gru_layers > 1 else 0,
        )
        self.classifier = nn.Sequential(
            nn.Dropout(dropout),
            nn.Linear(gru_hidden, n_classes),
        )

    def forward(self, x: torch.Tensor) -> torch.Tensor:
        # x: [B, T, S]
        x = x.transpose(1, 2)    # [B, S, T]
        x = self.cnn(x)          # [B, C, T]
        x = x.transpose(1, 2)    # [B, T, C]
        gru_out, _ = self.gru(x) # [B, T, H]
        last = gru_out[:, -1, :] # [B, H]
        return self.classifier(last)
