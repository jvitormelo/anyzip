//! Compiles the vendored UnRAR (RARLAB) as a static library for the DLL API.
//!
//! The vendored tree stays byte-for-byte equal to the release tarball (see `vendor/VERSION`).
//! The single patch we need is applied to a copy in `OUT_DIR`:
//!
//! - `ErrHandler` becomes `thread_local`. Upstream keeps one global error handler; two archives
//!   opened at the same time on different libuv worker threads would race on it.
//! - `MergeArchive` refuses to open a next volume on the filesystem: we extract from a single
//!   in-memory buffer (`memfd` or temp file) and must never probe neighbour paths or fds.
//! - `anyzip_rar_is_truncated` exposes whether the archive cursor/block ran past EOF (`DataSet`
//!   is private to `dll.cpp`).
//!
//! `RAR_SMP` is deliberately not defined: with it, UnRAR decodes RAR5 on its own worker threads,
//! and a `MemoryError` thrown there escapes the DLL's `try` and terminates the process.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const SOURCES: &[&str] = &[
    "strlist",
    "strfn",
    "pathfn",
    "smallfn",
    "global",
    "file",
    "filefn",
    "filcreat",
    "archive",
    "arcread",
    "unicode",
    "system",
    "crypt",
    "crc",
    "rawread",
    "encname",
    "resource",
    "match",
    "timefn",
    "rdwrfn",
    "consio",
    "options",
    "errhnd",
    "rarvm",
    "secpassword",
    "rijndael",
    "getbits",
    "sha1",
    "sha256",
    "blake2s",
    "hash",
    "extinfo",
    "extract",
    "volume",
    "list",
    "find",
    "unpack",
    "headers",
    "threadpool",
    "rs16",
    "cmddata",
    "ui",
    "largepage",
    "filestr",
    "scantree",
    "dll",
    "qopen",
];

const ERRHANDLER_GLOBAL: &str = "EXTVAR ErrorHandler ErrHandler;";
const ERRHANDLER_THREAD_LOCAL: &str = "EXTVAR thread_local ErrorHandler ErrHandler;";

const MERGE_ARCHIVE_ANCHOR: &str = "  CommandData *Cmd=Arc.GetCommandData();";
const MERGE_ARCHIVE_DISABLED: &str = "  ErrHandler.SetErrorCode(RARX_OPEN);\n  return false;\n  CommandData *Cmd=Arc.GetCommandData();";

const TRUNCATED_PROBE: &str = r#"
extern "C" int anyzip_rar_is_truncated(HANDLE hArcData) {
  DataSet *Data = (DataSet *)hArcData;
  if (Data == nullptr) return 0;
  int64 len = Data->Arc.FileLength();
  return (Data->Arc.NextBlockPos > len || Data->Arc.CurBlockPos >= len || Data->Arc.Tell() >= len) ? 1 : 0;
}
"#;

