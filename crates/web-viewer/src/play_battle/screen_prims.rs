//! Screen-space PSX primitives: the field-to-battle intro emitter, the frame
//! grab it captures, and the prim buffers the page uploads.

use super::*;

// ---------------------------------------------------------------------------
// Screen-space PSX primitives (the field-to-battle transition)
// ---------------------------------------------------------------------------

impl LegaiaRuntime {
    /// Arm, advance or drop the field-to-battle intro emitter so it tracks
    /// the encounter session's `Transition` phase, and cache this frame's
    /// ordered geometry for the page's screen-prim pass.
    ///
    /// The browser twin of the native window's `take_battle_intro_frame`.
    /// The *simulation* half was always shared - `World::tick_encounter` runs
    /// `tick_transition` on both hosts, so the clock, the BGM swap and the
    /// battle that opens never differed - and the *emitter* is shared now
    /// too ([`legaia_render_kernels::battle_intro`]): all five style bodies (the
    /// confetti, the shattering tiles, the curtain strips, the swirl fan)
    /// plus the backdrop and the fade come out of the same `BattleIntro` the
    /// native window ticks. What stays per-host is the readback that lands
    /// the captured field frame the styles sample: the native window
    /// re-renders offscreen, this page hands its own drawn frame to
    /// [`Self::play_intro_land_capture`] via `gl.readPixels`.
    ///
    /// Called once per simulation tick - the emitter advances working sets,
    /// so the per-frame accessors below must read a cache rather than
    /// re-tick.
    pub(crate) fn tick_battle_intro(&mut self) {
        // Each layer this page projects itself, in a struct whose ORDER is
        // not decided here: `screen_layers::compose_screen_prims` (the native
        // window's composer too) appends them, plus every layer the world
        // decides alone - the fades, the pushes, the cinematic bars, the
        // PROT-0900 widgets, the Arts banner, the Cross Beam, the shop's
        // opening fade and the pause wipe.
        // The capture frame carries no transition layer. The page reads the
        // emitter's one-shot capture back off the frame it draws, after every
        // other layer (`play-view-frame.js`), so that frame has to be the
        // field as the player sees it - the fog-pool particles, the drop
        // shadows, the light pools, the volumetric fog, the enhanced
        // lighting - and not the transition's own backdrop or strips, which
        // sample the capture rather than belong to it. The native window
        // composes its capture target the same way (`render_scene_frame`).
        let transition = self.tick_battle_intro_prims().unwrap_or_default();
        let transition = if self.play_intro_wants_capture() {
            Vec::new()
        } else {
            transition
        };
        let host = legaia_engine_screens::screen_layers::HostScreenPrims {
            transition,
            // The weapon-trail bands and the move-FX afterimage streak.
            battle_fx: self.battle_fx_screen_prims(),
            field_fog: self.tick_field_fog_prims(),
            drop_shadows: self.field_drop_shadow_prims(),
            move_strips: self.tick_move_strip_prims(),
            field_lights: self.field_light_prims(),
            theeder: self.theeder_prims(),
            value_readout: self.battle_value_readout_prims(),
            dance_countin: self
                .scene_host
                .host()
                .map(|h| {
                    legaia_engine_screens::minigame_hud::dance_countin_prims(&h.world.minigames)
                })
                .unwrap_or_default(),
            dance_hud: self
                .scene_host
                .host()
                .map(|h| legaia_engine_screens::minigame_hud::dance_hud_prims(&h.world.minigames))
                .unwrap_or_default(),
            baka_hud: self.baka_hud_prims(),
            slot_cabinet: self.slot_cabinet_prims(),
            // The page strokes the paylines in JS, over the canvas.
            slot_paylines: Vec::new(),
            fishing_line: self.fishing_line_prims(),
            fishing_hud: self.fishing_hud_sprite_prims(),
            world_map_markers: self.world_map_marker_prims(),
            world_map_sky: self.world_map_sky_prims(),
        };
        self.screen_prims_base = host;
        self.rebuild_screen_geom();
    }

