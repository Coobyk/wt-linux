//! The sibling-crate types wt wires together, per docs/ARCHITECTURE.md. Each comes from the
//! real crate when its feature is on and from `crate::stub` otherwise, so the rest of wt is
//! written once against the contract.

#[cfg(feature = "real-backend")]
pub use wt_backend::{Pty, PtySpec};

#[cfg(not(feature = "real-backend"))]
pub use crate::stub::backend::{Pty, PtySpec};

#[cfg(feature = "real-vt")]
pub use wt_vt::Terminal;

#[cfg(not(feature = "real-vt"))]
pub use crate::stub::vt::Terminal;

/// A finished child's status, normalized so callers see one shape whichever backend ran it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExitStatus {
    code: Option<i32>,
    success: bool,
}

impl ExitStatus {
    /// The exit code; `None` when the child ended on a signal.
    pub fn code(&self) -> Option<i32> {
        self.code
    }

    pub fn success(&self) -> bool {
        self.success
    }
}

#[cfg(feature = "real-backend")]
impl From<wt_backend::ExitStatus> for ExitStatus {
    fn from(s: wt_backend::ExitStatus) -> ExitStatus {
        ExitStatus {
            code: (s.signal().is_none()).then(|| s.exit_code() as i32),
            success: s.success(),
        }
    }
}

#[cfg(not(feature = "real-backend"))]
impl From<std::process::ExitStatus> for ExitStatus {
    fn from(s: std::process::ExitStatus) -> ExitStatus {
        ExitStatus {
            code: s.code(),
            success: s.success(),
        }
    }
}
