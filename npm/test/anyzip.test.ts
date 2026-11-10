// Runs under both `bun test` and `node --test` (Node >= 22.18 strips the types).
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { createHash } from 'node:crypto'
import { readFileSync, existsSync, readdirSync } from 'node:fs'
import { join } from 'node:path'
import { AnyzipError, detect, extract, extractEach, list } from '../anyzip.js'
import type { Entry } from '../anyzip.js'

const FIXTURES = join(import.meta.dirname, '..', '..', 'fixtures')
const fixture = (name: string): Buffer => readFileSync(join(FIXTURES, name))
const md5 = (b: Uint8Array): string => createHash('md5').update(b).digest('hex')
const PDF = Buffer.concat([
  Buffer.from('%PDF-1.4\n'),
  Buffer.from('edital de licitacao '.repeat(2000)),
  Buffer.from('\n%%EOF\n'),
])

async function rejectsWith(promise: Promise<unknown>, code: string): Promise<void> {
  await assert.rejects(promise, (e: unknown) => {
    assert.ok(e instanceof AnyzipError, `expected AnyzipError, got ${String(e)}`)
    assert.equal(e.code, code, e.message)
    assert.equal(e.name, 'AnyzipError')
    return true
  })
}

test('detect by signature', () => {
  assert.equal(detect(fixture('generated/zip_utf8_flag.zip')), 'zip')
  assert.equal(detect(fixture('generated/7z_copy.7z')), '7z')
  assert.equal(detect(fixture('libarchive/rar_unicode.rar')), 'rar')
  assert.equal(detect(fixture('libarchive/rar5_stored.rar')), 'rar')
  assert.equal(detect(Buffer.from('%PDF-1.7')), null)
  assert.equal(detect(new Uint8Array()), null)
  assert.equal(detect(new Uint8Array([0x50, 0x4b, 0x03, 0x04]).buffer), 'zip')
})

test('zip: CP850 names without the UTF-8 flag, Buffers with the right bytes', async () => {
  const out = await extract(fixture('generated/zip_cp850_noflag.zip'))
  assert.equal(out.format, 'zip')
  assert.equal(out.recovered, false)
  assert.deepEqual(
    out.entries.map((e) => e.path),
    ['edição_ação/anexo único.pdf', 'Relação de Itens.txt'],
  )
  const [pdf, txt] = out.entries as [Entry, Entry]
  assert.ok(Buffer.isBuffer(pdf.data))
  assert.equal(md5(pdf.data), md5(PDF))
  assert.equal(txt.data.toString(), 'itens\n')
})

test('7z: solid LZMA2, UTF-16 names, symlink skipped, empty file kept', async () => {
  const out = await extract(fixture('generated/7z_utf8_solid.7z'))
  assert.equal(out.format, '7z')
  const byPath = new Map(out.entries.map((e) => [e.path, e.data]))
  assert.equal(md5(byPath.get('licitação/edital nº 12-2025.pdf') ?? Buffer.alloc(0)), md5(PDF))
  assert.equal(byPath.get('licitação/anexos/vazio.txt')?.length, 0)
  assert.equal(byPath.get('日本語.txt')?.toString(), 'テスト')
  assert.deepEqual(out.skipped, [{ path: 'link', reason: 'symlink' }])
})

test('rar4: Unicode and legacy names, symlink skipped', async () => {
  const out = await extract(fixture('libarchive/rar_unicode.rar'))
  assert.equal(out.format, 'rar')
  assert.deepEqual(out.entries.map((e) => e.path).sort(), [
    'abcdefghijklmnopqrsテスト.txt',
    '表だよ/新しいフォルダ/新規テキスト ドキュメント.txt',
    '表だよ/漢字長いファイル名long-filename-in-漢字.txt',
  ])
  assert.deepEqual(out.skipped, [{ path: '表だよ/ファイル', reason: 'symlink' }])
})

test('rar5: solid archive, hard link and symlink skipped', async () => {
  const solid = await extract(fixture('libarchive/rar5_multiple_files_solid.rar'))
  assert.equal(solid.entries.length, 4)
  const unicode = await extract(fixture('libarchive/rar5_unicode.rar'))
  assert.deepEqual(unicode.entries.map((e) => e.path), ['👋🌎.txt'])
  assert.deepEqual(unicode.skipped.map((s) => s.reason).sort(), ['notAFile', 'symlink'])
})

