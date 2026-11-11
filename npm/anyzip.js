'use strict'

// Thin wrapper over the generated N-API loader (index.js): typed errors and plain result objects.
const { AsyncLocalStorage } = require('node:async_hooks')
const native = require('./index.js')

const eachDepth = new AsyncLocalStorage()
let activeEach = 0
const eachQueue = []

function poolLimit() {
  const n = Number.parseInt(process.env.UV_THREADPOOL_SIZE ?? '', 10)
  return n >= 1 ? Math.min(n, 1024) : 4
}

function acquireEach() {
  if (eachDepth.getStore()) return Promise.resolve(false)
  if (activeEach < poolLimit()) {
    activeEach++
    return Promise.resolve(true)
  }
  return new Promise((resolve) => eachQueue.push(resolve))
}

function releaseEach(acquired) {
  if (!acquired) return
  const next = eachQueue.shift()
  if (next) next(true)
  else activeEach--
}

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

// One entry at a time: demand-driven, bounded by UV_THREADPOOL_SIZE for top-level calls (nested
// calls inside `onEntry` reuse the outer slot without deadlocking), and `it.close()` waits for the
// worker thread to exit before the promise settles. Peak memory: archive + 1 entry.
async function extractEach(bytes, onEntry, limits) {
  const buf = input(bytes)
  if (typeof onEntry !== 'function') {
    throw new TypeError('anyzip: expected onEntry to be a function')
  }
  const acquired = await acquireEach()
  try {
    const it = native.extractEach(buf, limits)
    try {
      return await eachDepth.run(true, async () => {
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
      })
    } finally {
      await it.close()
    }
  } finally {
    releaseEach(acquired)
  }
}

module.exports = { AnyzipError, detect, list, extract, extractEach }
