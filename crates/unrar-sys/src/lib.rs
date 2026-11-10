//! UnRAR (RARLAB) compiled from `vendor/unrar`, with raw bindings to the DLL API (`dll.hpp`) and a
//! small safe wrapper, [`Archive`], so the `anyzip` core can stay `#![forbid(unsafe_code)]`.
//!
//! The wrapper never writes to disk: entries are read with `RAR_TEST`, and the data arrives
//! through the `UCM_PROCESSDATA` callback.

// UnRAR license notice (Clause 2 of `LICENSE-UNRAR.txt`):
// UnRAR source code may be used in any software to handle RAR archives without limitations free
// of charge, but cannot be used to develop RAR (WinRAR) compatible archiver and to re-create RAR
// compression algorithm, which is proprietary. Distribution of modified UnRAR source code in
// separate form or as a part of other software is permitted, provided that full text of this
// paragraph, starting from "UnRAR source code" words, is included in license, or in documentation
// if license is not available, and in source code comments of resulting package.

use std::ffi::{CString, c_int, c_long, c_uint, c_void};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::Path;
use std::ptr::{self, NonNull};

pub use libc::wchar_t;

pub const ERAR_SUCCESS: c_int = 0;
pub const ERAR_END_ARCHIVE: c_int = 10;
pub const ERAR_NO_MEMORY: c_int = 11;
pub const ERAR_BAD_DATA: c_int = 12;
pub const ERAR_BAD_ARCHIVE: c_int = 13;
pub const ERAR_UNKNOWN_FORMAT: c_int = 14;
pub const ERAR_EOPEN: c_int = 15;
pub const ERAR_ECREATE: c_int = 16;
pub const ERAR_ECLOSE: c_int = 17;
pub const ERAR_EREAD: c_int = 18;
pub const ERAR_EWRITE: c_int = 19;
pub const ERAR_SMALL_BUF: c_int = 20;
pub const ERAR_UNKNOWN: c_int = 21;
pub const ERAR_MISSING_PASSWORD: c_int = 22;
pub const ERAR_EREFERENCE: c_int = 23;
pub const ERAR_BAD_PASSWORD: c_int = 24;
pub const ERAR_LARGE_DICT: c_int = 25;
pub const ERAR_USER_LIMIT: c_int = 26;

pub const RAR_OM_LIST: c_uint = 0;
pub const RAR_OM_EXTRACT: c_uint = 1;
pub const RAR_OM_LIST_INCSPLIT: c_uint = 2;

pub const RAR_SKIP: c_int = 0;
pub const RAR_TEST: c_int = 1;
pub const RAR_EXTRACT: c_int = 2;

pub const RHDF_SPLITBEFORE: c_uint = 0x01;
pub const RHDF_SPLITAFTER: c_uint = 0x02;
pub const RHDF_ENCRYPTED: c_uint = 0x04;
pub const RHDF_SOLID: c_uint = 0x10;
pub const RHDF_DIRECTORY: c_uint = 0x20;

pub const ROADF_VOLUME: c_uint = 0x0001;
pub const ROADF_SOLID: c_uint = 0x0008;
pub const ROADF_ENCHEADERS: c_uint = 0x0080;
pub const ROADF_FIRSTVOLUME: c_uint = 0x0100;

pub const UCM_CHANGEVOLUME: c_uint = 0;
pub const UCM_PROCESSDATA: c_uint = 1;
pub const UCM_NEEDPASSWORD: c_uint = 2;
pub const UCM_CHANGEVOLUMEW: c_uint = 3;
pub const UCM_NEEDPASSWORDW: c_uint = 4;
pub const UCM_LARGEDICT: c_uint = 5;

/// `FSREDIR_*` in `headers.hpp`.
pub const FSREDIR_NONE: c_uint = 0;

#[cfg(unix)]
pub type Lparam = c_long;
#[cfg(windows)]
pub type Lparam = isize;

