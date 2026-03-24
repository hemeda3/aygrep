// ayg — Ahmed Yousri Grep
// Indexed code search for large monorepos
// https://github.com/hemeda3/aygrep
//
// Author: Ahmed Yousri (hemeda3)
// Based on reverse engineering of: https://cursor.com/blog/fast-regex-search
// License: MIT

use memchr::memmem;
use memmap2::Mmap;
use std::fs::{self, File};
use std::hash::{BuildHasherDefault, Hasher};
use std::io;
use std::time::Instant;

// ── Verbose output ──────────────────────────────────────────────────────────

static mut VERBOSE: bool = false;

macro_rules! vprintln {
    ($($arg:tt)*) => {
        if unsafe { VERBOSE } {
            eprintln!($($arg)*);
        }
    };
}

// ── Constants ───────────────────────────────────────────────────────────────

const INDEX_DIR: &str = "ayg_index";
const POSTINGS_FILE: &str = "ayg_index/postings.bin";
const LOOKUP_FILE: &str = "ayg_index/lookup.bin";
const FILES_FILE: &str = "ayg_index/files.bin";
const META_FILE: &str = "ayg_index/meta.txt";
const FREQ_FILE: &str = "ayg_index/freq.bin";
const CONTENT_FILE: &str = "ayg_index/content.bin";

// ── FxHasher ────────────────────────────────────────────────────────────────

struct FxHasher(u64);

impl Default for FxHasher {
    fn default() -> Self {
        Self(0)
    }
}

impl Hasher for FxHasher {
    fn finish(&self) -> u64 {
        self.0
    }
    fn write(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.0 = (self.0.rotate_left(5) ^ b as u64).wrapping_mul(0x517cc1b727220a95);
        }
    }
}

type FxBuildHasher = BuildHasherDefault<FxHasher>;
type FxHashMap<K, V> = std::collections::HashMap<K, V, FxBuildHasher>;

// ── TrigramBitSet ───────────────────────────────────────────────────────────
// 2MB bitvec covering all 2^24 possible trigram values.

struct TrigramBitSet {
    bits: Vec<u64>,
}

impl TrigramBitSet {
    fn new() -> Self {
        Self {
            bits: vec![0u64; 262_144],
        }
    }

    fn insert(&mut self, t: [u8; 3]) {
        let idx = (t[0] as usize) << 16 | (t[1] as usize) << 8 | t[2] as usize;
        self.bits[idx >> 6] |= 1u64 << (idx & 63);
    }

    fn contains(&self, t: [u8; 3]) -> bool {
        let idx = (t[0] as usize) << 16 | (t[1] as usize) << 8 | t[2] as usize;
        self.bits[idx >> 6] & (1u64 << (idx & 63)) != 0
    }

    fn clear(&mut self) {
        self.bits.fill(0);
    }
}

// ── FrequencyTable ──────────────────────────────────────────────────────────

struct FrequencyTable {
    weights: [u8; 65536],
}

impl FrequencyTable {
    fn weight(&self, a: u8, b: u8) -> u8 {
        self.weights[a as usize * 256 + b as usize]
    }
}

// ── Sparse n-gram functions ─────────────────────────────────────────────────

fn compute_weights(bytes: &[u8], freq: &FrequencyTable) -> Vec<u8> {
    bytes
        .windows(2)
        .map(|p| freq.weight(p[0], p[1]))
        .collect()
}

fn find_boundaries(w: &[u8]) -> Vec<usize> {
    if w.is_empty() {
        return vec![];
    }
    if w.len() == 1 {
        return vec![0];
    }
    let mut boundaries = Vec::new();
    for i in 0..w.len() {
        let ge_prev = i == 0 || w[i] >= w[i - 1];
        let ge_next = i == w.len() - 1 || w[i] >= w[i + 1];
        if ge_prev && ge_next {
            boundaries.push(i);
        }
    }
    boundaries
}

fn find_interior_boundaries(w: &[u8]) -> Vec<usize> {
    if w.len() < 3 {
        return vec![];
    }
    let mut boundaries = Vec::new();
    for i in 1..w.len() - 1 {
        if w[i] >= w[i - 1] && w[i] >= w[i + 1] {
            boundaries.push(i);
        }
    }
    boundaries
}

fn build_all_ranges(bytes: &[u8], freq: &FrequencyTable) -> Vec<(usize, usize)> {
    if bytes.len() < 3 {
        return vec![];
    }
    let w = compute_weights(bytes, freq);
    let bounds = find_boundaries(&w);
    let mut ranges = Vec::new();
    if bounds.len() < 2 {
        if bytes.len() >= 3 {
            ranges.push((0, bytes.len()));
        }
        return ranges;
    }
    for i in 0..bounds.len() - 1 {
        let s = bounds[i];
        let e = (bounds[i + 1] + 2).min(bytes.len());
        if e > s && e - s >= 3 {
            ranges.push((s, e));
        }
    }
    ranges
}