    /// Compose this frame's screen primitives into the cached geometry: the
    /// tick's host layers ([`Self::tick_battle_intro`]) through the shared
    /// `screen_layers::compose_screen_prims`, with the world's live layers
    /// and the shop's opening fade / the pause wipe read now.
    ///
    /// Split out because the fades step with the shop and the pause menu,
    /// not with the field: the page skips `tick_frame` while a shop is up, so
    /// a fade built only on the tick was never drawn at all - the shop's
    /// first frames sat on the frozen field and then cut to black. The shop
    /// and menu steps call this after every tick they run.
    ///
    /// The two lists stay two: each goes through its own ordering-table walk
    /// and `over`'s runs follow `under`'s in one buffer, with the boundary
    /// handed to the page ([`Self::play_screen_prim_over_run_start`]). The
    /// page draws `under`, then its text layer, then `over` - the native
    /// window's present order, so a fade that covers the text covers it on
    /// both hosts.
    pub(crate) fn rebuild_screen_geom(&mut self) {
        use legaia_render_kernels::screen_prim::{
            DrawRun, PSX_DISPLAY_H, PSX_DISPLAY_W, build_geometry, split_backdrop,
        };
        let pause_wipe = self
            .scene_host
            .session()
            .and_then(|s| s.pause_wipe().fade_level());
        let (under, over) = match self.scene_host.host() {
            Some(h) => legaia_engine_screens::screen_layers::compose_screen_prims(
                &h.world,
                &self.menu,
                pause_wipe,
                self.screen_prims_base.clone(),
            ),
            None => (Vec::new(), Vec::new()),
        };
        let count = under.len() + over.len();
        if count == 0 {
            self.battle_intro_geom = None;
            self.screen_over_run_start = 0;
            self.screen_backdrop_run_end = 0;
            return;
        }
        let (w, h) = (PSX_DISPLAY_W as u32, PSX_DISPLAY_H as u32);
        // `under`'s backdrops lead the buffer as their own list: the page
        // draws them before the 3D scene, as the native renderer does.
        let (backdrop, under) = split_backdrop(&under);
        let mut geom = build_geometry(&backdrop, w, h);
        self.screen_backdrop_run_end = geom.runs.len() as u32;
        let append =
            |geom: &mut legaia_render_kernels::screen_prim::OverlayGeometry,
             list: &[legaia_render_kernels::screen_prim::ScreenPrim]| {
                if list.is_empty() {
                    return;
                }
                let top = build_geometry(list, w, h);
                let (vertex_base, index_base) =
                    (geom.vertices.len() as u32, geom.indices.len() as u32);
                geom.vertices.extend(top.vertices);
                geom.indices
                    .extend(top.indices.into_iter().map(|i| i + vertex_base));
                geom.runs.extend(top.runs.into_iter().map(|r| DrawRun {
                    index_start: r.index_start + index_base,
                    ..r
                }));
            };
        append(&mut geom, &under);
        self.screen_over_run_start = geom.runs.len() as u32;
        append(&mut geom, &over);
        self.battle_intro_geom = Some((count as u32, geom));
    }

    /// The intro emitter's half of [`Self::tick_battle_intro`]: arm /
    /// advance / drop the emitter, return this frame's primitives (`None`
    /// outside a transition or before the entity's first tick).
    fn tick_battle_intro_prims(
        &mut self,
    ) -> Option<Vec<legaia_render_kernels::screen_prim::ScreenPrim>> {
        use legaia_engine_battle::encounter::EncounterPhase;

        let Some(host) = self.scene_host.host() else {
            self.drop_battle_intro();
            return None;
        };
        let phase = host.world.encounters.session.as_ref().map(|s| s.phase());
        let Some(EncounterPhase::Transition { roll, .. }) = phase else {
            self.drop_battle_intro();
            return None;
        };
        let Some(entity) = host.world.battle.intro else {
            // The transition is armed but its entity has not ticked yet:
            // keep the emitter (the native window does the same), just show
            // nothing this frame.
            return None;
        };
        let total = host
            .world
            .encounters
            .session
            .as_ref()
            .map(|s| i32::from(s.transition_frames))
            .unwrap_or(0);
        if self.battle_intro.is_none() {
            self.battle_intro = Some(self.arm_battle_intro(roll.formation_id, total));
        }
        let mut intro = self.battle_intro.take().expect("armed above");
        // Stepped to the entity's clock, one step per clock unit - the same
        // call the native window makes, whose redraws do not run one per
        // world tick (`BattleIntro::advance_to`).
        let frame = intro.advance_to(entity.elapsed);
        // The curtain's CPU two-pass composition (and nothing else, for the
        // other styles) changes the captured page per frame; the page
        // re-uploads through the same dirty flag the field CLUT effects use.
        if intro.refresh_captured_page().is_some() {
            self.field_vram_dirty = true;
        }
        self.battle_intro = Some(intro);
        Some(frame.prims)
    }

