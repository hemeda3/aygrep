#!/bin/bash
# Full benchmark runner for Linux kernel and/or Chromium.
# Usage: ./scripts/benchmark-full.sh [linux|chromium|both]
set -euo pipefail

CORPUS="${1:-both}"
ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
AYG="${AYG_BIN:-$ROOT_DIR/target/release/ayg}"
BENCH_DIR="${BENCH_DIR:-/tmp/ayg-bench}"
RESULTS_FILE="$BENCH_DIR/results.md"
PUBLIC_DIR="$BENCH_DIR/public"

mkdir -p "$BENCH_DIR"
exec > >(tee "$RESULTS_FILE") 2>&1

SUMMARY_ROWS=()

timestamp_ms() {
    python3 - <<'PY'
import time
print(int(time.time() * 1000))
PY
}

float_add() {
    python3 - "$1" "$2" <<'PY'
import sys

a = float(sys.argv[1] or 0)
b = float(sys.argv[2] or 0)
print(f"{a + b:.1f}")
PY
}

speedup() {
    python3 - "$1" "$2" <<'PY'
import sys

slower = float(sys.argv[1] or 0)
faster = float(sys.argv[2] or 0)
if faster <= 0:
    print("—")
else:
    ratio = slower / faster
    print(f"{ratio:.0f}x" if ratio >= 10 else f"{ratio:.1f}x")
PY
}

extract_field() {
    local line="$1"
    local key="$2"
    awk -v key="$key" '
        {
            for (i = 1; i <= NF; i++) {
                if ($i == key "=") {
                    val = $(i + 1)
                    sub("ms$", "", val)
                    print val
                    exit
                }
                if (index($i, key "=") == 1) {
                    val = $i
                    sub("^" key "=", "", val)
                    sub("ms$", "", val)
                    print val
                    exit
                }
            }
        }
    ' <<< "$line"
}

drop_caches() {
    if [ -f /proc/sys/vm/drop_caches ]; then
        sync >/dev/null 2>&1 || true
        sudo -n sh -c 'echo 3 >/proc/sys/vm/drop_caches' >/dev/null 2>&1 || true
        sleep 1
    elif command -v purge >/dev/null 2>&1; then
        purge >/dev/null 2>&1 || true
        sleep 1
    fi
}

ensure_ayg() {
    if [ -x "$AYG" ]; then
        return
    fi

    (cd "$ROOT_DIR" && cargo build --release)
    AYG="$ROOT_DIR/target/release/ayg"
}

ensure_rg() {
    if command -v rg >/dev/null 2>&1; then
        return
    fi

    rm -rf /tmp/ripgrep
    git clone --depth=1 https://github.com/BurntSushi/ripgrep /tmp/ripgrep
    (cd /tmp/ripgrep && cargo install --path .)
}

host_os() {
    uname -srm 2>/dev/null || echo "unknown"
}

host_cpu() {
    grep 'model name' /proc/cpuinfo 2>/dev/null | head -1 | cut -d: -f2 | xargs \
        || sysctl -n machdep.cpu.brand_string 2>/dev/null \
        || echo "unknown"
}

host_cores() {
    nproc 2>/dev/null || sysctl -n hw.ncpu 2>/dev/null || echo "unknown"
}

host_ram() {
    if command -v free >/dev/null 2>&1; then
        free -h | awk '/Mem/{print $2}'
        return
    fi

    python3 - <<'PY'
import subprocess

for cmd in (["sysctl", "-n", "hw.memsize"],):
    try:
        raw = subprocess.check_output(cmd, text=True).strip()
        print(f"{int(raw) / (1024 ** 3):.0f}GB")
        break
    except Exception:
        pass
else:
    print("unknown")
PY
}

host_disk() {
    df -h "$BENCH_DIR" | awk 'NR==2 {print $2 " total / " $4 " free"}' 2>/dev/null || echo "unknown"
}