pub type UnrarCallback =
    Option<unsafe extern "C" fn(msg: c_uint, user_data: Lparam, p1: Lparam, p2: Lparam) -> c_int>;

#[repr(C, packed)]
pub struct RAROpenArchiveDataEx {
    pub arc_name: *mut libc::c_char,
    pub arc_name_w: *mut wchar_t,
    pub open_mode: c_uint,
    pub open_result: c_uint,
    pub cmt_buf: *mut libc::c_char,
    pub cmt_buf_size: c_uint,
    pub cmt_size: c_uint,
    pub cmt_state: c_uint,
    pub flags: c_uint,
    pub callback: UnrarCallback,
    pub user_data: Lparam,
    pub op_flags: c_uint,
    pub cmt_buf_w: *mut wchar_t,
    pub mark_of_the_web: *mut wchar_t,
    pub max_disk_write: c_uint,
    pub max_allocate: c_uint,
    pub max_runtime: c_uint,
    pub reserved: [c_uint; 20],
}

#[repr(C, packed)]
pub struct RARHeaderDataEx {
    pub arc_name: [libc::c_char; 1024],
    pub arc_name_w: [wchar_t; 1024],
    pub file_name: [libc::c_char; 1024],
    pub file_name_w: [wchar_t; 1024],
    pub flags: c_uint,
    pub pack_size: c_uint,
    pub pack_size_high: c_uint,
    pub unp_size: c_uint,
    pub unp_size_high: c_uint,
    pub host_os: c_uint,
    pub file_crc: c_uint,
    pub file_time: c_uint,
    pub unp_ver: c_uint,
    pub method: c_uint,
    pub file_attr: c_uint,
    pub cmt_buf: *mut libc::c_char,
    pub cmt_buf_size: c_uint,
    pub cmt_size: c_uint,
    pub cmt_state: c_uint,
    pub dict_size: c_uint,
    pub hash_type: c_uint,
    pub hash: [libc::c_char; 32],
    pub redir_type: c_uint,
    pub redir_name: *mut wchar_t,
    pub redir_name_size: c_uint,
    pub dir_target: c_uint,
    pub mtime_low: c_uint,
    pub mtime_high: c_uint,
    pub ctime_low: c_uint,
    pub ctime_high: c_uint,
    pub atime_low: c_uint,
    pub atime_high: c_uint,
    pub arc_name_ex: *mut wchar_t,
    pub arc_name_ex_size: c_uint,
    pub file_name_ex: *mut wchar_t,
    pub file_name_ex_size: c_uint,
    pub reserved: [c_uint; 982],
}

unsafe extern "C" {
    fn anyzip_rar_open(data: *mut RAROpenArchiveDataEx) -> *mut c_void;
    fn anyzip_rar_close(handle: *mut c_void) -> c_int;
    fn anyzip_rar_read_header(handle: *mut c_void, data: *mut RARHeaderDataEx) -> c_int;
    fn anyzip_rar_process_file(
        handle: *mut c_void,
        operation: c_int,
        dest_path: *mut wchar_t,
        dest_name: *mut wchar_t,
    ) -> c_int;
    fn anyzip_rar_is_truncated(handle: *mut c_void) -> c_int;
    fn anyzip_rar_dll_version() -> c_int;
    #[cfg(test)]
    fn anyzip_sizeof_header_data_ex() -> usize;
    #[cfg(test)]
    fn anyzip_sizeof_open_archive_data_ex() -> usize;
}

// ---------------------------------------------------------------------------------------------
// Safe wrapper
// ---------------------------------------------------------------------------------------------

/// Longest name we ask UnRAR for (`FileNameEx`). RAR5 allows 2048 UTF-8 bytes per name.
const NAME_CAPACITY: usize = 4096;

/// A non-`ERAR_SUCCESS` code from the DLL.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Code(pub c_int);

