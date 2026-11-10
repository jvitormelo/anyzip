//! Limits against bombs. Every check counts bytes actually produced by the decoder; declared
//! sizes from headers are only an allocation hint, and a capped one.

use std::io::Read;

use crate::error::{Error, ErrorCode};

const MIB: u64 = 1024 * 1024;
const GIB: u64 = 1024 * MIB;

/// Bytes decoded per step between limit checks. Bounds the overshoot past any limit.
const STEP: u64 = 4 * MIB;
/// Largest up-front allocation we trust a header for.
const MAX_HINT: u64 = 64 * MIB;
/// The ratio check starts after this much output (tiny archives legitimately compress well).
const RATIO_FLOOR: u64 = MIB;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// Entries (files, dirs, links) read from headers. Default 10 000.
    pub max_entries: u32,
    /// Decompressed bytes of one entry. Default 1 GiB.
    pub max_entry_bytes: u64,
    /// Decompressed bytes of all entries. Default 2 GiB.
    pub max_total_bytes: u64,
    /// Decompressed / archive size, checked once output passes 1 MiB. Default 1000.
    pub max_ratio: u32,
    /// Decoder memory (dictionary / model) a header may ask for, for 7z and rar. Default 1 GiB.
    pub max_memory_bytes: u64,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_entries: 10_000,
            max_entry_bytes: GIB,
            max_total_bytes: 2 * GIB,
            max_ratio: 1_000,
            max_memory_bytes: GIB,
        }
    }
}

/// Running totals for one archive.
pub(crate) struct Budget<'a> {
    limits: &'a Limits,
    archive_len: u64,
    entries: u32,
    total: u64,
}

impl<'a> Budget<'a> {
    pub(crate) fn new(limits: &'a Limits, archive_len: usize) -> Self {
        Self {
            limits,
            archive_len: (archive_len as u64).max(1),
            entries: 0,
            total: 0,
        }
    }

    /// Counts one header. Call before decompressing anything for it.
    pub(crate) fn count_entry(&mut self) -> Result<(), Error> {
        self.entries = self.entries.saturating_add(1);
        if self.entries > self.limits.max_entries {
            return Err(Error::too_large(format!(
                "more than {} entries",
                self.limits.max_entries
            )));
        }
        Ok(())
    }

    /// Total output the ratio check allows for this archive.
    fn ratio_budget(&self) -> u64 {
        self.archive_len
            .saturating_mul(u64::from(self.limits.max_ratio))
            .max(RATIO_FLOOR)
    }

    /// Bytes still allowed for the next entry under every limit.
    fn entry_allowance(&self) -> u64 {
        self.limits
            .max_entry_bytes
            .min(self.limits.max_total_bytes.saturating_sub(self.total))
            .min(self.ratio_budget().saturating_sub(self.total))
    }

    /// Capacity to reserve for an entry whose header declares `declared` bytes.
    pub(crate) fn hint(&self, declared: u64) -> usize {
        usize::try_from(declared.min(self.entry_allowance()).min(MAX_HINT)).unwrap_or(0)
    }

    /// Records `n` more decoded bytes for the current entry (`entry` bytes so far, including `n`).
    pub(crate) fn add(&mut self, n: u64, entry: u64) -> Result<(), Error> {
        self.total = self.total.saturating_add(n);
        if entry > self.limits.max_entry_bytes {
            return Err(Error::too_large(format!(
                "entry larger than {} bytes",
                self.limits.max_entry_bytes
            )));
        }
        if self.total > self.limits.max_total_bytes {
            return Err(Error::too_large(format!(
                "archive expands to more than {} bytes",
                self.limits.max_total_bytes
            )));
        }
        if self.total > self.ratio_budget() {
            return Err(Error::too_large(format!(
                "compression ratio above {}:1",
                self.limits.max_ratio
            )));
        }
        Ok(())
    }

