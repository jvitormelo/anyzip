//! Shared harness: every target runs `list`, `extract` and `extract_each` under small limits and
//! checks the invariants the API promises, whatever the input.

use std::ops::ControlFlow;

use anyzip::{Format, Limits};

/// Small enough that one input never takes more than a few MB or ms.
pub const LIMITS: Limits = Limits {
    max_entries: 200,
    max_entry_bytes: 8 << 20,
    max_total_bytes: 16 << 20,
    max_ratio: 1000,
    max_memory_bytes: 64 << 20,
};

/// Runs the whole API on `data` if it is `format` (or on anything, with `None`).
pub fn check(data: &[u8], format: Option<Format>) {
    let detected = anyzip::detect(data);
    if format.is_some() && detected != format {
        return;
    }
    let _ = anyzip::list(data, &LIMITS);
    let extracted = anyzip::extract(data, &LIMITS);
    if let Ok(out) = &extracted {
        assert_eq!(Some(out.format), detected);
        let total: u64 = out.entries.iter().map(|e| e.data.len() as u64).sum();
        assert!(out.entries.len() <= LIMITS.max_entries as usize);
        assert!(total <= LIMITS.max_total_bytes);
        for e in &out.entries {
            assert_eq!(anyzip::normalize_path(&e.path).as_deref(), Some(e.path.as_str()));
            assert!(!e.path.split('/').any(|s| s == ".."), "unsafe path {:?}", e.path);
        }
    }
    // Streaming must agree with the buffered call on every entry it yields.
    let mut seen = Vec::new();
    let summary = anyzip::extract_each(data, &LIMITS, |e| {
        seen.push((e.path, e.data.len()));
        ControlFlow::Continue(())
    });
    match (&extracted, &summary) {
        (Ok(out), Ok(s)) => {
            assert_eq!(s.entries, out.entries.len());
            let buffered: Vec<_> = out.entries.iter().map(|e| (e.path.clone(), e.data.len())).collect();
            assert_eq!(seen, buffered);
        }
        (Err(a), Err(b)) => assert_eq!(a.code, b.code),
        (a, b) => panic!("extract and extract_each disagree: {:?} vs {:?}", a.as_ref().err(), b.as_ref().err()),
    }
}