fn main() {
    let manifest = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap_or_default());
    let vendor = manifest.join("vendor/unrar");
    let out = PathBuf::from(env::var_os("OUT_DIR").unwrap_or_default());
    let src = out.join("unrar");
    println!("cargo:rerun-if-changed=vendor");
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=src/shim.cpp");
    println!("cargo:rerun-if-env-changed=ANYZIP_DYNAMIC_STDCXX");

    copy_dir(&vendor, &src);
    let global = src.join("global.hpp");
    let text = fs::read_to_string(&global).unwrap_or_default();
    assert!(
        text.contains(ERRHANDLER_GLOBAL),
        "global.hpp changed upstream; review the thread_local patch in build.rs"
    );
    fs::write(
        &global,
        text.replace(ERRHANDLER_GLOBAL, ERRHANDLER_THREAD_LOCAL),
    )
    .unwrap_or_else(|e| panic!("write {}: {e}", global.display()));

    let volume = src.join("volume.cpp");
    let vol_text = fs::read_to_string(&volume).unwrap_or_default();
    assert_eq!(
        vol_text.matches(MERGE_ARCHIVE_ANCHOR).count(),
        1,
        "volume.cpp changed upstream; review the MergeArchive patch in build.rs"
    );
    fs::write(
        &volume,
        vol_text.replacen(MERGE_ARCHIVE_ANCHOR, MERGE_ARCHIVE_DISABLED, 1),
    )
    .unwrap_or_else(|e| panic!("write {}: {e}", volume.display()));

    let dll = src.join("dll.cpp");
    let mut dll_text = fs::read_to_string(&dll).unwrap_or_default();
    assert!(
        dll_text.contains("struct DataSet") && dll_text.contains("Archive Arc;"),
        "dll.cpp changed upstream; review anyzip_rar_is_truncated in build.rs"
    );
    dll_text.push_str(TRUNCATED_PROBE);
    fs::write(&dll, dll_text).unwrap_or_else(|e| panic!("write {}: {e}", dll.display()));

    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let target_env = env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();
    let windows = target_os == "windows";

    let mut build = cc::Build::new();
    build
        .cpp(true)
        .std("c++11") // as upstream's makefile; also what old cross g++ (4.8) accepts
        .cpp_link_stdlib(None)
        .warnings(false)
        .extra_warnings(false)
        .define("RARDLL", None)
        .define("SILENT", None)
        .define("_FILE_OFFSET_BITS", Some("64"))
        .define("_LARGEFILE_SOURCE", None);
    if !windows {
        build.define("_UNIX", None).flag_if_supported("-fPIC");
    }
    for flag in [
        "-Wno-switch",
        "-Wno-parentheses",
        "-Wno-macro-redefined",
        "-Wno-dangling-else",
        "-Wno-logical-op-parentheses",
        "-Wno-unused-parameter",
        "-Wno-unused-variable",
        "-Wno-unused-function",
        "-Wno-missing-braces",
        "-Wno-unknown-pragmas",
        "-Wno-deprecated-declarations",
        "-Wno-class-memaccess",
    ] {
        build.flag_if_supported(flag);
    }
    for name in SOURCES {
        build.file(src.join(format!("{name}.cpp")));
    }
    if windows {
        build.file(src.join("isnt.cpp"));
    }
    build.file(manifest.join("src/shim.cpp")).include(&src);
    build.compile("unrar");

    link_cxx_runtime(&target_os, &target_env);
    if windows {
        println!("cargo:rustc-link-lib=powrprof");
        println!("cargo:rustc-link-lib=shell32");
        println!("cargo:rustc-link-lib=advapi32");
    } else {
        println!("cargo:rustc-link-lib=pthread");
    }
}

/// On linux-gnu we link libstdc++ statically, so the `.node` depends only on libc, libm and
/// `libgcc_s` and runs on any distro. `ANYZIP_DYNAMIC_STDCXX=1` opts out (e.g. a distro without
/// `libstdc++.a`).
fn link_cxx_runtime(target_os: &str, target_env: &str) {
    match (target_os, target_env) {
        ("linux", "gnu") if env::var_os("ANYZIP_DYNAMIC_STDCXX").is_none() => {
            if let Some(dir) = static_libstdcxx_dir() {
                println!("cargo:rustc-link-search=native={}", dir.display());
                println!("cargo:rustc-link-lib=static=stdc++");
            } else {
                println!("cargo:warning=libstdc++.a not found; linking libstdc++ dynamically");
                println!("cargo:rustc-link-lib=stdc++");
            }
        }
        ("linux", _) => println!("cargo:rustc-link-lib=stdc++"),
        ("macos" | "ios" | "freebsd", _) => println!("cargo:rustc-link-lib=c++"),
        _ => {}
    }
}

fn static_libstdcxx_dir() -> Option<PathBuf> {
    let compiler = cc::Build::new().cpp(true).get_compiler();
    let output = Command::new(compiler.path())
        .arg("-print-file-name=libstdc++.a")
        .output()
        .ok()?;
    let path = PathBuf::from(String::from_utf8(output.stdout).ok()?.trim());
    if path.is_absolute() && path.exists() {
        path.parent().map(Path::to_path_buf)
    } else {
        None
    }
}

fn copy_dir(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap_or_else(|e| panic!("mkdir {}: {e}", to.display()));
    let entries = fs::read_dir(from).unwrap_or_else(|e| panic!("read {}: {e}", from.display()));
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_file() {
            let dest = to.join(entry.file_name());
            fs::copy(&path, &dest).unwrap_or_else(|e| panic!("copy {}: {e}", path.display()));
        }
    }
}
