//! Battle model formats and their glTF export: the monster archive
//! (PROT 867) and its models, the player battle files and the per-character
//! equipment assembly, face animation, battle textures and palettes, the
//! `summon.dat` / `readef.DAT` side-band and `"ME"` archives, the software
//! mesh rasteriser, and the monster / scene / character `.glb` builders.
//!
//! `legaia-asset` re-exports every module here at its old path. Doc links
//! that pointed back up at it are plain code spans, since rustdoc cannot
//! resolve a link into a dependent crate.

#![forbid(unsafe_code)]

// Items these files name as `crate::...`, which live in a crate below this
// one; binding them here keeps the moved files' paths unchanged.
use legaia_game_tables::equip_stats;
use legaia_game_tables::item_names;

pub mod battle_char_assembly;
pub mod battle_char_pack;
pub mod battle_char_palette;
pub mod battle_data_pack;
pub mod battle_texture_catalog;
pub mod character_gltf;
pub mod face_anim;
pub mod gltf_color;
pub mod me_archive;
pub mod mesh_raster;
pub mod monster_archive;
pub mod monster_gltf;
pub mod monster_model;
pub mod scene_gltf;
pub mod summon_creatures;
pub mod summon_readef;
