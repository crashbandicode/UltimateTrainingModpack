mod containers;
pub use containers::*;
pub mod macro_panel;
pub mod replay_panel;
pub mod clip_panel;
pub mod clip_browser;
pub mod roster_grid;
pub mod mii_menu;
pub mod stage_menu;
mod structures;
pub use structures::*;

pub const NX_SUBMENU_ROWS: usize = 8;
pub const NX_SUBMENU_COLUMNS: usize = 4;

pub mod resume;
