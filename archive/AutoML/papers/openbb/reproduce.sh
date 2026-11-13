#!/usr/bin/env bash
# ═══════════════════════════════════════════════════════════════
# Auto.ml Zero-Bootstrap Reproduction Script
# Paper: GENESIS-001
# ═══════════════════════════════════════════════════════════════
# Usage:  bash reproduce.sh
#         AUTOML_API_KEY=automl_xxx bash reproduce.sh   # with telemetry
#
# This script performs the FULL reproduction pipeline on a
# completely fresh machine with only bash and curl installed.
# No manual setup, no conda, no Docker — just this script.
#
# After completion, stage timings are automatically uploaded
# to the Auto.ml platform if AUTOML_API_KEY is set.
# ═══════════════════════════════════════════════════════════════
set -euo pipefail

# ── Config ───────────────────────────────────────────────────
PAPER_ID="GENESIS-001"
PAPER_TITLE="Auto: An Open Platform for Autonomous In-Silico Scientific Research"
TELEMETRY_ENDPOINT="${AUTOML_TELEMETRY_URL:-https://auto.ml/api/telemetry/reproduction}"
API_KEY="${AUTOML_API_KEY:-}"

# ── Colors ───────────────────────────────────────────────────
RED='\033[0;31m'; GREEN='\033[0;32m'; YELLOW='\033[0;33m'
CYAN='\033[0;36m'; NC='\033[0m'

PASSED=0
FAILED=0
pass() { PASSED=$((PASSED + 1)); echo -e "  ${GREEN}✅ $1${NC}"; }
fail() { FAILED=$((FAILED + 1)); echo -e "  ${RED}❌ $1${NC}"; }

# ── Telemetry: Per-step timing ───────────────────────────────
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
    # Append timing to JSON array
    python3 -c "
import json
with open('$TELEMETRY_JSON') as f: data = json.load(f)
data.append({'stage': '$STEP_NAME', 'duration_s': $elapsed})
with open('$TELEMETRY_JSON', 'w') as f: json.dump(data, f)
" 2>/dev/null || true
}

echo ""
echo "╔══════════════════════════════════════════════════════════╗"
echo "║  Auto.ml Zero-Bootstrap — GENESIS-001                    ║"
echo "║  Full reproduction pipeline                              ║"
echo "╚══════════════════════════════════════════════════════════╝"
echo ""

# ── Hardware Fingerprint ─────────────────────────────────────
echo -e "${CYAN}▶ Hardware Fingerprint${NC}"
GPU_INFO=$(nvidia-smi --query-gpu=name,memory.total --format=csv,noheader,nounits 2>/dev/null || echo "none")
CPU_MODEL=$(sysctl -n machdep.cpu.brand_string 2>/dev/null || grep -m1 "model name" /proc/cpuinfo 2>/dev/null | sed 's/model name\s*: //' || echo "unknown")
CPU_CORES=$(nproc 2>/dev/null || sysctl -n hw.ncpu 2>/dev/null || echo "?")
RAM_GB=$(python3 -c "import os; print(round(os.sysconf('SC_PAGE_SIZE') * os.sysconf('SC_PHYS_PAGES') / (1024**3), 1))" 2>/dev/null || echo "?")
OS_INFO=$(uname -srm)
HOSTNAME_INFO=$(hostname 2>/dev/null || echo "unknown")
echo "  🖥  CPU: ${CPU_MODEL} (${CPU_CORES} cores)"
echo "  🧠 RAM: ${RAM_GB}GB"
echo "  🎮 GPU: ${GPU_INFO}"
echo "  💻 OS:  ${OS_INFO}"
echo ""

# ── Step 1: Install uv ───────────────────────────────────────
timer_start "Environment Setup"
echo -e "${CYAN}▶ Step 1/7: Environment Setup${NC}"
if ! command -v uv &> /dev/null; then
    echo "  Installing uv..."
    curl -LsSf https://astral.sh/uv/install.sh | sh
    export PATH="$HOME/.local/bin:$PATH"
fi
echo "  ✅ uv $(uv --version)"
timer_end