impl Code {
    #[must_use]
    pub fn describe(self) -> &'static str {
        match self.0 {
            ERAR_END_ARCHIVE => "end of archive",
            ERAR_NO_MEMORY => "not enough memory",
            ERAR_BAD_DATA => "bad data (checksum failed or archive broken)",
            ERAR_BAD_ARCHIVE => "not a valid RAR archive",
            ERAR_UNKNOWN_FORMAT => "unknown archive format or compression method",
            ERAR_EOPEN => "cannot open archive (or next volume)",
            ERAR_ECREATE => "cannot create file",
            ERAR_ECLOSE => "cannot close file",
            ERAR_EREAD => "read error",
            ERAR_EWRITE => "write error",
            ERAR_SMALL_BUF => "buffer too small",
            ERAR_UNKNOWN => "unexpected archive warning or user abort",
            ERAR_MISSING_PASSWORD => "password required",
            ERAR_EREFERENCE => "cannot resolve file reference",
            ERAR_BAD_PASSWORD => "wrong password",
            ERAR_LARGE_DICT => "dictionary larger than allowed",
            ERAR_USER_LIMIT => "limit exceeded (memory or runtime)",
            _ => "unknown UnRAR error",
        }
    }
}

/// Limits UnRAR enforces itself (RAR 7.x DLL API, `RAROpenArchiveDataEx`).
#[derive(Debug, Clone, Copy, Default)]
pub struct OpenOptions {
    /// Maximum dictionary allocation, in MiB. 0 means UnRAR's default.
    pub max_allocate_mb: u32,
    /// Maximum wall time for the whole archive, in seconds. 0 means none.
    pub max_runtime_secs: u32,
    /// Open in `RAR_OM_LIST_INCSPLIT` mode: headers only (including split headers so callers can
    /// detect multi-volume entries), skipping is cheap even in solid archives, and
    /// [`Archive::test`] is not available.
    pub list_only: bool,
}

/// Archive-level flags from `RAROpenArchiveEx`.
#[derive(Debug, Clone, Copy)]
pub struct OpenInfo {
    pub flags: u32,
}

impl OpenInfo {
    #[must_use]
    pub fn is_volume(self) -> bool {
        self.flags & ROADF_VOLUME != 0
    }
    #[must_use]
    pub fn is_first_volume(self) -> bool {
        self.flags & ROADF_FIRSTVOLUME != 0
    }
    #[must_use]
    pub fn has_encrypted_headers(self) -> bool {
        self.flags & ROADF_ENCHEADERS != 0
    }
    #[must_use]
    pub fn is_solid(self) -> bool {
        self.flags & ROADF_SOLID != 0
    }
}

/// One file header.
#[derive(Debug, Clone)]
pub struct Header {
    /// Name as UTF-16/UTF-32 code units decoded to a `String` (lossy only for unpaired surrogates).
    pub name: String,
    /// Set when UnRAR could not convert a legacy (RAR 1.5-4, no Unicode flag) name with the
    /// process locale: the raw name bytes, recovered from UnRAR's private-use mapping
    /// (U+FFFE mark, then byte `b` as U+E000+b). The caller picks the charset.
    pub raw_name: Option<Vec<u8>>,
    pub flags: u32,
    pub unpacked_size: u64,
    pub packed_size: u64,
    pub file_attr: u32,
    pub host_os: u32,
    /// `FSREDIR_*`: 0 none, 1 unix symlink, 2 windows symlink, 3 junction, 4 hardlink, 5 file copy.
    pub redir_type: u32,
    /// `0x30` is stored (uncompressed); `0x31..=0x35` are compressed.
    pub method: u32,
    pub dict_size_kb: u32,
}

