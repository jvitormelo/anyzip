//! 7z via `sevenz-rust2`, with liblzma for blocks that are a single LZMA2 stream (the common
//! case, ~1.5x faster). Names are UTF-16 in the format, so there is no charset guessing.

use std::cell::Cell;
use std::io::{Cursor, Read};
use std::num::NonZeroUsize;
use std::ops::ControlFlow;

use sevenz_rust2::{Archive, ArchiveEntry, BlockDecoder, Error as SevenzError, Password};

use crate::error::{Error, ErrorCode};
use crate::limits::{Budget, ReadError};
use crate::out::{Kind, Out};

const FILE_ATTRIBUTE_DIRECTORY: u32 = 0x10;
const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
/// p7zip/7-Zip: the high 16 bits hold a Unix mode.
const FILE_ATTRIBUTE_UNIX_EXTENSION: u32 = 0x8000;
const S_IFMT: u32 = 0o170_000;
const S_IFREG: u32 = 0o100_000;
const S_IFDIR: u32 = 0o040_000;
const S_IFLNK: u32 = 0o120_000;

/// LZMA2 streams written by multi-threaded 7-Zip decode in parallel. More threads than this buys
/// little and competes with the other libuv workers.
const MAX_DECODER_THREADS: usize = 4;
/// 7-Zip's multi-threaded LZMA2 encoder cuts independent chunks of max(4 × dictionary, 1 MiB).
const MIB: u64 = 1024 * 1024;

/// Whether an LZMA2 block is large enough, for its dictionary, to hold more than one
/// independently decodable chunk. Otherwise decoder threads only add spawn and copy overhead.
fn may_have_parallel_chunks(block: &sevenz_rust2::Block) -> bool {
    let Some(coder) = block
        .coders
        .iter()
        .find(|c| c.encoder_method_id() == sevenz_rust2::EncoderMethod::ID_LZMA2)
    else {
        return false;
    };
    let Some(dict) = coder
        .properties()
        .first()
        .copied()
        .and_then(lzma2_dict_size)
    else {
        return false;
    };
    block.get_unpack_size() >= 2 * (4 * dict).max(MIB)
}

pub(crate) fn run(bytes: &[u8], budget: &mut Budget<'_>, out: &mut Out<'_>) -> Result<(), Error> {
    if is_empty_archive(bytes) {
        return Ok(());
    }
    let mut source = Cursor::new(bytes);
    let password = Password::empty();
    let memory_kb = budget.memory_limit_kb();
    let archive = Archive::read_with_memory_limit(&mut source, &password, memory_kb)
        .map_err(|e| map_error(&e))?;

    for _ in &archive.files {
        budget.count_entry()?;
    }

    if out.list_only() {
        for (index, entry) in archive.files.iter().enumerate() {
            out.admit(
                entry.name.clone(),
                classify(entry, archive.is_file_encrypted(index)),
                entry.size,
            );
        }
        return Ok(());
    }

    let threads = std::thread::available_parallelism()
        .map_or(1, NonZeroUsize::get)
        .min(MAX_DECODER_THREADS);
    let threads = u32::try_from(threads).unwrap_or(1);

    for block_index in 0..archive.blocks.len() {
        let encrypted = archive.blocks[block_index]
            .coders
            .iter()
            .any(|c| c.encoder_method_id() == sevenz_rust2::EncoderMethod::ID_AES256_SHA256);
        if encrypted {
            for (index, entry) in archive.files.iter().enumerate() {
                if archive.stream_map.file_block_index[index] == Some(block_index) {
                    out.admit(entry.name.clone(), classify(entry, true), entry.size);
                }
            }
            continue;
        }

        if let Some(fast) = fast_lzma2(bytes, &archive, block_index, budget)? {
            if fast.decode(&archive, budget, out)?.is_break() {
                return Ok(());
            }
            continue;
        }

        let block_threads = if may_have_parallel_chunks(&archive.blocks[block_index]) {
            threads
        } else {
            1
        };
        let decoder = BlockDecoder::new_with_memory_limit(
            block_threads,
            memory_kb,
            block_index,
            &archive,
            &password,
            &mut source,
        );
        if decode_block(decoder, budget, out)?.is_break() {
            return Ok(());
        }
    }

    // Entries with no data stream and outside any block (empty files).
    for (index, entry) in archive.files.iter().enumerate() {
        if archive.stream_map.file_block_index[index].is_none()
            && let Some(path) = out.admit(entry.name.clone(), classify(entry, false), 0)
            && out.entry(path, Vec::new()).is_break()
        {
            return Ok(());
        }
    }
    Ok(())
}