fn build_covering_ranges(bytes: &[u8], freq: &FrequencyTable) -> Vec<(usize, usize)> {
    if bytes.len() < 3 {
        return vec![];
    }
    let w = compute_weights(bytes, freq);
    let bounds = find_interior_boundaries(&w);
    let mut ranges = Vec::new();
    if bounds.len() < 2 {
        return ranges;
    }
    for i in 0..bounds.len() - 1 {
        let s = bounds[i];
        let e = (bounds[i + 1] + 2).min(bytes.len());
        if e > s && e - s >= 3 {
            ranges.push((s, e));
        }
    }
    ranges
}

// ── Hash functions ──────────────────────────────────────────────────────────

type Trigram = [u8; 3];

fn hash_ngram(ngram: &[u8]) -> u32 {
    let mut hash: u64 = 0;
    for &byte in ngram {
        hash = hash.wrapping_mul(0x517cc1b727220a95).wrapping_add(byte as u64);
    }
    (hash & 0x00FF_FFFF) as u32
}

fn hash_to_trigram(h: u32) -> Trigram {
    [(h >> 16) as u8, (h >> 8) as u8, h as u8]
}

// ── File collection via git ls-files ────────────────────────────────────────

fn collect_files_git(repo_dir: &str) -> Vec<String> {
    let output = std::process::Command::new("git")
        .args(["ls-files", "-z"])
        .current_dir(repo_dir)
        .output()
        .expect("git ls-files failed — is this a git repository?");

    let skip_ext: std::collections::HashSet<&str> = [
        "png", "jpg", "jpeg", "gif", "ico", "bmp", "webp", "svg", "mp3", "mp4", "wav", "avi",
        "mov", "flac", "ogg", "zip", "gz", "tar", "bz2", "xz", "7z", "rar", "zst", "exe",
        "dll", "so", "dylib", "o", "a", "lib", "pdf", "doc", "docx", "ppt", "pptx", "xls",
        "xlsx", "pack", "idx", "woff", "woff2", "ttf", "eot", "otf", "pyc", "pyo", "class",
        "jar", "war", "bin", "dat", "db", "sqlite", "sqlite3",
    ]
    .into_iter()
    .collect();

    output
        .stdout
        .split(|&b| b == 0)
        .filter(|p| !p.is_empty())
        .filter_map(|p| std::str::from_utf8(p).ok())
        .filter(|p| {
            if let Some(ext) = std::path::Path::new(p)
                .extension()
                .and_then(|e| e.to_str())
            {
                !skip_ext.contains(ext.to_lowercase().as_str())
            } else {
                true
            }
        })
        .map(|s| s.to_string())
        .collect()
}

// ── Build index ─────────────────────────────────────────────────────────────

