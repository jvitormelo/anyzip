//! rar (RAR4 and RAR5) via the official UnRAR, built by our `unrar-sys`.
//!
//! UnRAR only opens archives by path. On Linux the bytes go to a `memfd` (anonymous RAM file,
//! opened through `/proc/self/fd/N`), so nothing touches a disk even where `/tmp` is a disk.
//! Elsewhere, and if `memfd`/`/proc` is unavailable, a temp file deleted on drop.

use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};

use unrar_sys::{Archive, Code, Flow, Header, OpenOptions};

use crate::error::{Error, ErrorCode};
use crate::limits::Budget;
use crate::out::{Kind, Out};

/// `FSREDIR_*` from UnRAR's headers.hpp.
const FSREDIR_HARDLINK: u32 = 4;
const FSREDIR_FILECOPY: u32 = 5;
const METHOD_STORED: u32 = 0x30;
const HOST_UNIX: u32 = 3;
const S_IFMT: u32 = 0o170_000;
const S_IFREG: u32 = 0o100_000;
const S_IFDIR: u32 = 0o040_000;
const S_IFLNK: u32 = 0o120_000;

pub(crate) fn run(bytes: &[u8], budget: &mut Budget<'_>, out: &mut Out<'_>) -> Result<(), Error> {
    let spool = Spool::new(bytes)?;
    let options = OpenOptions {
        max_allocate_mb: budget.memory_limit_mb(),
        max_runtime_secs: 0,
        list_only: out.list_only(),
    };
    let (mut archive, info) =
        Archive::open(spool.path(), options).map_err(|c| map_code(c, false))?;
    if info.has_encrypted_headers() {
        return Err(Error::new(
            ErrorCode::Encrypted,
            "rar with encrypted headers",
        ));
    }

    let mut missing_solid_window = info.is_volume() && info.is_solid() && !info.is_first_volume();
    let mut saw_split = info.is_volume();
    let mut partial = info.is_volume();

    loop {
        if budget.is_cancelled() {
            break;
        }
        let Some(header) = next_header(&mut archive, saw_split, &mut partial, out)? else {
            break;
        };
        budget.count_entry()?;
        // An entry continuing into the next volume is the last entry of this volume, and incomplete.
        if header.is_split_after() {
            saw_split = true;
            partial = true;
            break;
        }
        // An entry started in a previous volume is incomplete here; skip past its tail bytes.
        if header.is_split_before() {
            saw_split = true;
            partial = true;
            if info.is_solid() {
                missing_solid_window = true;
            }
            if !skip_incomplete_volume_entry(&mut archive, out)? {
                break;
            }
            continue;
        }
        if missing_solid_window {
            if header.is_solid() {
                partial = true;
                if !skip_incomplete_volume_entry(&mut archive, out)? {
                    break;
                }
                continue;
            }
            missing_solid_window = false;
        }

        let kind = classify(&header);
        let declared = header.unpacked_size;
        let (method, dict_size_kb) = (header.method, header.dict_size_kb);
        let name = header
            .raw_name
            .as_deref()
            .map_or(header.name, decode_legacy_name);
        let Some(path) = out.admit(name, kind, declared) else {
            let solid_file = !out.list_only() && info.is_solid() && kind == Kind::File;
            match skip_unextracted(
                &mut archive,
                solid_file,
                (method, dict_size_kb),
                budget,
                saw_split,
                out,
            )? {
                Step::Ok(()) => continue,
                Step::PartialEnd => {
                    partial = true;
                    break;
                }
                Step::Cancelled => break,
            }
        };
        check_dict_limit(method, dict_size_kb, budget)?;
        let data = match extract_entry(&mut archive, declared, budget, saw_split, out)? {
            Step::Ok(data) => data,
            Step::PartialEnd => {
                partial = true;
                break;
            }
            Step::Cancelled => break,
        };
        if out.entry(path, data).is_break() {
            break;
        }
    }

    if partial {
        finish_partial(saw_split, out)?;
    }
    Ok(())
}

