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

**Built for AI coding agents and humans** who run many searches per session.

Based on reverse-engineering [Cursor's "Fast regex search"](https://cursor.com/blog/fast-regex-search) blog post (March 2026, Vicent Marti).

---

## Install

### From binary (recommended)

```bash
curl -fsSL https://raw.githubusercontent.com/hemeda2/aygrep/main/scripts/install.sh | bash
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

The benchmark report now uses the same format locally and in GitHub Actions:

- device spec
- build time and index size
- per-query cold + hot timings for `ayg` and `rg`
- total search time summary for four rows:
  Chromium cold, Chromium hot, Linux cold, Linux hot

The push/PR CI job keeps a Linux-kernel benchmark on GitHub Actions runners so CI stays fast.
On pushes to `main` or `master`, that same CI benchmark job also auto-publishes the latest public benchmark page from GitHub Actions.
For the full local report on Linux and Chromium, run:

```bash
./scripts/benchmark-full.sh linux
./scripts/benchmark-full.sh chromium
./scripts/benchmark-full.sh both
```

There is also a manual GitHub Actions workflow named `Benchmarks` for running `linux`, `chromium`, or `both` on demand with the same report format.
That workflow now generates:

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
  howpublished = {\url{https://github.com/hemeda2/aygrep}},
  note         = {Based on reverse engineering of Cursor's sparse n-gram approach}
}
```
