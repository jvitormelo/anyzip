// anyzip vs bsdtar (spawned child process), libarchive-wasm and fflate (zip only).
//
//   bun bench.ts [FILE...]        node bench.ts [FILE...]
//
// Each (implementation, archive) pair runs in its own child process so the peak RSS belongs to it
// alone. Times cover bytes-in-memory to every file's bytes-in-memory; bsdtar also pays for the
// temp file, the process and reading the files back.
import { spawn, spawnSync } from 'node:child_process'
import { existsSync, mkdtempSync, readFileSync, readdirSync, rmSync, statSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { basename, join } from 'node:path'

const HERE = import.meta.dirname
const FIXTURES = join(HERE, '..', 'fixtures')
const DEFAULT_FILES = [
  'real/s2_windows.zip',
  'real/m8_windows.zip',
  'real/l4_windows.zip',
  'real/s2_solid.7z',
  'real/copied_a0.7z',
  'real/m8_solid.7z',
  'real/l4_solid.7z',
  'libarchive/rar5_multiple_files_solid.rar',
  'libarchive/rar_unicode.rar',
]
const MIN_RUNS = 5
const MAX_RUNS = 50
const TIME_BUDGET_MS = 1500
const BURST = 50

type Impl = 'anyzip' | 'bsdtar' | 'libarchive-wasm' | 'fflate'
type Extract = (bytes: Uint8Array) => Promise<{ files: number; bytes: number }>
type Result = {
  impl: Impl
  file: string
  ok: boolean
  error?: string
  files?: number
  out?: number
  p50?: number
  p99?: number
  burst?: number
  rssKb?: number
  burstRssKb?: number
}

async function load(impl: Impl): Promise<Extract> {
  switch (impl) {
    case 'anyzip': {
      const { extract } = await import('anyzip')
      return async (bytes) => {
        const r = await extract(bytes)
        return { files: r.entries.length, bytes: r.entries.reduce((n, e) => n + e.data.length, 0) }
      }
    }
    case 'fflate': {
      const { unzipSync } = await import('fflate')
      return async (bytes) => {
        const files = Object.entries(unzipSync(bytes)).filter(([name]) => !name.endsWith('/'))
        return { files: files.length, bytes: files.reduce((n, [, d]) => n + d.length, 0) }
      }
    }
    case 'libarchive-wasm': {
      const { libarchiveWasm, ArchiveReader } = await import('libarchive-wasm')
      const mod = await libarchiveWasm()
      return async (bytes) => {
        const reader = new ArchiveReader(mod, new Int8Array(bytes.buffer, bytes.byteOffset, bytes.byteLength))
        let files = 0
        let total = 0
        try {
          for (const entry of reader.entries()) {
            if (entry.getFiletype() !== 'File') continue
            const data = entry.readData()
            files++
            total += data?.length ?? 0
          }
        } finally {
          reader.free()
        }
        return { files, bytes: total }
      }
    }
    case 'bsdtar':
      return async (bytes) => {
        const dir = mkdtempSync(join(tmpdir(), 'anyzip-bench-'))
        try {
          const archive = join(dir, 'in')
          writeFileSync(archive, bytes)
          await new Promise<void>((resolve, reject) => {
            const p = spawn('bsdtar', ['-xf', archive, '-C', dir, '--no-same-owner', '--options', 'zip:hdrcharset=CP850'], {
              stdio: ['ignore', 'ignore', 'pipe'],
            })
            let err = ''
            p.stderr.on('data', (d) => (err += d))
            p.on('error', reject)
            p.on('close', (code) => (code === 0 ? resolve() : reject(new Error(`bsdtar exit ${code}: ${err.trim()}`))))
          })
          rmSync(archive)
          let files = 0
          let total = 0
          for (const name of readdirSync(dir, { recursive: true, withFileTypes: true })) {
            if (!name.isFile()) continue
            files++
            total += readFileSync(join(name.parentPath, name.name)).length
          }
          return { files, bytes: total }
        } finally {
          rmSync(dir, { recursive: true, force: true })
        }
      }
  }
}

function peakRssKb(): number | undefined {
  try {
    const m = /VmHWM:\s+(\d+)/.exec(readFileSync('/proc/self/status', 'utf8'))
    return m ? Number(m[1]) : undefined
  } catch {
    return undefined
  }
}

const quantile = (sorted: number[], q: number): number =>
  sorted[Math.min(sorted.length - 1, Math.floor(q * sorted.length))] ?? Number.NaN

async function child(impl: Impl, file: string): Promise<Result> {
  const bytes = new Uint8Array(readFileSync(file))
  const run = await load(impl)
  const first = await run(bytes)
  const times: number[] = []
  const start = performance.now()
  while (times.length < MAX_RUNS && (times.length < MIN_RUNS || performance.now() - start < TIME_BUDGET_MS)) {
    const t = performance.now()
    await run(bytes)
    times.push(performance.now() - t)
  }
  times.sort((a, b) => a - b)
  const rssKb = peakRssKb()
  // Burst: BURST extractions started at once, like the serverless load test. Synchronous engines
  // (libarchive-wasm, fflate) run them back to back on the JS thread.
  const t = performance.now()
  await Promise.all(Array.from({ length: BURST }, () => run(bytes)))
  const burst = performance.now() - t
  return {
    impl,
    file,
    ok: true,
    files: first.files,
    out: first.bytes,
    p50: quantile(times, 0.5),
    p99: quantile(times, 0.99),
    burst,
    rssKb,
    burstRssKb: peakRssKb(),
  }
}

function supports(impl: Impl, file: string): boolean {
  if (impl === 'fflate') return file.endsWith('.zip')
  if (impl === 'bsdtar') return spawnSync('bsdtar', ['--version']).status === 0
  return true
}

function parent(files: string[]): void {
  const impls: Impl[] = ['anyzip', 'bsdtar', 'libarchive-wasm', 'fflate']
  const runtime = 'Bun' in globalThis ? `bun ${process.versions.bun}` : `node ${process.versions.node}`
  console.log(`runtime: ${runtime}; burst = ${BURST} concurrent extractions\n`)
  console.log('| archive | size | impl | files | out MB | p50 ms | p99 ms | MB/s | burst ms | RSS MB | burst RSS MB |')
  console.log('|---|---:|---|---:|---:|---:|---:|---:|---:|---:|---:|')
  for (const file of files) {
    const size = statSync(file).size
    let reference: Result | undefined
    for (const impl of impls) {
      if (!supports(impl, file)) continue
      const p = spawnSync(process.execPath, [import.meta.filename, '--child', impl, file], {
        encoding: 'utf8',
        maxBuffer: 1 << 20,
        timeout: 300_000,
      })
      const line = p.stdout.trim().split('\n').at(-1) ?? ''
      let r: Result
      try {
        r = JSON.parse(line) as Result
      } catch {
        r = { impl, file, ok: false, error: (p.stderr || `exit ${p.status}`).trim().split('\n')[0] }
      }
      if (impl === 'anyzip') reference = r
      const name = basename(file)
      if (!r.ok) {
        console.log(`| ${name} | ${mb(size)} | ${impl} | error: ${r.error?.slice(0, 60)} | | | | | | | |`)
        continue
      }
      const mismatch = reference?.ok && (r.files !== reference.files || r.out !== reference.out) ? ' ≠' : ''
      console.log(
        `| ${name} | ${mb(size)} | ${impl} | ${r.files}${mismatch} | ${mb(r.out ?? 0)} | ${r.p50?.toFixed(1)} | ` +
          `${r.p99?.toFixed(1)} | ${((r.out ?? 0) / 1e6 / ((r.p50 ?? 1) / 1e3)).toFixed(0)} | ` +
          `${r.burst?.toFixed(0)} | ${mbKb(r.rssKb)} | ${mbKb(r.burstRssKb)} |`,
      )
    }
  }
}

const mb = (n: number): string => (n / 1e6).toFixed(2)
const mbKb = (kb: number | undefined): string => (kb ? (kb / 1024).toFixed(0) : '?')

const args = process.argv.slice(2)
if (args[0] === '--child') {
  const [, impl, file] = args as [string, Impl, string]
  try {
    console.log(JSON.stringify(await child(impl, file)))
  } catch (e) {
    console.log(JSON.stringify({ impl, file, ok: false, error: String((e as Error).message ?? e) }))
  }
} else {
  const files = (args.length ? args : DEFAULT_FILES.map((f) => join(FIXTURES, f))).filter((f) => {
    if (existsSync(f)) return true
    console.error(`skipping missing ${f} (run scripts/make-real-corpus.py for fixtures/real)`)
    return false
  })
  parent(files)
}
