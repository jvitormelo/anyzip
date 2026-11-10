# Baseline

2026-11-10, AMD Ryzen 7 5800X (8 cores / 16 threads), 32 GB, Linux 7.3 (CachyOS). Addon built in
`manylinux_2_28` (the release recipe). bsdtar 3.8.11 (libarchive, liblzma 5.8.3),
libarchive-wasm 1.2.0, fflate 0.8.2.

    cd bench && bun bench.ts && node bench.ts

- **Time** is bytes in memory → every file's bytes in memory. bsdtar also pays for the temp file,
  the child process and reading the files back.
- **burst**: 50 extractions started at once. anyzip runs them on the libuv pool (4 threads by
  default; `UV_THREADPOOL_SIZE`), bsdtar as 50 processes over 16 hardware threads, and the wasm/JS
  engines one after another on the JS thread.
- **RSS** is the child process peak (`VmHWM`) after the sequential runs, then after the burst.
- Archives: `fixtures/real` (real edital PDFs, see `scripts/make-real-corpus.py`); `copied_a0.7z`
  is a real archive from a portal (7-Zip multi-threaded LZMA2, 256 KiB dictionary).

## Key benchmarks

| target | result |
|---|---|
| zip of ~4 MB with PDFs ≤ bsdtar | m8_windows.zip (5.9 MB): 14.5 ms vs 25.0 ms |
| 7z of 3 MB ≤ 0.6× bsdtar | copied_a0.7z (3.3 MB): 35 ms vs 79 ms = 0.45× |

## Node

| archive | size | impl | files | out MB | p50 ms | p99 ms | MB/s | burst ms | RSS MB | burst RSS MB |
|---|---:|---|---:|---:|---:|---:|---:|---:|---:|---:|
| s2_windows.zip | 0.21 | anyzip | 2 | 0.22 | 0.6 | 1.1 | 357 | 8 | 112 | 112 |
| s2_windows.zip | 0.21 | bsdtar | 2 | 0.22 | 4.1 | 7.9 | 54 | 71 | 84 | 87 |
| s2_windows.zip | 0.21 | libarchive-wasm | 2 | 0.22 | 0.8 | 2.3 | 294 | 41 | 99 | 110 |
| s2_windows.zip | 0.21 | fflate | 2 | 0.22 | 1.7 | 7.8 | 132 | 81 | 120 | 123 |
| m8_windows.zip | 5.92 | anyzip | 8 | 6.81 | 14.5 | 16.1 | 469 | 196 | 166 | 186 |
| m8_windows.zip | 5.92 | bsdtar | 8 | 6.81 | 25.0 | 28.2 | 272 | 381 | 148 | 235 |
| m8_windows.zip | 5.92 | libarchive-wasm | 8 | 6.81 | 19.9 | 24.6 | 342 | 1033 | 214 | 213 |
| m8_windows.zip | 5.92 | fflate | 8 | 6.81 | 42.0 | 51.3 | 162 | 2049 | 152 | 153 |
| l4_windows.zip | 13.68 | anyzip | 4 | 15.50 | 35.8 | 43.6 | 433 | 477 | 194 | 275 |
| l4_windows.zip | 13.68 | bsdtar | 4 | 15.50 | 50.9 | 65.7 | 304 | 670 | 175 | 260 |
| l4_windows.zip | 13.68 | libarchive-wasm | 4 | 15.50 | 48.4 | 54.8 | 320 | 2335 | 251 | 251 |
| l4_windows.zip | 13.68 | fflate | 4 | 15.50 | 106.4 | 125.9 | 146 | 5271 | 182 | 186 |
| s2_solid.7z | 0.21 | anyzip | 2 | 0.22 | 4.8 | 6.2 | 47 | 67 | 111 | 112 |
| s2_solid.7z | 0.21 | bsdtar | 2 | 0.22 | 14.0 | 17.2 | 16 | 118 | 86 | 88 |
| s2_solid.7z | 0.21 | libarchive-wasm | 2 | 0.22 | 11.7 | 15.9 | 19 | 476 | 105 | 115 |
| copied_a0.7z | 3.32 | anyzip | 2 | 3.95 | 35.4 | 46.6 | 112 | 877 | 181 | 243 |
| copied_a0.7z | 3.32 | bsdtar | 2 | 3.95 | 78.8 | 83.7 | 50 | 415 | 140 | 212 |
| copied_a0.7z | 3.32 | libarchive-wasm | 2 | 3.95 | 140.1 | 167.1 | 28 | 6800 | 138 | 177 |
| m8_solid.7z | 4.93 | anyzip | 8 | 6.81 | 102.2 | 135.0 | 67 | 1209 | 186 | 219 |
| m8_solid.7z | 4.93 | bsdtar | 8 | 6.81 | 100.9 | 107.1 | 68 | 649 | 138 | 177 |
| m8_solid.7z | 4.93 | libarchive-wasm | 8 | 6.81 | 181.9 | 187.4 | 37 | 9301 | 147 | 213 |
| l4_solid.7z | 11.00 | anyzip | 4 | 15.50 | 265.8 | 277.0 | 58 | 3355 | 208 | 291 |
| l4_solid.7z | 11.00 | bsdtar | 4 | 15.50 | 246.4 | 265.9 | 63 | 1525 | 142 | 176 |
| l4_solid.7z | 11.00 | libarchive-wasm | 4 | 15.50 | 489.7 | 501.0 | 32 | 23206 | 192 | 257 |
| rar5_multiple_files_solid.rar | 0.00 | anyzip | 4 | 0.02 | 0.2 | 0.5 | 85 | 2 | 92 | 92 |
| rar5_multiple_files_solid.rar | 0.00 | bsdtar | 4 | 0.02 | 3.8 | 7.4 | 4 | 55 | 79 | 81 |
| rar5_multiple_files_solid.rar | 0.00 | libarchive-wasm | 4 | 0.02 | 0.2 | 0.7 | 91 | 7 | 92 | 93 |
| rar_unicode.rar | 0.00 | anyzip | 3 | 0.00 | 0.6 | 2.0 | 0 | 6 | 107 | 107 |
| rar_unicode.rar | 0.00 | bsdtar | 3 | 0.00 | 3.8 | 5.4 | 0 | 54 | 78 | 85 |
| rar_unicode.rar | 0.00 | libarchive-wasm | 3 | 0.00 | 0.1 | 0.2 | 0 | 3 | 88 | 89 |

