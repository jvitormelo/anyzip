# Baseline

2026-11-11, AMD Ryzen 7 5800X (8 cores / 16 threads), 32 GB, Linux 7.3 (CachyOS). Node 26.13.0,
Bun 1.4.4, bsdtar 3.8.11 (libarchive, liblzma 5.8.3), libarchive-wasm 1.2.0, fflate 0.8.3.

    cd bench && bun bench.ts --check && node bench.ts --check

- **Time** is bytes in memory → every file's bytes in memory (`anyzip` via `extract`, `anyzip-each`
  via `extractEach`). bsdtar also pays for the temp file, the child process and reading the files
  back.
- **burst**: 50 extractions started at once. Under Node, `anyzip` and `anyzip-each` use the libuv
  pool (`UV_THREADPOOL_SIZE`, default 4); under Bun, `availableParallelism()` (capped at 16 unless
  `UV_THREADPOOL_SIZE` is set). bsdtar runs 50 processes over 16 hardware threads, and the wasm/JS
  engines run one after another on the JS thread.
- **RSS** is the child process peak (`VmHWM`) after the sequential runs, then after the burst.
- Archives: `fixtures/real` (real PDFs, see `scripts/make-real-corpus.py`); `copied_a0.7z` is a
  real archive from a portal (7-Zip multi-threaded LZMA2, 256 KiB dictionary).

## Key benchmarks

| target | result (Node / Bun) |
|---|---|
| zip of ~6 MB with PDFs ≤ bsdtar | m8_windows.zip: 14.8 ms vs 28.7 ms (Node), 14.3 ms vs 20.1 ms (Bun) |
| 7z of 3 MB ≤ 0.6× bsdtar | copied_a0.7z: 30.8 ms vs 78.2 ms = 0.39× (Node), 31.5 ms vs 76.5 ms = 0.41× (Bun) |
| `extractEach` batch ≤ 1.35× `extract` batch | 0.95×–1.29× across all ≥1 MB archives, with lower peak RSS |

## Node

