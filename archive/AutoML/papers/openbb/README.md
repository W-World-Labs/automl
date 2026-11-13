# OPENBB-001: OpenBB Multi-Modal LLM Training Framework

> Historical source snapshot. Manuscripts, datasets and weights are not included in this public distribution. Results below have not been independently reproduced for this release; consult the root README and source provenance.
> The OpenBB implementation, configuration and toy input data must be supplied separately; only the original callers and assertions are retained here.

Paper for the [Auto.ml](https://auto.ml) platform.

## Quick Start

```bash
# Install uv (if not present)
curl -LsSf https://astral.sh/uv/install.sh | sh

# Install dependencies
uv sync

# Run experiments
uv run python experiments/test_config.py     # Config validation
uv run python experiments/test_pretrain.py   # Pretrain smoke test

# Compile paper
latexmk -pdf openbb-paper.tex
```

## Structure

```
openbb/
├── openbb-paper.tex       # Main paper
├── references.bib          # Bibliography (18 references)
├── reproduce.yaml          # Reproducibility manifest
├── reproduce.sh            # Zero-Bootstrap script
├── pyproject.toml          # Python project
├── automl-paper.cls        # LaTeX class
├── experiments/
│   ├── test_config.py      # Config pipeline validation
│   └── test_pretrain.py    # Pretrain smoke test
├── data/                   # Bundled data
├── figures/                # Generated figures
├── outputs/                # Experiment outputs
└── refs/                   # Reference evidence files
```

## Dependencies

- Python 3.12+
- PyTorch 2.5+
- Transformers 4.46+
- OpenBB source (from minimind repository)
