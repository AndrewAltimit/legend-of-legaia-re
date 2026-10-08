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
//! [`monster`].
//!
//! ## Modules
//!
//! - [`rng`] - a version-stable seeded PRNG so a seed always reproduces a run.
//! - [`arts`] - reassign each Tactical Art's button combo (the `+8`
//!   command-glyph pointer in the SCUS arts-name table) so every art has a new,
//!   unique-within-character combo.
//! - [`items`] - the valid item-id pool (from the SCUS item-name table).
//! - [`unused`] - curated "unused content" sets (Evil Bat enemy ids, the
//!   Something Good / unnamed-accessory items) the opt-in toggles re-introduce.
//! - [`drops`] - the drop-table planner (shuffle / random).
//! - [`equipment`] - classify equipment ids + tier them, turning each monster's
//!   drop slot into a rare random weapon / armor / accessory drop.
//! - [`equip_bonus`] - redistribute the equipment passive stat tuples within
//!   each slot category (the SCUS `DAT_80074F68` bonus table).
//! - [`equip_mask`] - redistribute the equip-character mask (`+6`) of that same
//!   bonus table - who can wear each piece of gear - disjoint from the stat pass.
//! - [`shop`] - reassign what town stores sell (the gold-merchant stock is
//!   inline in each scene's field-VM script, op `0x49`).
//! - [`casino`] - reassign the casino prize-exchange table (a static overlay
//!   table that spends casino coins).
//! - [`monster`] - re-pack a monster slot in the `battle_data` archive.
//! - [`encounter`] - per-scene random-encounter formation-id shuffle.
//! - [`chest`] - treasure-chest item-give (field-VM op `0x39`) rewrite.
//! - [`disc`] - apply same-size PROT-entry edits to a real disc image
//!   (`DiscPatcher`), via the Mode 2/2352 sector write-back in `legaia_iso`.
//! - [`apply`] - high-level orchestration (`randomize_*`) the CLI drives.
//! - [`ppf`] - PPF 3.0 patch writer/reader (the portable deliverable).
//! - [`translation`] - language-pack export / import (community translations;
//!   YAML packs, same-size in-place text patching).

pub mod apply;
pub mod approach_fix;
pub mod arts;
pub mod arts_ap_grant;
pub mod arts_name_fix;
pub mod arts_power;
pub mod attack_count;
pub mod battle_texture;
pub mod bonus_drop;
pub mod casino;
pub mod charm_fix;
pub mod chest;
pub mod custom_items;
pub mod damage_ap;
pub mod delilas_cast;
pub mod delilas_challenge;
pub mod delilas_dome;
pub mod delilas_effects;
pub mod delilas_party;
pub mod delilas_signature_attack;
pub mod delilas_voice;
pub mod delilas_voice_fx;
pub mod delilas_xa_voice;
pub use legaia_disc_patch::disc;
pub(crate) use legaia_disc_patch::{compress_within, man_compressed_budget};
pub mod door;
pub mod drops;
pub mod earth_egg;
pub mod element_affinity;
pub mod encounter;
pub mod enemy_ally;
pub mod enemy_anim_mirror;
pub mod enemy_hp_bar;
pub mod equip_bonus;
pub mod equip_hand_frame;
pub mod equip_mask;
pub mod equip_transplant;
pub mod equipment;
pub mod fishing_price;
pub mod flee_exp;
pub mod house_door;
pub mod item_name;
pub mod item_price;
pub mod items;
pub mod jewel_fix;
pub mod kingdom;
pub mod location_name;
pub mod map_door;
pub(crate) mod mips;
#[cfg(test)]
pub(crate) mod mips_sim;
pub mod monster;
pub mod monster_class;
pub mod monster_stats;
pub mod monster_texture;
pub mod move_power;
pub mod nivora_field;
pub mod oscillating_ap;
pub mod party_swap;
pub use legaia_disc_patch::ppf;
pub mod rewards;
pub mod rng;
pub mod save_icon;
pub mod seru_overlay;
pub mod seru_trade;
pub mod shiny_seru;
pub mod shop;
pub use legaia_disc_patch::space_ledger;
#[cfg(test)]
mod space_ledger_rows_tests;
pub mod spell_cost;
pub mod spirit_ap;
pub mod starting_bag;
pub mod starting_items;
pub mod starting_level;
pub mod steal;
pub mod super_art_list;
pub mod super_art_menu;
pub mod super_art_power;
pub mod super_arts_pack;
pub mod texture;
pub mod texture_palettes;
pub mod translation;
pub mod unused;
pub mod weapon_specialty;
