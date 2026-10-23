//! The crate's integration tests as one test binary.
//!
//! Each `tests/<name>.rs` is a module here rather than its own binary
//! (`autotests = false` in Cargo.toml): one compile and one link instead of
//! one per file. Run a single file's tests with
//! `cargo test -p legaia-web-viewer --test integration <name>::`.

mod arts_view_real;
mod audio_extract;
mod audio_parity;
mod battle_palette_overlay;
mod bgm_chain;
mod catalog_palette_context_real;
mod character_anm_glb_real;
mod disc_extract;
mod equipment_view_real;
mod field_npc_catalog;
mod field_scene_anim;
mod field_scene_assembly;
mod field_scene_blend;
mod field_scene_light;
mod glb_packet_colour_real;
mod hub_palette_stp_real;
mod ocean_assets;
mod overworld_decoration_cue_js;
mod overworld_ground_cue_glsl;
mod scene_lighting_real;
mod sfx_cues_real;
mod shop_vendors_real;
// --- lane P ---
mod shop_items_real;
mod str_video;
mod summon_view_real;
mod town_identity;
mod vab_duration_sweep;
mod walk_ground_parity;
mod walk_placements_parity;
mod world_map_labels_real;
mod world_map_pack_hybrid_real;
mod world_overview_clut_walk_real;
mod world_overview_regression;
