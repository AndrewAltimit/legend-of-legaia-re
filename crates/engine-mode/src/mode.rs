//! Game-mode driver.
//!
//! Port of the 28-entry game-mode state table at SCUS RAM `0x8007078C`. Each
//! retail entry is 24 bytes:
//!
//! ```text
//!   +0x00  u32 name_string_ptr   ; ASCII label for debug ("CONFIG MODE", ...)
//!   +0x0A  i16 next_mode         ; mode to transition to when handler signals
//!                                ; completion (-1 = self-managed, no auto-tx)
//!   +0x10  u32 handler_fn_ptr    ; per-mode handler called every frame
//!   +0x14  u32 parameter         ; flag bits passed to the handler
//! ```
//!
//! The retail current-mode register is `gp[0x524]` (an `i16`); the dev
//! mode-transition writer is `FUN_800179C0` (gated on debug enable). Each
//! handler returns by either staying in the same mode (per-frame loop), or
//! transitioning to `next_mode` (init -> run pattern).
//!
//! In the port we map each mode to a [`GameMode`] enum variant,
//! the handler to a [`ModeHandler`] trait, and the parameter to the
//! [`ModeEntry::param`] flag bits. The Sony function pointers are NOT used;
//! engine integrations supply Rust closures that drive the
//! [`legaia_engine_field::world::World`]. The table's name/param/next fields are
//! reconciled against the disc-recovered map (`legaia_game_tables::mode_table`)
//! by the disc-gated `mode_table_reconcile` test.

use legaia_engine_system::input::InputState;

mod driver;
mod frame_stage;
mod game_mode;
mod init_stage;
mod seat;

pub use driver::*;
pub use frame_stage::*;
pub use game_mode::*;
pub use init_stage::*;
pub use seat::*;

#[cfg(test)]
mod tests;
