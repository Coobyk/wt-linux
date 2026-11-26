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