fn build_index(repo_dir: &str, no_content: bool) {
    let repo_path = fs::canonicalize(repo_dir).expect("Invalid repo dir");
    let t0 = Instant::now();

    vprintln!("Collecting files via git ls-files...");
    let file_list = collect_files_git(&repo_path.to_string_lossy());
    vprintln!(
        "Found {} files in {:.1}s",
        file_list.len(),
        t0.elapsed().as_secs_f64()
    );

    if no_content {
        vprintln!("Pass 1: Reading files + building freq table (no content.bin)...");
    } else {
        vprintln!("Pass 1: Reading files + building freq table + streaming content...");
    }
    let t1 = Instant::now();

    fs::create_dir_all(INDEX_DIR).unwrap();

    use std::io::Write;
    let mut content_writer = if no_content {
        None
    } else {
        Some(std::io::BufWriter::new(File::create(CONTENT_FILE).unwrap()))
    };

    let placeholder_count = file_list.len();
    let header_size = 4 + placeholder_count * 12;
    if let Some(ref mut w) = content_writer {
        let placeholder = vec![0u8; header_size];
        w.write_all(&placeholder).unwrap();
    }

    struct IndexedFile {
        rel_path: String,
        offset: u64,
        length: u32,
    }

    let mut indexed_files: Vec<IndexedFile> = Vec::new();
    let mut current_offset = header_size as u64;
    let mut byte_pair_counts = [0u64; 65536];
    let mut total_files = 0usize;

    for rel in &file_list {
        let path = repo_path.join(rel);
        let data = match fs::read(&path) {
            Ok(d) => d,
            Err(_) => continue,
        };
        if data.len() < 3 || data.len() > 1_000_000 {
            continue;
        }
        if data[..data.len().min(8192)].contains(&0) {
            continue;
        }

        for w in data.windows(2) {
            byte_pair_counts[w[0] as usize * 256 + w[1] as usize] += 1;
        }

        if let Some(ref mut w) = content_writer {
            w.write_all(&data).unwrap();
            indexed_files.push(IndexedFile {
                rel_path: rel.clone(),
                offset: current_offset,
                length: data.len() as u32,
            });
            current_offset += data.len() as u64;
        } else {
            indexed_files.push(IndexedFile {
                rel_path: rel.clone(),
                offset: 0,
                length: 0,
            });
        }
        total_files += 1;
    }

    if let Some(w) = content_writer.take() {
        let mut w = w;
        w.flush().unwrap();
        drop(w);

        use std::os::unix::fs::FileExt;
        let f = std::fs::OpenOptions::new()
            .write(true)
            .open(CONTENT_FILE)
            .unwrap();
        let mut header = Vec::with_capacity(header_size);
        header.extend_from_slice(&(total_files as u32).to_le_bytes());
        for entry in &indexed_files {
            header.extend_from_slice(&entry.offset.to_le_bytes());
            header.extend_from_slice(&entry.length.to_le_bytes());
        }
        header.resize(header_size, 0);
        f.write_all_at(&header, 0).unwrap();
    }

    vprintln!(
        "Pass 1 done: {} files, {:.1}s",
        total_files,
        t1.elapsed().as_secs_f64()
    );

    // Compute frequency weights
    let max_count = *byte_pair_counts.iter().max().unwrap_or(&1).max(&1);
    let mut weights = [0u8; 65536];
    for i in 0..65536 {
        if byte_pair_counts[i] == 0 {
            weights[i] = 255;
        } else {
            let w = (-(byte_pair_counts[i] as f64 / max_count as f64).ln() * 37.0)
                .min(255.0)
                .max(0.0);
            weights[i] = w as u8;
        }
    }
    let freq_table = FrequencyTable { weights };
    fs::write(FREQ_FILE, &freq_table.weights).unwrap();

    // Pass 2: Re-read files, extract n-grams
    vprintln!("Pass 2: Extracting n-grams...");
    let t2 = Instant::now();

    struct FileEntry {
        keys: Vec<[u8; 3]>,
    }
    let mut entries: Vec<FileEntry> = Vec::with_capacity(total_files);
    let mut bitset = TrigramBitSet::new();

    for entry in &indexed_files {
        let path = repo_path.join(&entry.rel_path);
        let data = match fs::read(&path) {
            Ok(d) => d,
            Err(_) => {
                entries.push(FileEntry { keys: vec![] });
                continue;
            }
        };

        bitset.clear();
        let mut keys = Vec::new();

        if data.len() >= 3 {
            for i in 0..data.len() - 2 {
                let h = hash_ngram(&data[i..i + 3]);
                let key = hash_to_trigram(h);
                if !bitset.contains(key) {
                    bitset.insert(key);
                    keys.push(key);
                }
            }
        }
        let ranges = build_all_ranges(&data, &freq_table);
        for (start, end) in &ranges {
            if end - start > 3 {
                let h = hash_ngram(&data[*start..*end]);
                let key = hash_to_trigram(h);
                if !bitset.contains(key) {
                    bitset.insert(key);
                    keys.push(key);
                }
            }
        }

        entries.push(FileEntry { keys });
    }

    vprintln!(
        "Extracted from {} files in {:.1}s",
        entries.len(),
        t2.elapsed().as_secs_f64()
    );

    // Build inverted index
    vprintln!("Building inverted index...");
    let t3 = Instant::now();
    let mut index: FxHashMap<[u8; 3], Vec<u32>> = FxHashMap::default();
    for (fid, entry) in entries.iter().enumerate() {
        for &key in &entry.keys {
            index.entry(key).or_default().push(fid as u32);
        }
    }
    let num_unique = index.len();
    vprintln!(
        "{} unique keys in {:.1}s",
        num_unique,
        t3.elapsed().as_secs_f64()
    );

    // Write postings + lookup + files
    vprintln!("Writing index...");
    let t4 = Instant::now();

    let mut sorted_keys: Vec<[u8; 3]> = index.keys().copied().collect();
    sorted_keys.sort();

    let mut postings_buf: Vec<u8> = Vec::new();
    let mut lookup_entries: Vec<([u8; 3], u64, u32)> = Vec::with_capacity(sorted_keys.len());
    for &key in &sorted_keys {
        let mut fids = index.remove(&key).unwrap();
        fids.sort_unstable();
        fids.dedup();
        let count = fids.len() as u32;
        let offset = postings_buf.len() as u64;
        let mut prev = 0u32;
        for &fid in &fids {
            let mut val = fid - prev;
            prev = fid;
            loop {
                if val < 128 {
                    postings_buf.push(val as u8);
                    break;
                }
                postings_buf.push((val & 0x7F) as u8 | 0x80);
                val >>= 7;
            }
        }
        lookup_entries.push((key, offset, count));
    }
    fs::write(POSTINGS_FILE, &postings_buf).unwrap();

    let mut lookup_buf = Vec::with_capacity(8 + lookup_entries.len() * 16);
    lookup_buf.extend_from_slice(&(lookup_entries.len() as u64).to_le_bytes());
    for &(key, off, cnt) in &lookup_entries {
        lookup_buf.extend_from_slice(&key);
        lookup_buf.push(0);
        lookup_buf.extend_from_slice(&off.to_le_bytes());
        lookup_buf.extend_from_slice(&cnt.to_le_bytes());
    }
    fs::write(LOOKUP_FILE, &lookup_buf).unwrap();

    let mut files_buf = Vec::new();
    files_buf.extend_from_slice(&(indexed_files.len() as u32).to_le_bytes());
    for entry in &indexed_files {
        let pb = entry.rel_path.as_bytes();
        files_buf.extend_from_slice(&(pb.len() as u16).to_le_bytes());
        files_buf.extend_from_slice(pb);
    }
    fs::write(FILES_FILE, &files_buf).unwrap();

    let total_time = t0.elapsed();
    let postings_size = postings_buf.len();
    let lookup_size = lookup_buf.len();
    let files_size = files_buf.len();
    let content_size = if no_content { 0 } else { current_offset as usize };
    let total_size = postings_size + lookup_size + files_size + content_size;

    let meta = format!(
        "repo_dir={}\nfiles_indexed={}\nunique_keys={}\npostings_mb={:.1}\nlookup_mb={:.1}\nfiles_mb={:.1}\ncontent_mb={:.1}\ntotal_index_mb={:.1}\ntotal_s={:.2}\n",
        repo_path.display(),
        total_files,
        num_unique,
        postings_size as f64 / 1048576.0,
        lookup_size as f64 / 1048576.0,
        files_size as f64 / 1048576.0,
        content_size as f64 / 1048576.0,
        total_size as f64 / 1048576.0,
        total_time.as_secs_f64()
    );
    fs::write(META_FILE, &meta).unwrap();

    let _write_time = t4.elapsed();
    vprintln!(
        "\nDone in {:.1}s. Index: {:.1}MB (postings={:.1}MB, lookup={:.1}MB, content={:.1}MB)",
        total_time.as_secs_f64(),
        total_size as f64 / 1048576.0,
        postings_size as f64 / 1048576.0,
        lookup_size as f64 / 1048576.0,
        content_size as f64 / 1048576.0
    );
    print!("{}", meta);
}

