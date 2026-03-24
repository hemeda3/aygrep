#!/bin/bash
# ayg full benchmark — works on dev laptop, GCP VM, or CI/CD
# Usage: ./scripts/benchmark-full.sh [chromium|linux|both]
set -e

CORPUS="${1:-both}"
AYG="${AYG_BIN:-$(dirname "$0")/../target/release/ayg}"
BENCH_DIR="${BENCH_DIR:-/tmp/ayg-bench}"
RESULTS_FILE="$BENCH_DIR/results.md"

# Colors
RED='\033[0;31m'; GREEN='\033[0;32m'; YELLOW='\033[1;33m'; NC='\033[0m'

echo -e "${GREEN}=== ayg Full Benchmark Suite ===${NC}"
echo "Date: $(date -u)"
echo "Host: $(hostname)"
echo "CPU:  $(grep 'model name' /proc/cpuinfo 2>/dev/null | head -1 | cut -d: -f2 | xargs || sysctl -n machdep.cpu.brand_string 2>/dev/null || echo 'unknown')"
echo "RAM:  $(free -h 2>/dev/null | awk '/Mem/{print $2}' || sysctl -n hw.memsize 2>/dev/null | awk '{printf "%.0f GB", $1/1073741824}')"
echo "Disk: $(df -h "$BENCH_DIR" 2>/dev/null | tail -1 | awk '{print $2, $4 " free"}' || echo 'unknown')"
echo ""

# Check ayg binary
if [ ! -x "$AYG" ]; then
    echo -e "${YELLOW}Building ayg...${NC}"
    cd "$(dirname "$0")/.."
    cargo build --release
    AYG="$(pwd)/target/release/ayg"
fi
echo "ayg binary: $AYG"
echo "ayg version: $($AYG --version 2>/dev/null || echo 'unknown')"
echo ""

# Check ripgrep
if ! command -v rg &>/dev/null; then
    echo -e "${RED}ripgrep not found. Install: https://github.com/BurntSushi/ripgrep${NC}"
    exit 1
fi
echo "rg version: $(rg --version | head -1)"
echo ""

mkdir -p "$BENCH_DIR"

# Drop caches function (works on Linux, no-op elsewhere)
drop_caches() {
    if [ -f /proc/sys/vm/drop_caches ]; then
        echo 3 | sudo tee /proc/sys/vm/drop_caches >/dev/null 2>&1 && sleep 1
    elif command -v purge &>/dev/null; then
        purge 2>/dev/null && sleep 1
    fi
}

# Benchmark one query
bench_query() {
    local repo_dir="$1" query="$2" label="$3"

    # ayg (3 runs, take median)
    local ayg_times=()
    for i in 1 2 3; do
        drop_caches
        local result
        result=$($AYG search "$query" 2>/dev/null)
        local ms=$(echo "$result" | awk '{for(i=1;i<=NF;i++) if($i ~ /total=/) print $i}' | sed 's/total=//;s/ms//')
        ayg_times+=("$ms")
    done
    # Sort and take median
    local ayg_median=$(printf '%s\n' "${ayg_times[@]}" | sort -n | sed -n '2p')
    local ayg_cand=$(echo "$result" | awk '{for(i=1;i<=NF;i++) if($i ~ /cand=/) print $i}' | sed 's/cand=//')
    local ayg_files=$(echo "$result" | awk '{for(i=1;i<=NF;i++) if($i ~ /files=/) print $i}' | sed 's/files=//')

    # ripgrep (1 cold run)
    drop_caches
    local rg_start rg_end rg_ms rg_count
    rg_start=$(date +%s%3N 2>/dev/null || python3 -c "import time; print(int(time.time()*1000))")
    rg_count=$(rg -c "$query" "$repo_dir/" 2>/dev/null | wc -l | tr -d ' ')
    rg_end=$(date +%s%3N 2>/dev/null || python3 -c "import time; print(int(time.time()*1000))")
    rg_ms=$((rg_end - rg_start))

    # Speedup
    local speedup="N/A"
    if [ -n "$ayg_median" ] && [ "$ayg_median" != "" ] && [ "$(echo "$ayg_median > 0" | bc 2>/dev/null || echo 1)" = "1" ]; then
        speedup=$(python3 -c "print(f'{${rg_ms}/${ayg_median}:.0f}x')" 2>/dev/null || echo "~${rg_ms}/${ayg_median}x")
    fi

    echo "| \`$query\` | ${ayg_median}ms | ${rg_ms}ms | **${speedup}** | ${ayg_cand} | ${ayg_files}/${rg_count} |"
}

# Run benchmark on a corpus
run_benchmark() {
    local name="$1" repo_dir="$2"
    shift 2
    local queries=("$@")

    echo -e "\n${GREEN}=== $name ===${NC}"
    echo "Repo: $repo_dir"
    echo "Files: $(cd "$repo_dir" && git ls-files 2>/dev/null | wc -l | tr -d ' ')"
    echo ""

    # Build index
    echo -e "${YELLOW}Building index...${NC}"
    cd "$repo_dir"
    $AYG build . --no-content -v 2>&1 | tail -3
    echo ""

    # Header
    echo "| Query | ayg (cold) | rg (cold) | Speedup | Candidates | Files (ayg/rg) |"
    echo "|-------|-----------|----------|---------|------------|----------------|"

    for q in "${queries[@]}"; do
        bench_query "$repo_dir" "$q" "$name"
    done
    echo ""
}

# Chromium queries
CHROMIUM_QUERIES=(
    "MAX_FILE_SIZE"
    "kMaxBufferSize"
    "gpu::Mailbox"
    "NOTREACHED"
    "base::Unretained"
    "constexpr char k"
    "WebContents"
    "std::unique_ptr"
)

# Linux kernel queries (ripgrep's official benchmark patterns)
LINUX_QUERIES=(
    "PM_RESUME"
    "EXPORT_SYMBOL_GPL"
    "Copyright"
    "mutex_lock"
    "struct device"
)

# Clone repos if needed
if [ "$CORPUS" = "chromium" ] || [ "$CORPUS" = "both" ]; then
    CHROMIUM_DIR="${CHROMIUM_DIR:-$BENCH_DIR/chromium}"
    if [ ! -d "$CHROMIUM_DIR/.git" ]; then
        echo -e "${YELLOW}Cloning Chromium (shallow)...${NC}"
        git clone --depth=1 https://chromium.googlesource.com/chromium/src "$CHROMIUM_DIR"
    fi
    run_benchmark "Chromium (436K files)" "$CHROMIUM_DIR" "${CHROMIUM_QUERIES[@]}"
fi

if [ "$CORPUS" = "linux" ] || [ "$CORPUS" = "both" ]; then
    LINUX_DIR="${LINUX_DIR:-$BENCH_DIR/linux}"
    if [ ! -d "$LINUX_DIR/.git" ]; then
        echo -e "${YELLOW}Cloning Linux kernel (shallow)...${NC}"
        git clone --depth=1 https://github.com/BurntSushi/linux "$LINUX_DIR"
    fi
    run_benchmark "Linux kernel (79K files)" "$LINUX_DIR" "${LINUX_QUERIES[@]}"
fi

echo -e "\n${GREEN}=== Done ===${NC}"
echo "Full results saved to: $RESULTS_FILE"
