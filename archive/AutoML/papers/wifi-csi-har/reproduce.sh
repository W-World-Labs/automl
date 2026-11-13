#!/usr/bin/env bash
# ═══════════════════════════════════════════════════════════════
# Auto.ml Zero-Bootstrap Reproduction Script
# Paper: AUTOML-26-00002 — CSIFormer WiFi CSI HAR
# ═══════════════════════════════════════════════════════════════
# Usage:  bash reproduce.sh
#         AUTOML_API_KEY=automl_xxx bash reproduce.sh   # with telemetry
# ═══════════════════════════════════════════════════════════════
set -euo pipefail

# ── Config ───────────────────────────────────────────────────
PAPER_ID="AUTOML-26-00002"
PAPER_TITLE="CSIFormer: WiFi CSI Human Activity Recognition"
TELEMETRY_ENDPOINT="${AUTOML_TELEMETRY_URL:-https://auto.ml/api/telemetry/reproduction}"
API_KEY="${AUTOML_API_KEY:-}"

# ── Colors ───────────────────────────────────────────────────
RED='\033[0;31m'; GREEN='\033[0;32m'; YELLOW='\033[0;33m'
CYAN='\033[0;36m'; NC='\033[0m'

PASSED=0
FAILED=0
pass() { PASSED=$((PASSED + 1)); echo -e "  ${GREEN}✅ $1${NC}"; }
fail() { FAILED=$((FAILED + 1)); echo -e "  ${RED}❌ $1${NC}"; }

# ── Telemetry ────────────────────────────────────────────────
TELEMETRY_JSON="/tmp/automl_telemetry_${PAPER_ID}.json"
RUN_START_S=$(date +%s)
echo '[]' > "$TELEMETRY_JSON"

timer_start() {
    STEP_NAME="$1"
    STEP_START_S=$(date +%s)
}

timer_end() {
    local end_s=$(date +%s)
    local elapsed=$((end_s - STEP_START_S))
    echo -e "  ${YELLOW}⏱  ${STEP_NAME}: ${elapsed}s${NC}"
    python3 -c "
import json
with open('$TELEMETRY_JSON') as f: data = json.load(f)
data.append({'stage': '$STEP_NAME', 'duration_s': $elapsed})
with open('$TELEMETRY_JSON', 'w') as f: json.dump(data, f)
" 2>/dev/null || true
}

echo ""
echo "╔══════════════════════════════════════════════════════════╗"
echo "║  Auto.ml Zero-Bootstrap — AUTOML-26-00002               ║"
echo "║  CSIFormer: WiFi CSI Human Activity Recognition         ║"
echo "╚══════════════════════════════════════════════════════════╝"
echo ""

# ═══════════════════════════════════════════════════════════════
# STAGE 1: Validate reproduce.yaml
# ═══════════════════════════════════════════════════════════════
echo -e "${CYAN}━━━ Stage 1: Validate reproduce.yaml ━━━${NC}"
timer_start "Validate"

if [ -f "reproduce.yaml" ]; then
    pass "reproduce.yaml exists"
else
    fail "reproduce.yaml not found"
    exit 1
fi

if [ -f "pyproject.toml" ]; then
    pass "pyproject.toml exists"
else
    fail "pyproject.toml not found"
    exit 1
fi

timer_end

# ═══════════════════════════════════════════════════════════════
# STAGE 2: Install uv (if needed)
# ═══════════════════════════════════════════════════════════════
echo ""
echo -e "${CYAN}━━━ Stage 2: Install uv ━━━${NC}"
timer_start "InstallUV"

if command -v uv &>/dev/null; then
    pass "uv already installed: $(uv --version)"
else
    echo "  Installing uv..."
    curl -LsSf https://astral.sh/uv/install.sh | sh
    export PATH="$HOME/.local/bin:$HOME/.cargo/bin:$PATH"
    if command -v uv &>/dev/null; then
        pass "uv installed: $(uv --version)"
    else
        fail "uv installation failed"
        exit 1
    fi
fi

timer_end

# ═══════════════════════════════════════════════════════════════
# STAGE 3: Sync Dependencies
# ═══════════════════════════════════════════════════════════════
echo ""
echo -e "${CYAN}━━━ Stage 3: Sync Dependencies ━━━${NC}"
timer_start "SyncDeps"

uv sync 2>&1 | tail -3
pass "Dependencies synced"

timer_end

# ═══════════════════════════════════════════════════════════════
# STAGE 4: Download Datasets
# ═══════════════════════════════════════════════════════════════
echo ""
echo -e "${CYAN}━━━ Stage 4: Download Datasets ━━━${NC}"
timer_start "DownloadData"

mkdir -p data/{ut_har,ntu_fi,widar,esp32_collected}

# UT-HAR
if [ -d "data/ut_har" ] && [ "$(ls -A data/ut_har 2>/dev/null)" ]; then
    pass "UT-HAR data present"
else
    echo "  ⚠ UT-HAR data not found — will use synthetic fallback"
fi

# NTU-Fi
if [ -d "data/ntu_fi" ] && [ "$(ls -A data/ntu_fi 2>/dev/null)" ]; then
    pass "NTU-Fi data present"