// ── ScanMode + memory detection ─────────────────────────────────────────────

#[derive(Debug)]
enum ScanMode {
    ContentMmap,
    ContentPread,
    Filesystem,
}

fn get_available_memory() -> u64 {
    #[cfg(target_os = "linux")]
    {
        if let Ok(s) = fs::read_to_string("/proc/meminfo") {
            for line in s.lines() {
                if line.starts_with("MemAvailable:") {
                    if let Some(kb) = line
                        .split_whitespace()
                        .nth(1)
                        .and_then(|v| v.parse::<u64>().ok())
                    {
                        return kb * 1024;
                    }
                }
            }
        }
        0
    }
    #[cfg(target_os = "macos")]
    {
        unsafe {
            let mut size: u64 = 0;
            let mut len = std::mem::size_of::<u64>();
            libc::sysctlbyname(
                b"hw.memsize\0".as_ptr() as *const i8,
                &mut size as *mut _ as *mut libc::c_void,
                &mut len as *mut _,
                std::ptr::null_mut(),
                0,
            );
            size / 2
        }
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        0
    }
}

fn choose_scan_mode() -> ScanMode {
    let content_size = fs::metadata(CONTENT_FILE).map(|m| m.len()).unwrap_or(0);
    if content_size == 0 {
        return ScanMode::Filesystem;
    }
    let available = get_available_memory();
    if available > content_size * 2 {
        ScanMode::ContentMmap
    } else if available > content_size {
        ScanMode::ContentPread
    } else {
        ScanMode::Filesystem
    }
}

// ── Content header parsing ──────────────────────────────────────────────────

fn parse_content_header(data: &[u8]) -> Vec<(u64, u32)> {
    let count = u32::from_le_bytes(data[0..4].try_into().unwrap()) as usize;
    let mut offsets = Vec::with_capacity(count);
    let mut pos = 4;
    for _ in 0..count {
        let offset = u64::from_le_bytes(data[pos..pos + 8].try_into().unwrap());
        let length = u32::from_le_bytes(data[pos + 8..pos + 12].try_into().unwrap());
        offsets.push((offset, length));
        pos += 12;
    }
    offsets
}

fn read_content_header(file: &File) -> io::Result<Vec<(u64, u32)>> {
    use std::os::unix::fs::FileExt;
    let mut count_buf = [0u8; 4];
    file.read_exact_at(&mut count_buf, 0)?;
    let count = u32::from_le_bytes(count_buf) as usize;
    let header_size = 4 + count * 12;
    let mut buf = vec![0u8; header_size];
    file.read_exact_at(&mut buf, 0)?;
    Ok(parse_content_header(&buf))
}

// ── Searcher ────────────────────────────────────────────────────────────────

struct Searcher {
    file_paths: Vec<String>,
    lookup_mmap: Mmap,
    postings_file: File,
    entry_count: u64,
    repo_dir: String,
    freq_table: FrequencyTable,
    scan_mode: ScanMode,
    content_mmap: Option<Mmap>,
    content_offsets: Option<Vec<(u64, u32)>>,
    content_file: Option<File>,
}

