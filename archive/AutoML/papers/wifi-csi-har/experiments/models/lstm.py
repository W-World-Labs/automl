"""
LSTM for WiFi CSI HAR.
Processes CSI sequences with bidirectional LSTM to capture temporal dynamics.
"""

import torch
import torch.nn as nn


class LSTMModel(nn.Module):
    def __init__(
        self,
        n_subcarriers: int,
        n_classes: int,
        hidden_dim: int = 128,
        n_layers: int = 2,
        bidirectional: bool = True,
        dropout: float = 0.3,
    ):
        super().__init__()
        self.lstm = nn.LSTM(
            input_size=n_subcarriers,
            hidden_size=hidden_dim,
            num_layers=n_layers,
            batch_first=True,
            bidirectional=bidirectional,
            dropout=dropout if n_layers > 1 else 0,
        )
        fc_input = hidden_dim * 2 if bidirectional else hidden_dim
        self.classifier = nn.Sequential(
            nn.Dropout(dropout),
            nn.Linear(fc_input, n_classes),
        )

    def forward(self, x: torch.Tensor) -> torch.Tensor:
        # x: [B, T, S]
        lstm_out, _ = self.lstm(x)  # [B, T, H*2]
        # Use last time step output
        last = lstm_out[:, -1, :]  # [B, H*2]
        return self.classifier(last)