fn decode_block<R: Read + std::io::Seek>(
    decoder: BlockDecoder<'_, R>,
    budget: &mut Budget<'_>,
    out: &mut Out<'_>,
) -> Result<ControlFlow<()>, Error> {
    let mut failure: Option<Error> = None;
    let stop = Cell::new(false);
    let result = decoder.for_each_entries(&mut |entry, reader| {
        let Some(path) = out.admit(entry.name.clone(), classify(entry, false), entry.size) else {
            // Skipped entries in solid blocks still decode: count their bytes without buffering.
            return match budget.drain_entry(reader, |_| {}) {
                Ok(_) => Ok(true),
                Err(ReadError::Limit(e)) => {
                    failure = Some(e);
                    Err(SevenzError::Other("anyzip limit".into()))
                }
                Err(ReadError::Io(e)) => Err(SevenzError::Io(e, "entry".into())),
            };
        };
        match budget.read_entry(reader, entry.size) {
            Ok(data) => {
                if out.entry(path, data).is_break() {
                    stop.set(true);
                    return Ok(false);
                }
                Ok(true)
            }
            Err(ReadError::Limit(e)) => {
                failure = Some(e);
                Err(SevenzError::Other("anyzip limit".into()))
            }
            Err(ReadError::Io(e)) => Err(SevenzError::Io(e, "entry".into())),
        }
    });
    if let Some(e) = failure {
        return Err(e);
    }
    result.map_err(|e| map_error(&e))?;
    Ok(if stop.get() {
        ControlFlow::Break(())
    } else {
        ControlFlow::Continue(())
    })
}

/// A block that is one LZMA2 stream, decoded with liblzma instead of `sevenz-rust2`.
struct FastLzma2<'a> {
    packed: &'a [u8],
    dict_size: u32,
    block_index: usize,
}

/// The fast path applies to the common 7-Zip output: one LZMA2 coder (no BCJ, no AES) whose
/// stream is a single chunk. Multi-chunk streams stay on the multi-threaded `sevenz-rust2` path.
/// Anything unusual returns `None` and goes the general way.
fn fast_lzma2<'a>(
    bytes: &'a [u8],
    archive: &Archive,
    block_index: usize,
    budget: &Budget<'_>,
) -> Result<Option<FastLzma2<'a>>, Error> {
    let block = &archive.blocks[block_index];
    let [coder] = block.coders.as_slice() else {
        return Ok(None);
    };
    if coder.encoder_method_id() != sevenz_rust2::EncoderMethod::ID_LZMA2
        || may_have_parallel_chunks(block)
    {
        return Ok(None);
    }
    let Some(dict) = coder
        .properties()
        .first()
        .copied()
        .and_then(lzma2_dict_size)
    else {
        return Ok(None);
    };
    if dict > budget.memory_limit_bytes() {
        return Err(Error::too_large(format!(
            "7z dictionary of {dict} bytes is above the memory limit"
        )));
    }
    let map = &archive.stream_map;
    let Some(&pack_index) = map.block_first_pack_stream_index().get(block_index) else {
        return Ok(None);
    };
    let (Some(&offset), Some(&size)) = (
        map.pack_stream_offsets().get(pack_index),
        archive.pack_sizes().get(pack_index),
    ) else {
        return Ok(None);
    };
    let range = sevenz_rust2::SIGNATURE_HEADER_SIZE
        .checked_add(archive.pack_pos())
        .and_then(|start| start.checked_add(offset))
        .and_then(|start| {
            Some(usize::try_from(start).ok()?..usize::try_from(start.checked_add(size)?).ok()?)
        });
    let Some(packed) = range.and_then(|r| bytes.get(r)) else {
        return Err(Error::new(
            ErrorCode::Corrupt,
            "7z packed stream is past the end of the archive",
        ));
    };
    // The decoder never looks further back than what it has produced, so a dictionary larger
    // than the block only costs memory. liblzma's minimum is 4 KiB.
    let unpack = block.get_unpack_size();
    let dict_size = u32::try_from(dict.min(unpack).max(4096)).unwrap_or(u32::MAX);
    Ok(Some(FastLzma2 {
        packed,
        dict_size,
        block_index,
    }))
}

