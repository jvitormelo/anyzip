'use strict'

// Thin wrapper over the generated N-API loader (index.js): typed errors and plain result objects.
const native = require('./index.js')

class AnyzipError extends Error {
  constructor(code, message) {
    super(message)
    this.name = 'AnyzipError'
    this.code = code
  }
}

function input(bytes) {
  if (bytes instanceof Uint8Array) return bytes
  if (bytes instanceof ArrayBuffer) return new Uint8Array(bytes)
  throw new TypeError('anyzip: expected a Uint8Array, Buffer or ArrayBuffer')
}

function unwrap(outcome) {
  if (!outcome.ok) throw new AnyzipError(outcome.code, outcome.message)
  return outcome
}

function detect(bytes) {
  return native.detect(input(bytes))
}

async function list(bytes, limits) {
  const o = unwrap(await native.list(input(bytes), limits))
  return { format: o.format, entries: o.listed, skipped: o.skipped, recovered: o.recovered }
}

async function extract(bytes, limits) {
  const o = unwrap(await native.extract(input(bytes), limits))
  return { format: o.format, entries: o.entries, skipped: o.skipped, recovered: o.recovered }
}

// One entry at a time: the next entry is decoded while `onEntry` runs, and the one after waits
// until `onEntry` (or the promise it returns) finishes. Peak memory: archive + about 2 entries.
async function extractEach(bytes, onEntry, limits) {
  if (typeof onEntry !== 'function') {
    throw new TypeError('anyzip: expected onEntry to be a function')
  }
  const it = native.extractEach(input(bytes), limits)
  try {
    for (;;) {
      const o = unwrap(await it.next())
      if (o.count == null && o.entries.length === 1) {
        await onEntry(o.entries[0])
        continue
      }
      return {
        format: o.format,
        entries: o.count ?? 0,
        skipped: o.skipped ?? [],
        recovered: o.recovered ?? false,
      }
    }
  } finally {
    it.close()
  }
}

module.exports = { AnyzipError, detect, list, extract, extractEach }