impl Header {
    #[must_use]
    pub fn is_directory(&self) -> bool {
        self.flags & RHDF_DIRECTORY != 0
    }
    #[must_use]
    pub fn is_encrypted(&self) -> bool {
        self.flags & RHDF_ENCRYPTED != 0
    }
    #[must_use]
    pub fn is_solid(&self) -> bool {
        self.flags & RHDF_SOLID != 0
    }
    #[must_use]
    pub fn is_split_before(&self) -> bool {
        self.flags & RHDF_SPLITBEFORE != 0
    }
    #[must_use]
    pub fn is_split_after(&self) -> bool {
        self.flags & RHDF_SPLITAFTER != 0
    }
    #[must_use]
    pub fn is_split(&self) -> bool {
        self.flags & (RHDF_SPLITBEFORE | RHDF_SPLITAFTER) != 0
    }
}

/// What the data sink tells UnRAR after each chunk.
pub enum Flow {
    Continue,
    Abort,
}

type Sink<'a> = dyn FnMut(&[u8]) -> Flow + 'a;

struct CallbackState {
    /// Set only for the duration of [`Archive::test`]. Lifetime erased; see the SAFETY note there.
    sink: Option<NonNull<Sink<'static>>>,
    /// The sink panicked; the panic was caught inside the callback and UnRAR was told to abort.
    sink_panicked: bool,
    /// UnRAR asked for a password (encrypted entry or encrypted headers).
    password_requested: bool,
    /// UnRAR asked for the next volume.
    volume_requested: bool,
}

/// An open RAR archive; entries are delivered in memory.
pub struct Archive {
    handle: NonNull<c_void>,
    state: Box<CallbackState>,
    header: Box<RARHeaderDataEx>,
    name: Box<[wchar_t; NAME_CAPACITY]>,
}

// SAFETY: the handle is owned exclusively by this value; UnRAR keeps no thread affinity for a
// handle (the only global state, `ErrHandler`, is patched to `thread_local` in build.rs and is
// reset by every DLL call that uses it).
unsafe impl Send for Archive {}

impl Archive {
    /// Opens `path` for extraction.
    ///
    /// # Errors
    /// Returns the DLL's `OpenResult` when the archive cannot be opened.
    pub fn open(path: &Path, options: OpenOptions) -> Result<(Self, OpenInfo), Code> {
        let path_c = path_to_cstring(path).ok_or(Code(ERAR_EOPEN))?;
        let mut state = Box::new(CallbackState {
            sink: None,
            sink_panicked: false,
            password_requested: false,
            volume_requested: false,
        });
        // SAFETY: an all-zero RAROpenArchiveDataEx is valid (null pointers, None callback, zeros),
        // and the DLL documentation requires Reserved to be zero.
        let mut data: RAROpenArchiveDataEx = unsafe { std::mem::zeroed() };
        data.arc_name = path_c.as_ptr().cast_mut();
        data.open_mode = if options.list_only {
            RAR_OM_LIST_INCSPLIT
        } else {
            RAR_OM_EXTRACT
        };
        data.callback = Some(callback);
        data.user_data = ptr::from_mut::<CallbackState>(&mut state) as Lparam;
        data.max_allocate = options.max_allocate_mb;
        data.max_runtime = options.max_runtime_secs;
        // SAFETY: `data` is a valid, initialized struct; `path_c` and `state` outlive the call, and
        // `state` (boxed, never moved) outlives the handle because both live in `Archive`.
        let handle = unsafe { anyzip_rar_open(&raw mut data) };
        let flags = data.flags;
        let Some(handle) = NonNull::new(handle) else {
            let result = data.open_result;
            #[expect(clippy::cast_possible_wrap, reason = "ERAR codes are small")]
            let code = if result == 0 {
                ERAR_UNKNOWN
            } else {
                result as c_int
            };
            return Err(Code(code));
        };
        // SAFETY: zeroed header is valid; pointers are filled below.
        let mut header: Box<RARHeaderDataEx> = Box::new(unsafe { std::mem::zeroed() });
        let mut name = Box::new([0 as wchar_t; NAME_CAPACITY]);
        header.file_name_ex = name.as_mut_ptr();
        #[expect(clippy::cast_possible_truncation, reason = "NAME_CAPACITY is 4096")]
        {
            header.file_name_ex_size = NAME_CAPACITY as c_uint;
        }
        Ok((
            Self {
                handle,
                state,
                header,
                name,
            },
            OpenInfo { flags },
        ))
    }