impl FastLzma2<'_> {
    fn decode(
        &self,
        archive: &Archive,
        budget: &mut Budget<'_>,
        out: &mut Out<'_>,
    ) -> Result<ControlFlow<()>, Error> {
        let corrupt = |what: &str| Error::new(ErrorCode::Corrupt, format!("7z: {what}"));
        let mut options =
            liblzma::stream::LzmaOptions::new_preset(0).map_err(|e| corrupt(&e.to_string()))?;
        options.dict_size(self.dict_size);
        let mut filters = liblzma::stream::Filters::new();
        filters.lzma2(&options);
        let stream = liblzma::stream::Stream::new_raw_decoder(&filters)
            .map_err(|e| corrupt(&e.to_string()))?;
        let mut decoder = liblzma::bufread::XzDecoder::new_stream(self.packed, stream);

        let block = &archive.blocks[self.block_index];
        let mut block_crc = block.has_crc.then(crc32fast::Hasher::new);
        let mut block_decoded: u64 = 0;

        for (index, entry) in archive.files.iter().enumerate() {
            if archive.stream_map.file_block_index[index] != Some(self.block_index) {
                continue;
            }
            let size = if entry.has_stream { entry.size } else { 0 };
            // Skipped entries are decoded too (solid stream) and count against the limits.
            if let Some(path) = out.admit(entry.name.clone(), classify(entry, false), size) {
                let data = match budget.read_entry(&mut (&mut decoder).take(size), size) {
                    Ok(data) => data,
                    Err(ReadError::Limit(e)) => return Err(e),
                    Err(ReadError::Io(e)) => return Err(corrupt(&e.to_string())),
                };
                if data.len() as u64 != size {
                    return Err(corrupt("data ends before the declared size"));
                }
                if entry.has_stream
                    && entry.has_crc
                    && u64::from(crc32fast::hash(&data)) != entry.crc
                {
                    return Err(corrupt(&format!("CRC mismatch in {}", entry.name)));
                }
                if let Some(h) = block_crc.as_mut() {
                    h.update(&data);
                }
                block_decoded = block_decoded.saturating_add(size);
                if out.entry(path, data).is_break() {
                    return Ok(ControlFlow::Break(()));
                }
            } else {
                let mut entry_crc =
                    (entry.has_stream && entry.has_crc).then(crc32fast::Hasher::new);
                let drained = budget
                    .drain_entry(&mut (&mut decoder).take(size), |chunk| {
                        if let Some(h) = entry_crc.as_mut() {
                            h.update(chunk);
                        }
                        if let Some(h) = block_crc.as_mut() {
                            h.update(chunk);
                        }
                    })
                    .map_err(|e| match e {
                        ReadError::Limit(e) => e,
                        ReadError::Io(e) => corrupt(&e.to_string()),
                    })?;
                if drained != size {
                    return Err(corrupt("data ends before the declared size"));
                }
                if let Some(h) = entry_crc
                    && u64::from(h.finalize()) != entry.crc
                {
                    return Err(corrupt(&format!("CRC mismatch in {}", entry.name)));
                }
                block_decoded = block_decoded.saturating_add(size);
            }
        }
        // The stream must end exactly where the entries and block declare.
        match decoder.read(&mut [0u8; 1]) {
            Ok(0) => {
                if block_decoded != block.get_unpack_size() {
                    return Err(corrupt(
                        "decoded block size does not match its declared size",
                    ));
                }
                if let Some(h) = block_crc
                    && u64::from(h.finalize()) != block.crc
                {
                    return Err(corrupt("block CRC mismatch"));
                }
                Ok(ControlFlow::Continue(()))
            }
            Ok(_) => Err(corrupt("block has more data than its entries declare")),
            Err(e) => Err(corrupt(&e.to_string())),
        }
    }
}

/// Dictionary size from the LZMA2 property byte (7z/xz encoding).
fn lzma2_dict_size(prop: u8) -> Option<u64> {
    match prop {
        0..40 => Some((2 | u64::from(prop & 1)) << (prop / 2 + 11)),
        40 => Some(u64::from(u32::MAX)),
        _ => None,
    }
}

/// 7-Zip writes an archive with no entries as a bare 32-byte signature header whose "next header"
/// is empty (offset, size and CRC all zero), with major version 0 and a valid `StartHeaderCRC`.
/// `sevenz-rust2` rejects it; 7-Zip and libarchive list it.
fn is_empty_archive(bytes: &[u8]) -> bool {
    let Ok(header): Result<&[u8; 32], _> = bytes.try_into() else {
        return false;
    };
    let start_crc = u32::from_le_bytes([header[8], header[9], header[10], header[11]]);
    header[6] == 0
        && header[12..32].iter().all(|&b| b == 0)
        && start_crc == crc32fast::hash(&header[12..32])
}