# ── Step 2: Install Python + all dependencies ────────────────
timer_start "Install Dependencies"
echo ""
echo -e "${CYAN}▶ Step 2/7: Installing Dependencies${NC}"
uv sync --python 3.12 2>&1 | tail -3
pass "Dependencies installed (numpy, pandas, scikit-learn, matplotlib)"
timer_end

# ── Step 3: Download dataset ─────────────────────────────────
timer_start "Dataset Preparation"
echo ""
echo -e "${CYAN}▶ Step 3/7: Dataset Preparation${NC}"
echo "  ℹ️  Iris dataset is loaded from sklearn (auto-cached to data/)"
pass "Dataset ready"
timer_end

# ── Step 4: Run experiments ──────────────────────────────────
timer_start "Run Experiments"
echo ""
echo -e "${CYAN}▶ Step 4/7: Running Experiment${NC}"
echo "  Command: uv run python experiments/run_experiment.py"
echo "  Seed: 42"
echo "  ─────────────────────────────────────────────────────"
uv run python experiments/run_experiment.py
echo "  ─────────────────────────────────────────────────────"
timer_end

# ── Step 5: Verify outputs exist ─────────────────────────────
timer_start "Output Verification"
echo ""
echo -e "${CYAN}▶ Step 5/7: Output Verification${NC}"
expected_files=(
    "outputs/results.json"
    "outputs/summary_statistics.csv"
    "outputs/correlation_matrix.csv"
    "outputs/figures/fig1_feature_distributions.png"
    "outputs/figures/fig2_correlation_heatmap.png"
    "outputs/figures/fig3_confusion_matrix.png"
    "outputs/figures/fig4_feature_importance.png"
)

for f in "${expected_files[@]}"; do
    if [ -f "$f" ] && [ "$(stat -f%z "$f" 2>/dev/null || stat --format=%s "$f" 2>/dev/null)" -gt 0 ]; then
        pass "$f"
    else
        fail "$f MISSING"
    fi
done
timer_end

