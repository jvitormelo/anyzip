use std::fmt;

/// Why extraction failed. Stable: the JS binding exposes these as `AnyzipError.code`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ErrorCode {
    /// Not a zip, 7z or rar; or a compression method we do not decode.
    Unsupported,
    /// Damaged, truncated or malicious archive; checksum mismatch.
    Corrupt,
    /// Encrypted headers (7z `-mhe`, rar `-hp`): nothing can be listed without the password.
    Encrypted,
    /// Part of a multi-volume set (rar `.partN.rar`, spanned zip).
    MultiVolume,
    /// A limit was hit: entries, bytes per entry, total bytes, ratio or decoder memory.
    TooLarge,
    /// The operating system failed us (temp file for rar, out of memory).
    Io,
}

impl ErrorCode {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Unsupported => "unsupported",
            Self::Corrupt => "corrupt",
            Self::Encrypted => "encrypted",
            Self::MultiVolume => "multiVolume",
            Self::TooLarge => "tooLarge",
            Self::Io => "io",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error {
    pub code: ErrorCode,
    pub message: String,
}

impl Error {
    pub(crate) fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
    pub(crate) fn corrupt(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::Corrupt, message)
    }
    pub(crate) fn too_large(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::TooLarge, message)
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code.as_str(), self.message)
    }
}

impl std::error::Error for Error {}
