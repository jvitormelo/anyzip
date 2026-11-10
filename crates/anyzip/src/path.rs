//! Entry path normalization. Same policy as the `bsdtar` setup it replaces, but stricter:
//! anything that could land outside the extraction directory is refused, never rewritten.

/// Normalizes an entry path. `None` means unsafe (or empty) and the entry must be skipped.
///
/// 1. `\` becomes `/` (zips made on Windows).
/// 2. Empty and `.` segments are dropped (`./a//b` is `a/b`); a trailing `/` disappears.
/// 3. Refused: empty result, absolute path, drive letter (`C:`), any `..` segment, NUL or other
///    control characters.
#[must_use]
pub fn normalize(raw: &str) -> Option<String> {
    if raw.is_empty() || raw.chars().any(char::is_control) {
        return None;
    }
    let unified;
    let path = if raw.contains('\\') {
        unified = raw.replace('\\', "/");
        unified.as_str()
    } else {
        raw
    };
    if path.starts_with('/') || has_drive_prefix(path) {
        return None;
    }
    let mut out = String::with_capacity(path.len());
    for segment in path.split('/') {
        match segment {
            "" | "." => {}
            ".." => return None,
            s => {
                if !out.is_empty() {
                    out.push('/');
                }
                out.push_str(s);
            }
        }
    }
    if out.is_empty() || has_drive_prefix(&out) {
        None
    } else {
        Some(out)
    }
}

fn has_drive_prefix(path: &str) -> bool {
    let b = path.as_bytes();
    b.len() >= 2 && b[0].is_ascii_alphabetic() && b[1] == b':'
}

#[cfg(test)]
mod tests {
    use super::normalize;
    use proptest::prelude::*;
    use std::path::{Component, Path};

    #[test]
    fn cases() {
        let ok = |a: &str, b: &str| assert_eq!(normalize(a).as_deref(), Some(b), "{a:?}");
        let no = |a: &str| assert_eq!(normalize(a), None, "{a:?}");
        ok("a.txt", "a.txt");
        ok("dir/a.txt", "dir/a.txt");
        ok("./dir//a.txt", "dir/a.txt");
        ok("dir\\sub\\a.txt", "dir/sub/a.txt");
        ok("dir/", "dir");
        ok("a/./b", "a/b");
        ok("edição/anexo único.pdf", "edição/anexo único.pdf");
        ok("..a/b..", "..a/b..");
        ok("dir/x:y", "dir/x:y");
        no("");
        no("/");
        no(".");
        no("./");
        no("/etc/passwd");
        no("\\\\server\\share\\x");
        no("C:\\Windows\\x");
        no("c:x");
        no("./C:\\Windows\\x");
        no(".\\c:x");
        no("../x");
        no("a/../../x");
        no("a/..");
        no("a\\..\\x");
        no("a\0b");
        no("a\nb");
        no("a\u{7f}b");
    }

    proptest! {
        #[test]
        fn never_escapes(raw in ".*") {
            if let Some(p) = normalize(&raw) {
                prop_assert!(!p.is_empty());
                prop_assert!(!p.starts_with('/'));
                prop_assert!(!super::has_drive_prefix(&p));
                prop_assert!(!p.contains('\\'));
                prop_assert!(!p.chars().any(char::is_control));
                prop_assert_eq!(normalize(&p), Some(p.clone()));
                let joined = Path::new("/base").join(&p);
                prop_assert!(joined.starts_with("/base"));
                for c in Path::new(&p).components() {
                    prop_assert!(matches!(c, Component::Normal(_)), "{:?}", c);
                }
            }
        }

        #[test]
        fn never_escapes_pathish(raw in "[a./\\\\:C\u{0}]{0,12}") {
            if let Some(p) = normalize(&raw) {
                prop_assert!(!super::has_drive_prefix(&p));
                prop_assert_eq!(normalize(&p), Some(p.clone()));
                for c in Path::new(&p).components() {
                    prop_assert!(matches!(c, Component::Normal(_)), "{:?}", c);
                }
            }
        }
    }
}
