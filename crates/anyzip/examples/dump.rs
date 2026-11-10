//! `cargo run --release --example dump -- FILE...`: one JSON line per archive
//! (`{file, ms, format, entries: [{path, size, md5}], skipped, recovered}` or `{file, error}`).

use md5::{Digest, Md5};
use serde_json::json;
use std::time::Instant;

fn main() {
    for path in std::env::args().skip(1) {
        let Ok(bytes) = std::fs::read(&path) else {
            println!("{}", json!({"file": path, "error": "unreadable file"}));
            continue;
        };
        let start = Instant::now();
        let result = anyzip::extract(&bytes, &anyzip::Limits::default());
        let ms = start.elapsed().as_secs_f64() * 1000.0;
        let line = match result {
            Ok(out) => json!({
                "file": path,
                "ms": (ms * 100.0).round() / 100.0,
                "format": out.format.as_str(),
                "entries": out.entries.iter().map(|e| json!({
                    "path": e.path,
                    "size": e.data.len(),
                    "md5": hex::encode(Md5::digest(&e.data)),
                })).collect::<Vec<_>>(),
                "skipped": out.skipped.iter().map(|s| json!({"path": s.path, "reason": s.reason.as_str()})).collect::<Vec<_>>(),
                "recovered": out.recovered,
            }),
            Err(e) => {
                json!({"file": path, "ms": (ms * 100.0).round() / 100.0, "error": e.code.as_str(), "message": e.message})
            }
        };
        println!("{line}");
    }
    if std::env::var_os("ANYZIP_RSS").is_some() {
        eprintln!("peak_rss_kb={}", peak_rss_kb());
    }
}

/// Peak resident set size of this process (Linux `VmHWM`), for the bomb checks.
fn peak_rss_kb() -> u64 {
    std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|s| {
            s.lines()
                .find_map(|l| l.strip_prefix("VmHWM:"))
                .and_then(|v| v.trim().trim_end_matches("kB").trim().parse().ok())
        })
        .unwrap_or(0)
}
