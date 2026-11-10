//! Shared desktop primitives. Application/provider state deliberately lives elsewhere.
pub mod images;
pub mod mark;
pub mod motion;
pub mod removal;
pub mod results;
pub mod scroll;
pub mod surface;
pub mod theme;
pub use gtk;

mod result_row;
