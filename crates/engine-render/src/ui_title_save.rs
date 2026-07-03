//! Title-screen + save/load-screen UI draw builders: title menu,
//! 9-slice window chrome, save-slot grid + info panel, and the
//! "Now checking" dialog. Extracted from the crate root.

mod title;
pub use title::*;

mod save_select;
pub use save_select::*;

mod panel;
pub use panel::*;

mod now_checking;
pub use now_checking::*;

mod slot_grid;
pub use slot_grid::*;

mod slot_info;
pub use slot_info::*;
