# anyzip

[![npm](https://img.shields.io/npm/v/@jvitormelo/anyzip)](https://www.npmjs.com/package/@jvitormelo/anyzip)
[![ci](https://github.com/jvitormelo/anyzip/actions/workflows/ci.yml/badge.svg)](https://github.com/jvitormelo/anyzip/actions/workflows/ci.yml)

Extracts zip, 7z and rar from bytes in memory, for Node and Bun (N-API) and Rust. Built for
serverless pipelines (such as Vercel Functions on Node and Bun) without spawning system binaries or
touching disk.

- Format detected from content, never from the file name.
- Off the event loop: work runs on the libuv thread pool.
- Safe against hostile archives:
  - limits count real decoded bytes;
  - `..`, absolute paths, symlinks and hard links are never returned;
  - engine panics become typed errors.
- Correct names:
  - CP850 for Windows zips without the UTF-8 flag;
  - the 0x7075 Unicode path extra field;
  - UTF-16 names in 7z;
  - Unicode and legacy (RAR4) names in rar.

```sh
npm install @jvitormelo/anyzip
```

```ts
import { extract, extractEach, list, detect, AnyzipError } from '@jvitormelo/anyzip'

const { format, entries, skipped, recovered } = await extract(bytes) // Uint8Array | ArrayBuffer
for (const { path, data } of entries) upload(path, data) // data: Buffer, no copy

// One entry at a time, with backpressure (the next waits for your promise).
await extractEach(bytes, async (e) => upload(e.path, e.data), { maxTotalBytes: 512 << 20 })

try {
  await extract(bytes)
} catch (e) {
  if (e instanceof AnyzipError && e.code === 'encrypted') { /* ... */ }
}
```

Types: [`npm/anyzip.d.ts`](npm/anyzip.d.ts). Rust: `anyzip::extract(&bytes, &Limits::default())`.

## Contract

**Errors** (`AnyzipError.code`, or `anyzip::ErrorCode`):

| code | when |
|---|---|
| `unsupported` | Not zip/7z/rar, or a compression method we don't decode. |
| `corrupt` | Bad structure, bad CRC, truncated data with no complete entry, or an engine panic. Truncated zip/rar archives that still hold complete entries are *recovered* instead (`recovered: true`). |
| `encrypted` | Encrypted headers (7z, rar), so even the names are hidden. Encrypted *entries* are skipped instead. |
| `multiVolume` | Spanned zip, or a `.partN.rar` volume that contains no complete self-contained entry (complete entries inside a rar volume are recovered with `recovered: true`). |
| `tooLarge` | A limit was hit. |
| `io` | Out of memory, or the rar temp file could not be written. |

**Skipped entries** (`skipped[]`, never an error):

| reason | entries |
|---|---|
| `unsafePath` | `..`, absolute paths, drive letters, control characters. |
| `symlink` | Symbolic links. |
| `encrypted` | Encrypted entries. |
| `notAFile` | Hard links, devices, FIFOs. |

Directories are dropped silently.

**Limits** (all optional):

| limit | default | what it bounds |
|---|---|---|
| `maxEntries` | 10 000 | Headers read. |
| `maxEntryBytes` | 1 GiB | Decoded bytes of one entry. |
| `maxTotalBytes` | 2 GiB | Decoded bytes of the whole archive. |
| `maxRatio` | 1000 | Decoded bytes ÷ archive size, checked once output passes 1 MiB. |
| `maxMemoryBytes` | 1 GiB | 7z/rar dictionary a header may ask for. |

Limits are checked every 4 MiB of output. A bomb fails fast: the overlapping-entry bomb hits
`tooLarge` in 8 ms at 40 MB RSS. With the defaults, a single 2 GiB entry in a small archive still
decodes up to 1 GiB before `tooLarge`. Lower `maxEntryBytes`/`maxRatio` on small serverless
functions.

**Operational notes**

- **Do not mutate or transfer the input** until the promise settles. It is read in place, not
  copied.
- **Concurrency:** one extraction uses one libuv thread (7z multi-chunk blocks: up to 4 decoder
  threads). `UV_THREADPOOL_SIZE` (default 4) caps parallel extractions per process.
- **rar** goes through a memfd (Linux) or a temp file, because UnRAR only opens paths.

**Platforms**

- `linux-x64-gnu` and `linux-arm64-gnu` (glibc ≥ 2.28; `libstdc++` linked statically) — runs out of
  the box on **Vercel Functions** (Node.js and Bun runtimes on AWS Lambda / Amazon Linux 2023,
  `glibc 2.34`), AWS Lambda (x86_64 and Graviton ARM64), Debian 11+, and Ubuntu 20.04+.
- `darwin-arm64` (macOS Apple Silicon).
- Prebuilt binaries only cover the targets above (what we run in production); Alpine (`musl`) and
  Windows are not built yet — PRs adding targets to `release.yml` are welcome.
- On Next.js (Vercel), mark the native addon as external in `next.config.ts`:
  `serverExternalPackages: ['@jvitormelo/anyzip']`.

## Nested archives

`extract` and `extractEach` unpack one level: an archive inside an archive comes back as a regular
`Entry`. Formats such as `.docx`, `.xlsx`, `.pptx`, `.odt`, `.epub` and `.jar` are zip files
(`PK\x03\x04`), so recursing on `detect()` alone would unpack documents into their internal XML;
nested bombs (`42.zip`) also need a depth cap and a budget shared across levels.

To flatten nested `zip`/`7z`/`rar` archives into leaf files only:

```ts
import { extname } from 'node:path'
import { AnyzipError, detect, extractEach, type Entry } from '@jvitormelo/anyzip'

const ZIP_CONTAINERS = new Set([
  '.docx', '.xlsx', '.pptx', '.docm', '.xlsm', '.pptm',
  '.odt', '.ods', '.odp', '.epub', '.jar', '.apk', '.whl',
])

const isNestedArchive = (path: string, data: Buffer): boolean =>
  !ZIP_CONTAINERS.has(extname(path).toLowerCase()) && detect(data) !== null

export async function extractFlat(
  bytes: Uint8Array | ArrayBuffer,
  { maxDepth = 2, maxEntries = 10_000, maxTotalBytes = 512 << 20, maxRatio = 1000 } = {},
): Promise<Entry[]> {
  const files: Entry[] = []
  let remainingEntries = maxEntries
  let remainingBytes = maxTotalBytes

  async function walk(buf: Uint8Array | ArrayBuffer, prefix: string, depth: number): Promise<void> {
    await extractEach(
      buf,
      async (entry) => {
        remainingEntries -= 1
        remainingBytes -= entry.data.length
        if (remainingEntries < 0 || remainingBytes < 0) {
          throw new AnyzipError('tooLarge', 'nested archive limit exceeded')
        }
        const path = prefix ? `${prefix}/${entry.path}` : entry.path
        if (depth < maxDepth && isNestedArchive(entry.path, entry.data)) {
          await walk(entry.data, path, depth + 1)
          return
        }
        files.push({ path, data: entry.data })
      },
      {
        maxEntries: remainingEntries,
        maxEntryBytes: remainingBytes,
        maxTotalBytes: remainingBytes,
        maxRatio,
      },
    )
  }

  await walk(bytes, '', 0)
  return files
}
```

## Performance

[`bench/BASELINE.md`](bench/BASELINE.md):

- zip: about 1.5× faster than bsdtar and 3–7× faster than fflate.
- 7z: about 2× faster than libarchive-wasm.
- 7z vs bsdtar: on par for 7-Zip default output, and about 2× faster on multi-threaded 7-Zip
  output (decoded in parallel).
- rar: about 15× faster than spawning bsdtar.

## Development

```sh
cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace
cd npm && bun install && bun run build && bun test && node --test test/*.test.ts
```

**Tests**

- **Golden:** `crates/anyzip/tests/golden.rs` runs every fixture against `fixtures/expected/`. To
  accept a change after review, run `UPDATE_EXPECTED=1 cargo test -p anyzip --test golden`.
- **Differential:**
  - Run `python3 -I scripts/diff-bsdtar.py FILE...` to compare anyzip with bsdtar.
  - `fixtures/diffs.json` lists every expected difference with its reason.
  - Run `scripts/make-real-corpus.py` to build `fixtures/real/` (git-ignored) from real editais.
- **Fuzz** (`fuzz/`, nightly):
  - `cargo +nightly fuzz run fuzz_zip corpus/fuzz_zip ../fixtures/generated ../fixtures/libarchive`.
  - The targets are `fuzz_detect`, `fuzz_zip`, `fuzz_sevenz` and `fuzz_rar`.
- **Fixtures:**
  - `scripts/fetch-libarchive-fixtures.sh` fetches the libarchive fixtures (pinned commit, BSD-2).
  - `scripts/make-fixtures.py` writes `fixtures/generated`.
- **UnRAR updates:** run `scripts/update-unrar.sh VERSION SHA256`.

## Why it is built this way

**Workspace**

- **Core, `-sys` and binding are separate crates.**
  - The core (`#![forbid(unsafe_code)]`) is tested, fuzzed and reusable without N-API.
  - `unrar-sys` is the only place with C++ FFI.
  - The binding only converts types.

**rar**

- **Own `unrar-sys`, vendoring UnRAR 7.3.2.** We need:
  - `RedirType` to identify symlinks, junctions and hard links reliably;
  - a current UnRAR release;
  - control of the C++ build.
- **`ErrHandler` is made `thread_local` at build time.** UnRAR keeps it as a process global. With
  concurrent extractions, one archive's error would leak into another. The build asserts the
  pattern, so an update that changes it fails loudly.
- **No `RAR_SMP`.** UnRAR's worker threads would throw C++ exceptions outside the DLL's catch
  blocks.
- **Data comes through `UCM_PROCESSDATA` in test mode.** Nothing is written to disk.
- **`MergeArchive` is disabled at build time.** UnRAR never derives or opens a next-volume path on
  the filesystem; complete entries inside a single `.partN.rar` volume (or before a truncated tail)
  are extracted with `recovered: true`.
- **Legacy RAR4 names (no Unicode field) are recovered from UnRAR's private-use mapping.** The
  bytes are then decoded as UTF-8 or else CP850, avoiding replacement characters on Unix UTF-8
  names stored without the Unicode flag.

**zip**

- **Name decoding:**
  - The UTF-8 flag (bit 11) or a valid 0x7075 field means UTF-8.
  - Unix/macOS creators with valid UTF-8 are read as UTF-8.
  - Everything else is CP850, what Windows Explorer writes in pt-BR.
- **Truncated zips are recovered.** A truncated download still yields its complete entries,
  flagged with `recovered: true`.

**7z**

- **liblzma for single-stream LZMA2 blocks.** It is about 1.5× faster than `lzma-rust2` and
  covers 7-Zip's default output.
- **`sevenz-rust2` decodes multi-chunk streams in parallel.** It also handles everything else:
  BCJ, LZMA1, PPMd and the rest.
- **The CRC is checked either way.**

**Limits and memory**

- **Limits count decoded bytes, not header claims.** Declared sizes are only an allocation hint,
  capped at 64 MiB and by the remaining budget.
- **mimalloc with immediate purge in the binding.**
  - Entry buffers are large and freed on the JS thread.
  - glibc then kept hundreds of MB resident after each burst.

**Node binding**

- **Errors resolve as `{ ok: false, code }` and the JS wrapper throws `AnyzipError`.** napi-rs
  rejections cannot carry our own `code`.
- **`extractEach` is pull-based** (rendezvous channel). A slow consumer holds at most one decoded
  entry.

**Behaviour that differs from bsdtar**

- **Encrypted entries are skipped.** bsdtar writes a zero-filled file of the declared size.

## License

- `anyzip`'s own code is dual-licensed under **MIT OR Apache-2.0** ([`LICENSE-MIT`](LICENSE-MIT),
  [`LICENSE-APACHE`](LICENSE-APACHE)).
- Prebuilt npm binaries and the `anyzip` Rust crate with default features embed **UnRAR** source
  from RARLAB ([`crates/unrar-sys/LICENSE-UNRAR.txt`](crates/unrar-sys/LICENSE-UNRAR.txt)). UnRAR
  is freeware: it permits commercial and open-source use and redistribution for extracting RAR
  archives free of charge, but it is **not an OSI-approved license** (it prohibits using the code
  to recreate the RAR compression algorithm).
- In Rust, compiling `anyzip` with `default-features = false` disables `rar` (`unrar-sys`), leaving
  zip and 7z under `MIT OR Apache-2.0` only.
