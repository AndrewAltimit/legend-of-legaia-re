//! The redraw's 2D and effect layers: the HUD text and the sprite-overlay
//! draw lists (boot screens, menu chrome, the field and battle HUDs, the
//! caption), the frame's clear colour, the 3D effect meshes, and the
//! screen-space primitive lists composited over the scene - split out of
//! `handle_redraw`.

use super::super::*;
use super::redraw_passes::CutsceneCam;

/// The frame's text and sprite-overlay draw lists, owned by the frame.
pub(super) struct OverlayDraws {
    /// The glyph layer.
    pub hud: Vec<TextDraw>,
    pub logo_draw_vec: Vec<legaia_engine_render::SpriteDraw>,
    pub title_draw_vec: Vec<legaia_engine_render::SpriteDraw>,
    pub menu_glyph_draw_vec: Vec<legaia_engine_render::SpriteDraw>,
    pub muscle_hub_draw_vec: Vec<legaia_engine_render::SpriteDraw>,
    pub muscle_hub_blend: Vec<legaia_engine_render::OverlayBlendSpan>,
    /// The system-UI atlas slot: the field party HUD, save / menu / dialog /
    /// shop / name-entry / battle chrome.
    pub save_chrome_draw_vec: Vec<legaia_engine_render::SpriteDraw>,
    pub save_chrome_blend: Vec<legaia_engine_render::OverlayBlendSpan>,
    /// The opening caption quad.
    pub caption_draw_vec: Vec<legaia_engine_render::SpriteDraw>,
}

/// The frame's effect meshes (built and uploaded here, drawn in the scene
/// pass) and the cameras they ride.
pub(super) struct FxFrame {
    /// The battle FX camera (`cam` with the stage scale composed), `cam`
    /// itself off the battle stage.
    pub fx_cam: Mat4,
    pub effect_billboard: Option<UploadedVramMesh>,
    pub effect_lines: Option<legaia_engine_render::UploadedLines>,
    pub world_map_slot4_lines: Option<legaia_engine_render::UploadedLines>,
    pub effect_model_draws: Vec<(UploadedVramMesh, Mat4)>,
    pub summon_part_draws: Vec<(UploadedVramMesh, Mat4)>,
    pub field_fx_draws: Vec<(UploadedVramMesh, Mat4)>,
    pub screen_fx_tex: Option<UploadedVramMesh>,
    pub screen_fx_mvp: Mat4,
}

impl FxFrame {
    /// Append the effect meshes to the scene's textured draws, in the order
    /// the passes run: billboards, effect models, summon parts, field FX
    /// parts, the screen-space streak.
    pub(super) fn push_draws<'x>(&'x self, draws: &mut Vec<SceneDraw<'x>>) {
        let fx_cam = self.fx_cam;
        let effect_billboard = &self.effect_billboard;
        let effect_model_draws = &self.effect_model_draws;
        let summon_part_draws = &self.summon_part_draws;
        let field_fx_draws = &self.field_fx_draws;
        let screen_fx_tex = &self.screen_fx_tex;
        let screen_fx_mvp = self.screen_fx_mvp;
        if let Some(mesh) = effect_billboard.as_ref() {
            draws.push(SceneDraw {
                mesh,
                mvp: fx_cam,
                cue: None,
            });
        }
        for (mesh, model) in effect_model_draws {
            draws.push(SceneDraw {
                mesh,
                mvp: fx_cam * *model,
                cue: None,
            });
        }
        for (mesh, model) in summon_part_draws {
            draws.push(SceneDraw {
                mesh,
                mvp: fx_cam * *model,
                cue: None,
            });
        }
        for (mesh, model) in field_fx_draws {
            draws.push(SceneDraw {
                mesh,
                mvp: fx_cam * *model,
                cue: None,
            });
        }
        if let Some(m) = screen_fx_tex {
            draws.push(SceneDraw {
                mesh: m,
                mvp: screen_fx_mvp,
                cue: None,
            });
        }
    }
}