fn next_header(
    archive: &mut Archive,
    saw_split: bool,
    partial: &mut bool,
    out: &Out<'_>,
) -> Result<Option<Header>, Error> {
    match archive.read_header() {
        Ok(header) => Ok(header),
        Err(code) => {
            if can_stop_after_partial(archive, code, saw_split, out) {
                *partial = true;
                return Ok(None);
            }
            if saw_split && !has_recovered(out) {
                return Err(Error::new(ErrorCode::MultiVolume, "multi-volume rar"));
            }
            Err(map_code(code, archive.password_requested()))
        }
    }
}

fn finish_partial(saw_split: bool, out: &mut Out<'_>) -> Result<(), Error> {
    if !has_recovered(out) {
        return if saw_split {
            Err(Error::new(ErrorCode::MultiVolume, "multi-volume rar"))
        } else {
            Err(Error::corrupt("truncated rar with no complete entries"))
        };
    }
    out.recovered = true;
    Ok(())
}

fn has_recovered(out: &Out<'_>) -> bool {
    out.emitted > 0 || !out.listed.is_empty()
}

fn can_stop_after_partial(archive: &Archive, code: Code, saw_split: bool, out: &Out<'_>) -> bool {
    if archive.password_requested() {
        return false;
    }
    if saw_split && code.0 == unrar_sys::ERAR_EOPEN {
        return true;
    }
    archive.is_truncated() && has_recovered(out)
}

fn skip_unextracted(
    archive: &mut Archive,
    solid_file: bool,
    (method, dict_size_kb): (u32, u32),
    budget: &mut Budget<'_>,
    saw_split: bool,
    out: &Out<'_>,
) -> Result<Step<()>, Error> {
    if solid_file {
        check_dict_limit(method, dict_size_kb, budget)?;
        drain_solid(archive, budget, saw_split, out)
    } else {
        skip_entry(archive, saw_split, out)
    }
}

fn skip_entry(archive: &mut Archive, saw_split: bool, out: &Out<'_>) -> Result<Step<()>, Error> {
    match archive.skip() {
        Ok(()) => Ok(Step::Ok(())),
        Err(code) if can_stop_after_partial(archive, code, saw_split, out) => Ok(Step::PartialEnd),
        Err(code) => Err(map_code(code, archive.password_requested())),
    }
}

/// Skips a `SplitBefore` entry or a solid entry whose dictionary lives in a missing prior volume.
/// In the last volume of a solid set UnRAR still compares the (uncomputed) CRC of a `SplitBefore`
/// entry and returns `ERAR_BAD_DATA` after advancing the cursor to the next block.
fn skip_incomplete_volume_entry(archive: &mut Archive, out: &Out<'_>) -> Result<bool, Error> {
    match archive.skip() {
        Ok(()) => Ok(true),
        Err(code) if !archive.password_requested() && code.0 == unrar_sys::ERAR_BAD_DATA => {
            Ok(true)
        }
        Err(code) if can_stop_after_partial(archive, code, true, out) => Ok(false),
        Err(code) => Err(map_code(code, archive.password_requested())),
    }
}

enum Step<T> {
    Ok(T),
    PartialEnd,
    Cancelled,
}

fn extract_entry(
    archive: &mut Archive,
    declared: u64,
    budget: &mut Budget<'_>,
    saw_split: bool,
    out: &Out<'_>,
) -> Result<Step<Vec<u8>>, Error> {
    if budget.is_cancelled() {
        return Ok(Step::Cancelled);
    }
    let mut data = Vec::new();
    let mut failure: Option<Error> = None;
    let mut cancelled = false;
    if data.try_reserve_exact(budget.hint(declared)).is_err() {
        return Err(Error::new(ErrorCode::Io, "out of memory"));
    }
    let result = archive.test(&mut |chunk| {
        if budget.is_cancelled() {
            cancelled = true;
            return Flow::Abort;
        }
        if data.try_reserve(chunk.len()).is_err() {
            failure = Some(Error::new(ErrorCode::Io, "out of memory"));
            return Flow::Abort;
        }
        data.extend_from_slice(chunk);
        match budget.add(chunk.len() as u64, data.len() as u64) {
            Ok(()) => Flow::Continue,
            Err(e) => {
                failure = Some(e);
                Flow::Abort
            }
        }
    });
    if cancelled {
        return Ok(Step::Cancelled);
    }
    if let Some(e) = failure {
        return Err(e);
    }
    match result {
        Ok(()) => Ok(Step::Ok(data)),
        Err(code) if can_stop_after_partial(archive, code, saw_split, out) => Ok(Step::PartialEnd),
        Err(code) => Err(map_code(code, archive.password_requested())),
    }
}