    /// True if UnRAR asked for a password since the archive was opened.
    #[must_use]
    pub fn password_requested(&self) -> bool {
        self.state.password_requested
    }

    /// True if UnRAR asked for another volume since the archive was opened.
    #[must_use]
    pub fn volume_requested(&self) -> bool {
        self.state.volume_requested
    }

    /// True if UnRAR's current block or file cursor ran into or past EOF (truncated archive).
    #[must_use]
    pub fn is_truncated(&self) -> bool {
        // SAFETY: handle is open and valid.
        unsafe { anyzip_rar_is_truncated(self.handle.as_ptr()) != 0 }
    }

    /// Reads the next header. `Ok(None)` at the end of the archive.
    ///
    /// # Errors
    /// The DLL's error code.
    pub fn read_header(&mut self) -> Result<Option<Header>, Code> {
        self.name[0] = 0;
        // FileNameEx must keep pointing at our buffer (the struct is reused).
        self.header.file_name_ex = self.name.as_mut_ptr();
        // SAFETY: handle is open; header is a valid struct whose FileNameEx points to a live buffer
        // of FileNameExSize wide chars, which UnRAR fills with a NUL-terminated copy (wcsncpyz).
        let code = unsafe { anyzip_rar_read_header(self.handle.as_ptr(), &raw mut *self.header) };
        match code {
            ERAR_SUCCESS => {}
            ERAR_END_ARCHIVE => return Ok(None),
            other => return Err(Code(other)),
        }
        let h = &*self.header;
        let len = self
            .name
            .iter()
            .position(|&c| c == 0)
            .unwrap_or(NAME_CAPACITY);
        let wide = &self.name[..len];
        Ok(Some(Header {
            name: wide_to_string(wide),
            raw_name: unmap_legacy_name(wide),
            flags: h.flags,
            unpacked_size: u64::from(h.unp_size) | (u64::from(h.unp_size_high) << 32),
            packed_size: u64::from(h.pack_size) | (u64::from(h.pack_size_high) << 32),
            file_attr: h.file_attr,
            host_os: h.host_os,
            redir_type: h.redir_type,
            method: h.method,
            dict_size_kb: h.dict_size,
        }))
    }

    /// Skips the current entry without decompressing (UnRAR still decodes in solid archives).
    ///
    /// # Errors
    /// The DLL's error code.
    pub fn skip(&mut self) -> Result<(), Code> {
        // SAFETY: handle is open; null dest pointers are allowed for RAR_SKIP.
        let code = unsafe {
            anyzip_rar_process_file(
                self.handle.as_ptr(),
                RAR_SKIP,
                ptr::null_mut(),
                ptr::null_mut(),
            )
        };
        if code == ERAR_SUCCESS {
            Ok(())
        } else {
            Err(Code(code))
        }
    }

