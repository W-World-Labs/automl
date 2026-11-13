"""
CSIFormer: Lightweight Transformer for WiFi CSI HAR.

Architecture:
    1. Subcarrier Embedding: Conv1D per timestep → project S subcarriers to D dimensions
    2. Positional Encoding: Learnable temporal position embeddings
    3. Transformer Encoder (×L): Multi-head self-attention + FFN
    4. Global Average Pooling: [T, D] → [D]
    5. Classification Head: Linear → [C]

Key innovation: Subcarrier-Temporal Attention (STA) — the attention operates
over time steps where each token already encodes subcarrier spatial information,
enabling joint spatial-temporal reasoning with O(T²) complexity instead of O((T×S)²).
"""

import math
import torch
import torch.nn as nn


class SubcarrierEmbedding(nn.Module):
    """Project S subcarriers into D-dimensional embedding per timestep."""

    def __init__(self, n_subcarriers: int, d_model: int):
        super().__init__()
        self.projection = nn.Sequential(
            nn.Linear(n_subcarriers, d_model),
            nn.LayerNorm(d_model),
            nn.GELU(),
        )

    def forward(self, x: torch.Tensor) -> torch.Tensor:
        # x: [B, T, S] → [B, T, D]
        return self.projection(x)


class LearnablePositionalEncoding(nn.Module):
    """Learnable position embeddings for temporal positions."""

    def __init__(self, max_len: int, d_model: int, dropout: float = 0.1):
        super().__init__()
        self.pe = nn.Parameter(torch.randn(1, max_len, d_model) * 0.02)
        self.dropout = nn.Dropout(dropout)

    def forward(self, x: torch.Tensor) -> torch.Tensor:
        # x: [B, T, D]
        T = x.size(1)
        return self.dropout(x + self.pe[:, :T, :])


class STATransformerBlock(nn.Module):
    """
    Subcarrier-Temporal Attention Transformer Block.

    Standard Transformer encoder block with pre-norm and GELU activation.
    The 'STA' aspect comes from the input: each token at time t encodes
    all subcarrier spatial information, so attention naturally captures
    cross-temporal dependencies of subcarrier features.
    """

    def __init__(self, d_model: int, n_heads: int, d_ff: int, dropout: float = 0.1):
        super().__init__()
        # Pre-norm attention
        self.norm1 = nn.LayerNorm(d_model)
        self.attn = nn.MultiheadAttention(
            d_model, n_heads, dropout=dropout, batch_first=True
        )
        # Pre-norm FFN
        self.norm2 = nn.LayerNorm(d_model)
        self.ffn = nn.Sequential(
            nn.Linear(d_model, d_ff),
            nn.GELU(),
            nn.Dropout(dropout),
            nn.Linear(d_ff, d_model),
            nn.Dropout(dropout),
        )

    def forward(self, x: torch.Tensor) -> torch.Tensor:
        # Self-attention with pre-norm
        normed = self.norm1(x)
        attn_out, self.attn_weights = self.attn(normed, normed, normed)
        x = x + attn_out

        # FFN with pre-norm
        x = x + self.ffn(self.norm2(x))

        return x


class CSIFormer(nn.Module):
    """
    CSIFormer: Lightweight Transformer for WiFi CSI-based HAR.

    Args:
        n_subcarriers: Number of CSI subcarriers (input features per timestep)
        n_classes: Number of activity classes
        d_model: Transformer model dimension (default: 64)
        n_heads: Number of attention heads (default: 4)
        n_layers: Number of Transformer blocks (default: 2)
        d_ff: Feed-forward dimension (default: 128)
        max_len: Maximum sequence length (default: 500)
        dropout: Dropout rate (default: 0.1)
    """

    def __init__(
        self,
        n_subcarriers: int,
        n_classes: int,
        d_model: int = 64,
        n_heads: int = 4,
        n_layers: int = 2,
        d_ff: int = 128,
        max_len: int = 500,
        dropout: float = 0.1,
    ):
        super().__init__()

        # 1. Subcarrier Embedding
        self.embedding = SubcarrierEmbedding(n_subcarriers, d_model)

        # 2. Positional Encoding
        self.pos_encoding = LearnablePositionalEncoding(max_len, d_model, dropout)

        # 3. Transformer Encoder stack
        self.encoder = nn.ModuleList([
            STATransformerBlock(d_model, n_heads, d_ff, dropout)
            for _ in range(n_layers)
        ])

        # 4. Final norm
        self.norm = nn.LayerNorm(d_model)

        # 5. Classification head
        self.classifier = nn.Sequential(
            nn.Linear(d_model, d_model // 2),
            nn.GELU(),
            nn.Dropout(dropout),
            nn.Linear(d_model // 2, n_classes),
        )

        # Initialize weights
        self.apply(self._init_weights)

    def _init_weights(self, module):
        if isinstance(module, nn.Linear):
            nn.init.trunc_normal_(module.weight, std=0.02)
            if module.bias is not None:
                nn.init.zeros_(module.bias)
        elif isinstance(module, nn.LayerNorm):
            nn.init.ones_(module.weight)
            nn.init.zeros_(module.bias)

    def forward(self, x: torch.Tensor) -> torch.Tensor:
        """
        Forward pass.

        Args:
            x: CSI amplitude tensor [B, T, S]
               B = batch, T = time steps, S = subcarriers

        Returns:
            Logits [B, n_classes]
        """
        # Embed subcarriers → [B, T, D]
        x = self.embedding(x)

        # Add positional encoding
        x = self.pos_encoding(x)

        # Transformer encoder blocks
        for block in self.encoder:
            x = block(x)

        # Final norm
        x = self.norm(x)

        # Global average pooling over time → [B, D]
        x = x.mean(dim=1)

        # Classify
        return self.classifier(x)

    def get_attention_maps(self, x: torch.Tensor) -> list[torch.Tensor]:
        """
        Extract attention maps for visualization.

        Returns:
            List of attention weight tensors [B, H, T, T] per layer.
        """
        x = self.embedding(x)
        x = self.pos_encoding(x)

        attn_maps = []
        for block in self.encoder:
            x = block(x)
            attn_maps.append(block.attn_weights.detach())

        return attn_maps