impl Searcher {
    fn open() -> io::Result<Self> {
        let t0 = Instant::now();

        let meta_str = fs::read_to_string(META_FILE)?;
        let repo_dir = meta_str
            .lines()
            .find(|l| l.starts_with("repo_dir="))
            .map(|l| l.trim_start_matches("repo_dir=").to_string())
            .unwrap_or_default();

        let lookup_file = File::open(LOOKUP_FILE)?;
        let lookup_mmap = unsafe { Mmap::map(&lookup_file)? };
        #[cfg(unix)]
        {
            unsafe {
                libc::madvise(
                    lookup_mmap.as_ptr() as *mut libc::c_void,
                    lookup_mmap.len(),
                    libc::MADV_WILLNEED,
                );
            }
        }
        let entry_count = u64::from_le_bytes(lookup_mmap[0..8].try_into().unwrap());

        let postings_file = File::open(POSTINGS_FILE)?;

        let freq_data = fs::read(FREQ_FILE)?;
        let mut freq_weights = [0u8; 65536];
        freq_weights.copy_from_slice(&freq_data);
        let freq_table = FrequencyTable {
            weights: freq_weights,
        };

        let files_data = fs::read(FILES_FILE)?;
        let file_count = u32::from_le_bytes(files_data[0..4].try_into().unwrap()) as usize;
        let mut file_paths = Vec::with_capacity(file_count);
        let mut pos = 4;
        for _ in 0..file_count {
            let len =
                u16::from_le_bytes(files_data[pos..pos + 2].try_into().unwrap()) as usize;
            pos += 2;
            let path = String::from_utf8_lossy(&files_data[pos..pos + len]).into_owned();
            pos += len;
            file_paths.push(path);
        }

        let scan_mode = choose_scan_mode();
        vprintln!("Scan mode: {:?}", scan_mode);

        let (content_mmap, content_offsets, content_file) = match scan_mode {
            ScanMode::ContentMmap => {
                let f = File::open(CONTENT_FILE)?;
                let mmap = unsafe { Mmap::map(&f)? };
                #[cfg(unix)]
                {
                    unsafe {
                        libc::madvise(
                            mmap.as_ptr() as *mut libc::c_void,
                            mmap.len(),
                            libc::MADV_SEQUENTIAL,
                        );
                    }
                }
                let offsets = parse_content_header(&mmap);
                (Some(mmap), Some(offsets), None)
            }
            ScanMode::ContentPread => {
                let f = File::open(CONTENT_FILE)?;
                let header = read_content_header(&f)?;
                (None, Some(header), Some(f))
            }
            ScanMode::Filesystem => (None, None, None),
        };

        let load_ms = t0.elapsed().as_secs_f64() * 1000.0;
        vprintln!(
            "Index loaded in {:.1}ms ({} trigrams, {} files)",
            load_ms, entry_count, file_paths.len()
        );

        Ok(Searcher {
            file_paths,
            lookup_mmap,
            postings_file,
            entry_count,
            repo_dir,
            freq_table,
            scan_mode,
            content_mmap,
            content_offsets,
            content_file,
        })
    }

    #[cfg(unix)]
    fn read_posting_bytes(&self, offset: u64, max_bytes: usize) -> Vec<u8> {
        use std::os::unix::fs::FileExt;
        let mut buf = vec![0u8; max_bytes];
        let n = self.postings_file.read_at(&mut buf, offset).unwrap_or(0);
        buf.truncate(n);
        buf
    }

    fn probe_trigram(&self, trigram: &Trigram) -> Option<(u64, usize)> {
        let data = &self.lookup_mmap;
        let entry_count = self.entry_count as usize;
        if entry_count == 0 {
            return None;
        }

        let mut lo = 0usize;
        let mut hi = entry_count;
        while hi - lo > 1 {
            let mid = lo + (hi - lo) / 2;
            let mid_off = 8 + mid * 16;
            let mid_tri = &data[mid_off..mid_off + 3];
            if mid_tri < trigram.as_slice() {
                lo = mid;
            } else {
                hi = mid;
            }
        }

        for &idx in &[lo, hi] {
            if idx < entry_count {
                let off = 8 + idx * 16;
                if &data[off..off + 3] == trigram.as_slice() {
                    let posting_offset =
                        u64::from_le_bytes(data[off + 4..off + 12].try_into().unwrap());
                    let count =
                        u32::from_le_bytes(data[off + 12..off + 16].try_into().unwrap())
                            as usize;
                    return Some((posting_offset, count));
                }
            }
        }
        None
    }

    fn fetch_posting_list(&self, posting_offset: u64, count: usize) -> Vec<u32> {
        let max_bytes = count * 5;
        let postings_data = self.read_posting_bytes(posting_offset, max_bytes);

        let mut result = Vec::with_capacity(count);
        let mut p = 0;
        let mut prev: u32 = 0;

        for _ in 0..count {
            let mut val: u32 = 0;
            let mut shift = 0;
            loop {
                if p >= postings_data.len() {
                    break;
                }
                let byte = postings_data[p];
                p += 1;
                val |= ((byte & 0x7F) as u32) << shift;
                if byte < 128 {
                    break;
                }
                shift += 7;
            }
            prev += val;
            result.push(prev);
        }
        result
    }