test('unsafe paths are skipped, never returned', async () => {
  const out = await extract(fixture('generated/zip_traversal.zip'))
  assert.deepEqual(out.entries.map((e) => e.path).sort(), ['dir/sub/back.txt', 'dot/double.txt', 'ok.txt'])
  assert.equal(out.skipped.length, 7)
  assert.ok(out.skipped.every((s) => s.reason === 'unsafePath'))
  for (const e of out.entries) {
    assert.ok(!e.path.startsWith('/') && !e.path.split('/').includes('..'))
  }
})

test('encrypted entries are skipped; encrypted headers are an error', async () => {
  const zip = await extract(fixture('generated/zip_zipcrypto_partial.zip'))
  assert.deepEqual(zip.entries.map((e) => e.path), ['aberto.txt'])
  assert.deepEqual(zip.skipped, [{ path: 'secreto.pdf', reason: 'encrypted' }])
  const sevenz = await extract(fixture('generated/7z_encrypted.7z'))
  assert.ok(sevenz.skipped.length >= 3 && sevenz.skipped.every((s) => s.reason === 'encrypted'))
  await rejectsWith(extract(fixture('generated/7z_encrypted_header.7z')), 'encrypted')
  await rejectsWith(extract(fixture('libarchive/rar_encryption_header.rar')), 'encrypted')
})

test('error codes', async () => {
  await rejectsWith(extract(Buffer.from('not an archive at all')), 'unsupported')
  await rejectsWith(extract(fixture('generated/zip_unknown_method.zip')), 'unsupported')
  await rejectsWith(extract(fixture('generated/zip_bad_crc.zip')), 'corrupt')
  await rejectsWith(extract(fixture('generated/7z_truncated.7z')), 'corrupt')
  await rejectsWith(extract(fixture('libarchive/rar5_truncated_huff.rar')), 'corrupt')
  await rejectsWith(extract(fixture('libarchive/rar_multivolume.part0001.rar')), 'multiVolume')
  await rejectsWith(extract(fixture('generated/zip_spanned.zip')), 'multiVolume')
  await rejectsWith(extract(fixture('generated/bomb_overlap.zip')), 'tooLarge')
  await rejectsWith(extract(fixture('generated/7z_dict_4gib.7z')), 'tooLarge')
})

test('truncated zip: complete entries recovered and flagged', async () => {
  const out = await extract(fixture('generated/zip_truncated_cd.zip'))
  assert.equal(out.recovered, true)
  assert.equal(out.entries.length, 3)
  const listed = await list(fixture('generated/zip_truncated_cd.zip'))
  assert.equal(listed.recovered, true)
  assert.equal(listed.entries.length, 3)
})

test('truncated rar and single multi-volume rar parts recover complete entries', async () => {
  const trunc = await extract(fixture('generated/rar5_truncated_data.rar'))
  assert.equal(trunc.recovered, true)
  assert.deepEqual(
    trunc.entries.map((e) => e.path),
    ['completo_0.pdf', 'completo_1.txt'],
  )
  assert.equal(md5(trunc.entries[0]?.data ?? Buffer.alloc(0)), md5(PDF))

  const vol2 = await extract(fixture('generated/rar5_volume_part2.rar'))
  assert.equal(vol2.recovered, true)
  assert.deepEqual(
    vol2.entries.map((e) => e.path),
    ['meio/anexo_a.pdf', 'meio/orcamento.txt'],
  )
  assert.equal(md5(vol2.entries[0]?.data ?? Buffer.alloc(0)), md5(PDF))

  const solidPart1 = await extract(fixture('libarchive/rar5_multiarchive_solid.part01.rar'))
  assert.equal(solidPart1.recovered, true)
  assert.equal(solidPart1.entries.length, 8)
  await rejectsWith(extract(fixture('libarchive/rar5_multiarchive_solid.part02.rar')), 'multiVolume')

  const rar4Part4 = await extract(fixture('libarchive/rar_multivolume.part0004.rar'))
  assert.equal(rar4Part4.recovered, true)
  assert.equal(rar4Part4.entries.length, 2)
})

test('limits are honoured and validated', async () => {
  await rejectsWith(extract(fixture('generated/zip_traversal.zip'), { maxEntries: 2 }), 'tooLarge')
  await rejectsWith(extract(fixture('generated/zip_cp850_noflag.zip'), { maxEntryBytes: 1000 }), 'tooLarge')
  await rejectsWith(extract(fixture('generated/7z_utf8_solid.7z'), { maxMemoryBytes: 1024 }), 'tooLarge')
  await rejectsWith(extract(fixture('libarchive/rar5_compressed.rar'), { maxMemoryBytes: 1024 }), 'tooLarge')
  await assert.rejects(extract(fixture('generated/zip_empty.zip'), { maxEntries: -1 }), /non-negative/)
  await assert.rejects(extract(fixture('generated/zip_empty.zip'), { maxRatio: 1.5 }), /non-negative/)
})

