//! The field party HUD: when it is suppressed, where the lead projects on
//! screen, its idle-countdown tick and its draws.

use super::*;

impl PlayWindowApp {
    /// Is the field party HUD allowed to be on screen at all this frame?
    ///
    /// Retail's `_DAT_8007B868` suppress global, asked of the shared kernel
    /// [`legaia_engine_core::world_map_panel_host::field_hud_suppressed`].
    /// This host answers only the term the world cannot: a window-side panel
    /// with no `World` state behind it. The enumeration used to live here in
    /// full and a second copy lived on the browser play page, and the copies
    /// had drifted - the page's lacked all three of the host terms below, and
    /// this one gated the badge column (`FUN_801d095c`) differently again.
    pub(in crate::window) fn field_party_hud_suppressed(&self) -> bool {
        legaia_engine_core::world_map_panel_host::field_hud_suppressed(
            &self.session.host.world,
            self.boot_ui.is_active()
                || self.menu_runtime.is_open()
                || self.cutscene.is_some()
                || self.active_dialog.is_some(),
        )
    }

    /// Where the player projects on the 240-line stage this frame.
    ///
    /// Retail hands the projection the player object's position with `0x80`
    /// subtracted from Y (`addiu v0,v0,-0x80` at `0x801D0F8C`) - the torso,
    /// not the feet - and the HUD drops to its low row when the result sits
    /// above stage `y = 0x30`. `None` is retail's own staged-load path, which
    /// also forces the low row.
    pub(super) fn field_hud_projected_player_y(&self) -> Option<i16> {
        let (sw, sh) = self.win.surface_size();
        if sh == 0 {
            return None;
        }
        let world = &self.session.host.world;
        let slot = world.player_actor_slot.map(usize::from).unwrap_or(0);
        let p = world
            .actors
            .get(slot)
            .filter(|a| a.active || a.tmd_binding.is_some())?;
        // The projection the 3D pass draws with, into the stage rect whose
        // NDC `y` is exactly the 240-line stage row.
        let (_, aspect) = scene_viewport_for(sw, sh);
        let in_world_map = world.mode == SceneMode::WorldMap;
        let cam = self.compute_scene_camera(aspect, in_world_map, None);
        let v = cam
            * glam::Vec4::new(
                p.move_state.world_x as f32,
                p.move_state.world_y as f32 - 128.0,
                p.move_state.world_z as f32,
                1.0,
            );
        if v.w <= 0.0 {
            return None;
        }
        // wgpu NDC has +Y up; the stage is 240 lines with +Y down.
        let stage_y: f32 = (1.0 - v.y / v.w) * 120.0;
        Some(stage_y.clamp(-4096.0, 4096.0) as i16)
    }