    fn intersect_sorted(a: &[u32], b: &[u32]) -> Vec<u32> {
        let mut result = Vec::new();
        let (mut i, mut j) = (0, 0);
        while i < a.len() && j < b.len() {
            if a[i] == b[j] {
                result.push(a[i]);
                i += 1;
                j += 1;
            } else if a[i] < b[j] {
                i += 1;
            } else {
                j += 1;
            }
        }
        result
    }

    fn galloping_intersect(short: &[u32], long: &[u32]) -> Vec<u32> {
        let mut result = Vec::new();
        let mut long_pos = 0;

        for &target in short {
            let mut bound = 1;
            while long_pos + bound < long.len() && long[long_pos + bound] < target {
                bound *= 2;
            }
            let search_end = (long_pos + bound).min(long.len());
            match long[long_pos..search_end].binary_search(&target) {
                Ok(idx) => {
                    result.push(target);
                    long_pos += idx + 1;
                }
                Err(idx) => {
                    long_pos += idx;
                }
            }
        }
        result
    }

    fn adaptive_intersect(a: &[u32], b: &[u32]) -> Vec<u32> {
        let (short, long) = if a.len() < b.len() { (a, b) } else { (b, a) };
        if long.len() > 64 * short.len() {
            Self::galloping_intersect(short, long)
        } else {
            Self::intersect_sorted(short, long)
        }
    }

    fn search(&self, pattern: &str) -> SearchResult {
        let pat_bytes = pattern.as_bytes();
        if pat_bytes.len() < 3 {
            return SearchResult {
                pattern: pattern.to_string(),
                trigrams_used: 0,
                candidates: 0,
                matches: 0,
                files_matched: 0,
                index_lookup_us: 0,
                scan_ms: 0.0,
                total_ms: 0.0,
                matched_paths: vec![],
            };
        }

        let t_start = Instant::now();

        let covering_ranges = build_covering_ranges(pat_bytes, &self.freq_table);

        let trigram_keys: Vec<Trigram> = if covering_ranges.len() >= 3 {
            covering_ranges
                .iter()
                .map(|(s, e)| hash_to_trigram(hash_ngram(&pat_bytes[*s..*e])))
                .collect()
        } else {
            let mut raw: Vec<Trigram> = Vec::new();
            if pat_bytes.len() >= 3 {
                for i in 0..pat_bytes.len() - 2 {
                    let t = hash_to_trigram(hash_ngram(&pat_bytes[i..i + 3]));
                    if !raw.contains(&t) {
                        raw.push(t);
                    }
                }
            }
            let mut probed: Vec<(Trigram, usize)> = raw
                .iter()
                .filter_map(|t| self.probe_trigram(t).map(|(_, count)| (*t, count)))
                .collect();
            probed.sort_by_key(|&(_, count)| count);
            probed.iter().take(8).map(|(t, _)| *t).collect()
        };

        let mut probes: Vec<(Trigram, u64, usize)> = trigram_keys
            .iter()
            .filter_map(|t| self.probe_trigram(t).map(|(off, count)| (*t, off, count)))
            .collect();

        if probes.is_empty() {
            return SearchResult {
                pattern: pattern.to_string(),
                trigrams_used: 0,
                candidates: 0,
                matches: 0,
                files_matched: 0,
                index_lookup_us: t_start.elapsed().as_micros() as u64,
                scan_ms: 0.0,
                total_ms: t_start.elapsed().as_secs_f64() * 1000.0,
                matched_paths: vec![],
            };
        }

        probes.sort_by_key(|&(_, _, count)| count);

        let (_, first_off, first_count) = probes[0];
        let mut candidates = self.fetch_posting_list(first_off, first_count);
        let mut num_fetched = 1;

        for &(_, off, count) in &probes[1..] {
            if candidates.len() <= 50 && num_fetched >= 2 {
                break;
            }
            if count > candidates.len() * 50 && num_fetched >= 2 {
                continue;
            }
            let posting = self.fetch_posting_list(off, count);
            candidates = Self::adaptive_intersect(&candidates, &posting);
            num_fetched += 1;
            if num_fetched >= 5 {
                break;
            }
        }

        let index_us = t_start.elapsed().as_micros() as u64;
        let num_candidates = candidates.len();

        let t_scan = Instant::now();
        let mut matches = 0u64;
        let mut files_matched = 0u64;
        let mut matched_paths: Vec<String> = Vec::new();

        if !candidates.is_empty() {
            let finder = memmem::Finder::new(pat_bytes);

            match &self.scan_mode {
                ScanMode::ContentMmap => {
                    let mmap = self.content_mmap.as_ref().unwrap();
                    let offsets = self.content_offsets.as_ref().unwrap();
                    candidates.sort_unstable_by_key(|&fid| offsets[fid as usize].0);
                    for &fid in &candidates {
                        let (off, len) = offsets[fid as usize];
                        let start = off as usize;
                        let end = start + len as usize;
                        if end > mmap.len() {
                            continue;
                        }
                        let data = &mmap[start..end];
                        let count = finder.find_iter(data).count() as u64;
                        if count > 0 {
                            matches += count;
                            files_matched += 1;
                            matched_paths.push(self.file_paths[fid as usize].clone());
                        }
                    }
                }
                ScanMode::ContentPread => {
                    let file = self.content_file.as_ref().unwrap();
                    let offsets = self.content_offsets.as_ref().unwrap();
                    let mut buf = Vec::with_capacity(1_000_000);
                    for &fid in &candidates {
                        let (off, len) = offsets[fid as usize];
                        buf.resize(len as usize, 0);
                        use std::os::unix::fs::FileExt;
                        if file.read_exact_at(&mut buf, off).is_ok() {
                            let count = finder.find_iter(&buf).count() as u64;
                            if count > 0 {
                                matches += count;
                                files_matched += 1;
                                matched_paths.push(self.file_paths[fid as usize].clone());
                            }
                        }
                    }
                }
                ScanMode::Filesystem => {
                    let repo = &self.repo_dir;
                    let mut buf = Vec::with_capacity(1_000_000);
                    for &fid in &candidates {
                        let path =
                            std::path::Path::new(repo).join(&self.file_paths[fid as usize]);
                        buf.clear();
                        if let Ok(mut f) = File::open(&path) {
                            use std::io::Read;
                            if f.read_to_end(&mut buf).is_ok() {
                                let count = finder.find_iter(&buf).count() as u64;
                                if count > 0 {
                                    matches += count;
                                    files_matched += 1;
                                    matched_paths.push(self.file_paths[fid as usize].clone());
                                }
                            }
                        }
                    }
                }
            }
        }

        let scan_ms = t_scan.elapsed().as_secs_f64() * 1000.0;
        let total_ms = t_start.elapsed().as_secs_f64() * 1000.0;

        SearchResult {
            pattern: pattern.to_string(),
            trigrams_used: num_fetched,
            candidates: num_candidates,
            matches,
            files_matched,
            index_lookup_us: index_us,
            scan_ms,
            total_ms,
            matched_paths,
        }
    }
}