    /// This frame's in-battle screen FX primitives - the browser seat of
    /// the two effects the native window composites over its battle scene:
    ///
    /// * the **weapon-trail** bands (`World::battle_weapon_trail_draws` -
    ///   retail `FUN_8005112C` / `FUN_80048310`; packets from the shared
    ///   `legaia_render_kernels::battle_trail::weapon_trail_prims` port of
    ///   `FUN_800485BC`), projected with the page's own `battle_vp` in its
    ///   scaled placement space (`(pos + local) * BATTLE_WORLD_SCALE`, the
    ///   same composition the actor draws use - world Y goes through
    ///   as-is, the VP's trailing flip handles it);
    /// * the **move-FX afterimage streak** (the shared
    ///   `legaia_render_kernels::streak_pass` schedule of `FUN_801E09F8`, quad
    ///   packets from the ported `FUN_801E1AB0` / `FUN_801E1D98`) - the
    ///   layer this page previously did not draw at all.
    // REF: FUN_800485BC / FUN_801E1AB0 - per-host projection seat; the
    // packets and schedules live in engine-ui.
    fn theeder_prims(&self) -> Vec<legaia_render_kernels::screen_prim::ScreenPrim> {
        let Some(world) = self.scene_host.host().map(|h| &h.world) else {
            return Vec::new();
        };
        let Some(packet) = world.theeder_draw() else {
            return Vec::new();
        };
        // The page's placement law (as for the weapon trail): retail points
        // scaled by BATTLE_WORLD_SCALE with Y as-is - the battle VP's
        // trailing flip carries the Y-down axis.
        let scale = crate::play_battle_render::BATTLE_WORLD_SCALE;
        let pose = self.battle_cam_pose();
        let vp = legaia_engine_camera::projection::battle_vp(&pose, scale, 4.0 / 3.0);
        legaia_engine_ui::cast_theeder::theeder_prims(&packet, world.theeder_trail(), |p| {
            legaia_render_kernels::battle_trail::project_stage_point_cols(
                &vp,
                [
                    f32::from(p[0]) * scale,
                    f32::from(p[1]) * scale,
                    f32::from(p[2]) * scale,
                ],
            )
        })
    }

