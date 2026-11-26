//! wt-linux startup: command line, config, and the wiring from `Pty` to the window.
//! `main.rs` is a thin shell over this so everything here is unit-testable.

pub mod cli;
pub mod config;
pub mod contract;
#[cfg(feature = "ui")]
pub mod input;
pub mod launch;
pub mod session;
mod stub;
#[cfg(feature = "ui")]
pub mod ui;