test('list reads headers only', async () => {
  const out = await list(fixture('generated/zip_cp850_noflag.zip'))
  assert.deepEqual(out.entries, [
    { path: 'edição_ação/anexo único.pdf', size: PDF.length },
    { path: 'Relação de Itens.txt', size: 6 },
  ])
  const rar = await list(fixture('libarchive/rar5_multiple_files_solid.rar'))
  assert.equal(rar.entries.length, 4)
})

test('nested archives come back as plain entries', async () => {
  const out = await extract(fixture('generated/zip_nested_archives.zip'))
  assert.deepEqual(out.entries.map((e) => e.path).sort(), ['inner.7z', 'inner.zip'])
  const inner = out.entries.find((e) => e.path === 'inner.zip')
  assert.ok(inner)
  const again = await extract(inner.data)
  assert.equal(again.entries.length, 2)
})

test('extractEach: entries in order, backpressure, summary', async () => {
  const seen: string[] = []
  let active = 0
  const summary = await extractEach(fixture('libarchive/rar5_multiple_files_solid.rar'), async (e) => {
    active++
    assert.equal(active, 1, 'onEntry must not overlap')
    seen.push(e.path)
    await new Promise((r) => setTimeout(r, 5))
    active--
  })
  assert.equal(summary.format, 'rar')
  assert.equal(summary.entries, 4)
  assert.equal(seen.length, 4)
})

test('extractEach: a throwing callback stops the extraction', async () => {
  let calls = 0
  await assert.rejects(
    extractEach(fixture('generated/7z_nonsolid.7z'), () => {
      calls++
      throw new Error('stop here')
    }),
    /stop here/,
  )
  assert.equal(calls, 1)
})

test('extractEach: errors surface as AnyzipError', async () => {
  await rejectsWith(extractEach(fixture('generated/zip_bad_crc.zip'), () => {}), 'corrupt')
})

test('concurrent rar extractions on the thread pool agree', async () => {
  const bytes = fixture('libarchive/rar_unicode.rar')
  const results = await Promise.all(Array.from({ length: 32 }, () => extract(bytes)))
  const first = JSON.stringify(results[0]?.entries.map((e) => [e.path, md5(e.data)]))
  for (const r of results) {
    assert.equal(JSON.stringify(r.entries.map((e) => [e.path, md5(e.data)])), first)
  }
  const errors = await Promise.allSettled(
    Array.from({ length: 16 }, () => extract(fixture('libarchive/rar5_truncated_huff.rar'))),
  )
  assert.ok(errors.every((r) => r.status === 'rejected' && (r.reason as AnyzipError).code === 'corrupt'))
})

test('100 parallel extractions do not block the event loop', async () => {
  const realDir = join(FIXTURES, 'real')
  const real = existsSync(realDir) ? readdirSync(realDir).find((n) => n === 'm3_solid.7z') : undefined
  const bytes = real ? readFileSync(join(realDir, real)) : fixture('generated/7z_nonsolid.7z')
  let maxDelay = 0
  let last = performance.now()
  const timer = setInterval(() => {
    const now = performance.now()
    maxDelay = Math.max(maxDelay, now - last - 10)
    last = now
  }, 10)
  try {
    const results = await Promise.all(Array.from({ length: 100 }, () => extract(bytes)))
    assert.ok(results.every((r) => r.entries.length > 0))
  } finally {
    clearInterval(timer)
  }
  assert.ok(maxDelay < 50, `event loop blocked for ${maxDelay.toFixed(1)} ms`)
})

test('real consultation corpus (git-ignored when present)', { timeout: 120_000 }, async () => {
  const corpusDir = join(FIXTURES, 'real', 'consultation-7d')
  if (!existsSync(corpusDir)) return
  const names = readdirSync(corpusDir).sort()
  assert.ok(names.length > 0)
  const batchSize = 4
  for (let i = 0; i < names.length; i += batchSize) {
    const batch = names.slice(i, i + batchSize)
    await Promise.all(
      batch.map(async (name) => {
        const out = await extract(readFileSync(join(corpusDir, name)))
        assert.ok(out.entries.length > 0, `${name}: expected at least one entry`)
        for (const entry of out.entries) {
          assert.ok(!entry.path.includes('\uFFFD'), `${name}: replacement char in ${entry.path}`)
        }
      }),
    )
  }
})
