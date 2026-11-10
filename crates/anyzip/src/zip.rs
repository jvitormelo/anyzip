//! zip via the `zip` crate. Entry names without the UTF-8 flag are CP850 (Windows in Portuguese),
//! except names written by Unix/macOS tools, which are UTF-8 in practice.

use std::io::{Cursor, Read};

use zip::read::read_zipfile_from_stream;
use zip::result::ZipError;
use zip::{System, ZipArchive};

use crate::error::{Error, ErrorCode};
use crate::limits::{Budget, ReadError};
use crate::out::{Kind, Out};

/// General purpose flag bit 11: name and comment are UTF-8. The `zip` crate also sets it when it
/// applies an Info-ZIP Unicode Path extra field (0x7075) whose CRC matches.
const FLAG_UTF8: u16 = 1 << 11;

const S_IFMT: u32 = 0o170_000;
const S_IFREG: u32 = 0o100_000;
const S_IFDIR: u32 = 0o040_000;
const S_IFLNK: u32 = 0o120_000;

pub(crate) fn run(bytes: &[u8], budget: &mut Budget<'_>, out: &mut Out<'_>) -> Result<(), Error> {
    match ZipArchive::new(Cursor::new(bytes)) {
        Ok(archive) => read_archive(archive, budget, out),
        Err(e) => {
            let err = map_error(e);
            // Truncated download: the central directory at the end is missing or cut. The local
            // headers in front are still readable one after the other.
            if err.code == ErrorCode::Corrupt && bytes.starts_with(b"PK\x03\x04") {
                recover(bytes, budget, out, err)
            } else {
                Err(err)
            }
        }
    }
}

fn read_archive(
    mut archive: ZipArchive<Cursor<&[u8]>>,
    budget: &mut Budget<'_>,
    out: &mut Out<'_>,
) -> Result<(), Error> {
    for index in 0..archive.len() {
        budget.count_entry()?;
        let (name, kind, declared) = {
            let meta = archive.by_index_data(index).map_err(map_error)?;
            let name = decode_name(meta.name_raw(), meta.flags().as_u16(), meta.system());
            let kind = classify(meta.is_dir(), meta.encrypted(), meta.unix_mode());
            (name, kind, meta.size())
        };
        let Some(path) = out.admit(name, kind, declared) else {
            continue;
        };
        let mut file = archive.by_index(index).map_err(map_error)?;
        let data = read(budget, &mut file, declared)?;
        drop(file);
        if out.entry(path, data).is_break() {
            return Ok(());
        }
    }
    Ok(())
}

/// Sequential read of local headers, for archives whose central directory is gone.
fn recover(
    bytes: &[u8],
    budget: &mut Budget<'_>,
    out: &mut Out<'_>,
    open_error: Error,
) -> Result<(), Error> {
    let mut cursor = Cursor::new(bytes);
    loop {
        // The end, or a cut local header or data: keep what we have.
        let Ok(Some(mut file)) = read_zipfile_from_stream(&mut cursor) else {
            break;
        };
        budget.count_entry()?;
        // Local headers carry no "version made by"; treat the host as DOS (CP850 unless flagged).
        let name = decode_name(file.name_raw(), file.flags().as_u16(), System::Dos);
        let kind = classify(file.is_dir(), file.encrypted(), file.unix_mode());
        let declared = file.size();
        let Some(path) = out.admit(name, kind, declared) else {
            continue; // dropping `file` drains its data so the next header lines up
        };
        let data = match budget.read_entry(&mut file, declared) {
            Ok(data) => data,
            Err(ReadError::Limit(e)) => return Err(e),
            // The entry itself is cut: stop at the last complete one.
            Err(ReadError::Io(_)) => break,
        };
        drop(file);
        if out.entry(path, data).is_break() {
            break;
        }
    }
    // Nothing usable (only directories, or garbage after a PK signature): still broken.
    if out.emitted == 0 && out.listed.is_empty() && out.skipped.is_empty() {
        return Err(open_error);
    }
    out.recovered = true;
    Ok(())
}

fn read(budget: &mut Budget<'_>, reader: &mut dyn Read, declared: u64) -> Result<Vec<u8>, Error> {
    budget.read_entry(reader, declared).map_err(|e| match e {
        ReadError::Limit(e) => e,
        ReadError::Io(e) => io_to_error(&e),
    })
}

