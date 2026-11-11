//! Extract zip, 7z and rar archives from bytes in memory.
//!
//! - Format detected from content, never from the file name.
//! - Hostile input becomes a typed [`Error`] or a [`Skipped`] entry: no panic escapes, limits
//!   ([`Limits`]) count real decoded bytes, unsafe paths and links are never returned.
//! - zip is pure Rust; 7z is `sevenz-rust2` plus liblzma (C) for single-stream LZMA2 blocks; rar
//!   is the official UnRAR (C++) through `unrar-sys`.
//!
//! ```no_run
//! let bytes = std::fs::read("edital.zip")?;
//! let out = anyzip::extract(&bytes, &anyzip::Limits::default())?;
//! for entry in out.entries {
//!     println!("{} {}", entry.path, entry.data.len());
//! }
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

#![forbid(unsafe_code)]

mod detect;
mod error;
mod limits;
mod out;
mod path;
#[cfg(feature = "rar")]
mod rar;
mod sevenz;
mod zip;

use std::ops::ControlFlow;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicBool, Ordering};

pub use detect::{Format, detect};
pub use error::{Error, ErrorCode};
pub use limits::Limits;
pub use path::normalize as normalize_path;

use detect::Signature;
use limits::Budget;
use out::Out;

/// A regular file from the archive. `path` is relative, `/`-separated, without `.` or `..`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub path: String,
    pub data: Vec<u8>,
}

/// Why an entry was not returned.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SkipReason {
    /// Absolute, `..`, drive letter, control characters or empty.
    UnsafePath,
    Symlink,
    /// Encrypted entry (archive readable, this entry needs a password).
    Encrypted,
    /// Hard link, device, fifo, socket or other non-regular entry.
    NotAFile,
}