## Bun

| archive | size | impl | files | out MB | p50 ms | p99 ms | MB/s | burst ms | RSS MB | burst RSS MB |
|---|---:|---|---:|---:|---:|---:|---:|---:|---:|---:|
| s2_windows.zip | 0.21 | anyzip | 2 | 0.22 | 0.6 | 1.5 | 397 | 17 | 54 | 132 |
| s2_windows.zip | 0.21 | bsdtar | 2 | 0.22 | 4.3 | 10.0 | 52 | 60 | 44 | 51 |
| s2_windows.zip | 0.21 | libarchive-wasm | 2 | 0.22 | 0.9 | 1.9 | 256 | 39 | 75 | 80 |
| s2_windows.zip | 0.21 | fflate | 2 | 0.22 | 4.8 | 6.1 | 46 | 239 | 53 | 53 |
| m8_windows.zip | 5.92 | anyzip | 8 | 6.81 | 15.4 | 19.4 | 443 | 112 | 96 | 250 |
| m8_windows.zip | 5.92 | bsdtar | 8 | 6.81 | 23.3 | 27.5 | 292 | 348 | 60 | 84 |
| m8_windows.zip | 5.92 | libarchive-wasm | 8 | 6.81 | 18.7 | 20.7 | 365 | 899 | 158 | 158 |
| m8_windows.zip | 5.92 | fflate | 8 | 6.81 | 113.0 | 132.5 | 60 | 5765 | 90 | 102 |
| l4_windows.zip | 13.68 | anyzip | 4 | 15.50 | 35.4 | 41.6 | 438 | 531 | 128 | 487 |
| l4_windows.zip | 13.68 | bsdtar | 4 | 15.50 | 51.1 | 56.9 | 304 | 663 | 81 | 154 |
| l4_windows.zip | 13.68 | libarchive-wasm | 4 | 15.50 | 45.8 | 48.0 | 338 | 2266 | 185 | 185 |
| l4_windows.zip | 13.68 | fflate | 4 | 15.50 | 304.4 | 318.3 | 51 | 15404 | 108 | 127 |
| s2_solid.7z | 0.21 | anyzip | 2 | 0.22 | 4.8 | 8.4 | 47 | 44 | 41 | 58 |
| s2_solid.7z | 0.21 | bsdtar | 2 | 0.22 | 9.8 | 12.1 | 23 | 55 | 43 | 51 |
| s2_solid.7z | 0.21 | libarchive-wasm | 2 | 0.22 | 9.5 | 10.8 | 23 | 528 | 63 | 66 |
| copied_a0.7z | 3.32 | anyzip | 2 | 3.95 | 37.8 | 76.4 | 105 | 678 | 75 | 243 |
| copied_a0.7z | 3.32 | bsdtar | 2 | 3.95 | 81.2 | 89.9 | 49 | 528 | 53 | 78 |
| copied_a0.7z | 3.32 | libarchive-wasm | 2 | 3.95 | 150.4 | 161.0 | 26 | 8110 | 85 | 134 |
| m8_solid.7z | 4.93 | anyzip | 8 | 6.81 | 94.7 | 103.6 | 72 | 514 | 88 | 339 |
| m8_solid.7z | 4.93 | bsdtar | 8 | 6.81 | 103.5 | 109.5 | 66 | 561 | 49 | 82 |
| m8_solid.7z | 4.93 | libarchive-wasm | 8 | 6.81 | 202.7 | 215.1 | 34 | 11734 | 123 | 143 |
| l4_solid.7z | 11.00 | anyzip | 4 | 15.50 | 305.8 | 533.7 | 51 | 1886 | 145 | 568 |
| l4_solid.7z | 11.00 | bsdtar | 4 | 15.50 | 306.0 | 424.1 | 51 | 1802 | 78 | 159 |
| l4_solid.7z | 11.00 | libarchive-wasm | 4 | 15.50 | 586.8 | 606.9 | 26 | 25030 | 176 | 242 |
| rar5_multiple_files_solid.rar | 0.00 | anyzip | 4 | 0.02 | 0.2 | 0.4 | 85 | 3 | 39 | 61 |
| rar5_multiple_files_solid.rar | 0.00 | bsdtar | 4 | 0.02 | 3.1 | 5.2 | 5 | 26 | 36 | 42 |
| rar5_multiple_files_solid.rar | 0.00 | libarchive-wasm | 4 | 0.02 | 0.2 | 0.4 | 84 | 7 | 59 | 60 |
| rar_unicode.rar | 0.00 | anyzip | 3 | 0.00 | 0.3 | 1.5 | 0 | 28 | 49 | 137 |
| rar_unicode.rar | 0.00 | bsdtar | 3 | 0.00 | 4.4 | 8.1 | 0 | 41 | 36 | 40 |
| rar_unicode.rar | 0.00 | libarchive-wasm | 3 | 0.00 | 0.1 | 0.8 | 0 | 3 | 56 | 59 |