    fn battle_fx_screen_prims(&self) -> Vec<legaia_render_kernels::screen_prim::ScreenPrim> {
        use legaia_render_kernels::battle_trail as bt;
        use legaia_render_kernels::battle_trail::TRAIL_POINTS;
        use legaia_render_kernels::streak_pass::{
            MOVE_FX_STREAK_OT, StreakSource, clip_ribbon_quads, streak_quads_scheduled,
        };

        let Some(host) = self.scene_host.host() else {
            return Vec::new();
        };
        let world = &host.world;
        if world.mode != legaia_engine_mode::mode::SceneMode::Battle {
            return Vec::new();
        }
        let scale = crate::play_battle_render::BATTLE_WORLD_SCALE;
        let pose = self.battle_cam_pose();
        let vp = legaia_engine_camera::projection::battle_vp(&pose, scale, 4.0 / 3.0);
        let mut out = Vec::new();
        for d in world.battle_weapon_trail_draws() {
            let Some(actor) = world.actors.get(d.actor_slot as usize) else {
                continue;
            };
            let pos = [
                f32::from(actor.move_state.world_x),
                f32::from(actor.move_state.world_y),
                f32::from(actor.move_state.world_z),
            ];
            let mut steps = Vec::with_capacity(d.steps.len());
            'draw: for s in &d.steps {
                let mut pts = [(0i16, 0i16); TRAIL_POINTS];
                for (i, t) in s.iter().enumerate() {
                    // Page placement law (see the actor draw loop in
                    // play-app.js): posed local point + raw translation,
                    // both scaled by BATTLE_WORLD_SCALE; no Y negation -
                    // the battle VP's trailing flip carries it.
                    let p = [
                        (pos[0] + f32::from(t[0])) * scale,
                        (pos[1] + f32::from(t[1])) * scale,
                        (pos[2] + f32::from(t[2])) * scale,
                    ];
                    match bt::project_stage_point_cols(&vp, p) {
                        Some(xy) => pts[i] = xy,
                        None => break 'draw,
                    }
                }
                steps.push(pts);
            }
            out.extend(bt::weapon_trail_prims(&steps, d.rgb, bt::WEAPON_TRAIL_OT));
        }
        // Move-FX streak: same source block + emitter schedule as the
        // native window's `move_fx_streak_quads`.
        if let Some(trail) = world.active_move_fx_trail_texpage() {
            let block = world.move_fx_streak();
            if let Some(src) =
                StreakSource::from_block(block.launch, block.half_width(), Some(trail))
            {
                let mvp = glam::Mat4::from_cols_array(&vp)
                    * glam::Mat4::from_scale(glam::Vec3::splat(scale));
                // `ctx[+0x13] < 3` is a pool-slot test (`0x801E0C78`).
                let party = world
                    .battle_seat_map()
                    .is_party(world.battle_ctx.active_actor);
                let frame = world.clock.display_frames as u32;
                for q in streak_quads_scheduled(&src, &mvp, frame, block.counter_word, party) {
                    out.push(q.to_screen_prim(MOVE_FX_STREAK_OT));
                }
            }
        }
        // Clip-tag ribbon (`FUN_8004CE2C` tag `0x67` ->
        // `FUN_801E1D98(&target[+0x3C], 0xC)`): same kernel as the native
        // window's `move_fx_streak_quads`, through this page's scaled MVP.
        if let Some(c) = world.battle.clip_ribbon {
            let mvp =
                glam::Mat4::from_cols_array(&vp) * glam::Mat4::from_scale(glam::Vec3::splat(scale));
            let frame = world.clock.display_frames as u32;
            for q in clip_ribbon_quads(c.seat.map(f32::from), c.trail_id, &mvp, frame) {
                out.push(q.to_screen_prim(MOVE_FX_STREAK_OT));
            }
        }
        out
    }

    /// Tear the emitter down when the transition ends (battle handoff, or a
    /// cancelled session). The captured VRAM clone drops with it, so
    /// [`Self::field_vram_bytes`] snaps back to the pristine scene page - the
    /// dirty flag makes the page re-upload it (the battle branch uploads its
    /// own VRAM on entry either way).
    fn drop_battle_intro(&mut self) {
        if self.battle_intro.take().is_some() {
            self.field_vram_dirty = true;
        }
        self.battle_intro_geom = None;
    }

    /// Build the emitter for the battle about to open - the mirror of the
    /// native window's `arm_battle_intro`, resolving the same three selector
    /// inputs: the formation's **first monster id** (not the row index -
    /// reading the row is what made every id-keyed override unreachable), the
    /// row's own per-battle flags byte (`DAT_8007BD60` bit `0x80`, the only
    /// bit the selector reads), and the scene's PROT base (`DAT_80084540`).
    fn arm_battle_intro(&self, formation_id: u16, total: i32) -> BattleIntro {
        let host = self.scene_host.host().expect("caller checked");
        // One engine-side resolver for all three inputs
        // (`SceneHost::battle_intro_style_inputs`) and one arming
        // (`BattleIntro::arm_for_battle`, the native window's call too): the
        // PROT 0979 tables with their disc-free fallbacks, the shade pack,
        // the env seeds.
        let inputs = host.battle_intro_style_inputs(formation_id);
        let overlay = host
            .index
            .entry_bytes_extended(legaia_render_kernels::battle_intro::INTRO_OVERLAY_PROT)
            .ok();
        let shade = host
            .index
            .entry_bytes(legaia_actor_formats::field_char_textures::PROT_ENTRY_INDEX)
            .ok();
        BattleIntro::arm_for_battle(
            &inputs,
            total,
            overlay.as_deref(),
            shade.as_deref().map(Vec::as_slice),
            host.world.rng_state,
        )
        // `gl.readPixels` hands rows bottom-up; the shared blit flips them.
        .with_flipped_capture()
    }
}

#[wasm_bindgen]
impl LegaiaRuntime {
    /// Whether the armed transition still needs its one-shot field-frame
    /// capture. The page answers by reading back its own finished frame
    /// (`gl.readPixels` after the 3D pass, the volumetric fog and both
    /// screen-prim lists - a frame [`Self::tick_battle_intro`] composed
    /// without the transition layer) and handing it to
    /// [`Self::play_intro_land_capture`].
    pub fn play_intro_wants_capture(&self) -> bool {
        self.battle_intro
            .as_ref()
            .map(|i| i.needs_capture())
            .unwrap_or(false)
    }