    /// Decompresses the current entry and verifies its checksum, handing the data to `sink`
    /// chunk by chunk. Nothing is written to disk.
    ///
    /// # Errors
    /// The DLL's error code. When `sink` returns [`Flow::Abort`] the code is `ERAR_UNKNOWN`
    /// (UnRAR reports a user break); the caller knows why it aborted. If `sink` panics, the panic
    /// is caught, the extraction is aborted and the panic resumes after UnRAR returns.
    pub fn test(&mut self, sink: &mut dyn FnMut(&[u8]) -> Flow) -> Result<(), Code> {
        // SAFETY (lifetime erasure): the pointer is stored only for this call and cleared before
        // returning; the callback only runs inside RARProcessFileW below, while `sink` is borrowed.
        let erased: NonNull<Sink<'static>> = unsafe {
            std::mem::transmute::<NonNull<Sink<'_>>, NonNull<Sink<'static>>>(NonNull::from(sink))
        };
        self.state.sink = Some(erased);
        self.state.sink_panicked = false;
        // SAFETY: handle is open; null dest pointers are allowed for RAR_TEST.
        let code = unsafe {
            anyzip_rar_process_file(
                self.handle.as_ptr(),
                RAR_TEST,
                ptr::null_mut(),
                ptr::null_mut(),
            )
        };
        self.state.sink = None;
        if self.state.sink_panicked {
            std::panic::resume_unwind(Box::new("anyzip: RAR data sink panicked"));
        }
        if code == ERAR_SUCCESS {
            Ok(())
        } else {
            Err(Code(code))
        }
    }
}

impl Drop for Archive {
    fn drop(&mut self) {
        // SAFETY: handle was returned by anyzip_rar_open and is closed exactly once.
        unsafe {
            anyzip_rar_close(self.handle.as_ptr());
        }
    }
}

/// The single callback UnRAR calls. Never unwinds.
unsafe extern "C" fn callback(msg: c_uint, user_data: Lparam, p1: Lparam, p2: Lparam) -> c_int {
    // SAFETY: user_data is the `Box<CallbackState>` pointer set in `Archive::open`; the box lives
    // as long as the handle, and UnRAR calls back only from inside our DLL calls (same thread).
    let Some(state) = (unsafe { (user_data as *mut CallbackState).as_mut() }) else {
        return -1;
    };
    match msg {
        UCM_PROCESSDATA => {
            let Some(mut sink) = state.sink else {
                return -1;
            };
            let Ok(len) = usize::try_from(p2) else {
                return -1;
            };
            let data: &[u8] = if len == 0 || p1 == 0 {
                &[]
            } else {
                // SAFETY: UnRAR passes a buffer of `p2` bytes at `p1`, valid during the call.
                unsafe { std::slice::from_raw_parts(p1 as *const u8, len) }
            };
            // SAFETY: see `Archive::test`; the sink is alive and exclusively borrowed.
            let sink = unsafe { sink.as_mut() };
            match catch_unwind(AssertUnwindSafe(|| sink(data))) {
                Ok(Flow::Continue) => 1,
                Ok(Flow::Abort) => -1,
                Err(_) => {
                    state.sink_panicked = true;
                    -1
                }
            }
        }
        UCM_NEEDPASSWORD | UCM_NEEDPASSWORDW => {
            state.password_requested = true;
            -1
        }
        UCM_CHANGEVOLUME | UCM_CHANGEVOLUMEW => {
            state.volume_requested = true;
            -1
        }
        // Including UCM_LARGEDICT (dictionary above UnRAR's default limit): refused here, while
        // MaxAllocate is the real guard.
        _ => 0,
    }
}

#[cfg(unix)]
fn path_to_cstring(path: &Path) -> Option<CString> {
    use std::os::unix::ffi::OsStrExt;
    CString::new(path.as_os_str().as_bytes()).ok()
}

#[cfg(not(unix))]
fn path_to_cstring(path: &Path) -> Option<CString> {
    CString::new(path.to_str()?).ok()
}

#[cfg(windows)]
fn wchar_code(c: wchar_t) -> u32 {
    u32::from(c)
}

/// On Unix `wchar_t` is 32-bit, signed (`i32`) on `x86_64`/macOS and unsigned (`u32`) on ARM64 Linux.
#[cfg(not(windows))]
fn wchar_code(c: wchar_t) -> u32 {
    u32::from_ne_bytes(c.to_ne_bytes())
}