| archive | size | impl | files | out MB | p50 ms | p99 ms | MB/s | burst ms | RSS MB | burst RSS MB |
|---|---:|---|---:|---:|---:|---:|---:|---:|---:|---:|
| s2_windows.zip | 0.21 | anyzip | 2 | 0.22 | 0.7 | 1.1 | 343 | 7 | 116 | 124 |
| s2_windows.zip | 0.21 | anyzip-each | 2 | 0.22 | 0.8 | 1.1 | 270 | 12 | 104 | 121 |
| s2_windows.zip | 0.21 | bsdtar | 2 | 0.22 | 5.2 | 7.6 | 43 | 78 | 90 | 92 |
| s2_windows.zip | 0.21 | libarchive-wasm | 2 | 0.22 | 0.9 | 1.6 | 241 | 45 | 103 | 114 |
| s2_windows.zip | 0.21 | fflate | 2 | 0.22 | 1.7 | 7.3 | 131 | 80 | 116 | 119 |
| m8_windows.zip | 5.92 | anyzip | 8 | 6.81 | 14.8 | 17.2 | 460 | 199 | 172 | 193 |
| m8_windows.zip | 5.92 | anyzip-each | 8 | 6.81 | 16.0 | 22.2 | 426 | 239 | 161 | 164 |
| m8_windows.zip | 5.92 | bsdtar | 8 | 6.81 | 28.7 | 35.2 | 237 | 469 | 151 | 217 |
| m8_windows.zip | 5.92 | libarchive-wasm | 8 | 6.81 | 19.1 | 22.3 | 357 | 954 | 193 | 191 |
| m8_windows.zip | 5.92 | fflate | 8 | 6.81 | 42.0 | 50.9 | 162 | 2086 | 153 | 154 |
| l4_windows.zip | 13.68 | anyzip | 4 | 15.50 | 34.2 | 36.3 | 454 | 456 | 201 | 299 |
| l4_windows.zip | 13.68 | anyzip-each | 4 | 15.50 | 35.3 | 38.5 | 439 | 490 | 178 | 202 |
| l4_windows.zip | 13.68 | bsdtar | 4 | 15.50 | 54.1 | 59.1 | 287 | 843 | 175 | 253 |
| l4_windows.zip | 13.68 | libarchive-wasm | 4 | 15.50 | 47.0 | 49.7 | 330 | 2331 | 253 | 253 |
| l4_windows.zip | 13.68 | fflate | 4 | 15.50 | 102.9 | 148.7 | 151 | 5113 | 180 | 180 |
| s2_solid.7z | 0.21 | anyzip | 2 | 0.22 | 4.0 | 4.6 | 55 | 52 | 119 | 127 |
| s2_solid.7z | 0.21 | anyzip-each | 2 | 0.22 | 4.4 | 6.1 | 50 | 58 | 100 | 116 |
| s2_solid.7z | 0.21 | bsdtar | 2 | 0.22 | 9.5 | 12.0 | 24 | 87 | 89 | 92 |
| s2_solid.7z | 0.21 | libarchive-wasm | 2 | 0.22 | 8.4 | 10.9 | 27 | 429 | 101 | 112 |
| copied_a0.7z | 3.32 | anyzip | 2 | 3.95 | 30.8 | 35.7 | 128 | 568 | 181 | 251 |
| copied_a0.7z | 3.32 | anyzip-each | 2 | 3.95 | 31.0 | 33.1 | 127 | 555 | 180 | 243 |
| copied_a0.7z | 3.32 | bsdtar | 2 | 3.95 | 78.2 | 82.6 | 51 | 432 | 132 | 160 |
| copied_a0.7z | 3.32 | libarchive-wasm | 2 | 3.95 | 133.5 | 137.7 | 30 | 6677 | 140 | 170 |
| m8_solid.7z | 4.93 | anyzip | 8 | 6.81 | 92.9 | 103.9 | 73 | 1252 | 186 | 206 |
| m8_solid.7z | 4.93 | anyzip-each | 8 | 6.81 | 86.6 | 92.8 | 79 | 1189 | 156 | 188 |
| m8_solid.7z | 4.93 | bsdtar | 8 | 6.81 | 99.9 | 104.1 | 68 | 687 | 143 | 211 |
| m8_solid.7z | 4.93 | libarchive-wasm | 8 | 6.81 | 184.2 | 190.7 | 37 | 9108 | 147 | 213 |
| l4_solid.7z | 11.00 | anyzip | 4 | 15.50 | 213.8 | 227.9 | 73 | 2827 | 215 | 315 |
| l4_solid.7z | 11.00 | anyzip-each | 4 | 15.50 | 223.2 | 236.4 | 69 | 3009 | 168 | 253 |
| l4_solid.7z | 11.00 | bsdtar | 4 | 15.50 | 244.3 | 272.3 | 63 | 1243 | 146 | 177 |
| l4_solid.7z | 11.00 | libarchive-wasm | 4 | 15.50 | 429.9 | 430.8 | 36 | 22471 | 190 | 254 |
| rar5_multiple_files_solid.rar | 0.00 | anyzip | 4 | 0.02 | 0.2 | 0.5 | 88 | 2 | 95 | 97 |
| rar5_multiple_files_solid.rar | 0.00 | anyzip-each | 4 | 0.02 | 0.3 | 0.7 | 50 | 6 | 90 | 94 |
| rar5_multiple_files_solid.rar | 0.00 | bsdtar | 4 | 0.02 | 3.4 | 6.7 | 5 | 58 | 84 | 86 |
| rar5_multiple_files_solid.rar | 0.00 | libarchive-wasm | 4 | 0.02 | 0.2 | 0.4 | 99 | 6 | 92 | 94 |
| rar_unicode.rar | 0.00 | anyzip | 3 | 0.00 | 0.4 | 2.4 | 0 | 6 | 113 | 113 |
| rar_unicode.rar | 0.00 | anyzip-each | 3 | 0.00 | 0.5 | 0.8 | 0 | 10 | 95 | 111 |
| rar_unicode.rar | 0.00 | bsdtar | 3 | 0.00 | 3.3 | 4.8 | 0 | 59 | 85 | 88 |
| rar_unicode.rar | 0.00 | libarchive-wasm | 3 | 0.00 | 0.1 | 0.2 | 0 | 3 | 89 | 91 |

