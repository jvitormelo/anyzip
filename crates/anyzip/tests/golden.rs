//! Golden tests: `extract` on every fixture in `fixtures/{libarchive,generated}`, compared with
//! `fixtures/expected/<dir>/<name>.json`. A change in behaviour shows up as a diff in review:
//!
//!     UPDATE_EXPECTED=1 cargo test -p anyzip --test golden

use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

use md5::{Digest, Md5};
use serde_json::{Value, json};

const DIRS: [&str; 2] = ["libarchive", "generated"];
/// Not archives: metadata that sits next to the libarchive fixtures.
const IGNORED: [&str; 2] = ["COMMIT", "COPYING"];

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures")
}

fn describe(bytes: &[u8]) -> Value {
    match anyzip::extract(bytes, &anyzip::Limits::default()) {
        Ok(out) => json!({
            "format": out.format.as_str(),
            "entries": out.entries.iter().map(|e| json!({
                "path": e.path,
                "size": e.data.len(),
                "md5": hex::encode(Md5::digest(&e.data)),
            })).collect::<Vec<_>>(),
            "skipped": out.skipped.iter()
                .map(|s| json!({"path": s.path, "reason": s.reason.as_str()}))
                .collect::<Vec<_>>(),
            "recovered": out.recovered,
        }),
        Err(e) => json!({ "error": e.code.as_str() }),
    }
}

#[test]
fn golden() {
    let update = std::env::var_os("UPDATE_EXPECTED").is_some();
    let root = fixtures();
    let mut failures = String::new();
    let mut checked = 0;
    for dir in DIRS {
        let Ok(listing) = fs::read_dir(root.join(dir)) else {
            panic!(
                "missing fixtures/{dir}: run scripts/fetch-libarchive-fixtures.sh and scripts/make-fixtures.py"
            );
        };
        let mut names: Vec<_> = listing
            .filter_map(Result::ok)
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| !IGNORED.contains(&n.as_str()))
            .collect();
        names.sort();
        for name in names {
            let bytes = fs::read(root.join(dir).join(&name)).unwrap();
            let actual = describe(&bytes);
            let expected_path = root.join("expected").join(dir).join(format!("{name}.json"));
            checked += 1;
            if update {
                fs::create_dir_all(expected_path.parent().unwrap()).unwrap();
                let text = serde_json::to_string_pretty(&actual).unwrap() + "\n";
                fs::write(&expected_path, text).unwrap();
                continue;
            }
            let Ok(text) = fs::read_to_string(&expected_path) else {
                let _ = writeln!(failures, "{dir}/{name}: no expected file");
                continue;
            };
            let expected: Value = serde_json::from_str(&text).unwrap();
            if actual != expected {
                let _ = writeln!(
                    failures,
                    "{dir}/{name}:\n  expected {expected}\n  actual   {actual}"
                );
            }
        }
    }
    assert!(checked > 0, "no fixtures found");
    assert!(
        failures.is_empty(),
        "golden mismatches (UPDATE_EXPECTED=1 to accept after review):\n{failures}"
    );
}