impl PlayWindowApp {
    /// The frame's text layer and sprite-overlay draw lists.
    pub(super) fn build_overlay_draws(&self, w: u32, h: u32, cam: Mat4) -> OverlayDraws {
        // Retail's field party-status readout - name / LV / HP / MP per
        // present member over a translucent plate, top-left of every
        // walkable frame. Two halves: the plate + label / numeral cells
        // sample the system-UI atlas and ride the sprite slot below, the
        // names ride the glyph layer here.
        let field_hud_draws = self.field_party_hud_draws(w, h);
        // The shop-family overlay (shop / prize / inn / banners), built
        // once and read by both the text pass and the chrome sprite pass.
        let screens = self.shop_overlay_frame(w, h);
        let mut hud = self.build_hud(w, h, &screens);
        hud.extend(field_hud_draws.text.iter().copied());
        // Post-battle spoils panel. The XP / gold / drops a victory
        // credits used to land with no on-screen acknowledgement at all
        // (`World::battle.last_rewards` had no reader outside its own
        // declaration); this is the shared `engine-ui` builder both hosts
        // draw. Suppressed while a boot-UI panel owns the frame.
        if !self.boot_ui.is_active() {
            hud.extend(self.battle_spoils_draws(w, h));
            hud.extend(self.encounter_hint_draws(w, h));
            // The field overlay's passive-ability badge column, floated
            // over the player's head (`FUN_801d095c`). Shares the scene
            // camera with everything else this frame; the browser play
            // page draws the same list off the same World seat.
            hud.extend(self.passive_hud_draws(cam, w, h));
            // The battle value readout's **font fallback**, for the
            // frames before the battle VRAM makes retail's numeral sheet
            // resident. The browser play page has had one since its own
            // prim pass landed; this host drew nothing at all in that
            // window, so the same fight opened with numbers in the tab
            // and none in the window. Mutually exclusive with
            // `battle_value_readout_prims` by construction - each checks
            // the same `battle_vram` residency, opposite ways.
            hud.extend(self.battle_value_readout_draws(cam, w, h));
        }

        // Boot-phase sprite overlay: alternates between the
        // publisher-logos atlas (during PublisherLogos) and
        // the title-screen atlas (during Title). PROKION/SCEA
        // are vertically-packed sprite atlases -
        // `publisher_logo_sprite_draws` unfolds them into N
        // side-by-side strips; Contrail/WARNING + the title
        // TIM produce a single quad each.
        let logo_draw_vec = self.publisher_logo_sprite_draws(w, h);
        let title_draw_vec = self.title_screen_sprite_draws(w, h);
        let menu_glyph_draw_vec = self.title_menu_glyph_sprite_draws(w, h);
        // Muscle Dome hub screens (intro card / ROUND banner / INTERVAL +
        // score tally), placed by the shared `other_game_hud` emitters.
        // Rides sprite slot 1: the boot-UI overlays that own it are all
        // inactive while a dome leg or its between-legs beat is up.
        let (muscle_hub_draw_vec, muscle_hub_blend) = self.muscle_hub_sprite_draws(w, h);
        // Slot-2 chrome samples the resident system-UI atlas.
        // Save-select pills/panel and the field-menu window
        // frame are mutually-exclusive boot states, so both
        // share this one vec (the field-menu frame draws
        // behind its text, which is emitted in the text layer).
        // The field party HUD leads this vec so its translucent plate
        // lands under its own label / numeral cells - within one overlay
        // the draw order is the vec order. It is suppressed whenever any
        // other surface that samples this atlas is up, so there is no
        // contention with the chrome appended after it.
        let mut save_chrome_draw_vec = field_hud_draws.sprites;
        // The save screen's subtractive darkening under its active panel
        // (`SaveScreenDarken`): one `B - F` draw inside this overlay, so
        // everything before it in the vec is darkened and the panel after
        // it is not - the same split the browser play page makes.
        let mut save_chrome_blend = Vec::new();
        {
            let base = save_chrome_draw_vec.len() as u32;
            let (save_sprites, darken) = self.save_select_chrome_sprite_draws(w, h);
            if let Some(i) = darken {
                save_chrome_blend.push(legaia_engine_render::OverlayBlendSpan {
                    start: base + i as u32,
                    count: 1,
                    abr: 2,
                });
            }
            save_chrome_draw_vec.extend(save_sprites);
        }
        // The post-battle report's two framed windows (level-up above,
        // spoils below). Same atlas, and mutually exclusive with the
        // boot/menu chrome.
        if !self.boot_ui.is_active() {
            save_chrome_draw_vec.extend(self.battle_spoils_chrome_sprite_draws(w, h));
        }
        save_chrome_draw_vec.extend(self.field_menu_chrome_sprite_draws(w, h));
        // The shop-family overlay's sprites: the gold shop / prize
        // exchange window frames and atlas markers, and the fallback
        // panel's gold frame - the same frame `build_hud` drew the texts
        // of.
        save_chrome_draw_vec.extend(screens.sprites);
        // Dialog-window chrome (gradient fill + gold frame + hand
        // cursors) shares the system-UI atlas slot; a dialog box
        // and the boot/menu chrome are mutually exclusive states.
        save_chrome_draw_vec.extend(self.dialog_chrome_sprite_draws(w, h));
        // Name-entry window chrome (grid + name-field filigree
        // windows + hand cursor) shares the same atlas slot.
        save_chrome_draw_vec.extend(self.name_entry_chrome_sprite_draws(w, h));
        // Battle HUD chrome (party-strip + plaque lozenges and the
        // gold HP / green MP label cells) comes out of the same
        // atlas; battle and the boot/menu chrome never coexist. Drawn
        // first of the three battle surfaces so the prompt box and the
        // command chips below layer over the readout, not under it.
        save_chrome_draw_vec.extend(self.battle_chrome_sprite_draws(w, h));
        // Sparring-tutorial prompt box: the same window skin, framed at
        // the rect the retail emitter registers the prompt with. In
        // battle, so it cannot coexist with the boot/menu chrome above.
        save_chrome_draw_vec.extend(self.battle_tutorial_chrome_sprite_draws(w, h));
        // Arts command-input chrome (direction chips + D-pad, the
        // pennant input bar, the AP plate). Also system-UI-atlas
        // sampled, and mutually exclusive with every state above -
        // it only draws inside a battle. Coexists with the prompt box
        // above: retail shows the drill's instruction window over the
        // chips it is describing.
        save_chrome_draw_vec.extend(self.arts_input_chrome_sprite_draws(w, h));
        // Opening-cutscene "It was the Seru." caption: the opdeene baked TIM
        // (`World::cutscene.caption`) blitted centered and faded
        // (`cutscene_caption_alpha`) over the gap between the two narration
        // crawls. One textured quad sampling the caption atlas - the
        // background palette entry is transparent, so only the white text
        // draws over the scene; alpha 0 emits nothing. Placed through the
        // stage transform the rest of the stage-space overlay uses (retail
        // centers it horizontally, mid-screen ~y110 over the villager
        // tableau): scaling by `h / 240` in window pixels drew it larger
        // than the stage and off its centre whenever the window is not a
        // stage multiple, where the page's overlay canvas is the stage.
        let caption_draw_vec: Vec<legaia_engine_render::SpriteDraw> = {
            let alpha = self.session.host.world.cutscene.caption_alpha;
            match self.caption_atlas.as_ref() {
                Some((_, cw, ch)) if alpha > 0.001 => {
                    let (origin, s) = self.save_select_stage(w, h);
                    let s = s.max(1);
                    let dw = *cw * s;
                    let dh = *ch * s;
                    let dx = origin.0 + (320 * s as i32 - dw as i32) / 2;
                    let dy = origin.1 + 110 * s as i32 - dh as i32 / 2;
                    vec![legaia_engine_render::SpriteDraw {
                        dst: (dx, dy, dw, dh),
                        src: (0, 0, *cw, *ch),
                        color: [1.0, 1.0, 1.0, alpha.clamp(0.0, 1.0)],
                    }]
                }
                _ => Vec::new(),
            }
        };
        OverlayDraws {
            hud,
            logo_draw_vec,
            title_draw_vec,
            menu_glyph_draw_vec,
            muscle_hub_draw_vec,
            muscle_hub_blend,
            save_chrome_draw_vec,
            save_chrome_blend,
            caption_draw_vec,
        }
    }