## Bun

| archive | size | impl | files | out MB | p50 ms | p99 ms | MB/s | burst ms | RSS MB | burst RSS MB |
|---|---:|---|---:|---:|---:|---:|---:|---:|---:|---:|
| s2_windows.zip | 0.21 | anyzip | 2 | 0.22 | 0.6 | 1.5 | 387 | 7 | 54 | 108 |
| s2_windows.zip | 0.21 | anyzip-each | 2 | 0.22 | 0.8 | 1.7 | 291 | 16 | 50 | 115 |
| s2_windows.zip | 0.21 | bsdtar | 2 | 0.22 | 3.8 | 6.8 | 59 | 32 | 44 | 48 |
| s2_windows.zip | 0.21 | libarchive-wasm | 2 | 0.22 | 0.8 | 1.5 | 287 | 36 | 73 | 79 |
| s2_windows.zip | 0.21 | fflate | 2 | 0.22 | 4.6 | 6.1 | 49 | 231 | 53 | 52 |
| m8_windows.zip | 5.92 | anyzip | 8 | 6.81 | 14.3 | 16.7 | 475 | 74 | 103 | 267 |
| m8_windows.zip | 5.92 | anyzip-each | 8 | 6.81 | 14.7 | 16.5 | 464 | 90 | 73 | 107 |
| m8_windows.zip | 5.92 | bsdtar | 8 | 6.81 | 20.1 | 22.6 | 340 | 277 | 64 | 84 |
| m8_windows.zip | 5.92 | libarchive-wasm | 8 | 6.81 | 17.9 | 20.3 | 380 | 883 | 161 | 161 |
| m8_windows.zip | 5.92 | fflate | 8 | 6.81 | 111.6 | 283.0 | 61 | 6550 | 92 | 102 |
| l4_windows.zip | 13.68 | anyzip | 4 | 15.50 | 33.5 | 36.3 | 463 | 176 | 144 | 441 |
| l4_windows.zip | 13.68 | anyzip-each | 4 | 15.50 | 34.9 | 37.6 | 444 | 195 | 126 | 233 |
| l4_windows.zip | 13.68 | bsdtar | 4 | 15.50 | 48.5 | 67.7 | 319 | 678 | 89 | 143 |
| l4_windows.zip | 13.68 | libarchive-wasm | 4 | 15.50 | 45.9 | 51.9 | 338 | 2267 | 167 | 170 |
| l4_windows.zip | 13.68 | fflate | 4 | 15.50 | 469.1 | 544.6 | 33 | 14232 | 107 | 183 |
| s2_solid.7z | 0.21 | anyzip | 2 | 0.22 | 4.1 | 5.4 | 54 | 26 | 58 | 131 |
| s2_solid.7z | 0.21 | anyzip-each | 2 | 0.22 | 4.4 | 6.8 | 50 | 36 | 50 | 118 |
| s2_solid.7z | 0.21 | bsdtar | 2 | 0.22 | 8.7 | 11.6 | 26 | 48 | 43 | 51 |
| s2_solid.7z | 0.21 | libarchive-wasm | 2 | 0.22 | 9.6 | 11.2 | 23 | 469 | 63 | 66 |
| copied_a0.7z | 3.32 | anyzip | 2 | 3.95 | 31.5 | 36.0 | 125 | 519 | 88 | 555 |
| copied_a0.7z | 3.32 | anyzip-each | 2 | 3.95 | 32.5 | 56.9 | 122 | 558 | 89 | 553 |
| copied_a0.7z | 3.32 | bsdtar | 2 | 3.95 | 76.5 | 164.0 | 52 | 405 | 48 | 94 |
| copied_a0.7z | 3.32 | libarchive-wasm | 2 | 3.95 | 141.3 | 147.9 | 28 | 7114 | 88 | 134 |
| m8_solid.7z | 4.93 | anyzip | 8 | 6.81 | 90.4 | 95.5 | 75 | 448 | 91 | 355 |
| m8_solid.7z | 4.93 | anyzip-each | 8 | 6.81 | 91.0 | 96.4 | 75 | 505 | 67 | 263 |
| m8_solid.7z | 4.93 | bsdtar | 8 | 6.81 | 100.5 | 104.4 | 68 | 560 | 48 | 85 |
| m8_solid.7z | 4.93 | libarchive-wasm | 8 | 6.81 | 195.2 | 197.1 | 35 | 9783 | 123 | 143 |
| l4_solid.7z | 11.00 | anyzip | 4 | 15.50 | 228.1 | 242.8 | 68 | 1313 | 190 | 663 |
| l4_solid.7z | 11.00 | anyzip-each | 4 | 15.50 | 235.2 | 290.4 | 66 | 1693 | 83 | 496 |
| l4_solid.7z | 11.00 | bsdtar | 4 | 15.50 | 319.2 | 350.9 | 49 | 1522 | 89 | 161 |
| l4_solid.7z | 11.00 | libarchive-wasm | 4 | 15.50 | 483.9 | 508.5 | 32 | 24562 | 176 | 241 |
| rar5_multiple_files_solid.rar | 0.00 | anyzip | 4 | 0.02 | 0.2 | 0.7 | 71 | 4 | 42 | 63 |
| rar5_multiple_files_solid.rar | 0.00 | anyzip-each | 4 | 0.02 | 0.3 | 0.6 | 48 | 8 | 40 | 67 |
| rar5_multiple_files_solid.rar | 0.00 | bsdtar | 4 | 0.02 | 3.2 | 4.6 | 5 | 28 | 36 | 42 |
| rar5_multiple_files_solid.rar | 0.00 | libarchive-wasm | 4 | 0.02 | 0.2 | 0.5 | 84 | 7 | 60 | 61 |
| rar_unicode.rar | 0.00 | anyzip | 3 | 0.00 | 0.3 | 1.8 | 0 | 26 | 50 | 137 |
| rar_unicode.rar | 0.00 | anyzip-each | 3 | 0.00 | 0.5 | 1.4 | 0 | 48 | 46 | 163 |
| rar_unicode.rar | 0.00 | bsdtar | 3 | 0.00 | 3.4 | 4.8 | 0 | 26 | 36 | 41 |
| rar_unicode.rar | 0.00 | libarchive-wasm | 3 | 0.00 | 0.1 | 0.5 | 0 | 4 | 58 | 60 |

