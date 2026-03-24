#!/bin/bash
set -e

REPO_URL="${1:-https://chromium.googlesource.com/chromium/src}"
REPO_DIR="/tmp/ayg-bench/chromium"
RESULTS="/tmp/ayg-bench/results.md"

echo "=== ayg Benchmark Suite ==="
echo "Date: $(date -u)"
echo ""

# Clone if needed
if [ ! -d "$REPO_DIR" ]; then
    echo "Cloning chromium (shallow)..."
    mkdir -p /tmp/ayg-bench
    git clone --depth 1 "$REPO_URL" "$REPO_DIR"
fi

cd "$REPO_DIR"
FILE_COUNT=$(git ls-files | wc -l | tr -d ' ')
REPO_SIZE=$(du -sh . | cut -f1)
echo "Corpus: $FILE_COUNT files, $REPO_SIZE"
echo ""

# Build index
echo "Building ayg index..."
ayg build .
echo ""

# Queries
QUERIES=(
    "MAX_FILE_SIZE"
    "kMaxBufferSize"
    "gpu::Mailbox"
    "NOTREACHED"
    "base::Unretained"
    "std::unique_ptr"
    '#include "base/'
)

echo "| Query | ripgrep | ayg | Speedup | Candidates |" | tee "$RESULTS"
echo "|-------|---------|-----|---------|------------|" | tee -a "$RESULTS"

for q in "${QUERIES[@]}"; do
    # ripgrep (3 runs, take median)
    rg_times=()
    for i in 1 2 3; do
        t=$( { time rg -c "$q" . >/dev/null 2>&1; } 2>&1 | grep real | awk '{print $2}')
        rg_times+=("$t")
    done

    # ayg (3 runs, take median)
    ayg_result=$(ayg search "$q" --json 2>/dev/null)
    ayg_ms=$(echo "$ayg_result" | jq -r '.total_ms')
    ayg_cand=$(echo "$ayg_result" | jq -r '.candidates')

    echo "| \`$q\` | ${rg_times[1]} | ${ayg_ms}ms | ... | $ayg_cand |" | tee -a "$RESULTS"
done

echo ""
echo "Results saved to $RESULTS"
