<div align="center">
  <h1>ayg</h1>
  <p><strong>Indexed code search for AI agents and humans</strong></p>
  <p>
    <a href="https://github.com/hemeda3/aygrep/actions"><img src="https://github.com/hemeda3/aygrep/actions/workflows/ci.yml/badge.svg" alt="CI"></a>
    <a href="https://github.com/hemeda3/aygrep/releases"><img src="https://img.shields.io/github/v/release/hemeda3/aygrep" alt="Release"></a>
    <a href="https://crates.io/crates/aygrep"><img src="https://img.shields.io/crates/v/aygrep" alt="Crates.io"></a>
    <a href="LICENSE"><img src="https://img.shields.io/badge/license-MIT-blue.svg" alt="License"></a>
  </p>
</div>

---


ayg builds a sparse n-gram inverted index for code search in large repositories. Build once, then search candidate files instead of rescanning the whole tree.

On this local M3 Max, the Homebrew-installed CLI returned `MAX_FILE_SIZE` from Chromium in `59-64ms` steady-state and `0.25s` after cold prep. The matching `rg -n 'MAX_FILE_SIZE' . >/dev/null` run took `29.24s` warm and `48.20s` after cold prep.

**Built for AI coding agents and humans** who run many searches per session.

Based on reverse-engineering [Cursor's "Fast regex search"](https://cursor.com/blog/fast-regex-search) blog post (March 2026, Vicent Marti).

---

## Install

### From binary (recommended)

```bash
curl -fsSL https://raw.githubusercontent.com/hemeda3/aygrep/main/scripts/install.sh | bash
```

### From source

```bash
cargo install aygrep
```

### Homebrew (macOS/Linux)

```bash
brew install hemeda3/tap/ayg
```

## Quick start

`ayg build` creates `ayg_index/` inside the target repo. Run `ayg search` and `ayg benchmark` from that same repo directory.

```bash
# Build the index once
ayg build ~/code/chromium

# Search from inside the indexed repo
cd ~/code/chromium
ayg search "MAX_FILE_SIZE"

# Output modes
ayg search "std::unique_ptr" -c   # show total match count
ayg search "WebContents" -l       # list matching file paths
ayg search "TODO" --json          # JSON output for scripts

# Run the built-in benchmark suite
ayg benchmark
```

On machines with less than 8GB of available RAM, `ayg build` automatically skips `content.bin` and uses filesystem scan mode.

---

## Benchmarks

### Customer-facing macOS timings

These numbers are for the installed Homebrew binary at `/opt/homebrew/bin/ayg`, not `target/release/ayg`.

Chromium on this machine:

- checkout size: `9.6G`
- git-tracked files: `489,298`
- files indexed by `ayg`: `436,610`
- query below: `MAX_FILE_SIZE`

The ripgrep comparison command is explicit about the repo path so it searches the tree instead of stdin:

```bash
/opt/homebrew/bin/rg -n "MAX_FILE_SIZE" . >/dev/null
```

| Scenario | ayg CLI wall | ayg internal `total=` | rg wall | Speedup |
|----------|-------------:|----------------------:|--------:|--------:|
| Warm steady-state | **59-64ms** | **0.5-1.5ms** | **29.24s** | **~460x** |
| After cold prep (`dd ... && purge`) | **0.25s** | **45.4ms** | **48.20s** | **~193x** |

Representative warm output from the Homebrew binary:

```text
MAX_FILE_SIZE             tri= 2  cand=    24  match=    33  files=   15  idx=    48us  scan=     0.4ms  total=     0.5ms
```

The important distinction is that the `total=` field is the search core after the process is already hot. The shell command a customer runs also pays process startup and index-open cost.

### Benchmark report samples

Latest public benchmark report:

- [https://hemeda3.github.io/aygrep/](https://hemeda3.github.io/aygrep/)

#### GitHub Actions sample

March 24, 2026. `ubuntu-latest` runner, Linux kernel corpus, 79,225 indexed files, 2 vCPU / 7.8 GiB RAM, no `content.bin`.

| Query | ayg cold | ayg hot | rg cold | rg hot | Cold speedup | Hot speedup | Files |
|-------|---------:|--------:|--------:|-------:|-------------:|------------:|------:|
| `PM_RESUME` | **144.6ms** | **6.2ms** | 12,794ms | 751ms | **88x** | **121x** | 13 |
| `EXPORT_SYMBOL_GPL` | **1,304.3ms** | **61.6ms** | 12,787ms | 801ms | **9.8x** | **13x** | 3,130 |
| `Copyright` | **8,779.8ms** | **462.3ms** | 12,704ms | 1,563ms | **1.4x** | **3.4x** | 49,481 |
| `mutex_lock` | **1,506.4ms** | **72.2ms** | 12,712ms | 841ms | **8.4x** | **12x** | 5,472 |
| `struct device` | **4,545.3ms** | **224.6ms** | 12,707ms | 922ms | **2.8x** | **4.1x** | 11,408 |

| State | Build time | ayg total | ayg scan total | rg total | Speedup |
|-------|-----------:|----------:|---------------:|---------:|--------:|
| Cold | **29.79s** | **16,280.4ms** | **16,199.8ms** | 63,704.0ms | **3.9x** |
| Hot | **29.79s** | **826.9ms** | **822.8ms** | 4,878.0ms | **5.9x** |

#### Local macOS sample

March 24, 2026. Apple M3 Max MacBook Pro, Linux kernel corpus, 79,225 indexed files, 36 GB RAM, with `content.bin`.

| Query | ayg cold | ayg hot | rg cold | rg hot | Cold speedup | Hot speedup | Files |
|-------|---------:|--------:|--------:|-------:|-------------:|------------:|------:|
| `PM_RESUME` | **7.5ms** | **7.0ms** | 3,047ms | 2,761ms | **406x** | **394x** | 13 |
| `EXPORT_SYMBOL_GPL` | **29.7ms** | **30.1ms** | 2,811ms | 2,578ms | **95x** | **86x** | 3,129 |
| `Copyright` | **141.8ms** | **96.7ms** | 2,519ms | 2,550ms | **18x** | **26x** | 49,482 |
| `mutex_lock` | **46.7ms** | **45.2ms** | 2,582ms | 2,577ms | **55x** | **57x** | 5,471 |
| `struct device` | **105.8ms** | **75.6ms** | 2,596ms | 2,693ms | **25x** | **36x** | 11,408 |

| State | Build time | ayg total | ayg scan total | rg total | Speedup |
|-------|-----------:|----------:|---------------:|---------:|--------:|
| Cold | **22.61s** | **331.5ms** | **328.2ms** | 13,555.0ms | **41x** |
| Hot | **22.61s** | **254.6ms** | **252.2ms** | 13,159.0ms | **52x** |

On the default macOS case-insensitive filesystem, the Linux kernel checkout has a few case-colliding paths. Treat the GitHub Actions Linux run as the canonical Linux sample when exact file counts matter.

For the full local report on Linux and Chromium, run:

```bash
./scripts/benchmark-full.sh linux
./scripts/benchmark-full.sh chromium
./scripts/benchmark-full.sh both
```

There is also a manual GitHub Actions workflow named `Benchmarks` for running `linux`, `chromium`, or `both` on demand with the same report format.
That workflow generates:

- a formatted public GitHub Pages report
- a downloadable ZIP package with the full report
- the raw Markdown report as an artifact

For the public page to deploy, set the repository Pages source to `GitHub Actions` once in GitHub settings.

The default CI `benchmark` job runs `./scripts/benchmark-full.sh linux` and publishes the latest Linux report automatically on pushes.
Chromium uses the same cold + hot format through `./scripts/benchmark-full.sh chromium` or the manual `Benchmarks` workflow.

---

## How it works

ayg extracts **sparse n-grams** from every file — variable-length byte sequences bounded by rare character pairs. A 256×256 frequency table (built from your corpus) identifies which byte pairs are rare. Boundaries are placed at frequency peaks, producing longer, more selective keys than fixed trigrams.

At query time:

1. **Decompose** the pattern into sparse n-grams (or raw trigrams as fallback)
2. **Probe** the mmap'd lookup table via binary search (~5μs)
3. **Fetch** posting lists via pread, intersect with early termination
4. **Scan** candidate files with SIMD memmem (`memchr` crate)

```
"MAX_FILE_SIZE" → sparse n-grams → index lookup
→ candidate file IDs → scan matching files only
```

### Adaptive scan modes

ayg detects available RAM at startup and picks the fastest viable strategy:

| Mode | When | Strategy |
|------|------|----------|
| **ContentMmap** | content.bin exists and enough RAM is available | Memory-map indexed content for the fastest local scans |
| **ContentPread** | content.bin exists but RAM is tight | Read indexed content on demand |
| **Filesystem** | No content.bin or low RAM | Open candidate files directly from the repo |

### Index structure

| File | Size (Chromium) | Resident? | Purpose |
|------|----------------|-----------|---------|
| `lookup.bin` | 131 MB | Yes (mmap) | Sorted hash → offset+count |
| `postings.bin` | 668 MB | No (pread) | Delta+varint encoded file IDs |
| `files.bin` | 33 MB | Yes (loaded) | File path resolution |
| `freq.bin` | 64 KB | Yes (loaded) | Byte-pair frequency weights |
| `content.bin` | 2,293 MB | Depends | Optional stored file contents |

Memory footprint: ~164MB resident without content file.

### Architecture

- **533 KB** static binary
- **3 deps:** `memchr` (SIMD search), `memmap2` (mmap), `libc` (madvise/pread)
- No rayon, no walkdir, no std HashMap — FxHash + inline 2MB bitvec
- File discovery via `git ls-files -z` (respects `.gitignore`)
- Two-pass build: Pass 1 streams content + counts byte pairs, Pass 2 extracts n-grams

---

## What Cursor described vs what we built

| Aspect | Cursor's blog | What we reverse-engineered |
|--------|--------------|-----------------------------|
| N-gram selection | "Sparse n-grams with frequency boundaries" | 256×256 byte-pair table, interior-only boundaries for covering queries |
| Index format | "Two files: lookup (mmap) + postings (pread)" | 16-byte sorted entries, delta+varint posting lists |
| Memory model | "We mmap this table, and only this table" | Adaptive: content mmap / pread / filesystem based on available RAM |
| Query strategy | "build_covering at query time" | Two-tier: ≥3 covering n-grams → sparse path, else → raw trigram fallback |
| 8GB machines | Not discussed | Streaming build (no OOM), automatic no-content fallback, filesystem scan mode |

---

## Acknowledgments

Developed with AI assistance (Claude) for code generation, optimization iteration, and benchmarking. The author directed all architectural decisions and validated results on real hardware.

### References

- [Cursor: Fast regex search](https://cursor.com/blog/fast-regex-search) — the blog post that started this project
- [Zobel, Moffat, Sacks-Davis (1993)](https://www.vldb.org/conf/1993/P290.PDF) — inverted file indexing
- [Russ Cox (2012)](https://swtch.com/~rsc/regexp/regexp4.html) — trigram indexing for regex search
- [BurntSushi / ripgrep](https://github.com/BurntSushi/ripgrep) — ripgrep, memchr, regex crate
- [zoekt (Google/Sourcegraph)](https://github.com/sourcegraph/zoekt) — content-in-index architecture
- [ClickHouse](https://clickhouse.com/) — sparse n-gram text indexing

## License

MIT — Copyright (c) 2026 Ahmed Yousri

## Author

**Ahmed Yousri** — [github.com/hemeda3](https://github.com/hemeda3)

## Citation

```bibtex
@software{yousri2026ayg,
  author       = {Yousri, Ahmed},
  title        = {ayg: Indexed code search using sparse n-gram inverted indexes},
  year         = {2026},
  publisher    = {GitHub},
  howpublished = {\url{https://github.com/hemeda3/aygrep}},
  note         = {Based on reverse engineering of Cursor's sparse n-gram approach}
}
```
