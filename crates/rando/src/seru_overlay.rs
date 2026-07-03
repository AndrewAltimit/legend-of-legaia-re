//! Custom-overlay loading on retail - the vertical slice that proves we can
//! stream hand-written code from an (overwritten) pochi PROT slot into RAM and
//! execute it on real hardware, the foundation the full retail seru-trade UI
//! needs (its UI driver is far too big for the SCUS rodata gap, so it must ship
//! as a loadable overlay the way the fishing / slot-machine minigames do).
//!
//! ## The mechanism
//!
//! 1. The randomizer overwrites a **pochi-filler PROT slot** (265 exist, the
//!    largest >1 MB - reserved dev fillers with real allocated disc sectors) with
//!    a small custom overlay. Because the randomizer placed it, it knows that
//!    slot's exact start LBA + sector count from the disc TOC.
//! 2. A tiny **loader stub** in the preserved SCUS rodata gap calls the
//!    game's own synchronous CD reader [`LOADER_FN`]
//!    (`FUN_8005E4D4(sector_count, lba, dest)` - verified sync: it issues the
//!    read then waits) with those values **baked as literals**, so there is no
//!    runtime PROT-index arithmetic (the recurring ±2 index-space trap can't
//!    bite). It then `jalr`s the loaded code at [`DEST`], and on return replays
//!    the displaced hook instructions and jumps back.
//! 3. A detour at the shop-open path (field-VM op `0x49`) routes into the stub.
//!
//! ## The slice payload
//!
//! For the slice the overlay is the simplest observable: it writes a 32-bit
//! [`SENTINEL`] to [`SENTINEL_ADDR`] (a reserved cell in the SCUS rodata gap,
//! resident RAM we own) and returns. If the sentinel appears after the hook
//! fires on an emulator, the load→exec→return mechanism works on hardware; the
//! real trade UI then replaces this payload. The overlay is a position-
//! independent leaf (absolute data store + `jr ra`), so it runs correctly at any
//! load address.
//!
//! Nothing here embeds Sony bytes: the overlay + stub are the randomizer's own
//! code, and the LBA/sectors come from the user's disc.

mod consts;
mod encode;
mod routine_loader;
mod routine_trade;

#[cfg(test)]
mod tests;

pub use consts::*;
pub use routine_loader::*;
pub use routine_trade::*;
// Internal MIPS encoders: reached by the routine builders + tests via
// `use super::*;`. Not part of the public overlay API, so a plain (private)
// glob rather than a `pub use` re-export.
use encode::*;
