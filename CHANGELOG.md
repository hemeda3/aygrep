# Changelog

## v0.1.0 (2024-03-24)

Initial release.

- Sparse n-gram inverted index with frequency-weighted boundaries
- Adaptive scan: ContentMmap / ContentPread / Filesystem based on available RAM
- 1,300x faster than ripgrep on selective queries (cold, real hardware)
- Streaming index build (works on 8GB machines)
- 533KB static binary, 3 dependencies (memchr, memmap2, libc)
