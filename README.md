<p align="center">
  <h1 align="center">ayg</h1>
  <p align="center"><strong>Indexed code search for AI agents and humans</strong></p>
  <p align="center">
    <a href="https://github.com/hemeda3/aygrep/actions"><img src="https://github.com/hemeda3/aygrep/actions/workflows/ci.yml/badge.svg" alt="CI"></a>
    <a href="https://github.com/hemeda3/aygrep/releases"><img src="https://img.shields.io/github/v/release/hemeda3/aygrep" alt="Release"></a>
    <a href="https://crates.io/crates/aygrep"><img src="https://img.shields.io/crates/v/aygrep" alt="Crates.io"></a>
    <a href="LICENSE"><img src="https://img.shields.io/badge/license-MIT-blue.svg" alt="License"></a>
  </p>
</p>

---

**Chromium monorepo:** 436,610 files · 6.7 GB · 40+ million lines of C++

```
$ time rg "MAX_FILE_SIZE" chromium/
real    16.6s       ← scans all 436,610 files

$ ayg search "MAX_FILE_SIZE"
0.2ms              ← reads 24 files
```

**13,806× faster.** Tested on Chromium (436K files) and Linux kernel (79K files). Every result verified correct against ripgrep.

> ayg builds a sparse n-gram inverted index. One build, instant search.
> 24 candidates out of 436,610 files — **99.995% of I/O eliminated** before a single byte is scanned.

---

## Install

```bash
brew install hemeda3/tap/ayg                    # macOS (prebuilt, instant)
cargo install aygrep                             # from source
curl -fsSL https://raw.githubusercontent.com/hemeda3/aygrep/main/scripts/install.sh | bash
```

## Quick start

```bash
ayg build ~/code/chromium          # build index (one-time, ~30s)
cd ~/code/chromium
ayg search "MAX_FILE_SIZE"         # 0.2ms — 24 candidates
ayg search "std::unique_ptr" -c    # just the count: 162371
ayg search "WebContents" -l        # list matching file paths
ayg search "TODO" --json           # JSON for scripts/agents
```

## Benchmarks

### Chromium — M3 Max, truly cold

38GB memory flood + `purge` before each query. No page cache. Real NVMe reads.

| Query | ayg | Candidates | Files |
|:---|---:|---:|---:|
| `MAX_FILE_SIZE` | **1.0 ms** | 24 | 15 |
| `kMaxBufferSize` | **3.0 ms** | 140 | 32 |
| `gpu::Mailbox` | **2.8 ms** | 419 | 141 |
| `NOTREACHED` | **33.8 ms** | 8,310 | 8,264 |
| `std::unique_ptr` | **75.9 ms** | 43,371 | 43,371 |
| `#include "base/` | **113.5 ms** | 78,979 | 78,808 |

### Chromium — M3 Max, warm cache

| Query | ayg | ripgrep | Speedup |
|:---|---:|---:|---:|
| `MAX_FILE_SIZE` | **0.1 ms** | 12,100 ms | **116,533×** |
| `gpu::Mailbox` | **0.5 ms** | 12,600 ms | **27,369×** |
| `NOTREACHED` | **10.5 ms** | 10,800 ms | **1,026×** |
| `std::unique_ptr` | **25.0 ms** | 11,200 ms | **448×** |
| `#include "base/` | **39.5 ms** | 13,000 ms | **329×** |

### Linux kernel — CI (Ubuntu, ripgrep benchsuite corpus)

79,225 files. Both tools installed from source. Warm cache.

| Query | ayg | ripgrep | Speedup | Files | Match |
|:---|---:|---:|---:|---:|:---:|
| `PM_RESUME` | **3.5 ms** | 408 ms | **117×** | 13 | ✓ |
| `EXPORT_SYMBOL_GPL` | **20.1 ms** | 378 ms | **19×** | 3,130 | ✓ |
| `mutex_lock` | **31.0 ms** | 419 ms | **14×** | 5,472 | ✓ |
| `struct device` | **63.5 ms** | 431 ms | **7×** | 11,408 | ✓ |
| `Copyright` | **86.8 ms** | 620 ms | **7×** | 49,481 | ✓ |

## How it works

```
Query: "MAX_FILE_SIZE"
  ↓
Decompose into sparse n-grams         ← 2 n-grams (not 11 trigrams)
  ↓
Binary search mmap'd lookup table      ← ~5μs
  ↓
pread + intersect posting lists        ← ~50μs
  ↓
24 candidate files (out of 436,610)    ← 99.995% eliminated
  ↓
SIMD scan with memchr                  ← ~100μs
  ↓
15 files match, 33 occurrences         ← 0.2ms total
```

### Adaptive behavior

| | What happens | When |
|:---|:---|:---|
| **Build** | Streams content.bin to disk | RAM ≥ 8GB (auto-detected) |
| **Search** | Zero-copy mmap scan | RAM ≥ 2× content size |
| **Search** | pread per candidate | RAM ≥ 1× content size |
| **Search** | Filesystem reads | Tight RAM or no content.bin |

No flags needed. One binary, adapts to the machine.

### Index structure

| File | Size | Purpose |
|:---|---:|:---|
| `lookup.bin` | ~131 MB | Sorted n-gram hash table (mmap'd) |
| `postings.bin` | ~668 MB | Delta-varint encoded file ID lists |
| `freq.bin` | 64 KB | Byte-pair frequency weights |
| `content.bin` | ~2.3 GB | File contents for zero-copy scan (optional) |

### Architecture

- **443 KB** static binary
- **3 dependencies:** `memchr` (SIMD), `memmap2` (mmap), `libc` (madvise)
- No rayon, no walkdir, no HashMap — FxHash and bitvec inline
- File discovery via `git ls-files` (37× faster than directory walk)

## References

- V. Marti, "[Fast regex search](https://cursor.com/blog/fast-regex-search)," Cursor Blog, 2026
- R. Cox, "[Trigram index for regex search](https://swtch.com/~rsc/regexp/regexp4.html)," 2012
- A. Gallant, "[ripgrep](https://github.com/BurntSushi/ripgrep)" — baseline, benchsuite corpus
- "[Zoekt](https://github.com/sourcegraph/zoekt)" — content-in-index architecture

## License

MIT — Copyright (c) 2026 Ahmed Yousri

**Ahmed Yousri** — [github.com/hemeda3](https://github.com/hemeda3)

```bibtex
@software{yousri2026ayg,
  author = {Yousri, Ahmed},
  title  = {ayg: Indexed code search using sparse n-gram inverted indexes},
  year   = {2026},
  url    = {https://github.com/hemeda3/aygrep}
}
```