// ── SearchResult ────────────────────────────────────────────────────────────

struct SearchResult {
    pattern: String,
    trigrams_used: usize,
    candidates: usize,
    matches: u64,
    files_matched: u64,
    index_lookup_us: u64,
    scan_ms: f64,
    total_ms: f64,
    matched_paths: Vec<String>,
}

impl std::fmt::Display for SearchResult {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(
            f,
            "{:25} tri={:2}  cand={:6}  match={:6}  files={:5}  \
             idx={:>6}us  scan={:>8.1}ms  total={:>8.1}ms",
            self.pattern,
            self.trigrams_used,
            self.candidates,
            self.matches,
            self.files_matched,
            self.index_lookup_us,
            self.scan_ms,
            self.total_ms,
        )
    }
}

// ── Benchmark ───────────────────────────────────────────────────────────────

fn run_benchmark() {
    let searcher = Searcher::open().expect("No index. Run `ayg build <repo>` first.");

    let queries = [
        "MAX_FILE_SIZE",
        "kMaxBufferSize",
        "gpu::Mailbox",
        "NOTREACHED",
        "base::Unretained",
        "constexpr char k",
        "WebContents",
        "std::unique_ptr",
        "#include \"base/",
    ];

    println!(
        "\n{:25} {:>4}  {:>6}  {:>6}  {:>5}  {:>8}  {:>10}  {:>10}",
        "QUERY", "TRI", "CAND", "MATCH", "FILES", "INDEX", "SCAN", "TOTAL"
    );
    println!("{}", "-".repeat(95));

    for pattern in &queries {
        let mut results: Vec<SearchResult> = (0..3).map(|_| searcher.search(pattern)).collect();
        results.sort_by(|a, b| a.total_ms.partial_cmp(&b.total_ms).unwrap());
        let median = &results[1];
        println!("{}", median);
    }

    println!("\n--- Comparison with ripgrep baseline ---");
    println!(
        "{:25} {:>10}  {:>10}  {:>10}",
        "QUERY", "rg (s)", "ayg (ms)", "SPEEDUP"
    );
    println!("{}", "-".repeat(60));

    let rg_baselines = [
        ("MAX_FILE_SIZE", 12.1),
        ("kMaxBufferSize", 10.9),
        ("gpu::Mailbox", 12.6),
        ("NOTREACHED", 10.8),
        ("base::Unretained", 12.2),
        ("constexpr char k", 10.5),
        ("WebContents", 12.5),
        ("std::unique_ptr", 11.2),
        ("#include \"base/", 13.0),
    ];

    for (pattern, rg_time) in &rg_baselines {
        let mut results: Vec<SearchResult> = (0..3).map(|_| searcher.search(pattern)).collect();
        results.sort_by(|a, b| a.total_ms.partial_cmp(&b.total_ms).unwrap());
        let median = &results[1];
        let speedup = rg_time * 1000.0 / median.total_ms;
        println!(
            "{:25} {:>8.1}s  {:>8.1}ms  {:>8.0}x",
            pattern, rg_time, median.total_ms, speedup
        );
    }
}