## Notes

- **7z.** Blocks that are one LZMA2 stream with a single chunk (7-Zip's default output for files
  under 4 × dictionary) go through liblzma; multi-chunk streams (7-Zip multi-threaded with a small
  dictionary, like `copied_a0`) are decoded in parallel by `sevenz-rust2`, up to 4 threads.
- **Memory.** With glibc malloc, a burst left 250–540 MB resident under Bun (rising every burst)
  and peaked at 1.4 GB; mimalloc with immediate purging brought that down dramatically, and
  `extractEach` (`anyzip-each`) cuts peak burst RSS further (e.g. 233 MB vs 441 MB on
  `l4_windows.zip` under Bun, 202 MB vs 299 MB under Node) because only one entry per active
  archive is alive at a time.
- **Burst vs bsdtar** on 7z under Node: 50 processes on 16 hardware threads beat Node's default
  4-thread libuv pool (under Bun's wider default pool, `anyzip` matches or beats 50 `bsdtar`
  processes too). On a 1–2 vCPU function that multi-process advantage disappears; raise
  `UV_THREADPOOL_SIZE` on Node where cores allow.
- Rust-only (`target/release/examples/dump`): l4_solid.7z 225–235 ms, m8_solid.7z 88–91 ms,
  l4_windows.zip ~31 ms, 20 real portal 7z archives 471 ms in total (bsdtar: ~990 ms).
