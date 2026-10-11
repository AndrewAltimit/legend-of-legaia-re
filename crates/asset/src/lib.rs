//! The `asset` command-line tool's crate, and the old import paths of the
//! format crates it was divided into.
//!
//! No format is defined here. Every `pub use` below is a transitional alias
//! that keeps `legaia_asset::<module>` resolving for crates that have not yet
//! been rewritten to name the owning crate:
//!
//! - `legaia-container-formats` - asset-type dispatcher, descriptor,
//!   DATA_FIELD stream, pack, the container-level detectors, the sound
//!   containers.
//! - `legaia-scene-formats`, `legaia-minigame-formats`, `legaia-ui-formats` -
//!   the formats by what they describe.
//! - `legaia-actor-formats`, `legaia-game-tables`, `legaia-overlay-images` -
//!   models, static data tables, code-overlay images.
//! - `legaia-disc-audit` - byte accounting, classification, catalogs.
//!
//! New code names the owning crate directly.

#[doc(hidden)]
pub use legaia_actor_formats::battle_char_assembly;
#[doc(hidden)]
pub use legaia_actor_formats::battle_char_pack;
#[doc(hidden)]
pub use legaia_actor_formats::battle_char_palette;
#[doc(hidden)]
pub use legaia_actor_formats::battle_data_pack;
#[doc(hidden)]
pub use legaia_actor_formats::battle_texture_catalog;
#[doc(hidden)]
pub use legaia_actor_formats::befect_cluster;
#[doc(hidden)]
pub use legaia_actor_formats::character_gltf;
#[doc(hidden)]
pub use legaia_actor_formats::character_pack;
#[doc(hidden)]
pub use legaia_actor_formats::face_anim;
#[doc(hidden)]
pub use legaia_actor_formats::field_char_textures;
#[doc(hidden)]
pub use legaia_actor_formats::gltf_color;
#[doc(hidden)]
pub use legaia_actor_formats::me_archive;
#[doc(hidden)]
pub use legaia_actor_formats::mesh_raster;
#[doc(hidden)]
pub use legaia_actor_formats::monster_archive;
#[doc(hidden)]
pub use legaia_actor_formats::monster_gltf;
#[doc(hidden)]
pub use legaia_actor_formats::monster_model;
#[doc(hidden)]
pub use legaia_actor_formats::player_anm;
#[doc(hidden)]
pub use legaia_actor_formats::player_file_annex;
#[doc(hidden)]
pub use legaia_actor_formats::scene_gltf;
#[doc(hidden)]
pub use legaia_actor_formats::summon_creatures;
#[doc(hidden)]
pub use legaia_actor_formats::summon_readef;
#[doc(hidden)]
pub use legaia_container_formats::anm_detect;
#[doc(hidden)]
pub use legaia_container_formats::data_field_truncated;
#[doc(hidden)]
pub use legaia_container_formats::efect_pack;
#[doc(hidden)]
pub use legaia_container_formats::effect_bundle;
#[doc(hidden)]
pub use legaia_container_formats::pack;
#[doc(hidden)]
pub use legaia_container_formats::tim_scan;
#[doc(hidden)]
pub use legaia_container_formats::tmd_scan;
#[doc(hidden)]
pub use legaia_container_formats::tmd_size_prefix;
#[doc(hidden)]
pub use legaia_container_formats::{
    AssetType, Container, ContainerReport, DecodeMode, Descriptor, DescriptorReport, StreamChunk,
    StreamReport, StreamTerminator, decode, parse_player_lzs, parse_streaming,
    parse_streaming_with, validate, walk_descriptor_pairs,
};
#[doc(hidden)]
pub use legaia_disc_audit::battle_jump_tables;
#[doc(hidden)]
pub use legaia_disc_audit::byte_account;
#[doc(hidden)]
pub use legaia_disc_audit::categorize;
#[doc(hidden)]
pub use legaia_disc_audit::field_probe_tables;
#[doc(hidden)]
pub use legaia_disc_audit::formation_census;
#[doc(hidden)]
pub use legaia_disc_audit::inherited_tail;
#[doc(hidden)]
pub use legaia_disc_audit::other3_roster;
#[doc(hidden)]
pub use legaia_disc_audit::switch_tables;
#[doc(hidden)]
pub use legaia_disc_audit::tim_catalog;
#[doc(hidden)]
pub use legaia_disc_audit::tim_deep_catalog;
#[doc(hidden)]
pub use legaia_disc_audit::tim_labels;
#[doc(hidden)]
pub use legaia_game_tables::absorb_caption;
#[doc(hidden)]
pub use legaia_game_tables::accessory_passive;
#[doc(hidden)]
pub use legaia_game_tables::battle_attack_camera_table;
#[doc(hidden)]
pub use legaia_game_tables::battle_camera_table;
#[doc(hidden)]
pub use legaia_game_tables::element_affinity;
#[doc(hidden)]
pub use legaia_game_tables::equip_stats;
#[doc(hidden)]
pub use legaia_game_tables::fmv_dispatch;
#[doc(hidden)]
pub use legaia_game_tables::item_effect;
#[doc(hidden)]
pub use legaia_game_tables::item_names;
#[doc(hidden)]
pub use legaia_game_tables::level_up_tables;
#[doc(hidden)]
pub use legaia_game_tables::menu_windows;
#[doc(hidden)]
pub use legaia_game_tables::mode_table;
#[doc(hidden)]
pub use legaia_game_tables::move_power;
#[doc(hidden)]
pub use legaia_game_tables::new_game;
#[doc(hidden)]
pub use legaia_game_tables::seru_side_effect;
#[doc(hidden)]
pub use legaia_game_tables::seru_trade;
#[doc(hidden)]
pub use legaia_game_tables::sfx_table;
#[doc(hidden)]
pub use legaia_game_tables::spell_anim_pairs;
#[doc(hidden)]
pub use legaia_game_tables::spell_names;
#[doc(hidden)]
pub use legaia_game_tables::steal_table;
#[doc(hidden)]
pub use legaia_game_tables::str_fmv_table;
#[doc(hidden)]
pub use legaia_game_tables::victory_pose;
#[doc(hidden)]
pub use legaia_game_tables::widget_script;
#[doc(hidden)]
pub use legaia_game_tables::worldmap_menu;
#[doc(hidden)]
pub use legaia_game_tables::xa_cue_table;
#[doc(hidden)]
pub use legaia_minigame_formats::baka_opponents;
#[doc(hidden)]
pub use legaia_minigame_formats::dance_art;
#[doc(hidden)]
pub use legaia_minigame_formats::dance_cast;
#[doc(hidden)]
pub use legaia_minigame_formats::dance_chart;
#[doc(hidden)]
pub use legaia_minigame_formats::fishing_captions;
#[doc(hidden)]
pub use legaia_minigame_formats::fishing_exchange;
#[doc(hidden)]
pub use legaia_minigame_formats::fishing_species;
#[doc(hidden)]
pub use legaia_minigame_formats::fishing_sprites;
#[doc(hidden)]
pub use legaia_minigame_formats::minigame_art;
#[doc(hidden)]
pub use legaia_minigame_formats::minigame_sfx;
#[doc(hidden)]
pub use legaia_minigame_formats::minigame_slot_scene;
#[doc(hidden)]
pub use legaia_minigame_formats::muscle_dome;
#[doc(hidden)]
pub use legaia_minigame_formats::ringside_still;
#[doc(hidden)]
pub use legaia_minigame_formats::slot_payout;
#[doc(hidden)]
pub use legaia_overlay_images::boot_overlay;
#[doc(hidden)]
pub use legaia_overlay_images::cast_effect_pool;
#[doc(hidden)]
pub use legaia_overlay_images::mips_overlay;
#[doc(hidden)]
pub use legaia_overlay_images::overlay_ptr_table;
#[doc(hidden)]
pub use legaia_overlay_images::slot_b_module;
#[doc(hidden)]
pub use legaia_overlay_images::static_overlay;
#[doc(hidden)]
pub use legaia_overlay_images::summon_overlay;
#[doc(hidden)]
pub use legaia_scene_formats::battle_backdrop;
#[doc(hidden)]
pub use legaia_scene_formats::clut_walk;
#[doc(hidden)]
pub use legaia_scene_formats::cutscene_text;
#[doc(hidden)]
pub use legaia_scene_formats::field_disasm;
#[doc(hidden)]
pub use legaia_scene_formats::field_map;
#[doc(hidden)]
pub use legaia_scene_formats::field_objects;
#[doc(hidden)]
pub use legaia_scene_formats::field_pack;
#[doc(hidden)]
pub use legaia_scene_formats::inn_costs;
#[doc(hidden)]
pub use legaia_scene_formats::kingdom_bundle;
#[doc(hidden)]
pub use legaia_scene_formats::man_edit;
#[doc(hidden)]
pub use legaia_scene_formats::man_motion;
#[doc(hidden)]
pub use legaia_scene_formats::man_section;
#[doc(hidden)]
pub use legaia_scene_formats::ocean;
#[doc(hidden)]
pub use legaia_scene_formats::place_names;
#[doc(hidden)]
pub use legaia_scene_formats::scene_asset_table;
#[doc(hidden)]
pub use legaia_scene_formats::scene_event_scripts;
#[doc(hidden)]
pub use legaia_scene_formats::scene_scripted_asset_table;
#[doc(hidden)]
pub use legaia_scene_formats::scene_tmd_stream;
#[doc(hidden)]
pub use legaia_scene_formats::scene_v12_table;
#[doc(hidden)]
pub use legaia_scene_formats::scene_vdf;
#[doc(hidden)]
pub use legaia_scene_formats::shop_stock;
#[doc(hidden)]
pub use legaia_scene_formats::stage_geom;
#[doc(hidden)]
pub use legaia_scene_formats::world_map_overlay;
#[doc(hidden)]
pub use legaia_container_formats::bse_bank;
#[doc(hidden)]
pub use legaia_container_formats::scene_vab_stream;
#[doc(hidden)]
pub use legaia_container_formats::sound_pack;
#[doc(hidden)]
pub use legaia_container_formats::vab_multi_bank;
#[doc(hidden)]
pub use legaia_ui_formats::battle_ui_strings;
#[doc(hidden)]
pub use legaia_ui_formats::init_pak;
#[doc(hidden)]
pub use legaia_ui_formats::interior_page;
#[doc(hidden)]
pub use legaia_ui_formats::menu_glyph_atlas;
#[doc(hidden)]
pub use legaia_ui_formats::save_icon;
#[doc(hidden)]
pub use legaia_ui_formats::screen_elements;
#[doc(hidden)]
pub use legaia_ui_formats::system_ui_bundle;
#[doc(hidden)]
pub use legaia_ui_formats::tim_palette_context;
#[doc(hidden)]
pub use legaia_ui_formats::title_pak;
#[doc(hidden)]
pub use legaia_ui_formats::ui_widgets;
