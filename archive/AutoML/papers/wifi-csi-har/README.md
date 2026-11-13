# CSIFormer: WiFi CSI Human Activity Recognition

> Historical source snapshot. Manuscripts, datasets and weights are not included in this public distribution. Results below have not been independently reproduced for this release; consult the root README and source provenance.
> The original data loader is absent. Some runner plots/status output are illustrative and do not constitute measured experiment acceptance.

**Paper ID**: AUTOML-26-00002  
**Platform**: [Auto.ml](https://auto.ml)  
**Extends**: GENESIS-001

## Quick Start

```bash
# Install dependencies
uv sync

# Run all experiments (uses synthetic data if real data unavailable)
uv run python experiments/run_experiment.py

# Or run specific dataset/model
uv run python experiments/run_experiment.py --datasets ut_har --models csiformer
```

## ESP32-C6 Data Collection

See [firmware/README.md](firmware/README.md) for hardware setup.

```bash
# Collect CSI data
python experiments/data/collect_esp32.py --port /dev/tty.usbmodem* --activity walk --subject 1
```

## Project Structure

```
├── experiments/
│   ├── run_experiment.py       # Main entry point
│   ├── models/                 # MLP, CNN, LSTM, CNN-GRU, CSIFormer
│   ├── data/                   # Loaders, preprocessing, ESP32 collection
│   └── utils/                  # Metrics, cross-validation
├── reproduce.yaml              # Auto.ml reproduction config
├── reproduce.sh                # Zero-bootstrap script
├── paper.tex                   # LaTeX manuscript
└── references.bib              # Bibliography
```
