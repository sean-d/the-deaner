//! The the-deaner sidebar: state, keymap, and rendering. No IO lives here.

pub mod app;
pub mod browser;
pub mod cheatsheet;
pub mod icons;
pub mod keymap;
pub mod render;
pub mod theme;

pub use app::{App, Command, Mode};
pub use ratcn::Toast;
pub use render::draw;