    /// Whether a field-VM `43 12` copy is waiting on the drawn frame
    /// (`World::framebuffer_grab_pending` - the ending vignettes' photo
    /// grab). The page answers like the transition capture: read back its
    /// finished frame (after the volumetric fog and both screen-prim lists)
    /// and hand it to [`Self::play_land_frame_grab`].
    pub fn play_frame_grab_pending(&self) -> bool {
        self.scene_host
            .host()
            .is_some_and(|h| h.world.framebuffer_grab_pending())
    }

    /// Land the drawn frame (RGBA readback, bottom-up rows, WebGL order) in
    /// the scene VRAM's display page and release the held copy, which the
    /// next field-VRAM pass runs - the native window's twin of the same
    /// handshake (`legaia_render_kernels::vram_capture::land_display_frame`).
    pub fn play_land_frame_grab(&mut self, rgba: &[u8], width: u32, height: u32) {
        let row = width as usize * 4;
        if row == 0 || rgba.len() < row * height as usize {
            return;
        }
        let flipped: Vec<u8> = rgba[..row * height as usize]
            .chunks_exact(row)
            .rev()
            .flatten()
            .copied()
            .collect();
        let Some(host) = self.scene_host.host_mut() else {
            return;
        };
        let Some(res) = host.resources.as_mut() else {
            return;
        };
        legaia_render_kernels::vram_capture::land_display_frame(
            &flipped,
            width,
            height,
            &mut res.vram,
        );
        host.world.land_framebuffer();
        self.field_vram_dirty = true;
    }

    /// Land the field-frame capture: blit the RGBA readback (bottom-up rows,
    /// WebGL order) into the texture-page rects the armed style samples,
    /// inside a clone of the scene VRAM. From this call on
    /// [`Self::field_vram_bytes`] returns the captured page - the page
    /// should re-upload it immediately so the first styled frame samples the
    /// field, not stale texels.
    pub fn play_intro_land_capture(&mut self, rgba: &[u8], width: u32, height: u32) {
        let Some(mut intro) = self.battle_intro.take() else {
            return;
        };
        if let Some(r) = self.res() {
            intro.land_capture_rgba(rgba, width, height, &r.vram);
        }
        // Run the landing frame's compose immediately (the curtain's
        // intermediate; a no-op for the other styles): this frame's strips
        // were staged by the tick that ran before the capture existed, and
        // they sample the intermediate the moment the page re-uploads.
        let _ = intro.refresh_captured_page();
        self.battle_intro = Some(intro);
    }

    /// Which CLUT animator the scene rebuild installed: `0` none, `1` the
    /// slot-5 / type-6 **walker**, `2` the legacy **ocean-head** fallback
    /// (`ClutWalkAnim::Ocean`). The two are exclusive - the animator holds one
    /// `ClutWalkAnim` - so there is no combined value.
    ///
    /// A diagnostic, and specifically a *negative* one: the ocean fallback is
    /// the arm for a kingdom bundle whose slot-5 walker table does not parse,
    /// and every retail kingdom ships one - so a ladder that enters a kingdom
    /// scene and reads `1` here is measuring that no shipped content reaches
    /// the ocean fallback at all.
    pub fn play_field_anim_kind(&self) -> u32 {
        self.field_vram_anim
            .as_ref()
            .map(|a| a.kind_code())
            .unwrap_or(0)
    }

    /// How many field screen-effect washes the world published this frame
    /// (`World::screen_tint_push_args`), i.e. how many of this frame's
    /// screen primitives are op-`0x34`-sub-0 colour-tween pushes.
    ///
    /// The page-side twin of `play_fog_stats`: a diagnostic read of the
    /// producer, so a ladder can tell "the pass carries N prims" apart from
    /// "the pass carries the wash".
    pub fn play_screen_effect_push_count(&self) -> u32 {
        self.scene_host
            .host()
            .map(|h| h.world.screen_tint_push_args().len() as u32)
            .unwrap_or(0)
    }