    /// Reads one entry to the end, enforcing the limits every [`STEP`] bytes.
    pub(crate) fn read_entry(
        &mut self,
        reader: &mut dyn Read,
        declared: u64,
    ) -> Result<Vec<u8>, ReadError> {
        let mut buf = Vec::new();
        buf.try_reserve_exact(self.hint(declared))
            .map_err(|_| ReadError::Limit(Error::new(ErrorCode::Io, "out of memory")))?;
        loop {
            let before = buf.len();
            // `take` + `read_to_end` lets std fill uninitialized capacity without unsafe here.
            let n = reader
                .take(STEP)
                .read_to_end(&mut buf)
                .map_err(ReadError::Io)?;
            if n == 0 {
                return Ok(buf);
            }
            debug_assert_eq!(buf.len(), before + n);
            self.add(n as u64, buf.len() as u64)
                .map_err(ReadError::Limit)?;
        }
    }

    /// Drains an entry without keeping it in memory (for skipped entries inside solid blocks),
    /// still counting every decoded byte against the limits.
    pub(crate) fn drain_entry(
        &mut self,
        reader: &mut dyn Read,
        mut on_chunk: impl FnMut(&[u8]),
    ) -> Result<u64, ReadError> {
        let mut scratch = [0u8; 8 * 1024];
        let mut entry: u64 = 0;
        loop {
            let n = reader.read(&mut scratch).map_err(ReadError::Io)?;
            if n == 0 {
                return Ok(entry);
            }
            on_chunk(&scratch[..n]);
            entry = entry.saturating_add(n as u64);
            self.add(n as u64, entry).map_err(ReadError::Limit)?;
        }
    }

    pub(crate) fn memory_limit_bytes(&self) -> u64 {
        self.limits.max_memory_bytes
    }

    pub(crate) fn memory_limit_kb(&self) -> usize {
        usize::try_from(self.limits.max_memory_bytes / 1024).unwrap_or(usize::MAX)
    }

    /// UnRAR treats `MaxAllocate == 0` as unlimited, so clamp to at least 1 MiB (sub-MiB limits
    /// are checked explicitly against `Header::dict_size_kb` in `rar.rs`).
    #[cfg(feature = "rar")]
    pub(crate) fn memory_limit_mb(&self) -> u32 {
        u32::try_from(self.limits.max_memory_bytes.div_ceil(MIB).max(1)).unwrap_or(u32::MAX)
    }
}

pub(crate) enum ReadError {
    /// The decoder failed (corrupt data, checksum, I/O).
    Io(std::io::Error),
    /// A limit was hit.
    Limit(Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hint_is_capped() {
        let limits = Limits::default();
        let b = Budget::new(&limits, usize::try_from(MIB).unwrap());
        assert_eq!(b.hint(10), 10);
        assert_eq!(b.hint(u64::MAX), usize::try_from(MAX_HINT).unwrap());
    }

    #[test]
    fn ratio_kicks_in_after_floor() {
        let limits = Limits {
            max_ratio: 10,
            ..Limits::default()
        };
        let mut b = Budget::new(&limits, 1000);
        assert!(b.add(MIB, MIB).is_ok());
        assert_eq!(b.add(1, MIB + 1).unwrap_err().code, ErrorCode::TooLarge);
        let mut b = Budget::new(&limits, usize::try_from(MIB).unwrap());
        assert!(b.add(10 * MIB, 10 * MIB).is_ok());
        assert!(b.add(1, 10 * MIB + 1).is_err());
        let b = Budget::new(&limits, 1000);
        assert_eq!(b.hint(u64::MAX), usize::try_from(MIB).unwrap());
    }

    #[test]
    fn entry_and_total() {
        let limits = Limits {
            max_entry_bytes: 100,
            max_total_bytes: 150,
            max_ratio: u32::MAX,
            ..Limits::default()
        };
        let mut b = Budget::new(&limits, 1);
        assert!(b.add(100, 100).is_ok());
        assert!(b.add(1, 101).is_err());
        let mut b = Budget::new(&limits, 1);
        assert!(b.add(100, 100).is_ok());
        assert!(b.add(51, 51).is_err());
    }

    #[test]
    fn read_entry_stops_early() {
        let limits = Limits {
            max_entry_bytes: 10 * MIB,
            max_ratio: u32::MAX,
            ..Limits::default()
        };
        let mut b = Budget::new(&limits, 1);
        let mut endless = std::io::repeat(0);
        match b.read_entry(&mut endless, u64::MAX) {
            Err(ReadError::Limit(e)) => assert_eq!(e.code, ErrorCode::TooLarge),
            _ => panic!("expected limit"),
        }
        assert!(b.total <= 10 * MIB + STEP);
    }
}
