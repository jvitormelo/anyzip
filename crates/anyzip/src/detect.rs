//! Format detection by signature, never by extension (portals often get the extension wrong).

/// Archive formats anyzip reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Format {
    Zip,
    SevenZ,
    Rar,
}

impl Format {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Zip => "zip",
            Self::SevenZ => "7z",
            Self::Rar => "rar",
        }
    }
}

/// What the first bytes say.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Signature {
    Archive(Format),
    /// `PK\x07\x08`: first segment of a spanned/split zip.
    SpannedZip,
}

const ZIP_LOCAL: &[u8] = b"PK\x03\x04";
const ZIP_EMPTY: &[u8] = b"PK\x05\x06";
const ZIP_SPANNED: &[u8] = b"PK\x07\x08";
const SEVENZ: &[u8] = b"7z\xBC\xAF\x27\x1C";
const RAR5: &[u8] = b"Rar!\x1A\x07\x01\x00";
const RAR4: &[u8] = b"Rar!\x1A\x07\x00";

pub(crate) fn signature(bytes: &[u8]) -> Option<Signature> {
    if bytes.starts_with(ZIP_LOCAL) || bytes.starts_with(ZIP_EMPTY) {
        Some(Signature::Archive(Format::Zip))
    } else if bytes.starts_with(ZIP_SPANNED) {
        Some(Signature::SpannedZip)
    } else if bytes.starts_with(SEVENZ) {
        Some(Signature::Archive(Format::SevenZ))
    } else if bytes.starts_with(RAR5) || bytes.starts_with(RAR4) {
        Some(Signature::Archive(Format::Rar))
    } else {
        None
    }
}

/// Detects the archive format from the first bytes. A spanned zip segment reports `Zip`.
#[must_use]
pub fn detect(bytes: &[u8]) -> Option<Format> {
    match signature(bytes)? {
        Signature::Archive(format) => Some(format),
        Signature::SpannedZip => Some(Format::Zip),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signatures() {
        assert_eq!(detect(b"PK\x03\x04rest"), Some(Format::Zip));
        assert_eq!(detect(b"PK\x05\x06"), Some(Format::Zip));
        assert_eq!(
            signature(b"PK\x07\x08PK\x03\x04"),
            Some(Signature::SpannedZip)
        );
        assert_eq!(detect(b"7z\xBC\xAF\x27\x1C\x00\x04"), Some(Format::SevenZ));
        assert_eq!(detect(b"Rar!\x1A\x07\x00\xCF"), Some(Format::Rar));
        assert_eq!(detect(b"Rar!\x1A\x07\x01\x00"), Some(Format::Rar));
        assert_eq!(detect(b"Rar!\x1A\x07\x02"), None);
        assert_eq!(detect(b"%PDF-1.7"), None);
        assert_eq!(detect(b""), None);
        assert_eq!(detect(b"PK"), None);
    }
}