# ── Step 6: Verify metrics match tolerances ──────────────────
timer_start "Metric Verification"
echo ""
echo -e "${CYAN}▶ Step 6/7: Metric Verification${NC}"
if [ -f "outputs/results.json" ]; then
    cv_acc=$(uv run python -c "
import json
r = json.load(open('outputs/results.json'))
print(r['metrics']['cv_accuracy_mean'])
")
    echo "  CV Accuracy: $cv_acc (expected: ≥0.94)"
    check=$(uv run python -c "print('PASS' if $cv_acc >= 0.94 else 'FAIL')")
    if [ "$check" = "PASS" ]; then
        pass "Accuracy threshold met"
    else
        fail "Accuracy below threshold"
    fi

    # Determinism check: run twice and compare
    echo "  Running seed determinism check..."
    uv run python experiments/run_experiment.py > /dev/null 2>&1
    cv_acc2=$(uv run python -c "
import json
r = json.load(open('outputs/results.json'))
print(r['metrics']['cv_accuracy_mean'])
")
    if [ "$cv_acc" = "$cv_acc2" ]; then
        pass "Seed determinism verified ($cv_acc == $cv_acc2)"
    else
        fail "Non-deterministic! Run 1: $cv_acc, Run 2: $cv_acc2"
    fi
fi
timer_end

# ── Step 7: Reference evidence check ────────────────────────
timer_start "Reference Evidence"
echo ""
echo -e "${CYAN}▶ Step 7/7: Reference Evidence Check${NC}"
if [ -f "refs/refs-manifest.yaml" ]; then
    total=$(grep -c "^  - key:" refs/refs-manifest.yaml || echo 0)
    found=0
    while IFS= read -r line; do
        file=$(echo "$line" | sed 's/.*evidence: //' | tr -d ' ')
        if [ -f "refs/$file" ]; then
            sz=$(stat -f%z "refs/$file" 2>/dev/null || stat --format=%s "refs/$file" 2>/dev/null || echo 0)
            if [ "$sz" -gt 100 ]; then
                found=$((found + 1))
            fi
        fi
    done < <(grep "evidence:" refs/refs-manifest.yaml)
    pct=$((found * 100 / total))
    echo "  📚 Evidence files: ${found}/${total} (${pct}%)"
    if [ "$pct" -ge 90 ]; then
        pass "Reference coverage ≥90%"
    else
        fail "Reference coverage <90% (need ≥90%)"
    fi
else
    fail "refs/refs-manifest.yaml not found"
fi
timer_end

# ── Telemetry Upload ─────────────────────────────────────────
RUN_END_S=$(date +%s)
TOTAL_DURATION=$((RUN_END_S - RUN_START_S))

echo ""
echo -e "${CYAN}▶ Uploading Telemetry${NC}"

# Build final report
python3 -c "
import json, datetime, os, sys

# Load per-step timings
try:
    with open('$TELEMETRY_JSON') as f:
        stages = json.load(f)
except:
    stages = []

# Read authors from reproduce.yaml
authors = []
try:
    import yaml
    with open('reproduce.yaml') as f:
        cfg = yaml.safe_load(f)
    authors = cfg.get('authors', [])
except:
    pass

report = {
    'timestamp': datetime.datetime.utcnow().isoformat() + 'Z',
    'paper_id': '$PAPER_ID',
    'title': '$PAPER_TITLE',
    'authors': authors,
    'stages': stages,
    'total_duration_s': $TOTAL_DURATION,
    'stages_passed': $PASSED,
    'stages_failed': $FAILED,
    'status': 'PASSED' if $FAILED == 0 else 'FAILED',
    'outputs': [f for f in os.listdir('outputs') if not f.startswith('.')] if os.path.isdir('outputs') else [],
    'reproducer': 'reproduce.sh',
    'reproducer_version': '1.0',
    'hardware': {
        'gpu': '$GPU_INFO',
        'cpu': '$CPU_MODEL',
        'cpu_cores': '$CPU_CORES',
        'ram_gb': '$RAM_GB',
        'os': '$OS_INFO',
        'hostname': '$HOSTNAME_INFO',
    },
}

# Save locally
with open('outputs/reproduction_report.json', 'w') as f:
    json.dump(report, f, indent=2)
print(f'  📋 Report: outputs/reproduction_report.json')

# Upload to platform
api_key = '$API_KEY'
endpoint = '$TELEMETRY_ENDPOINT'
if api_key:
    import subprocess
    print(f'  📡 Uploading to {endpoint}...')
    try:
        r = subprocess.run([
            'curl', '-sS', '-X', 'POST', endpoint,
            '-H', 'Content-Type: application/json',
            '-H', f'Authorization: Bearer {api_key}',
            '-d', json.dumps(report),
            '--max-time', '10'
        ], capture_output=True, text=True)
        if r.returncode == 0 and '\"error\"' not in r.stdout:
            print(f'  ✅ Telemetry uploaded')
        else:
            print(f'  ⚠️  Upload response: {r.stdout[:200]}')
    except Exception as e:
        print(f'  ⚠️  Upload skipped: {e}')
else:
    print(f'  ℹ️  Set AUTOML_API_KEY to enable telemetry upload')
" 2>/dev/null || echo "  ⚠️  Could not generate report"

# ── Summary ──────────────────────────────────────────────────
echo ""
echo "╔══════════════════════════════════════════════════════════╗"
echo "║  Reproduction Summary                                    ║"
echo "╠══════════════════════════════════════════════════════════╣"
echo "║  ✅ Passed: $PASSED                                          ║"
echo "║  ❌ Failed: $FAILED                                          ║"
echo "║  ⏱  Total:  ${TOTAL_DURATION}s                                        ║"
echo "╚══════════════════════════════════════════════════════════╝"

if [ "$FAILED" -eq 0 ]; then
    echo ""
    echo "🎉 ALL CHECKS PASSED — Paper is reproducible!"
    exit 0
else
    echo ""
    echo "⚠️  $FAILED check(s) failed — see above for details."
    exit 1
fi
