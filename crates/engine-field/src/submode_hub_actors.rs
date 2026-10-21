//! The field overlay's **op-`0x49` submode system-actor handler family** in
//! the `0x801F0000+` band: one dispatcher plus the per-state handlers it
//! indexes.
//!
//! ## Which image this code belongs to
//!
//! The corpus holds these VAs under six program names (`baka_fighter`,
//! `dance`, `fishing`, `slot_machine`, `debug_menu`, `overlay_0897`) and the
//! five minigame-named dumps are **byte-identical**. That is not five copies -
//! it is the same resident field-overlay code seen through five RAM-derived
//! captures, the artifact [`docs/tooling/dump-corpus-integrity.md`] catalogues
//! and that the sibling port [`crate::field_subsystem_enter`] already diagnoses
//! at `FUN_801F1278`. The statically extracted Baka Fighter overlay is PROT
//! 976 at base `0x801CE818` and `0xE000` bytes long, so it does not reach
//! `0x801F0ADC` at all; PROT 0897 (`0x4F800` bytes at the same base) does.
//!
//! This matters beyond attribution, because the minigame-named dumps are also
//! **truncated**: `overlay_baka_fighter_801f0adc.txt` stops after 46
//! instructions, while `overlay_0897_801f0adc.txt` carries the whole
//! discontiguous 264-instruction body. The port of [`coin_exchange`] below is
//! written from the long one.
//!
//! ## What the family is
//!
//! The dispatcher `FUN_801F159C` is the **resume / close half of the field
//! VM's op-`0x49` submode** ([`docs/subsystems/script-vm.md`]); the enter half
//! is `FUN_801F1278` ([`crate::field_subsystem_enter`]). It indexes the 52-entry
//! table `PTR_FUN_801F33B4` by the actor's `+0x50` word.
//!
//! The routines here come from **two** tables, and conflating them is the
//! error to avoid. The state machines ([`slot`]) are `PTR_FUN_801F33B4`
//! entries: read `+0x54`, install a panel, and on a confirm hand the actor
//! back by stashing `+0x50` into the submode cursor context and re-arming
//! `+0x50` to [`HUB_RETURN_STATE`]. The panel painters ([`HubPainter`]) are
//! **not** in that table at all - they are the `+0x14` callback of a
//! [`PANEL_WINDOW_TABLE`] record, reached only once a state machine has
//! installed the descriptor that names the window.
//!
//! The op-`0x49` contract is what makes the dispatcher load-bearing: the
//! script parks on the same PC until this family drops the busy flag
//! `_DAT_8007B450` to `1`, which is the VM's "Done" signal.
//!
//! ## Actor fields
//!
//! | Field | Role |
//! |---|---|
//! | `+0x0A` / `+0x0C` | panel origin x / y |
//! | `+0x0E` | panel width (the right-edge cursor anchor) |
//! | `+0x10` | actor flag word; bit `3` (`\|= 8`) retires the actor this frame |
//! | `+0x1A` | dispatcher gate: non-zero suppresses the re-arm pass |
//! | `+0x50` | handler id - the `PTR_FUN_801F33B4` index |
//! | `+0x54` | the handler's own sub-state |
//!
//! ## Globals
//!
//! Named here by VA because the family is pure glue over them. `0x801C6EA4`
//! is the submode **cursor context**, whose `+0x2E` is the "hand-back"
//! sentinel, `+0x3E` the completion gate the dispatcher polls and `+0x40` the
//! stashed handler id. `0x8007B450` is the op-`0x49` operand pointer doubling
//! as the busy flag, `0x8007B454` the text palette index, `0x8007B458` a
//! frame-paced hold timer, `0x8007BB80` a suppression flag that blocks every
//! confirm, `0x8007BB88` / `0x8007BB98` the two cursor rows, `0x8007BB90` the
//! published coin ceiling, `0x8007BB9C` the coin counter's digit cursor,
//! `0x801F35F0..+7` its eight digit cells, `0x8008459C` party gold and
//! `0x800845A4` the casino coin bank.
//!
//! `0x80084594` / `0x80084598` are the **party roster** - member count and
//! member ids - not a generic entry table; [`docs/subsystems/script-vm.md`]
//! pins them from the enter half's portrait seeding.
//!
//! Read from `overlay_0897_801f0adc.txt` and `overlay_baka_fighter_801f{1138,
//! 159c,16c0,17d8,1890,1950,1a1c,1ab0,1b64,1d90,1e48,1fdc,20b0,2134}.txt`
//! plus `overlay_menu_801d0f1c.txt` (the item-info panel, whose misbased
//! `overlay_baka_fighter_801f90dc.txt` print this module first read).
//!
//! Host: `legaia_engine_core::field_submode_screen` runs the dispatcher over
//! the live `SubmodeDriver` pool actor every field frame.

use legaia_engine_dev_menu::dev_menu_leaves::{
    EquipPanelDraw, EquipPanelInput, EquipProps, ItemProps, equip_stat_panel,
};

use legaia_save::minigame_save::COIN_BANK_MAX;

mod model;
pub use model::*;
mod panels;
pub use panels::*;
mod dispatch;
pub use dispatch::*;
mod coin_exchange;
pub use coin_exchange::*;
mod painters;
pub use painters::*;

/// Text palette the panel handlers select before a string
/// (`_DAT_8007B454`).
pub const PALETTE_PANEL: i32 = 7;

/// The alternate palette the three-line panel uses for its lower two lines.
pub const PALETTE_DIM: i32 = 5;

/// The palette the acquisition caption uses for the item name.
pub const PALETTE_CAPTION: i32 = 6;

/// The handler id every hand-back re-arms `+0x50` to.
pub const HUB_RETURN_STATE: u16 = 0x1A;

/// The alternate re-arm id `FUN_801F1D90` picks.
pub const HUB_DEACTIVATE_STATE: u16 = 0x2C;

/// The other id it picks.
pub const HUB_SKIP_STATE: u16 = 0x02;

/// Confirm sting id.
pub const CUE_CONFIRM: u8 = 0x20;

/// Entry sting id.
pub const CUE_ENTRY: u8 = 0x26;

/// Bit of the pad latch word that suspends the actor.
pub const PAD_LATCH_SUSPEND: u32 = 0x8000;

/// Bit of `_DAT_8007C364 + 0x10` the dispatcher clears.
pub const BUSY_BIT: u32 = 0x0008_0000;

/// The actor flag bit that retires the actor.
pub const ACTOR_RETIRE: u32 = 0x8;

/// The three submode values the dispatcher runs under (`DAT_801F2734`).
pub const ACTIVE_SUBMODES: [i32; 3] = [1, 4, 7];

/// Gold per coin.
pub const GOLD_PER_COIN: i32 = 100;

/// The item id whose caption arm prints a counter rather than an item.
///
/// `0xFE` is the **Point Card** (`engine-menus::shop::POINT_CARD_ITEM_ID`), and
/// the arm prints its bank `_DAT_800845B4`. It is not a money pseudo-item.
pub const CAPTION_POINT_CARD_ID: i32 = 0xFE;

#[cfg(test)]
mod tests;