measure_ayg_pair() {
    local repo_dir="$1"
    local query="$2"

    cd "$repo_dir"

    drop_caches
    local cold_out warm_out
    cold_out=$("$AYG" search "$query" 2>/dev/null)
    warm_out=$("$AYG" search "$query" 2>/dev/null)

    local cold_total hot_total cold_scan hot_scan cand files
    cold_total=$(extract_field "$cold_out" "total")
    hot_total=$(extract_field "$warm_out" "total")
    cold_scan=$(extract_field "$cold_out" "scan")
    hot_scan=$(extract_field "$warm_out" "scan")
    cand=$(extract_field "$cold_out" "cand")
    files=$(extract_field "$cold_out" "files")

    printf '%s|%s|%s|%s|%s|%s\n' "$cold_total" "$hot_total" "$cold_scan" "$hot_scan" "$cand" "$files"
}

measure_rg_once() {
    local repo_dir="$1"
    local query="$2"

    local start_ms end_ms count
    start_ms=$(timestamp_ms)
    count=$(rg -c -- "$query" "$repo_dir" 2>/dev/null | wc -l | tr -d ' ')
    end_ms=$(timestamp_ms)

    printf '%s|%s\n' "$((end_ms - start_ms))" "$count"
}

measure_rg_pair() {
    local repo_dir="$1"
    local query="$2"

    drop_caches
    local cold_pair hot_pair
    cold_pair=$(measure_rg_once "$repo_dir" "$query")
    hot_pair=$(measure_rg_once "$repo_dir" "$query")

    local cold_ms cold_files hot_ms hot_files
    IFS='|' read -r cold_ms cold_files <<< "$cold_pair"
    IFS='|' read -r hot_ms hot_files <<< "$hot_pair"

    printf '%s|%s|%s|%s\n' "$cold_ms" "$hot_ms" "$cold_files" "$hot_files"
}

print_environment() {
    echo "# ayg Benchmark Report"
    echo ""
    echo "## Device Spec"
    echo ""
    echo "| Field | Value |"
    echo "|-------|-------|"
    echo "| Environment | $([ "${GITHUB_ACTIONS:-}" = "true" ] && echo "GitHub Actions" || echo "Local") |"
    echo "| Generated | $(date -u +"%Y-%m-%dT%H:%M:%SZ") |"
    echo "| Host | $(hostname) |"
    echo "| OS | $(host_os) |"
    echo "| CPU | $(host_cpu) |"
    echo "| Cores | $(host_cores) |"
    echo "| RAM | $(host_ram) |"
    echo "| Disk | $(host_disk) |"
    echo "| ayg | $("$AYG" --version 2>/dev/null || echo unknown) |"
    echo "| rg | $(rg --version | head -1) |"
    echo ""
}

print_query_header() {
    echo "| Query | ayg cold | ayg hot | ayg scan cold | ayg scan hot | rg cold | rg hot | Cold speedup | Hot speedup | Candidates | Files |"
    echo "|-------|----------|---------|---------------|--------------|---------|--------|--------------|-------------|------------|-------|"
}

