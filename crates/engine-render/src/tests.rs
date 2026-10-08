use super::*;
use crate::renderer::letterbox_scale;
use crate::shaders::*;
// These UI helpers live in `legaia-engine-ui` and are on the crate root's
// explicit re-export list.
use crate::{apply_alpha, hp_bar_color_index, mp_bar_color_index};
use glam::Mat4;

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