fn check_dict_limit(method: u32, dict_size_kb: u32, budget: &Budget<'_>) -> Result<(), Error> {
    if method != METHOD_STORED {
        let dict_bytes = u64::from(dict_size_kb) * 1024;
        if dict_bytes > budget.memory_limit_bytes() {
            return Err(Error::too_large(format!(
                "rar dictionary of {dict_bytes} bytes is above the memory limit"
            )));
        }
    }
    Ok(())
}

/// In a solid RAR, `RAR_SKIP` still decompresses the entry (without calling `UCM_PROCESSDATA`), so
/// a skipped `../bomb` would bypass `Budget`. `RAR_TEST` streams chunks through the budget without
/// buffering them.
fn drain_solid(
    archive: &mut Archive,
    budget: &mut Budget<'_>,
    saw_split: bool,
    out: &Out<'_>,
) -> Result<Step<()>, Error> {
    if budget.is_cancelled() {
        return Ok(Step::Cancelled);
    }
    let mut entry: u64 = 0;
    let mut failure: Option<Error> = None;
    let mut cancelled = false;
    let result = archive.test(&mut |chunk| {
        if budget.is_cancelled() {
            cancelled = true;
            return Flow::Abort;
        }
        entry = entry.saturating_add(chunk.len() as u64);
        match budget.add(chunk.len() as u64, entry) {
            Ok(()) => Flow::Continue,
            Err(e) => {
                failure = Some(e);
                Flow::Abort
            }
        }
    });
    if cancelled {
        return Ok(Step::Cancelled);
    }
    if let Some(e) = failure {
        return Err(e);
    }
    match result {
        Ok(()) => Ok(Step::Ok(())),
        Err(code) if can_stop_after_partial(archive, code, saw_split, out) => Ok(Step::PartialEnd),
        Err(code) => Err(map_code(code, archive.password_requested())),
    }
}

/// RAR 1.5-4 names without the Unicode flag are in the creator's charset: UTF-8 when made on Unix
/// (what `bsdtar` assumes too), the OEM code page when made by DOS/old Windows RAR, which for us
/// means CP850.
fn decode_legacy_name(raw: &[u8]) -> String {
    match std::str::from_utf8(raw) {
        Ok(name) => name.to_owned(),
        Err(_) => {
            oem_cp::decode_string_complete_table(raw, &oem_cp::code_table::DECODING_TABLE_CP850)
        }
    }
}

fn classify(header: &Header) -> Kind {
    if header.is_directory() {
        return Kind::Dir;
    }
    match header.redir_type {
        0 => {}
        FSREDIR_HARDLINK | FSREDIR_FILECOPY => return Kind::Other,
        _ => return Kind::Symlink,
    }
    if header.host_os == HOST_UNIX {
        match header.file_attr & S_IFMT {
            0 | S_IFREG => {}
            S_IFDIR => return Kind::Dir,
            S_IFLNK => return Kind::Symlink,
            _ => return Kind::Other,
        }
    }
    if header.is_encrypted() {
        Kind::Encrypted
    } else {
        Kind::File
    }
}

