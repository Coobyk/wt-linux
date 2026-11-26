//! wt-render — draws a `wt-vt` terminal grid as Rune scene nodes (see docs/ARCHITECTURE.md).
//!
//! [`TerminalView`] turns a grid, its cursor, a selection and a scrollback offset into a Rune
//! `grid` element: cell backgrounds as fill quads, text as shaped runs through Rune's glyph
//! atlas, and rows unchanged since the last frame reused so Rune doesn't re-shade them.

#[cfg(any(test, feature = "mock"))]
pub mod mock;
mod model;
mod palette;
mod row;
mod view;

pub use model::Attrs;
pub use model::Cell;
pub use model::CursorShape;
pub use model::CursorState;
pub use model::Point;
pub use model::Selection;
pub use model::TermSource;
pub use model::VtColor;
pub use model::Width;
pub use palette::Palette;
pub use palette::color;
pub use row::Metrics;
pub use view::Damage;
pub use view::TerminalView;

#[cfg(test)]
mod snapshot_tests;
#[cfg(test)]
mod tests;
