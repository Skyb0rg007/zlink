/// Error types that can occur during Varlink interface code generation.
#[derive(Debug)]
pub enum Error {
    /// An invalid argument was provided.
    InvalidArgument,

    /// An I/O error occurred during file operations.
    Io(std::io::Error),

    /// An error from the zlink-core library.
    Zlink(zlink::Error),

    /// Writing to the internal output buffer failed during code generation.
    Fmt(std::fmt::Error),

    /// `rustfmt` produced output that was not valid UTF-8.
    InvalidUtf8(std::string::FromUtf8Error),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidArgument => write!(f, "Invalid argument provided"),
            Self::Io(e) => write!(f, "I/O error: {e}"),
            Self::Zlink(e) => write!(f, "Zlink error: {e}"),
            Self::Fmt(e) => write!(f, "Formatting error: {e}"),
            Self::InvalidUtf8(e) => write!(f, "Invalid UTF-8 from rustfmt: {e}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(e) => Some(e),
            Self::Zlink(e) => Some(e),
            Self::Fmt(e) => Some(e),
            Self::InvalidUtf8(e) => Some(e),
            Self::InvalidArgument => None,
        }
    }
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

impl From<zlink::Error> for Error {
    fn from(e: zlink::Error) -> Self {
        Self::Zlink(e)
    }
}

impl From<std::fmt::Error> for Error {
    fn from(e: std::fmt::Error) -> Self {
        Self::Fmt(e)
    }
}

impl From<std::string::FromUtf8Error> for Error {
    fn from(e: std::string::FromUtf8Error) -> Self {
        Self::InvalidUtf8(e)
    }
}