else
    echo "  ⚠ NTU-Fi data not found — will use synthetic fallback"
fi

# Widar
if [ -d "data/widar" ] && [ "$(ls -A data/widar 2>/dev/null)" ]; then
    pass "Widar data present"
else
    echo "  ⚠ Widar data not found — will use synthetic fallback"
fi

# ESP32 collected
if [ -d "data/esp32_collected" ] && [ "$(ls -A data/esp32_collected 2>/dev/null)" ]; then
    pass "ESP32-C6 collected data present"
else
    echo "  ⚠ ESP32 data not found — will use synthetic fallback"
fi

timer_end

# ═══════════════════════════════════════════════════════════════
# STAGE 5: Run Experiments
# ═══════════════════════════════════════════════════════════════
echo ""
echo -e "${CYAN}━━━ Stage 5: Run Experiments ━━━${NC}"
timer_start "Experiments"

uv run python experiments/run_experiment.py --epochs 50 --folds 5

if [ -f "outputs/results.json" ]; then
    pass "Results generated"
else
    fail "results.json not found"
fi

if [ -d "outputs/figures" ]; then
    FIG_COUNT=$(ls outputs/figures/*.png 2>/dev/null | wc -l | tr -d ' ')
    pass "Generated ${FIG_COUNT} figures"
else
    fail "No figures generated"
fi

timer_end

# ═══════════════════════════════════════════════════════════════
# STAGE 6: Compile Paper (if LaTeX available)
# ═══════════════════════════════════════════════════════════════
echo ""
echo -e "${CYAN}━━━ Stage 6: Compile Paper ━━━${NC}"
timer_start "CompilePaper"

if [ -f "paper.tex" ]; then
    if command -v latexmk &>/dev/null; then
        latexmk -pdf paper.tex -interaction=nonstopmode -quiet 2>&1 | tail -3
        if [ -f "paper.pdf" ]; then
            pass "Paper compiled: paper.pdf"
        else
            fail "Paper compilation failed"
        fi
    elif command -v tectonic &>/dev/null; then
        tectonic paper.tex 2>&1 | tail -3
        if [ -f "paper.pdf" ]; then
            pass "Paper compiled: paper.pdf"
        else
            fail "Paper compilation failed"
        fi
    else
        echo "  ⚠ No LaTeX compiler found (latexmk or tectonic). Skipping."
    fi
else
    echo "  ⚠ paper.tex not found. Skipping compilation."
fi

timer_end

# ═══════════════════════════════════════════════════════════════
# STAGE 7: Verify Outputs
# ═══════════════════════════════════════════════════════════════
echo ""
echo -e "${CYAN}━━━ Stage 7: Verify Outputs ━━━${NC}"
timer_start "Verify"

REQUIRED_FILES=(
    "outputs/results.json"
    "outputs/reproduction_report.json"
    "outputs/figures/fig1_csi_visualization.png"
    "outputs/figures/fig2_model_comparison.png"
    "outputs/figures/fig3_confusion_matrices.png"
    "outputs/figures/fig4_attention_heatmap.png"
    "outputs/figures/fig5_efficiency_plot.png"
)

for f in "${REQUIRED_FILES[@]}"; do
    if [ -f "$f" ]; then
        pass "$f"
    else
        fail "$f missing"
    fi
done

timer_end

# ═══════════════════════════════════════════════════════════════
# SUMMARY
# ═══════════════════════════════════════════════════════════════
echo ""
RUN_END_S=$(date +%s)
TOTAL_S=$((RUN_END_S - RUN_START_S))

echo "╔══════════════════════════════════════════════════════════╗"
if [ "$FAILED" -eq 0 ]; then
    echo -e "║  ${GREEN}VERDICT: PASSED${NC}                                        ║"
else
    echo -e "║  ${RED}VERDICT: FAILED${NC}                                        ║"
fi
echo "╚══════════════════════════════════════════════════════════╝"
echo -e "  Passed: ${GREEN}${PASSED}${NC}  Failed: ${RED}${FAILED}${NC}  Total: ${TOTAL_S}s"
echo ""

# ── Upload telemetry ─────────────────────────────────────────
if [ -n "$API_KEY" ]; then
    echo "  📡 Uploading telemetry..."
    STATUS="PASSED"
    [ "$FAILED" -gt 0 ] && STATUS="FAILED"

    STAGES=$(cat "$TELEMETRY_JSON")
    curl -s -X POST "$TELEMETRY_ENDPOINT" \
        -H "Authorization: Bearer $API_KEY" \
        -H "Content-Type: application/json" \
        -d "{
            \"paper_id\": \"$PAPER_ID\",
            \"status\": \"$STATUS\",
            \"stages_passed\": $PASSED,
            \"stages_failed\": $FAILED,
            \"total_duration_s\": $TOTAL_S,
            \"stages\": $STAGES
        }" && echo -e "  ${GREEN}✅ Telemetry uploaded${NC}" \
           || echo -e "  ${YELLOW}⚠ Telemetry upload failed (non-fatal)${NC}"
fi

rm -f "$TELEMETRY_JSON"
exit $FAILED
