export type Format = 'zip' | '7z' | 'rar'

/** Defaults: 10 000 entries, 1 GiB per entry, 2 GiB total, ratio 1000:1 (past 1 MiB of
 * output), 1 GiB of decoder memory for 7z/rar dictionaries. */
export interface Limits {
  maxEntries?: number
  maxEntryBytes?: number
  maxTotalBytes?: number
  maxRatio?: number
  maxMemoryBytes?: number
}

/** A regular file. `path` is relative, `/`-separated, never absolute and never contains `..`. */
export interface Entry {
  path: string
  data: Buffer
}

export type SkipReason = 'unsafePath' | 'symlink' | 'encrypted' | 'notAFile'

export interface Skipped {
  /** The name as stored in the archive (decoded), before normalization. */
  path: string
  reason: SkipReason
}

export interface Extracted {
  format: Format
  entries: Entry[]
  skipped: Skipped[]
  /** zip and rar: archive was truncated or a single multi-volume rar part; complete, checksum-verified entries recovered. */
  recovered: boolean
}

export interface ListedEntry {
  path: string
  /** Uncompressed size declared by the header (not verified). */
  size: number
}

export interface Listing {
  format: Format
  entries: ListedEntry[]
  skipped: Skipped[]
  recovered: boolean
}

export interface Summary {
  format: Format
  /** Number of entries passed to `onEntry`. */
  entries: number
  skipped: Skipped[]
  recovered: boolean
}

export type AnyzipErrorCode = 'unsupported' | 'corrupt' | 'encrypted' | 'multiVolume' | 'tooLarge' | 'io'

export declare class AnyzipError extends Error {
  readonly name: 'AnyzipError'
  readonly code: AnyzipErrorCode
  constructor(code: AnyzipErrorCode, message: string)
}

/** Format from the first bytes, or `null`. Synchronous. */
export declare function detect(bytes: Uint8Array | ArrayBuffer): Format | null

/** Lists entries from headers without decompressing. */
export declare function list(bytes: Uint8Array | ArrayBuffer, limits?: Limits): Promise<Listing>

/** Extracts every regular file. Runs on the libuv thread pool. Do not mutate `bytes` until it
 * settles. Rejects with `AnyzipError`. */
export declare function extract(bytes: Uint8Array | ArrayBuffer, limits?: Limits): Promise<Extracted>

/** Extracts entries one at a time with backpressure: the next entry waits until the promise
 * returned by `onEntry` settles. If `onEntry` throws, extraction stops and the error propagates. */
export declare function extractEach(
  bytes: Uint8Array | ArrayBuffer,
  onEntry: (entry: Entry) => void | Promise<void>,
  limits?: Limits,
): Promise<Summary>
