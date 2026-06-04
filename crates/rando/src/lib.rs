//! Legend of Legaia randomizer / disc patcher — Track-1 tooling.
//!
//! Builds patches for a **user-supplied** retail disc: it shuffles gameplay
//! data (monster item drops, random-encounter formations, and treasure-chest
//! contents) and produces a patched copy plus a portable patch
//! file. It does **not** touch the clean-room engine.
//!
//! ## No Sony bytes
//!
//! This crate ships only *code*. It never embeds, commits, or redistributes any
//! game bytes: the user provides their own disc, the tool reads it, and the
//! output (patched image / patch file) stays on the user's machine. Every test
//! that needs real game data is disc-gated and skips when the data is absent.
//!
//! ## How edits are applied
//!
//! Most editable values live *inside* a Legaia LZS stream (the asset
//! dispatcher decompresses them at load), so an edit is
//! decompress → mutate → recompress, using [`legaia_lzs::compress`] to produce
//! a stream the retail decoder accepts. Where the data sits in a fixed-size
//! slot (the monster archive's `0x14000`-byte records), the re-packed stream is
//! padded back to the original slot size so no offset downstream moves — see
//! [`monster`].
//!
//! ## Modules
//!
//! - [`rng`] — a version-stable seeded PRNG so a seed always reproduces a run.
//! - [`items`] — the valid item-id pool (from the SCUS item-name table).
//! - [`unused`] — curated "unused content" sets (Evil Bat enemy ids, the
//!   Something Good / unnamed-accessory items) the opt-in toggles re-introduce.
//! - [`drops`] — the drop-table planner (shuffle / random).
//! - [`equipment`] — classify equipment ids + tier them, turning each monster's
//!   drop slot into a rare random weapon / armor / accessory drop.
//! - [`shop`] — reassign what town stores sell (the gold-merchant stock is
//!   inline in each scene's field-VM script, op `0x49`).
//! - [`casino`] — reassign the casino prize-exchange table (a static overlay
//!   table that spends casino coins).
//! - [`monster`] — re-pack a monster slot in the `battle_data` archive.
//! - [`encounter`] — per-scene random-encounter formation-id shuffle.
//! - [`chest`] — treasure-chest item-give (field-VM op `0x39`) rewrite.
//! - [`disc`] — apply same-size PROT-entry edits to a real disc image
//!   (`DiscPatcher`), via the Mode 2/2352 sector write-back in `legaia_iso`.
//! - [`apply`] — high-level orchestration (`randomize_*`) the CLI drives.
//! - [`ppf`] — PPF 3.0 patch writer/reader (the portable deliverable).

pub mod apply;
pub mod casino;
pub mod chest;
pub mod disc;
pub mod door;
pub mod drops;
pub mod encounter;
pub mod equipment;
pub mod house_door;
pub mod item_name;
pub mod items;
pub mod monster;
pub mod ppf;
pub mod rng;
pub mod shop;
pub mod starting_items;
pub mod steal;
pub mod unused;
