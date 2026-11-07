//! Why reading or writing failed.
use std::{fmt, io};

/// Why reading or writing failed.
#[derive(Debug)]
pub enum GltfError {
    /// A file couldn't be read or written: the file (a path or a URI) and why.
    Io { file: String, error: io::Error },
    /// The input isn't valid glTF: where (a JSON path such as
    /// `meshes[0].primitives[1].indices`, or `GLB`) and why.
    Malformed { at: String, message: String },
    /// The file requires an extension this reader doesn't read.
    Unsupported(String),
    /// The file asks more than the read's budget allows.
    Limit(String),
    /// The model can't be written: where and why.
    Invalid { at: String, message: String },
    /// The read was cancelled.
    Cancelled,
}

/// The read's budget refusing: cancelled, or over a limit.
impl From<kinogaki_studio_api::Error> for GltfError {
    fn from(error: kinogaki_studio_api::Error) -> GltfError {
        match error.is_cancelled() {
            true => GltfError::Cancelled,
            false => GltfError::Limit(error.message().to_owned()),
        }
    }
}

impl GltfError {
    pub(crate) fn malformed(at: impl Into<String>, message: impl Into<String>) -> GltfError {
        GltfError::Malformed {
            at: at.into(),
            message: message.into(),
        }
    }

    pub(crate) fn invalid(at: impl Into<String>, message: impl Into<String>) -> GltfError {
        GltfError::Invalid {
            at: at.into(),
            message: message.into(),
        }
    }
}

impl fmt::Display for GltfError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            GltfError::Io { file, error } => write!(f, "{file}: {error}"),
            GltfError::Malformed { at, message } | GltfError::Invalid { at, message } => {
                write!(f, "{at}: {message}")
            }
            GltfError::Unsupported(extension) => {
                write!(f, "the file requires {extension}, which isn't read")
            }
            GltfError::Limit(message) => f.write_str(message),
            GltfError::Cancelled => f.write_str("cancelled"),
        }
    }
}

impl std::error::Error for GltfError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            GltfError::Io { error, .. } => Some(error),
            _ => None,
        }
    }
}
