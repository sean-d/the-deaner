//! The deaner sidebar: state, keymap, and rendering. No IO lives here.

pub mod app;
pub mod keymap;
pub mod render;

pub use app::{App, Command, Mode};
pub use ratcn::Toast;
pub use render::draw;