fn classify(is_dir: bool, encrypted: bool, unix_mode: Option<u32>) -> Kind {
    if is_dir {
        return Kind::Dir;
    }
    if let Some(mode) = unix_mode {
        match mode & S_IFMT {
            0 | S_IFREG => {}
            S_IFDIR => return Kind::Dir,
            S_IFLNK => return Kind::Symlink,
            _ => return Kind::Other,
        }
    }
    if encrypted {
        Kind::Encrypted
    } else {
        Kind::File
    }
}

/// Decodes an entry name.
///
/// - ASCII, or the UTF-8 flag (bit 11, also set for a valid 0x7075 extra field): UTF-8.
/// - Made on Unix or macOS and valid UTF-8: UTF-8 (Info-ZIP, `ditto` and friends omit the flag).
/// - Otherwise CP850, like `bsdtar --options hdrcharset=CP850`. Guessing "valid UTF-8 means
///   UTF-8" for Windows zips would be wrong when CP850 bytes happen to form valid UTF-8.
pub(crate) fn decode_name(raw: &[u8], flags: u16, system: System) -> String {
    if raw.is_ascii() || flags & FLAG_UTF8 != 0 {
        return String::from_utf8_lossy(raw).into_owned();
    }
    if matches!(system, System::Unix | System::OsDarwin)
        && let Ok(s) = std::str::from_utf8(raw)
    {
        return s.to_owned();
    }
    oem_cp::decode_string_complete_table(raw, &oem_cp::code_table::DECODING_TABLE_CP850)
}

fn map_error(e: ZipError) -> Error {
    match e {
        ZipError::Io(e) => io_to_error(&e),
        ZipError::InvalidArchive(msg) => {
            if msg.contains("ulti-disk") {
                Error::new(ErrorCode::MultiVolume, msg)
            } else {
                Error::corrupt(msg)
            }
        }
        ZipError::UnsupportedArchive(msg) => {
            if msg.contains("ulti-disk") {
                Error::new(ErrorCode::MultiVolume, msg)
            } else if msg.contains("assword") || msg.contains("ncrypt") {
                Error::new(ErrorCode::Encrypted, msg)
            } else {
                Error::new(ErrorCode::Unsupported, msg)
            }
        }
        ZipError::CompressionMethodNotSupported(method) => Error::new(
            ErrorCode::Unsupported,
            format!("compression method {method} not supported"),
        ),
        ZipError::InvalidPassword => Error::new(ErrorCode::Encrypted, "invalid password"),
        other => {
            let msg = other.to_string();
            if msg.contains("not supported") {
                Error::new(ErrorCode::Unsupported, msg)
            } else {
                Error::corrupt(msg)
            }
        }
    }
}

