//! The crate's integration tests as one test binary.
//!
//! Each `tests/<name>.rs` is a module here rather than its own binary
//! (`autotests = false` in Cargo.toml): one compile and one link instead of
//! one per file. Run a single file's tests with
//! `cargo test -p legaia-actor-formats --test integration <name>::`.

mod battle_char_assembly_real;
mod battle_char_pack_real;
mod battle_char_palette_real;
mod battle_data_pack_real;
mod battle_pose_orientation_real;
mod befect_cluster_real;
mod character_pack_real;
mod equip_alone_skin_real;
mod equip_isolate_real;
mod equip_item_real;
mod equip_repair_real;
mod equip_texture_coverage_real;
mod face_anim_real;
mod field_char_textures_real;
mod lzs_compress_roundtrip_real;
mod monster_archive_real;
mod monster_cross_table_integrity;
mod monster_model_roundtrip_real;
mod monster_names_pal_real;
mod monster_readef_group_real;
mod monster_slim_real;
mod monster_tmd_reencode_real;
mod player_action_table_real;
mod player_anm_real;
mod player_anm_sampler_real;
mod scene_gltf_real;
mod spell_names_real;
mod summon_creature_tmd_map_real;
mod summon_readef_aux_slot_real;
mod summon_readef_real;
mod swing_anim_real;