/// `wchar_t` is UTF-32 on Unix and UTF-16 on Windows. Invalid code points become U+FFFD.
fn wide_to_string(wide: &[wchar_t]) -> String {
    #[cfg(windows)]
    {
        String::from_utf16_lossy(&wide.iter().copied().collect::<Vec<_>>())
    }
    #[cfg(not(windows))]
    {
        // UnRAR on Unix stores surrogate pairs as two wchar_t when the source had them (RAR4
        // names encoded in UTF-16); join them before converting.
        let mut out = String::with_capacity(wide.len());
        let mut i = 0;
        while i < wide.len() {
            let c = wchar_code(wide[i]);
            if (0xD800..0xDC00).contains(&c)
                && let Some(&next) = wide.get(i + 1)
            {
                let low = wchar_code(next);
                if (0xDC00..0xE000).contains(&low) {
                    let cp = 0x10000 + ((c - 0xD800) << 10) + (low - 0xDC00);
                    out.push(char::from_u32(cp).unwrap_or(char::REPLACEMENT_CHARACTER));
                    i += 2;
                    continue;
                }
            }
            out.push(char::from_u32(c).unwrap_or(char::REPLACEMENT_CHARACTER));
            i += 1;
        }
        out
    }
}

/// `CharToWideMap` in unicode.cpp: on a failed multibyte conversion UnRAR writes U+FFFE once, then
/// each inconvertible byte `b >= 0x80` as `0xE000 + b`. Undo it to get the stored bytes back.
fn unmap_legacy_name(wide: &[wchar_t]) -> Option<Vec<u8>> {
    const MARK: u32 = 0xFFFE;
    const AREA: u32 = 0xE000;
    if !wide.iter().any(|&c| wchar_code(c) == MARK) {
        return None;
    }
    let mut bytes = Vec::with_capacity(wide.len() * 2);
    let mut buf = [0u8; 4];
    for &c in wide {
        let c = wchar_code(c);
        if c == MARK {
            continue;
        }
        if (AREA + 0x80..=AREA + 0xFF).contains(&c) {
            #[expect(clippy::cast_possible_truncation, reason = "range checked above")]
            bytes.push((c - AREA) as u8);
        } else {
            let ch = char::from_u32(c).unwrap_or(char::REPLACEMENT_CHARACTER);
            bytes.extend_from_slice(ch.encode_utf8(&mut buf).as_bytes());
        }
    }
    Some(bytes)
}

/// UnRAR DLL API version (`RAR_DLL_VERSION`).
#[must_use]
pub fn dll_version() -> i32 {
    // SAFETY: no arguments, no state.
    unsafe { anyzip_rar_dll_version() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn struct_sizes_match_dll_hpp() {
        // SAFETY: the probes only return sizeof.
        let (header, open) = unsafe {
            (
                anyzip_sizeof_header_data_ex(),
                anyzip_sizeof_open_archive_data_ex(),
            )
        };
        assert_eq!(size_of::<RARHeaderDataEx>(), header);
        assert_eq!(size_of::<RAROpenArchiveDataEx>(), open);
        assert!(dll_version() >= 9);
    }

    #[test]
    fn legacy_name_unmapping() {
        let w = |s: &[u32]| {
            s.iter()
                .map(|&c| wchar_t::try_from(c).unwrap())
                .collect::<Vec<_>>()
        };
        assert_eq!(unmap_legacy_name(&w(&[0x61, 0x62])), None);
        let mapped = w(&[0x61, 0xFFFE, 0xE0E3, 0xE083, 0xE086, 0x2E]);
        assert_eq!(
            unmap_legacy_name(&mapped).as_deref(),
            Some(&b"a\xe3\x83\x86."[..])
        );
    }

    #[test]
    fn missing_file_is_eopen() {
        let err = Archive::open(Path::new("/nonexistent/x.rar"), OpenOptions::default())
            .err()
            .map(|c| c.0);
        assert_eq!(err, Some(ERAR_EOPEN));
    }
}