    /// This frame's screen-effect pushes that wash over the **text layer**
    /// (`legaia_render_kernels::screen_prim::text_layer_washes`), as
    /// `[[abr, r, g, b], ...]` in draw order; `[]` when none does.
    ///
    /// Retail links every glyph at OT bucket `1` and a push at bucket `0`
    /// draws over it - the opening's `34 05` push to black dims the crawl
    /// with the scene. The GL pass draws every push over the scene, but the
    /// page's text is a 2D canvas above the GL canvas, so the page applies
    /// these to that canvas's own pixels (`wash_channel`'s equations) - the
    /// native window draws the same pushes as primitives over its text.
    pub fn play_text_layer_washes_json(&self) -> String {
        let washes = self
            .scene_host
            .host()
            .map(|h| {
                let mut w = legaia_render_kernels::screen_prim::text_layer_washes(
                    &h.world.screen_tint_push_args(),
                );
                // The world's one live fade washes the text too when its
                // ordering-table id is in front of the text's bucket (a
                // field warp's id `0`); the summon band's flashes (id `1`)
                // stay under it. The native window makes the same split.
                for (rgb, abr, ot) in h.world.screen_fade_draws() {
                    if legaia_render_kernels::screen_prim::push_covers_text(
                        i16::try_from(ot).unwrap_or(i16::MAX),
                    ) {
                        w.push((abr, [(rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8]));
                    }
                }
                w
            })
            .unwrap_or_default();
        serde_json::to_string(
            &washes
                .iter()
                .map(|(abr, [r, g, b])| [*abr, *r, *g, *b])
                .collect::<Vec<_>>(),
        )
        .unwrap_or_else(|_| "[]".to_string())
    }

    /// How many screen-space PSX primitives this frame carries. `0` is the
    /// page's early-out (every frame outside a transition).
    pub fn play_screen_prim_count(&self) -> u32 {
        self.battle_intro_geom
            .as_ref()
            .map(|(n, _)| *n)
            .unwrap_or(0)
    }

    /// The screen-prim vertex stream as raw bytes, in the shared
    /// `ScreenVertex` layout: stride 48, `pos: vec2<f32>` (already NDC) at 0,
    /// `uv: vec2<f32>` at 8, `cba_tsb: vec2<u32>` at 16, `color: vec4<f32>` at
    /// 24, `flags: u32` at 40 (bit 0 = textured, bit 1 = depth-tested),
    /// `depth: f32` at 44. The same bytes the native
    /// renderer maps into its wgpu vertex buffer.
    ///
    /// The page never sees the primitive list, only its output: the
    /// ordering-table walk (farthest bucket first, LIFO within a bucket) is
    /// baked into the index buffer by the **shared** builder before the data
    /// crosses the WASM boundary, so there is no second place for a host to
    /// order these and no way for the two hosts to disagree about it.
    pub fn play_screen_prim_vertex_bytes(&self) -> Vec<u8> {
        self.battle_intro_geom
            .as_ref()
            .map(|(_, g)| g.vertex_bytes())
            .unwrap_or_default()
    }

    /// The screen-prim triangle index buffer, already in ordering-table draw
    /// order.
    pub fn play_screen_prim_indices(&self) -> Vec<u32> {
        self.battle_intro_geom
            .as_ref()
            .map(|(_, g)| g.indices.clone())
            .unwrap_or_default()
    }

    /// How many of [`Self::play_screen_prim_runs`]' runs - the first ones -
    /// are the **backdrops** (`screen_prim::split_backdrop`): primitives at
    /// the far plane, retail's farthest ordering-table bucket, which the page
    /// draws after its clear and before the 3D scene so a blended scene
    /// primitive lands over them. Runs `[n, over_run_start)` are the rest of
    /// the under list.
    pub fn play_screen_prim_backdrop_run_end(&self) -> u32 {
        self.screen_backdrop_run_end
    }

    /// How many of [`Self::play_screen_prim_runs`]' runs belong to the
    /// **under** list: the page draws its backdrops
    /// ([`Self::play_screen_prim_backdrop_run_end`]) before the 3D scene, the
    /// rest of runs `[0, n)` after it, its text layer next, and runs
    /// `[n, ..)` - the `over` list - last. The
    /// split is `screen_layers::compose_screen_prims`' own, which follows
    /// retail's text bucket (`screen_prim::TEXT_OT_BUCKET`).
    pub fn play_screen_prim_over_run_start(&self) -> u32 {
        self.screen_over_run_start
    }

    /// The screen-prim run table, flattened to
    /// `[class_code, index_start, index_count]` triples. `class_code` is
    /// `legaia_render_kernels::screen_prim::BlendClass::code`: `0` = opaque,
    /// `1 + abr_mode` = semi-transparent - so `0` stays reserved for "no
    /// blending" and an ABR-0 (`0.5B + 0.5F`) run cannot be read as one.
    pub fn play_screen_prim_runs(&self) -> Vec<u32> {
        self.battle_intro_geom
            .as_ref()
            .map(|(_, g)| g.run_words())
            .unwrap_or_default()
    }
}