bench_query() {
    local repo_dir="$1"
    local query="$2"

    local ayg_pair rg_pair
    local ayg_cold ayg_hot ayg_scan_cold ayg_scan_hot ayg_cand ayg_files
    local rg_cold rg_hot rg_files_cold rg_files_hot

    ayg_pair=$(measure_ayg_pair "$repo_dir" "$query")
    IFS='|' read -r ayg_cold ayg_hot ayg_scan_cold ayg_scan_hot ayg_cand ayg_files <<< "$ayg_pair"

    rg_pair=$(measure_rg_pair "$repo_dir" "$query")
    IFS='|' read -r rg_cold rg_hot rg_files_cold rg_files_hot <<< "$rg_pair"

    local cold_speed hot_speed
    cold_speed=$(speedup "$rg_cold" "$ayg_cold")
    hot_speed=$(speedup "$rg_hot" "$ayg_hot")

    echo "| \`$query\` | ${ayg_cold}ms | ${ayg_hot}ms | ${ayg_scan_cold}ms | ${ayg_scan_hot}ms | ${rg_cold}ms | ${rg_hot}ms | $cold_speed | $hot_speed | $ayg_cand | ${ayg_files}/${rg_files_cold} |"

    QUERY_AYG_COLD_TOTAL=$(float_add "$QUERY_AYG_COLD_TOTAL" "$ayg_cold")
    QUERY_AYG_HOT_TOTAL=$(float_add "$QUERY_AYG_HOT_TOTAL" "$ayg_hot")
    QUERY_AYG_SCAN_COLD_TOTAL=$(float_add "$QUERY_AYG_SCAN_COLD_TOTAL" "$ayg_scan_cold")
    QUERY_AYG_SCAN_HOT_TOTAL=$(float_add "$QUERY_AYG_SCAN_HOT_TOTAL" "$ayg_scan_hot")
    QUERY_RG_COLD_TOTAL=$(float_add "$QUERY_RG_COLD_TOTAL" "$rg_cold")
    QUERY_RG_HOT_TOTAL=$(float_add "$QUERY_RG_HOT_TOTAL" "$rg_hot")
}

record_summary_rows() {
    local corpus="$1"
    local build_s="$2"

    local cold_speed hot_speed
    cold_speed=$(speedup "$QUERY_RG_COLD_TOTAL" "$QUERY_AYG_COLD_TOTAL")
    hot_speed=$(speedup "$QUERY_RG_HOT_TOTAL" "$QUERY_AYG_HOT_TOTAL")

    SUMMARY_ROWS+=("| $corpus | Cold | ${build_s}s | ${QUERY_AYG_COLD_TOTAL}ms | ${QUERY_AYG_SCAN_COLD_TOTAL}ms | ${QUERY_RG_COLD_TOTAL}ms | $cold_speed |")
    SUMMARY_ROWS+=("| $corpus | Hot | ${build_s}s | ${QUERY_AYG_HOT_TOTAL}ms | ${QUERY_AYG_SCAN_HOT_TOTAL}ms | ${QUERY_RG_HOT_TOTAL}ms | $hot_speed |")
}

print_corpus_summary() {
    local build_s="$1"

    local cold_speed hot_speed
    cold_speed=$(speedup "$QUERY_RG_COLD_TOTAL" "$QUERY_AYG_COLD_TOTAL")
    hot_speed=$(speedup "$QUERY_RG_HOT_TOTAL" "$QUERY_AYG_HOT_TOTAL")

    echo ""
    echo "### Totals"
    echo ""
    echo "| State | Build time | ayg total | ayg scan total | rg total | Speedup |"
    echo "|-------|------------|-----------|----------------|----------|---------|"
    echo "| Cold | ${build_s}s | ${QUERY_AYG_COLD_TOTAL}ms | ${QUERY_AYG_SCAN_COLD_TOTAL}ms | ${QUERY_RG_COLD_TOTAL}ms | $cold_speed |"
    echo "| Hot | ${build_s}s | ${QUERY_AYG_HOT_TOTAL}ms | ${QUERY_AYG_SCAN_HOT_TOTAL}ms | ${QUERY_RG_HOT_TOTAL}ms | $hot_speed |"
    echo ""
}