impl SkipReason {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::UnsafePath => "unsafePath",
            Self::Symlink => "symlink",
            Self::Encrypted => "encrypted",
            Self::NotAFile => "notAFile",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Skipped {
    /// The name as stored (decoded), before normalization.
    pub path: String,
    pub reason: SkipReason,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Extracted {
    pub format: Format,
    pub entries: Vec<Entry>,
    pub skipped: Vec<Skipped>,
    /// zip and rar: the archive was truncated or a single multi-volume rar part, and complete,
    /// checksum-verified entries were recovered from it.
    pub recovered: bool,
}

/// Header information without decompressing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListedEntry {
    pub path: String,
    /// Uncompressed size as declared by the header (not verified).
    pub size: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Listing {
    pub format: Format,
    pub entries: Vec<ListedEntry>,
    pub skipped: Vec<Skipped>,
    pub recovered: bool,
}

/// Result of [`extract_each`]: what [`Extracted`] has, minus the entries already handed out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Summary {
    pub format: Format,
    /// Entries passed to the callback.
    pub entries: usize,
    pub skipped: Vec<Skipped>,
    pub recovered: bool,
    /// The callback returned `Break`.
    pub stopped: bool,
}

/// Extracts every regular file into memory.
///
/// # Errors
/// See [`ErrorCode`]. A partially read archive is an error, never a silent partial result
/// (except zip recovery, flagged by [`Extracted::recovered`]).
pub fn extract(bytes: &[u8], limits: &Limits) -> Result<Extracted, Error> {
    let mut entries = Vec::new();
    let summary = extract_each(bytes, limits, |entry| {
        entries.push(entry);
        ControlFlow::Continue(())
    })?;
    Ok(Extracted {
        format: summary.format,
        entries,
        skipped: summary.skipped,
        recovered: summary.recovered,
    })
}

/// Extracts entries one at a time, so peak memory is the archive plus the largest entry.
/// Returning `ControlFlow::Break` from `on_entry` stops early without error.
///
/// # Errors
/// Same as [`extract`]. Entries already passed to `on_entry` stay delivered.
pub fn extract_each(
    bytes: &[u8],
    limits: &Limits,
    on_entry: impl FnMut(Entry) -> ControlFlow<()>,
) -> Result<Summary, Error> {
    extract_each_inner(bytes, limits, None, on_entry)
}

/// Like [`extract_each`], plus a cooperative `cancel` flag checked between decode chunks so an
/// external caller can abort mid-entry without waiting for a large entry to finish decoding.
///
/// # Errors
/// Same as [`extract`].
pub fn extract_each_with_cancel(
    bytes: &[u8],
    limits: &Limits,
    cancel: &AtomicBool,
    on_entry: impl FnMut(Entry) -> ControlFlow<()>,
) -> Result<Summary, Error> {
    extract_each_inner(bytes, limits, Some(cancel), on_entry)
}

fn extract_each_inner(
    bytes: &[u8],
    limits: &Limits,
    cancel: Option<&AtomicBool>,
    mut on_entry: impl FnMut(Entry) -> ControlFlow<()>,
) -> Result<Summary, Error> {
    let mut stopped = false;
    let mut callback = |entry| {
        let flow = on_entry(entry);
        stopped |= flow.is_break();
        flow
    };
    let (format, out) = run(bytes, limits, false, cancel, &mut callback)?;
    stopped |= cancel.is_some_and(|c| c.load(Ordering::Relaxed));
    Ok(Summary {
        format,
        entries: out.emitted,
        skipped: out.skipped,
        recovered: out.recovered,
        stopped,
    })
}

/// Lists entries from headers, without decompressing.
///
/// # Errors
/// Same codes as [`extract`]; data errors (bad checksums) are not detected.
pub fn list(bytes: &[u8], limits: &Limits) -> Result<Listing, Error> {
    let mut ignore = |_| ControlFlow::Continue(());
    let (format, out) = run(bytes, limits, true, None, &mut ignore)?;
    Ok(Listing {
        format,
        entries: out.listed,
        skipped: out.skipped,
        recovered: out.recovered,
    })
}

struct Finished {
    emitted: usize,
    listed: Vec<ListedEntry>,
    skipped: Vec<Skipped>,
    recovered: bool,
}

fn run(
    bytes: &[u8],
    limits: &Limits,
    list_only: bool,
    cancel: Option<&AtomicBool>,
    on_entry: &mut dyn FnMut(Entry) -> ControlFlow<()>,
) -> Result<(Format, Finished), Error> {
    let format = match detect::signature(bytes) {
        Some(Signature::Archive(format)) => format,
        Some(Signature::SpannedZip) => {
            return Err(Error::new(ErrorCode::MultiVolume, "spanned zip segment"));
        }
        None => {
            return Err(Error::new(
                ErrorCode::Unsupported,
                "not a zip, 7z or rar archive",
            ));
        }
    };
    let mut budget = Budget::new(limits, bytes.len(), cancel);
    let mut out = Out::new(list_only, on_entry);
    // A panic in a decoder is a bug triggered by hostile input; it must not take the process down.
    let result = catch_unwind(AssertUnwindSafe(|| match format {
        Format::Zip => zip::run(bytes, &mut budget, &mut out),
        Format::SevenZ => sevenz::run(bytes, &mut budget, &mut out),
        #[cfg(feature = "rar")]
        Format::Rar => rar::run(bytes, &mut budget, &mut out),
        #[cfg(not(feature = "rar"))]
        Format::Rar => Err(Error::new(
            ErrorCode::Unsupported,
            "built without rar support",
        )),
    }));
    match result {
        Ok(Ok(())) => Ok((
            format,
            Finished {
                emitted: out.emitted,
                listed: out.listed,
                skipped: out.skipped,
                recovered: out.recovered,
            },
        )),
        Ok(Err(e)) => Err(e),
        Err(panic) => {
            let message = panic
                .downcast_ref::<&str>()
                .map(ToString::to_string)
                .or_else(|| panic.downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "decoder panicked".to_owned());
            Err(Error::corrupt(format!("decoder panicked: {message}")))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancel_stops_across_formats() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures");
        for rel in [
            "generated/zip_cp850_noflag.zip",
            "generated/7z_utf8_solid.7z",
            "libarchive/rar5_multiple_files_solid.rar",
        ] {
            let bytes = std::fs::read(root.join(rel)).unwrap();
            let cancel = AtomicBool::new(false);
            let mut seen = 0usize;
            let summary = extract_each_with_cancel(&bytes, &Limits::default(), &cancel, |_| {
                seen += 1;
                cancel.store(true, Ordering::SeqCst);
                ControlFlow::Continue(())
            })
            .unwrap();
            assert_eq!(seen, 1, "{rel}");
            assert_eq!(summary.entries, 1, "{rel}");
            assert!(summary.stopped, "{rel}");
            assert!(!summary.recovered, "{rel}");
        }
    }
}
