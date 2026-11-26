//! In-crate stand-ins for the sibling crates, written to the signatures in
//! docs/ARCHITECTURE.md, so `wt` builds and its wiring runs before wt-backend / wt-vt land.
//! The `real-backend` / `real-vt` features swap in the real crates (see `crate::contract`).

#[cfg(not(feature = "real-backend"))]
pub mod backend;
#[cfg(not(feature = "real-vt"))]
pub mod vt;