fn classify(entry: &ArchiveEntry, encrypted: bool) -> Kind {
    if entry.is_directory || entry.is_anti_item {
        return Kind::Dir;
    }
    if entry.has_windows_attributes {
        let attrs = entry.windows_attributes;
        if attrs & FILE_ATTRIBUTE_DIRECTORY != 0 {
            return Kind::Dir;
        }
        if attrs & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return Kind::Symlink;
        }
        if attrs & FILE_ATTRIBUTE_UNIX_EXTENSION != 0 {
            match (attrs >> 16) & S_IFMT {
                0 | S_IFREG => {}
                S_IFDIR => return Kind::Dir,
                S_IFLNK => return Kind::Symlink,
                _ => return Kind::Other,
            }
        }
    }
    if encrypted {
        Kind::Encrypted
    } else {
        Kind::File
    }
}

fn map_error(e: &SevenzError) -> Error {
    let code = match e {
        SevenzError::PasswordRequired | SevenzError::MaybeBadPassword(_) => ErrorCode::Encrypted,
        SevenzError::MaxMemLimited { .. } => ErrorCode::TooLarge,
        SevenzError::BadSignature(_)
        | SevenzError::UnsupportedVersion { .. }
        | SevenzError::ExternalUnsupported
        | SevenzError::UnsupportedCompressionMethod(_)
        | SevenzError::Unsupported(_) => ErrorCode::Unsupported,
        SevenzError::Io(io, _) | SevenzError::FileOpen(io, _)
            if io.kind() == std::io::ErrorKind::OutOfMemory =>
        {
            ErrorCode::Io
        }
        // lzma-rust2 refuses dictionaries above its own maximum before allocating.
        SevenzError::Io(io, _) if io.to_string().contains("too large") => ErrorCode::TooLarge,
        _ => ErrorCode::Corrupt,
    };
    Error::new(code, e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_archive_validates_version_and_crc() {
        let mut header = [0u8; 32];
        header[..6].copy_from_slice(b"7z\xBC\xAF\x27\x1C");
        header[6..8].copy_from_slice(&[0, 4]);
        let crc = crc32fast::hash(&header[12..32]);
        header[8..12].copy_from_slice(&crc.to_le_bytes());

        let ok = crate::extract(&header, &crate::Limits::default()).unwrap();
        assert!(ok.entries.is_empty());

        // Zero-padded signature (bad StartHeaderCRC) must be rejected as corrupt.
        let mut bad_crc = header;
        bad_crc[8] ^= 0xff;
        assert_eq!(
            crate::extract(&bad_crc, &crate::Limits::default())
                .unwrap_err()
                .code,
            ErrorCode::Corrupt
        );

        // Unsupported major version must be rejected as unsupported, even with a matching CRC.
        let mut bad_ver = header;
        bad_ver[6] = 1;
        assert_eq!(
            crate::extract(&bad_ver, &crate::Limits::default())
                .unwrap_err()
                .code,
            ErrorCode::Unsupported
        );
    }

    #[test]
    fn fast_lzma2_rejects_block_unpack_size_mismatch() {
        let mut packed = Vec::new();
        {
            let mut opts = liblzma::stream::LzmaOptions::new_preset(0).unwrap();
            opts.dict_size(4096);
            let mut filters = liblzma::stream::Filters::new();
            filters.lzma2(&opts);
            let stream = liblzma::stream::Stream::new_raw_encoder(&filters).unwrap();
            let mut enc = liblzma::write::XzEncoder::new_stream(&mut packed, stream);
            std::io::Write::write_all(&mut enc, b"hi").unwrap();
            enc.finish().unwrap();
        }
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/libarchive/7zip_lzma2.7z");
        let bytes = std::fs::read(path).unwrap();
        let mut cursor = Cursor::new(bytes.as_slice());
        let mut archive =
            Archive::read_with_memory_limit(&mut cursor, &Password::empty(), usize::MAX).unwrap();
        assert!(archive.blocks[0].get_unpack_size() > 2);
        archive.blocks[0].has_crc = false;
        archive.files.truncate(1);
        archive.files[0].has_stream = true;
        archive.files[0].size = 2;
        archive.files[0].has_crc = true;
        archive.files[0].crc = u64::from(crc32fast::hash(b"hi"));
        archive.stream_map.file_block_index = vec![Some(0)];

        let limits = crate::Limits::default();
        let mut budget = Budget::new(&limits, bytes.len());
        let tampered = FastLzma2 {
            packed: &packed,
            dict_size: 4096,
            block_index: 0,
        };
        let mut ignore = |_| ControlFlow::Continue(());
        let mut out = Out::new(false, &mut ignore);
        let err = tampered
            .decode(&archive, &mut budget, &mut out)
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::Corrupt);
        assert!(err.message.contains("declared size"), "{}", err.message);
    }
}
