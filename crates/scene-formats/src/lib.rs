//! The formats a field or world-map scene is built from: the scene bundle
//! shapes and streams, the MAN and its sections, the field-VM bytecode as
//! data (`field_disasm`, `man_edit`), field objects and the walkability map,
//! and what a scene script carries inline (shop stock, scripted gold
//! charges, place names, cutscene narration), plus the kingdom bundle,
//! world-map animation bank, ocean tile, stage geometry and battle backdrop.
//!
//! Sits on `legaia-container-formats`. It reads `legaia-overlay-images` only
//! for the summon part records an event script can spawn, and names no UI,
//! minigame or battle-model format.

#![forbid(unsafe_code)]

pub mod battle_backdrop;
pub mod clut_walk;
pub mod cutscene_text;
pub mod encounter_record;
pub mod field_disasm;
pub mod field_map;
pub mod field_objects;
pub mod field_pack;
pub mod inn_costs;
pub mod kingdom_bundle;
pub mod man_edit;
pub mod man_motion;
pub mod man_section;
pub mod ocean;
pub mod place_names;
pub mod scene_asset_table;
pub mod scene_event_scripts;
pub mod scene_scripted_asset_table;
pub mod scene_tmd_stream;
pub mod scene_v12_table;
pub mod scene_vdf;
pub mod shop_stock;
pub mod stage_geom;
pub mod vdf_morph;
pub mod world_map_overlay;