fn map_code(code: Code, password_requested: bool) -> Error {
    use unrar_sys::{
        ERAR_BAD_PASSWORD, ERAR_ECLOSE, ERAR_ECREATE, ERAR_EOPEN, ERAR_EWRITE, ERAR_LARGE_DICT,
        ERAR_MISSING_PASSWORD, ERAR_NO_MEMORY, ERAR_UNKNOWN_FORMAT, ERAR_USER_LIMIT,
    };
    let kind = match code.0 {
        _ if password_requested => ErrorCode::Encrypted,
        ERAR_MISSING_PASSWORD | ERAR_BAD_PASSWORD => ErrorCode::Encrypted,
        ERAR_UNKNOWN_FORMAT => ErrorCode::Unsupported,
        ERAR_LARGE_DICT | ERAR_USER_LIMIT => ErrorCode::TooLarge,
        ERAR_EOPEN | ERAR_ECREATE | ERAR_ECLOSE | ERAR_EWRITE | ERAR_NO_MEMORY => ErrorCode::Io,
        _ => ErrorCode::Corrupt,
    };
    Error::new(kind, code.describe())
}

/// The archive bytes as a file UnRAR can open by name. Removed on drop.
enum Spool {
    #[cfg(target_os = "linux")]
    Memfd {
        _file: File,
        path: PathBuf,
    },
    Temp(tempfile::NamedTempFile),
}

impl Spool {
    fn new(bytes: &[u8]) -> Result<Self, Error> {
        #[cfg(target_os = "linux")]
        if let Some(spool) = Self::memfd(bytes) {
            return Ok(spool);
        }
        let io = |e: std::io::Error| Error::new(ErrorCode::Io, format!("temp file for rar: {e}"));
        let mut file = tempfile::Builder::new()
            .prefix("anyzip-")
            .suffix(".rar")
            .tempfile()
            .map_err(io)?;
        file.write_all(bytes).map_err(io)?;
        file.flush().map_err(io)?;
        Ok(Self::Temp(file))
    }

    #[cfg(target_os = "linux")]
    fn memfd(bytes: &[u8]) -> Option<Self> {
        use std::os::fd::AsRawFd;
        let fd = rustix::fs::memfd_create("anyzip.rar", rustix::fs::MemfdFlags::CLOEXEC).ok()?;
        let mut file = File::from(fd);
        file.write_all(bytes).ok()?;
        let path = PathBuf::from(format!("/proc/self/fd/{}", file.as_raw_fd()));
        path.exists().then_some(Self::Memfd { _file: file, path })
    }

    fn path(&self) -> &Path {
        match self {
            #[cfg(target_os = "linux")]
            Self::Memfd { path, .. } => path,
            Self::Temp(file) => file.path(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rar5_block(body: &[u8]) -> Vec<u8> {
        let len = u8::try_from(body.len()).unwrap();
        let mut crc_input = Vec::with_capacity(1 + body.len());
        crc_input.push(len);
        crc_input.extend_from_slice(body);
        let crc = crc32fast::hash(&crc_input);
        let mut out = Vec::with_capacity(4 + crc_input.len());
        out.extend_from_slice(&crc.to_le_bytes());
        out.extend_from_slice(&crc_input);
        out
    }

    #[test]
    fn list_and_extract_agree_on_split_entry_without_volume_flag() {
        // RAR5 archive with MHFL_VOLUME = 0 in the main header, but HFL_SPLITBEFORE (0x08) on a
        // file block. Both `list` and `extract` must reject it as `MultiVolume`.
        let mut rar = b"Rar!\x1A\x07\x01\x00".to_vec();
        rar.extend_from_slice(&rar5_block(&[1, 0, 0])); // main header: flags=0, arc_flags=0
        // file header: type=2, flags=HFL_DATA(0x02)|HFL_SPLITBEFORE(0x08)=0x0A, data_size=0,
        // file_flags=0, unp_size=1, attr=0, comp=0, host_os=0, name_len=1, name='a'
        rar.extend_from_slice(&rar5_block(&[2, 0x0A, 0, 0, 1, 0, 0, 0, 1, b'a']));
        rar.extend_from_slice(&rar5_block(&[5, 0, 0])); // end-of-archive header

        assert_eq!(
            crate::extract(&rar, &crate::Limits::default())
                .unwrap_err()
                .code,
            ErrorCode::MultiVolume
        );
        assert_eq!(
            crate::list(&rar, &crate::Limits::default())
                .unwrap_err()
                .code,
            ErrorCode::MultiVolume
        );
    }
}