    /// The frame's clear colour.
    pub(super) fn frame_clear_color(&self, game_over_hold: bool) -> Option<[f32; 4]> {
        // The clear colour is the shared engine-ui selector on every
        // frame, the one the browser play page reads too: retail black for
        // the boot UI, field / cutscene frames and stage battles alike
        // (a roofless stage shell shows black above it, as retail's
        // does). Passing it only for the boot UI and a
        // stage battle left every other frame on the renderer's own
        // fallback navy, a colour neither retail nor the page draws.
        let stage_battle =
            self.session.host.world.mode == SceneMode::Battle && self.battle_stage_mesh.is_some();
        Some(legaia_engine_screens::field_frame::frame_clear_color(
            &self.session.host.world,
            self.boot_ui.is_active() && !game_over_hold,
            &self.menu_runtime,
            stage_battle,
        ))
    }

    /// Build and upload the frame's effect meshes: the effect-pool
    /// billboards, the world map's slot-4 wireframe, the `etmd` effect
    /// models, the summon / move-FX scene-graph parts, the field move-VM
    /// parts and the move-FX afterimage streak.
    pub(super) fn build_fx_frame(
        &self,
        r: &legaia_engine_render::Renderer,
        cam: Mat4,
        in_world_map: bool,
        cutscene_cam: Option<CutsceneCam>,
    ) -> FxFrame {
        // FX model matrices pair with the active render frame:
        // battle cameras carry no world negation (keep the
        // per-model Y-flip); the field cameras compose
        // FIELD_WORLD_FLIP (draw raw PSX Y-down vertices).
        let fx_in_battle = self.session.host.world.mode == SceneMode::Battle;
        let fx_model_flip = if fx_in_battle {
            Mat4::from_scale(Vec3::new(1.0, -1.0, 1.0))
        } else {
            Mat4::IDENTITY
        };
        // Battle FX ride the actor camera composition (the retail
        // 4x world-scale base under the shared rotation) so
        // effects land on the scaled actor stage; field FX use
        // the field camera as-is.
        // The uniform scale `fx_cam` composes on top of `cam`. Effect
        // billboards need it separately from the matrix: retail forms a
        // sprite quad's corners in VIEW space, after the camera matrix has
        // scaled the centre (`FUN_800195a8` - the `MVMVA` transforms the
        // centre, the corner adds follow it, and the matrix is reset to
        // identity before the projection), so the half-extents must not go
        // through the 4x a second time. See
        // `legaia_engine_render::effect_billboard`.
        let fx_scale = if fx_in_battle && self.battle_stage_mesh.is_some() {
            BATTLE_WORLD_SCALE
        } else {
            1.0
        };
        let fx_cam = if fx_scale != 1.0 {
            cam * Mat4::from_scale(Vec3::splat(fx_scale))
        } else {
            cam
        };
        // Effect-pool billboards: bridge live effect child sprites
        // into the renderer as faithful camera-facing quads sized
        // and UV-addressed from the effect bundle's inline atlas
        // (`World::active_effect_sprites`). Each draws two ways: a
        // textured quad sampling the scene VRAM at the sprite's
        // atlas page/clut/uv (the retail FUN_801E0088 pass-2 path -
        // in battle the flame atlas + CLUT rows are resident via the
        // battle-entry blit, see `effect_billboard_mesh`), plus a
        // tinted outline through the Lines pipeline so the spawn
        // reads even where a sprite samples unloaded texels. See
        // docs/subsystems/effect-vm.md. The billboards ride `fx_cam`
        // like every other battle FX layer: in a stage-dome battle
        // the pool positions are actor-stage coordinates, so drawing
        // them under the unscaled `cam` landed each quad 4x too
        // small at the wrong stage position. The camera-facing basis
        // derives from the same matrix, so the quads face the camera
        // that actually draws them.
        let (effect_billboard, effect_lines) = self.build_effect_billboards(r, fx_cam, fx_scale);
        self.diag_effect_billboards(fx_cam);
        let effect_billboard = if std::env::var_os("LEGAIA_DIAG_NOFX").is_some() {
            None
        } else {
            effect_billboard
        };
        // World-map overlay lines: only the env-gated slot-4 inspection
        // wireframe now. The entity and player markers draw as screen
        // primitives through the kernel the browser play page shares
        // (`world_map_marker_prims`, appended to `screen_prims` below).
        let world_map_slot4_lines = self.build_world_map_overlay_lines(r, in_world_map);
        // Effect 3D models (`etmd.dat`): spell effects like Tail
        // Fire are small Gouraud-shaded `etmd` meshes textured by
        // the resident `etim` texels, not billboards. Build a
        // per-frame VRAM mesh + transform for each live effect that
        // has a model assigned (same per-frame model-matrix
        // convention as `actor_model`). Held in a local Vec so the
        // meshes outlive the render borrow.
        let effect_model_draws = self.build_effect_model_draws(r, fx_model_flip, in_world_map);

        // Active Seru-magic summon scene-graph (debug-spawned via
        // `G`): one textured mesh per move-VM-driven part, posed by
        // the part's interpreted transform (world pos + rotation
        // banks). The animation computation is faithful (move VM);
        // the transform composition is the open PROT 0900 piece.
        // The retail camera the parts' `+0x52` camera-relative bits
        // resolve against (`FUN_8001CF50`); `None` under a host vantage.
        let part_cam = self.part_camera_pose(in_world_map, cutscene_cam);
        let summon_part_draws = self.build_summon_and_move_fx_part_draws(
            r,
            fx_model_flip,
            in_world_map,
            part_cam.as_ref(),
        );
        // Field move-VM effect parts (op 0x34 sub-3 stagers): resolve
        // each mesh part against the SCENE's TMD pack - `env_tmds` =
        // `res.tmds` filtered to the scene_asset_table bundle entry,
        // the same source the field-placement renderer + the
        // asset-viewer use - NOT the battle `global_tmd_pool`. Retail
        // resolves a field stager's mesh as `DAT_8007C018[model_sel +
        // DAT_8007B6F8]`, where `DAT_8007B6F8 = 5` is the character-mesh
        // prefix and `DAT_8007C018[5..]` is exactly this scene pack; so
        // the part's relative `model_sel` (spawn base 0, surfaced as
        // `model_index`) indexes `env_tmds` directly, mirroring how a
        // placement's `pack_index` does.
        let field_fx_draws =
            self.build_field_fx_part_draws(r, fx_model_flip, in_world_map, part_cam.as_ref());
        // The move-FX afterimage streak, under an orthographic
        // screen-space MVP (PSX 320x240 frame). The PROT-0900
        // screen-effect widgets (mask / sprite / panel / letterbox) are
        // screen primitives instead - see `screen_layers::screen_fx_prims`.
        let screen_fx_tex = self.build_screen_fx_meshes(r);
        let screen_fx_mvp = Mat4::orthographic_rh(0.0, 320.0, 240.0, 0.0, 0.0, 1.0);
        FxFrame {
            fx_cam,
            effect_billboard,
            effect_lines,
            world_map_slot4_lines,
            effect_model_draws,
            summon_part_draws,
            field_fx_draws,
            screen_fx_tex,
            screen_fx_mvp,
        }
    }