## Notes

- **7z.** Blocks that are one LZMA2 stream with a single chunk (7-Zip's default output for files
  under 4 × dictionary) go through liblzma; multi-chunk streams (7-Zip multi-threaded with a small
  dictionary, like `copied_a0`) are decoded in parallel by `sevenz-rust2`, up to 4 threads. Before
  the liblzma path, l4_solid took ~330 ms and m8_solid ~140 ms (lzma-rust2).
- **Memory.** With glibc malloc, a burst left 250–540 MB resident under Bun (rising every burst)
  and peaked at 1.4 GB; mimalloc with immediate purging brought that to ~140 MB after GC and
  ~700 MB at the peak. The remaining Bun peak is the 50 × 15.5 MB results alive until its GC runs
  (Bun does not count external buffers as GC pressure; Node peaks at ~330 MB on the same load).
- **Burst vs bsdtar** on 7z: 50 processes on 16 hardware threads beat a 4-thread pool. On a 1–2 vCPU
  function that advantage disappears; raise `UV_THREADPOOL_SIZE` where cores allow.
- Rust-only (`target/release/examples/dump`): l4_solid.7z 225–235 ms, m8_solid.7z 88–91 ms,
  l4_windows.zip ~31 ms, 20 real portal 7z archives 471 ms in total (bsdtar: ~990 ms).
