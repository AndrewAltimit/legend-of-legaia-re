//! Legend of Legaia randomizer / disc patcher - Track-1 tooling.
//!
//! Builds patches for a **user-supplied** retail disc: it shuffles gameplay
//! data (monster item drops, random-encounter formations, and treasure-chest
//! contents) and produces a patched copy plus a portable patch
//! file. It does **not** touch the engine.
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
//! padded back to the original slot size so no offset downstream moves - see
//! `legaia_disc_patch::monster`.
//!
//! ## Where things live
//!
//! This crate is the top of the disc-patching family: the `apply` layer that
//! sequences every edit onto a disc ([`apply`]), the `legaia-patcher` CLI,
//! and the randomizer's own data shuffles and standalone mods (the `pub mod`
//! lines below). Everything those are built on sits in a crate underneath:
//!
//! - `legaia-disc-patch` - the write foundation (`DiscPatcher`, PPF, the
//!   free-space ledger, re-pack budgets, the seeded `rng`).
//! - `legaia-code-hooks` - the MIPS toolkit, the injection-arena map and the
//!   hook mods that need nothing else.
//! - `legaia-arts-patch`, `legaia-texture-replace`, `legaia-translate`,
//!   `legaia-delilas-party`, `legaia-delilas-challenge` - one crate per
//!   feature family.
//! - `legaia-party-swap` - the disc-free model-transform kernels.
//!
//! Every consumer, inside the family or outside it (the browser patcher,
//! the engine's runtime oracles), names the owning crate directly.

pub mod apply;
pub mod arts_name_fix;
pub mod attack_count;
pub mod casino;
pub mod charm_fix;
pub mod chest;
pub mod damage_ap;
pub mod door;
pub mod drops;
pub mod earth_egg;
pub mod element_affinity;
pub mod encounter;
pub mod enemy_ally;
pub mod equip_bonus;
pub mod equip_hand_frame;
pub mod equip_mask;
pub mod equip_transplant;
pub mod equipment;
pub mod fishing_price;
pub mod fortress_encounter_fix;
pub mod house_door;
pub mod item_name;
pub mod item_price;
pub mod items;
pub mod kingdom;
pub mod location_name;
pub mod map_door;
pub mod monster_class;
pub mod monster_stats;
pub mod move_power;
pub mod rewards;
pub mod shop;
#[cfg(test)]
mod space_ledger_rows_tests;
pub mod spell_cost;
pub mod spirit_ap;
pub mod starting_bag;
pub mod starting_items;
pub mod starting_level;
pub mod steal;
pub mod super_arts_pack;
pub mod unused;
pub mod weapon_specialty;