    /// Advance the field party HUD's countdown one frame.
    ///
    /// Runs in the mutation phase beside the other per-frame syncs; the draw
    /// pass reads the decision back off the driver.
    pub(in crate::window) fn tick_field_party_hud(&mut self) {
        // A scene change is retail's rearm condition (its own arm includes
        // "the staged scene load is still pending"), and without it the
        // stationary compare would run against the previous scene's
        // coordinates and pop the HUD up mid-transition.
        let scene = self.session.host.scene.as_ref().map(|s| s.name.clone());
        if scene != self.field_party_hud_scene {
            self.field_party_hud_scene = scene;
            self.field_party_hud.rearm();
        }
        // Retail's player-engaged rearm term: a script or conversation that
        // holds the player restarts the idle countdown every frame - read
        // a frame late, the order retail's actor lists run in
        // (`FieldPartyHud::rearm_term`).
        let held = legaia_engine_core::world_map_panel_host::field_hud_rearm_held(
            &self.session.host.world,
        );
        if self.field_party_hud.rearm_term(held) {
            self.field_party_hud.rearm();
        }
        // A capture taken where a gate is met (a script or battle phase)
        // has no tick to phase-align to: `capture_tick` is only its
        // deadline. A retail countdown of `0` there is a drawn readout, so
        // the countdown is pinned at `0` instead of held to the deadline.
        let gated_drawn = self.screenshot.as_ref().is_some_and(|sc| {
            (sc.script_gate.is_some() || sc.phase_gate.is_some()) && sc.hud_countdown == Some(0)
        });
        // Capture harness: phase-align the countdown to the retail state
        // being compared (`LEGAIA_HUD_COUNTDOWN`).
        if let Some(sc) = self.screenshot.as_ref()
            && let Some(n) = sc.hud_countdown
            && !gated_drawn
        {
            let mode = legaia_engine_core::world_map_panel_host::field_hud_view_mode(
                &self.session.host.world,
            );
            let idle = legaia_engine_field::world_map_panel_actors::hud_idle_frames(mode, false);
            if legaia_engine_field::world_map_panel_host::hud_phase_hold(
                self.tick_no,
                sc.capture_tick,
                n,
                idle,
            ) {
                self.field_party_hud.rearm();
            }
        }
        let suppressed = self.field_party_hud_suppressed();
        let projected_y = if suppressed {
            None
        } else {
            self.field_hud_projected_player_y()
        };
        let world = &self.session.host.world;
        // `_DAT_800845C4` is the options screen's Field HP Display row
        // (Immediate / Gradual / Display Off), not a camera mode - the one
        // engine read both hosts share.
        let view_mode = legaia_engine_core::world_map_panel_host::field_hud_view_mode(world);
        let player_pos = world
            .player_actor_slot
            .map(usize::from)
            .and_then(|s| world.actors.get(s))
            .map(|a| (a.move_state.world_x, a.move_state.world_z));
        // The suppress mask is the PACKED d-pad, so the raw word has to be
        // converted or nothing ever suppresses. The word is the one the
        // world was handed this tick (`World::input`), as on the browser
        // page - not the window's held keys, which keep the HUD hidden while
        // the world itself was fed a neutral pad (a shop, the narration
        // crawl, a locked cutscene).
        let pad = legaia_engine_field::world_map_panel_host::packed_pad(world.input.pad());
        self.field_party_hud
            .tick(suppressed, view_mode, pad, player_pos, 1, projected_y);
        // The overworld's idle (`0xA0`) outlasts the run before the capture
        // tick, so the rearm hold above cannot reach a short retail
        // countdown on its own: clamp the running countdown as well.
        if let Some(sc) = self.screenshot.as_ref()
            && let Some(n) = sc.hud_countdown
            && let Some(cap) = legaia_engine_field::world_map_panel_host::hud_countdown_cap(
                self.tick_no,
                sc.capture_tick,
                n,
            )
        {
            self.field_party_hud.cap_countdown(cap);
        }
        if gated_drawn {
            self.field_party_hud.cap_countdown(0);
        }
    }

    /// The field party HUD's two draw halves for this frame, or empty when
    /// the kernel's decision is anything but `Draw`.
    pub(in crate::window) fn field_party_hud_draws(
        &self,
        w: u32,
        h: u32,
    ) -> legaia_engine_ui::BattleHudDraws {
        use legaia_engine_ui::field_party_hud as fp;
        // Ask the suppress gate on the DRAW path too, not only on the tick.
        // Retail evaluates it once because `FUN_801D0D38` is both halves in
        // one function; the port splits them, and a split decision is only
        // as fresh as the last frame on which the host reached the tick. Any
        // arm that short-circuits the frame path without short-circuiting the
        // draw then paints the kernel's last pre-suppression answer - which
        // is exactly how this readout came to sit under the pause menu.
        if self.field_party_hud_suppressed() {
            return Default::default();
        }
        let Some(legaia_engine_field::world_map_panel_actors::HudDecision::Draw { y }) =
            self.field_party_hud.decision()
        else {
            return Default::default();
        };
        let rows = legaia_engine_core::world_map_panel_host::field_party_hud_members(
            &self.session.host.world,
        );
        let members: Vec<fp::FieldHudMember<'_>> = rows
            .iter()
            .map(|m| fp::FieldHudMember {
                name: &m.name,
                level: m.level,
                hp: m.hp,
                hp_max: m.hp_max,
                mp: m.mp,
                mp_max: m.mp_max,
                alive: m.alive,
            })
            .collect();
        let (origin, scale) = self.save_select_stage(w, h);
        fp::field_party_hud_draws_for(
            &self.font,
            &fp::FieldPartyHudFrame {
                members: &members,
                y: i32::from(y),
                chrome: self.save_menu.as_ref().map(|a| &a.rects),
                scrim_src: self.save_menu.as_ref().and_then(|a| a.solid),
                solid_src: self.battle_hud_solid_src(),
                origin,
                scale: scale as i32,
            },
        )
    }
}