/// Decoder errors are data errors (bad deflate stream, CRC mismatch, truncated data), not OS ones:
/// everything is in memory.
fn io_to_error(e: &std::io::Error) -> Error {
    let msg = e.to_string();
    if e.kind() == std::io::ErrorKind::OutOfMemory {
        Error::new(ErrorCode::Io, msg)
    } else if msg.contains("not supported") {
        // The crate reports an unknown compression method through io::Error.
        Error::new(ErrorCode::Unsupported, msg)
    } else {
        Error::corrupt(msg)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn minimal_stored_zip(version_made_by: u16, name: &[u8], payload: &[u8]) -> Vec<u8> {
        let crc = crc32fast::hash(payload);
        let len = u32::try_from(payload.len()).unwrap();
        let name_len = u16::try_from(name.len()).unwrap();
        let mut local = Vec::new();
        local.extend_from_slice(&0x0403_4b50u32.to_le_bytes());
        local.extend_from_slice(&20u16.to_le_bytes()); // version needed
        local.extend_from_slice(&0u16.to_le_bytes()); // flags (no UTF-8 bit)
        local.extend_from_slice(&0u16.to_le_bytes()); // method: stored
        local.extend_from_slice(&0u16.to_le_bytes()); // time
        local.extend_from_slice(&0u16.to_le_bytes()); // date
        local.extend_from_slice(&crc.to_le_bytes());
        local.extend_from_slice(&len.to_le_bytes());
        local.extend_from_slice(&len.to_le_bytes());
        local.extend_from_slice(&name_len.to_le_bytes());
        local.extend_from_slice(&0u16.to_le_bytes()); // extra len
        local.extend_from_slice(name);
        local.extend_from_slice(payload);

        let cd_offset = u32::try_from(local.len()).unwrap();
        let mut cd = Vec::new();
        cd.extend_from_slice(&0x0201_4b50u32.to_le_bytes());
        cd.extend_from_slice(&version_made_by.to_le_bytes());
        cd.extend_from_slice(&20u16.to_le_bytes()); // version needed
        cd.extend_from_slice(&0u16.to_le_bytes()); // flags
        cd.extend_from_slice(&0u16.to_le_bytes()); // method
        cd.extend_from_slice(&0u16.to_le_bytes()); // time
        cd.extend_from_slice(&0u16.to_le_bytes()); // date
        cd.extend_from_slice(&crc.to_le_bytes());
        cd.extend_from_slice(&len.to_le_bytes());
        cd.extend_from_slice(&len.to_le_bytes());
        cd.extend_from_slice(&name_len.to_le_bytes());
        cd.extend_from_slice(&0u16.to_le_bytes()); // extra len
        cd.extend_from_slice(&0u16.to_le_bytes()); // comment len
        cd.extend_from_slice(&0u16.to_le_bytes()); // disk start
        cd.extend_from_slice(&0u16.to_le_bytes()); // internal attr
        cd.extend_from_slice(&0u32.to_le_bytes()); // external attr
        cd.extend_from_slice(&0u32.to_le_bytes()); // local header offset
        cd.extend_from_slice(name);

        let cd_len = u32::try_from(cd.len()).unwrap();
        let mut eocd = Vec::new();
        eocd.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
        eocd.extend_from_slice(&0u16.to_le_bytes()); // disk
        eocd.extend_from_slice(&0u16.to_le_bytes()); // cd start disk
        eocd.extend_from_slice(&1u16.to_le_bytes()); // entries on disk
        eocd.extend_from_slice(&1u16.to_le_bytes()); // total entries
        eocd.extend_from_slice(&cd_len.to_le_bytes());
        eocd.extend_from_slice(&cd_offset.to_le_bytes());
        eocd.extend_from_slice(&0u16.to_le_bytes()); // comment len

        [local, cd, eocd].concat()
    }

    #[test]
    fn names() {
        // "edição_ação/anexo único.pdf" in CP850
        let cp850 = b"edi\x87\xc6o_a\x87\xc6o/anexo \xa3nico.pdf";
        assert_eq!(
            decode_name(cp850, 0, System::Dos),
            "edição_ação/anexo único.pdf"
        );
        assert_eq!(
            decode_name("ação".as_bytes(), FLAG_UTF8, System::Dos),
            "ação"
        );
        assert_eq!(decode_name("ação".as_bytes(), 0, System::Unix), "ação");
        assert_eq!(decode_name("ação".as_bytes(), 0, System::OsDarwin), "ação");
        // Valid UTF-8 by accident ("ã" + "é" in CP850 = C6 82 = U+0182) from Windows: CP850.
        assert_eq!(decode_name(b"\xc6\x82", 0, System::Dos), "ãé");
        assert_eq!(decode_name(b"plain.txt", 0, System::Dos), "plain.txt");

        // End-to-end through `ZipArchive`: host 19 (macOS `OsDarwin`) without bit 11 reads UTF-8.
        let osx_zip =
            minimal_stored_zip(u16::from_le_bytes([20, 19]), "ação.txt".as_bytes(), b"ok");
        let out = crate::extract(&osx_zip, &crate::Limits::default()).unwrap();
        assert_eq!(out.entries[0].path, "ação.txt");

        // Host 0 (DOS) with spec version 190 (`190 / 10 == 19`) must still decode as CP850, not macOS.
        let dos_zip = minimal_stored_zip(190, b"\xc6\x82.txt", b"ok");
        let out = crate::extract(&dos_zip, &crate::Limits::default()).unwrap();
        assert_eq!(out.entries[0].path, "ãé.txt");
    }
}
