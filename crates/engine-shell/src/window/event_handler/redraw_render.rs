//! The redraw's render block: the steps between the frame prep and the
//! present, in order, with the borrows they need held across them - split
//! out of `handle_redraw`.

use super::super::*;
use super::redraw_draws::{DrawCtx, DrawLists, MeshStore};
use super::redraw_overlay::OverlayDraws;
use super::redraw_prep::FramePrep;
use super::redraw_present::{self, present_target};
use super::redraw_stage::FrameView;

impl PlayWindowApp {
    /// The render block: stage the renderer, upload and pose the frame's
    /// meshes, build the scene draw lists and the 2D layers, and capture or
    /// present. Entered only when the renderer, the scene VRAM and the font
    /// atlas are all present.
    pub(super) fn render_scene_frame(
        &mut self,
        prep: FramePrep,
        battle_intro: &mut Option<legaia_engine_render::battle_intro::BattleIntro>,
        field_tail_ticks: u32,
        view_cells: Option<legaia_engine_core::field_view_window::ViewCells>,
        cutscene_cam: Option<super::redraw_passes::CutsceneCam>,
    ) {
        let FramePrep {
            field_morph_rebuilds,
            npc_morph_rebuilds,
            battle_intro: _,
            battle_intro_prims,
            field_fog_prims,
            move_strip_prims,
            fishing_line_prims,
            slot_prims,
        } = prep;
        let FrameView {
            w,
            h,
            cam,
            in_world_map,
        } = self.stage_frame_render_state(cutscene_cam);
        self.drain_frame_mesh_uploads();
        let posed =
            self.pose_frame_meshes(field_morph_rebuilds, npc_morph_rebuilds, field_tail_ticks);
        let (Some(r), Some(vram), Some(atlas)) = (
            self.win.renderer.as_ref(),
            self.uploaded_vram.as_ref(),
            self.font_atlas.as_ref(),
        ) else {
            unreachable!("render gate checked above");
        };
        // A live Baka duel draws against its own VRAM.
        // So does the dance venue - the hall's own upload, never the
        // walked-in scene's.
        let duel_gpu = self
            .baka_gpu
            .as_ref()
            .or(self.muscle_gpu.as_ref())
            .or(self.fishing_gpu.as_ref());
        // And the slot machine: every quad it draws samples its art pack.
        let vram = match (
            duel_gpu,
            self.dance_venue_gpu.as_ref(),
            self.slot_gpu.as_ref(),
        ) {
            (Some(g), _, _) => &g.vram,
            (None, Some(d), _) => &d.vram,
            (None, None, Some(v)) => v,
            (None, None, None) => vram,
        };
        // Re-borrow the memo immutably: `slot -> this frame's posed halves`.
        let npc_posed: std::collections::HashMap<u8, &NpcPosedHalves> = posed
            .npc_frames
            .iter()
            .filter_map(|k| self.npc_pose_cache.get(k).map(|m| (k.0, m)))
            .collect();
        // Everything above this mark is per-frame skinning: CPU mesh
        // re-pose + GPU re-upload for the player, the animated props and
        // every placed NPC.
        legaia_engine_render::profile::mark("pose");
        // Iterate every actor that has a `tmd_binding`. Scene-init
        // actors (slots 0..N from `init_scene_animations`) have
        // their bindings set but aren't necessarily `.active` -
        // the original draws iteration walked meshes directly,
        // so we preserve that behaviour by not gating on
        // `.active` here. Dynamically spawned actors set both
        // `.active` and a binding to their freshly uploaded
        // mesh slot (beyond `scene_tmd_data.len()`) via the
        // spawn pass above.
        //
        // Suppress 3D draws while the boot UI is active so the
        // last-loaded scene (e.g. a town) doesn't show through
        // behind publisher logos / title / save-select. The one
        // exception is the party-wipe hand-off: retail holds the
        // final battle frame while mode 22 CARD INIT streams the
        // menu overlay, so the GameOver hold keeps drawing the
        // (frozen, untick'd) battle scene underneath.
        let game_over_hold = matches!(self.boot_ui, BootUiState::GameOver(_));
        let draw_cx = DrawCtx {
            app: &*self,
            store: MeshStore {
                meshes: &self.meshes,
                color_meshes: &self.color_meshes,
                field_lit: &self.field_lit,
                field_morph_live: &self.field_morph_live,
                ground_heightfield: &self.ground_heightfield,
                ground_crop: &self.ground_crop,
                baka_gpu: &self.baka_gpu,
                muscle_gpu: &self.muscle_gpu,
                fishing_gpu: &self.fishing_gpu,
                dance_venue_gpu: &self.dance_venue_gpu,
                dance_cast_gpu: &self.dance_cast_gpu,
                npc_morph_static: &self.npc_morph_static,
                npc_posed: &npc_posed,
                posed: &posed,
            },
            r,
            cam,
            cutscene_cam,
            view_cells: &view_cells,
            in_world_map,
            game_over_hold,
        };
        let DrawLists {
            mut draws,
            color_draws,
            clip_marks,
            nclip_marks,
            color_clip_marks,
        } = draw_cx.build_scene_draws();
        let OverlayDraws {
            hud,
            logo_draw_vec,
            title_draw_vec,
            menu_glyph_draw_vec,
            muscle_hub_draw_vec,
            muscle_hub_blend,
            save_chrome_draw_vec,
            save_chrome_blend,
            caption_draw_vec,
        } = self.build_overlay_draws(w, h, cam);
        let overlay = TextOverlay {
            atlas,
            draws: &hud,
            blend: &[],
        };
        let logo_overlay = self.publisher_logos.as_ref().map(|p| TextOverlay {
            atlas: &p.atlas,
            draws: &logo_draw_vec,
            blend: &[],
        });
        let title_overlay = self.title_screen.as_ref().map(|t| TextOverlay {
            atlas: &t.atlas,
            draws: &title_draw_vec,
            blend: &[],
        });
        let menu_glyph_overlay = self.menu_glyphs.as_ref().map(|m| TextOverlay {
            atlas: &m.atlas,
            draws: &menu_glyph_draw_vec,
            blend: &[],
        });
        let save_chrome_overlay = self.save_menu.as_ref().map(|sm| TextOverlay {
            atlas: &sm.atlas,
            draws: &save_chrome_draw_vec,
            blend: &save_chrome_blend,
        });
        let muscle_hub_overlay = self.muscle_hub.as_ref().map(|m| TextOverlay {
            atlas: &m.atlas,
            draws: &muscle_hub_draw_vec,
            blend: &muscle_hub_blend,
        });
        let caption_overlay = self
            .caption_atlas
            .as_ref()
            .map(|(atlas, _, _)| TextOverlay {
                atlas,
                draws: &caption_draw_vec,
                blend: &[],
            });
        let scene_clear = self.frame_clear_color(game_over_hold);

        // Slot 1: logos OR title-art bands (title still
        // emits during SaveSelect, dimmed). Slot 2: either
        // the save-menu chrome (panel + slot pills) when
        // SaveSelect is active, or the menu-glyph atlas
        // (deprecated no-disc title-menu fallback) otherwise.
        // The opdeene caption takes slot 1 when active: during the opening
        // cutscene the boot-UI logo / title overlays are inactive (their
        // draw vecs empty), so there is no contention.
        let sprites_slot_1 = if !caption_draw_vec.is_empty() {
            caption_overlay.as_ref()
        } else if !logo_draw_vec.is_empty() {
            logo_overlay.as_ref()
        } else if !title_draw_vec.is_empty() {
            title_overlay.as_ref()
        } else if !muscle_hub_draw_vec.is_empty() {
            muscle_hub_overlay.as_ref()
        } else {
            None
        };
        let sprites_slot_2 = if !save_chrome_draw_vec.is_empty() {
            save_chrome_overlay.as_ref()
        } else if !menu_glyph_draw_vec.is_empty() {
            menu_glyph_overlay.as_ref()
        } else {
            None
        };
        let fx = self.build_fx_frame(r, cam, in_world_map, cutscene_cam);
        let fx_cam = fx.fx_cam;
        fx.push_draws(&mut draws);
        // The floating value readout is a screen-space primitive run, not
        // a scene mesh: see `screen_prims` below, where both hosts build
        // it from `legaia_engine_ui::battle_numerals`.
        // The scripted screen fade (op 0x4C 0x12) is NOT drawn as a wash
        // mesh here: it is a multiply tint staged into the colour grade +
        // depth-cue far colour (see the grade staging above), matching
        // the retail mechanism - the 3D scene darkens while the narration
        // overlay keeps scrolling bright.
        legaia_engine_render::profile::draw_counts(draws.len(), color_draws.len());
        self.write_draw_census(&draws);
        redraw_present::stage_draw_marks(
            r,
            draws.len(),
            color_draws.len(),
            clip_marks,
            color_clip_marks,
            nclip_marks,
        );
        let scene = RenderScene {
            vram,
            draws: &draws,
            color_draws: &color_draws,
            // Effect outlines share the billboards' `fx_cam` (the two
            // sources are mutually exclusive, and off the battle stage
            // `fx_cam == cam`, so the slot-4 inspection lines are unaffected).
            overlay_lines: fx
                .world_map_slot4_lines
                .as_ref()
                .or(fx.effect_lines.as_ref())
                .map(|m| (m, fx_cam)),
            overlay_sprites: sprites_slot_1,
            overlay_sprites_2: sprites_slot_2,
            overlay_text: Some(&overlay),
            clear_color: scene_clear,
        };
        legaia_engine_render::profile::mark("drawlist");
        // On the frame the transition arms, land the field frame in the
        // software VRAM the intro strips texture themselves with, and draw
        // the rest of this frame against that page. Retail gets it for
        // free - on the console the framebuffer *is* VRAM - so the port
        // re-renders this scene offscreen and blits the readback in.
        if let Some(v) = Self::capture_battle_intro_frame(
            battle_intro.as_mut(),
            r,
            &scene,
            self.cpu_vram_base.as_ref(),
        ) {
            // Keep the captured page GPU-resident for the whole
            // transition: the capture is a one-shot, but every
            // transition frame's primitives sample it (the curtain
            // strips, and the tile shatter's pages + shade page).
            self.battle_intro_vram = Some(v);
        }
        let scene = match (battle_intro.as_ref(), self.battle_intro_vram.as_ref()) {
            (Some(_), Some(v)) => RenderScene { vram: v, ..scene },
            _ => scene,
        };
        // A field-VM `43 12` copy that reads the display framebuffer
        // (the ending vignettes' photo grab into `(512, 0)`) waits for
        // this frame: land it in the CPU VRAM's display page, and the
        // next drain (`apply_world_clut_fx`) runs the copy. Shared
        // handshake `World::framebuffer_grab_pending`; the browser page
        // lands its own frame through `play_land_frame_grab`.
        if self.session.host.world.framebuffer_grab_pending()
            && let Some(base) = self.cpu_vram_base.as_mut()
        {
            match r.capture_scene_rgba(legaia_engine_render::RenderTarget::Scene(&scene)) {
                Ok(img) => {
                    legaia_engine_render::vram_capture::land_display_frame(
                        &img.rgba, img.width, img.height, base,
                    );
                    self.session.host.world.land_framebuffer();
                }
                Err(e) => log::error!("play-window: framebuffer grab: {e:#}"),
            }
        }
        let (screen_prims, light_prims) = self.build_screen_prims(
            battle_intro_prims,
            field_fog_prims,
            move_strip_prims,
            slot_prims,
            fishing_line_prims,
            fx_cam,
        );
        let target = |scene| present_target(scene, &screen_prims, &light_prims);
        redraw_present::capture_sweep_frame(
            r,
            target(&scene),
            self.screenshot.as_ref(),
            self.tick_no,
            &mut self.sweep_next_tick,
        );
        self.present_frame(r, &scene, &screen_prims, &light_prims);
    }
}
