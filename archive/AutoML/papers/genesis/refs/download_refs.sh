#!/usr/bin/env bash
# ═══════════════════════════════════════════════════════════════
# Download Reference Evidence Files for GENESIS-001
# Run: bash refs/download_refs.sh
# ═══════════════════════════════════════════════════════════════
set -euo pipefail
cd "$(dirname "$0")"

echo "📚 Downloading reference evidence files for GENESIS-001..."
echo "   Target directory: $(pwd)"
echo ""

# Helper: download with curl, skip if exists
dl() {
    local key="$1" url="$2" ext="${3:-pdf}"
    local file="${key}.${ext}"
    if [[ -f "$file" ]]; then
        echo "  ✓ ${file} (already exists)"
        return
    fi
    echo "  ↓ ${file}"
    curl -sL -o "$file" "$url" && echo "    ✅ done" || echo "    ⚠️  failed: $url"
}

echo "── arXiv Papers (direct PDF) ──────────────────────────"
dl sakana2025scientist       "https://arxiv.org/pdf/2408.06292"
dl sakana2025scientistv2     "https://arxiv.org/pdf/2504.08066"
dl vaswani2017attention      "https://arxiv.org/pdf/1706.03762"
dl brown2020gpt3             "https://arxiv.org/pdf/2005.14165"
dl openai2023gpt4            "https://arxiv.org/pdf/2303.08774"
dl touvron2023llama          "https://arxiv.org/pdf/2302.13971"
dl team2023gemini            "https://arxiv.org/pdf/2312.11805"
dl yao2023react              "https://arxiv.org/pdf/2210.03629"
dl schick2024toolformer      "https://arxiv.org/pdf/2302.04761"
dl wang2024survey_agents     "https://arxiv.org/pdf/2308.11432"
dl hong2024metagpt           "https://arxiv.org/pdf/2308.00352"
dl qian2024chatdev           "https://arxiv.org/pdf/2307.07924"
dl liang2024monitoring       "https://arxiv.org/pdf/2403.07183"
dl rombach2022stablediffusion "https://arxiv.org/pdf/2112.10752"
dl dosovitskiy2021vit        "https://arxiv.org/pdf/2010.11929"
dl devlin2019bert            "https://arxiv.org/pdf/1810.04805"
dl achiam2023gpt4            "https://arxiv.org/pdf/2303.08774"
dl ouyang2022instructgpt     "https://arxiv.org/pdf/2203.02155"
dl liang2023helm             "https://arxiv.org/pdf/2211.09110"
dl yang2024leandojo          "https://arxiv.org/pdf/2306.15626"

echo ""
echo "── Open Access / JMLR / USENIX ───────────────────────"
dl pineau2021improving       "https://jmlr.org/papers/volume22/20-303/20-303.pdf"
dl firecracker2020           "https://www.usenix.org/system/files/nsdi20-paper-agache.pdf"

echo ""
echo "── Nature / Science / DOI (open access attempts) ─────"
# These may redirect to publisher landing pages; we try direct PDF links
dl baker2016reproducibility  "https://www.nature.com/articles/533452a.pdf"
dl boiko2023autonomous       "https://www.nature.com/articles/s41586-023-06792-0.pdf"
dl wang2023sciagent          "https://www.nature.com/articles/s41586-023-06221-2.pdf"
dl alphafold2021             "https://www.nature.com/articles/s41586-021-03819-2.pdf"
dl abramson2024alphafold3    "https://www.nature.com/articles/s41586-024-07487-w.pdf"
dl wilkinson2016fair         "https://www.nature.com/articles/sdata201618.pdf"
dl stodden2016enhancing      "https://www.science.org/doi/pdf/10.1126/science.aah6168"
dl nosek2015reproducibility  "https://www.science.org/doi/pdf/10.1126/science.aac4716"
dl checco2021ai_review       "https://www.nature.com/articles/s41599-020-00703-8.pdf"

echo ""
echo "── Websites (save as HTML) ───────────────────────────"
dl peerzero2026     "https://peerzero.ai"             html
dl moltbook2026     "https://moltbook.com"            html
dl gvisor2019       "https://gvisor.dev"              html
dl lean4            "https://leanprover.github.io"    html
dl trinh2024alphaproof "https://deepmind.google/discover/blog/ai-solves-imo-problems-at-silver-medal-level/" html
dl arxiv1991        "https://arxiv.org"               html
dl semanticscholar  "https://api.semanticscholar.org" html
dl openreview2014   "https://openreview.net"          html
dl uv2024           "https://github.com/astral-sh/uv" html
dl ionet2024        "https://io.net"                  html

echo ""
echo "── Other (books / proceedings) ───────────────────────"
dl pearl2009causality "https://doi.org/10.1017/CBO9780511803161"  html
dl moretti2013distant "https://www.versobooks.com/books/1421-distant-reading" html
dl boinc2004          "https://boinc.berkeley.edu/boinc2.pdf"
dl docker2014         "https://www.seltzer.com/margo/teaching/CS508.19/papers/merkel14.pdf"
dl dean2012mapreduce  "https://proceedings.neurips.cc/paper/2012/file/6aca97005c68f1206823815f66102863-Paper.pdf"
dl gundersen2018state "https://ojs.aaai.org/index.php/AAAI/article/view/11503/11362"

echo ""
echo "═══════════════════════════════════════════════════════"
# Count results
total=$(grep -c "^  - key:" refs-manifest.yaml || echo 0)
downloaded=$(find . -maxdepth 1 \( -name "*.pdf" -o -name "*.html" \) | wc -l | tr -d ' ')
echo "📊 Downloaded: ${downloaded} / ${total} evidence files"
echo "   Coverage: $(( downloaded * 100 / total ))%"
echo "═══════════════════════════════════════════════════════"
