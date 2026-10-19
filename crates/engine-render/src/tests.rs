use super::*;
use crate::renderer::letterbox_scale;
use crate::shaders::*;
// These UI helpers live in `legaia-engine-ui`.
use glam::Mat4;
use legaia_engine_ui::{apply_alpha, hp_bar_color_index, mp_bar_color_index};

mod battle_hud;
mod battle_intro_emitter;
mod blend;
mod color_space;
mod menu_overlays;
mod overworld_flat_depth_gpu;
mod screen_overlay_gpu;
mod sprite_blend;
mod text_overlay;
mod title_save_screen;
mod vram_capture_gpu;
// --- lane B ---
mod fog_volume_parity;
mod page_glsl;
mod shader_eval;
mod shading_law_parity;
mod vertex_law_parity;
// --- end lane B ---
// --- lane L ---
mod bloom_parity;