run_benchmark() {
    local name="$1"
    local repo_dir="$2"
    shift 2
    local queries=("$@")

    echo "## $name"
    echo ""
    echo "| Field | Value |"
    echo "|-------|-------|"
    echo "| Repo dir | \`$repo_dir\` |"
    echo "| Files tracked | $(cd "$repo_dir" && git ls-files | wc -l | tr -d ' ') |"
    echo ""

    local build_log
    build_log=$(cd "$repo_dir" && "$AYG" build . -v 2>&1)

    local files_indexed unique_keys postings_mb lookup_mb files_mb content_mb total_index_mb build_s
    files_indexed=$(awk -F= '/^files_indexed=/{print $2; exit}' <<< "$build_log")
    unique_keys=$(awk -F= '/^unique_keys=/{print $2; exit}' <<< "$build_log")
    postings_mb=$(awk -F= '/^postings_mb=/{print $2; exit}' <<< "$build_log")
    lookup_mb=$(awk -F= '/^lookup_mb=/{print $2; exit}' <<< "$build_log")
    files_mb=$(awk -F= '/^files_mb=/{print $2; exit}' <<< "$build_log")
    content_mb=$(awk -F= '/^content_mb=/{print $2; exit}' <<< "$build_log")
    total_index_mb=$(awk -F= '/^total_index_mb=/{print $2; exit}' <<< "$build_log")
    build_s=$(awk -F= '/^total_s=/{print $2; exit}' <<< "$build_log")

    echo "### Build"
    echo ""
    echo "| Metric | Value |"
    echo "|--------|-------|"
    echo "| files_indexed | $files_indexed |"
    echo "| unique_keys | $unique_keys |"
    echo "| postings_mb | $postings_mb |"
    echo "| lookup_mb | $lookup_mb |"
    echo "| files_mb | $files_mb |"
    echo "| content_mb | $content_mb |"
    echo "| total_index_mb | $total_index_mb |"
    echo "| build_time_s | $build_s |"
    echo ""

    QUERY_AYG_COLD_TOTAL="0.0"
    QUERY_AYG_HOT_TOTAL="0.0"
    QUERY_AYG_SCAN_COLD_TOTAL="0.0"
    QUERY_AYG_SCAN_HOT_TOTAL="0.0"
    QUERY_RG_COLD_TOTAL="0.0"
    QUERY_RG_HOT_TOTAL="0.0"

    echo "### Queries"
    echo ""
    print_query_header
    for q in "${queries[@]}"; do
        bench_query "$repo_dir" "$q"
    done

    print_corpus_summary "$build_s"
    record_summary_rows "$name" "$build_s"
}

clone_if_missing() {
    local url="$1"
    local dir="$2"

    if [ -d "$dir/.git" ]; then
        return
    fi

    git clone --depth=1 "$url" "$dir"
}

print_global_summary() {
    echo "## Overall Summary"
    echo ""
    echo "| Corpus | State | Build time | ayg total | ayg scan total | rg total | Speedup |"
    echo "|--------|-------|------------|-----------|----------------|----------|---------|"
    for row in "${SUMMARY_ROWS[@]}"; do
        echo "$row"
    done
    echo ""
}

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

LINUX_QUERIES=(
    "PM_RESUME"
    "EXPORT_SYMBOL_GPL"
    "Copyright"
    "mutex_lock"
    "struct device"
)

ensure_ayg
ensure_rg
print_environment

if [ "$CORPUS" = "chromium" ] || [ "$CORPUS" = "both" ]; then
    CHROMIUM_DIR="${CHROMIUM_DIR:-$BENCH_DIR/chromium}"
    clone_if_missing "https://chromium.googlesource.com/chromium/src" "$CHROMIUM_DIR"
    run_benchmark "Chromium" "$CHROMIUM_DIR" "${CHROMIUM_QUERIES[@]}"
fi

if [ "$CORPUS" = "linux" ] || [ "$CORPUS" = "both" ]; then
    LINUX_DIR="${LINUX_DIR:-$BENCH_DIR/linux}"
    clone_if_missing "https://github.com/BurntSushi/linux" "$LINUX_DIR"
    run_benchmark "Linux kernel" "$LINUX_DIR" "${LINUX_QUERIES[@]}"
fi

if [ "${#SUMMARY_ROWS[@]}" -gt 0 ]; then
    print_global_summary
fi

python3 "$ROOT_DIR/scripts/render-benchmark-site.py" "$RESULTS_FILE" "$PUBLIC_DIR"
