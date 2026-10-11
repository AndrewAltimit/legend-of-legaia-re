//! What an actor is built from on the disc - the party in both its forms,
//! the monsters and the summon creatures - and the animation that poses
//! them: the monster archive (PROT 867) and its models, the player battle
//! files and the per-character equipment assembly, the field character pack
//! and its animation bundles, face animation, battle textures and palettes,
//! the `summon.dat` / `readef.DAT` side-band and `"ME"` archives, the
//! software mesh rasteriser, and the monster / scene / character `.glb`
//! builders.
//!
//! A doc reference to a crate that depends on this one is a plain code span,
//! since rustdoc cannot resolve a link upward.

#![forbid(unsafe_code)]

pub mod battle_char_assembly;
pub mod battle_char_pack;
pub mod battle_char_palette;
pub mod battle_data_pack;
pub mod battle_texture_catalog;
pub mod befect_cluster;
pub mod character_gltf;
pub mod character_pack;
pub mod face_anim;
pub mod field_char_textures;
pub mod gltf_color;
pub mod me_archive;
pub mod mesh_raster;
pub mod monster_archive;
pub mod monster_gltf;
pub mod monster_model;
pub mod player_anm;
pub mod player_file_annex;
pub mod scene_gltf;
pub mod summon_creatures;
pub mod summon_readef;