    /// The screen-space primitives composited over the scene: `.0` over the
    /// overlay text, `.1` under it (the field's ordering-table effects).
    #[allow(clippy::too_many_arguments)]
    pub(super) fn build_screen_prims(
        &self,
        battle_intro_prims: Vec<legaia_engine_render::screen_overlay::ScreenPrim>,
        field_fog_prims: Vec<legaia_engine_render::screen_overlay::ScreenPrim>,
        move_strip_prims: Vec<legaia_engine_render::screen_overlay::ScreenPrim>,
        slot_prims: Vec<legaia_engine_render::screen_overlay::ScreenPrim>,
        fishing_line_prims: Vec<legaia_engine_render::screen_overlay::ScreenPrim>,
        fx_cam: Mat4,
    ) -> (
        Vec<legaia_engine_render::screen_overlay::ScreenPrim>,
        Vec<legaia_engine_render::screen_overlay::ScreenPrim>,
    ) {
        // The intro's primitives composite *over* the scene in one frame.
        // `RenderTarget::ScreenOverlay` cannot do it: that is a whole-frame
        // mode which clears and draws nothing but quads, so it could never
        // carry a transition strip over a field scene.
        //
        // The target is built *before* the screenshot harness so a capture
        // sees the frame that is presented. Capturing `Scene(&scene)` here
        // instead would silently drop every transition style from the PNGs
        // - the harness's own blind spot, not the emitter's.
        //
        // The ORDER of the two lists - which layer appends where, and so
        // how every ordering-table tie breaks - is the shared
        // `screen_layers::compose_screen_prims` the browser play page
        // composes with; this host contributes only what needs its camera
        // or its VRAM residency.
        let host = legaia_engine_screens::screen_layers::HostScreenPrims {
            transition: battle_intro_prims,
            battle_fx: self.weapon_trail_screen_prims(),
            field_fog: field_fog_prims,
            // The actor drop shadows (`FUN_8001C394`), through the same
            // `World::field_drop_shadows` kernel as the page.
            drop_shadows: self.field_drop_shadow_prims(),
            move_strips: move_strip_prims,
            field_lights: self.field_light_screen_prims(),
            theeder: self.theeder_screen_prims(),
            // The floating value readout needs the struck actor's projected
            // screen position, which only a host holding the camera has.
            value_readout: self.battle_value_readout_prims(fx_cam),
            dance_countin: self.dance_countin_prims(),
            dance_hud: self.dance_hud_prims(),
            baka_hud: self.baka_hud_prims(),
            slot_cabinet: slot_prims,
            slot_paylines: self.slot_payline_screen_prims(),
            fishing_line: fishing_line_prims,
            fishing_hud: Vec::new(),
            world_map_markers: self.world_map_marker_prims(),
            world_map_sky: self.world_map_sky_prims(),
        };
        let (under, over) = legaia_engine_screens::screen_layers::compose_screen_prims(
            &self.session.host.world,
            &self.menu_runtime,
            self.session.pause_wipe().fade_level(),
            host,
        );
        (over, under)
    }
}