// ── Main ────────────────────────────────────────────────────────────────────

fn main() {
    let args: Vec<String> = std::env::args().collect();

    unsafe { VERBOSE = args.iter().any(|a| a == "--debug"); }

    if args.len() < 2 || args.iter().any(|a| a == "--help" || a == "-h") {
        println!("ayg — indexed code search for large monorepos");
        println!("https://github.com/hemeda3/aygrep");
        println!();
        println!("USAGE:");
        println!("  ayg build <repo>        Build search index for a git repo");
        println!("  ayg search <pattern>    Search the repo (run from inside the repo)");
        println!("  ayg benchmark           Run benchmark suite");
        println!();
        println!("EXAMPLES:");
        println!("  ayg build ~/code/chromium        # build index (one-time, ~30s)");
        println!("  cd ~/code/chromium               # go into the repo");
        println!("  ayg search \"MAX_FILE_SIZE\"        # search — 0.2ms, 24 candidates");
        println!("  ayg search \"std::unique_ptr\" -c   # just print match count");
        println!("  ayg search \"WebContents\" -l       # list matching file paths");
        println!("  ayg search \"TODO\" --json           # JSON output for scripts");
        println!();
        println!("SEARCH FLAGS:");
        println!("  -c, --count               Show only total match count");
        println!("  -l, --files-with-matches  Show only matching file paths");
        println!("  --json                    Output results as JSON");
        println!("  --debug                   Show index/scan timing details");
        println!();
        println!("GLOBAL FLAGS:");
        println!("  --version                 Show version");
        println!("  -h, --help               Show this help");
        println!();
        println!("NOTE: The index (ayg_index/) is created inside the repo directory.");
        println!("      Run `ayg search` from the same directory where you ran `ayg build`.");
        std::process::exit(0);
    }

    if args.iter().any(|a| a == "--version") {
        println!("ayg 0.1.0");
        std::process::exit(0);
    }

    match args[1].as_str() {
        "build" => {
            let repo = match args.get(2) {
                Some(r) => r,
                None => {
                    eprintln!("Error: Missing repo path.");
                    eprintln!("  ayg build /path/to/repo");
                    std::process::exit(1);
                }
            };
            let available_ram = get_available_memory();
            let skip_content = available_ram > 0 && available_ram < 8 * 1024 * 1024 * 1024;
            if skip_content {
                vprintln!("RAM < 8GB ({:.1}GB available) — skipping content.bin", available_ram as f64 / 1073741824.0);
            } else {
                vprintln!("RAM >= 8GB ({:.1}GB available) — building content.bin for fast scanning", available_ram as f64 / 1073741824.0);
            }
            build_index(repo, skip_content);
            println!();
            println!("Done! Now search:");
            println!("  cd {}", repo);
            println!("  ayg search \"your pattern\"");
        }
        "search" => {
            let pattern = match args.get(2) {
                Some(p) => p,
                None => {
                    eprintln!("Error: Missing search pattern.");
                    eprintln!("  ayg search \"MAX_FILE_SIZE\"");
                    std::process::exit(1);
                }
            };
            let json = args.iter().any(|a| a == "--json");
            let count_only = args.iter().any(|a| a == "--count" || a == "-c");
            let files_only = args.iter().any(|a| a == "--files-with-matches" || a == "-l");
            let searcher = match Searcher::open() {
                Ok(s) => s,
                Err(_) => {
                    eprintln!("Error: No index found in current directory.");
                    eprintln!();
                    eprintln!("  1. Build the index first:  ayg build /path/to/repo");
                    eprintln!("  2. Then cd into the repo:  cd /path/to/repo");
                    eprintln!("  3. Then search:            ayg search \"pattern\"");
                    eprintln!();
                    eprintln!("The index (ayg_index/) must be in the current directory.");
                    std::process::exit(1);
                }
            };
            let result = searcher.search(pattern);
            if json {
                println!("{{\"pattern\":\"{}\",\"candidates\":{},\"matches\":{},\"files_matched\":{},\"total_ms\":{:.1}}}",
                    result.pattern.replace('"', "\\\""), result.candidates, result.matches, result.files_matched, result.total_ms);
            } else if count_only {
                println!("{}", result.matches);
            } else if files_only {
                for path in &result.matched_paths {
                    println!("{}", path);
                }
            } else {
                println!("{}", result);
            }
        }
        "benchmark" => run_benchmark(),
        _ => {
            eprintln!("Unknown command: {}. Run `ayg --help`", args[1]);
            std::process::exit(1);
        }
    }
}
