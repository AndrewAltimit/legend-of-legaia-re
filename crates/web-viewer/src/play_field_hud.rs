//! Browser **field party-status HUD** - the name / `LV` / `HP` / `MP` readout
//! retail keeps in the top-left of every walkable frame.
//!
//! Pure wiring, like the rest of the play page's overlays: the decision is
//! [`legaia_engine_vm::world_map_panel_actors::field_hud_tick`] driven through
//! [`legaia_engine_core::world_map_panel_host::FieldPartyHud`], the rows come
//! from `field_party_hud_members`, and the geometry is
//! [`legaia_engine_ui::field_party_hud::field_party_hud_draws_for`] - the same
//! three pieces the native window calls, so the two hosts cannot draw
//! different HUDs.
//!
//! # Where the dodge line's input comes from
//!
//! Retail's kernel moves the readout to its low row when the player projects
//! above stage `y = 0x30`, and it takes that number from a GTE projection of
//! the player position. This host has a view-projection to transform against -
//! [`LegaiaRuntime::play_camera_vp`] builds one off the engine's own camera
//! frame, and [`Self::tick_passive_hud`] below uses it - but the readout's
//! number arrives the other way round: the page projects the lead each frame
//! and pushes the stage `y` in through
//! [`LegaiaRuntime::set_field_player_screen_y`], so the draw pass never has to
//! resolve one.
//!
//! Until the page has reported one, `None` is **not** the right stand-in: in
//! the kernel `None` is retail's staged-load arm, which forces the low row - so
//! passing it would park the browser's readout across the bottom of every frame
//! while the native window keeps it at the top. The tick declares the common
//! case instead (the player is below the dodge line), which is what the
//! projection answers on all but a handful of framings.

use crate::runtime::LegaiaRuntime;
use legaia_engine_core::world::SceneMode;
use legaia_engine_ui::{self as ui, SpriteDraw, TextDraw};

impl LegaiaRuntime {
    /// Retail's `_DAT_8007B868` suppress gate, as this host can see it: only
    /// free-roam on the field or the overworld shows a readout, and any
    /// panel, box or fight that owns the screen hides it.
    fn field_party_hud_suppressed(&self) -> bool {
        let Some(h) = self.scene_host.as_ref() else {
            return true;
        };
        let w = &h.world;
        !matches!(w.mode, SceneMode::Field | SceneMode::WorldMap)
            || self.menu.is_open()
            || w.dialog.current.is_some()
            || w.dialog.inline.is_some()
            || w.cutscene.text_balloon.is_some()
            || w.cutscene_timeline_active()
    }

    /// Advance the HUD's idle countdown one frame.
    pub(crate) fn tick_field_party_hud(&mut self) {
        let scene = self
            .scene_host
            .as_ref()
            .and_then(|h| h.scene.as_ref().map(|s| s.name.clone()));
        if scene != self.field_party_hud_scene {
            self.field_party_hud_scene = scene;
            self.field_party_hud.rearm();
        }
        let suppressed = self.field_party_hud_suppressed();
        let (view_mode, pad, player_pos) = match self.scene_host.as_ref() {
            Some(h) => {
                let w = &h.world;
                (
                    i32::from(w.mode == SceneMode::WorldMap),
                    legaia_engine_core::world_map_panel_host::packed_pad(w.input.pad()),
                    w.player_actor_slot
                        .map(usize::from)
                        .and_then(|s| w.actors.get(s))
                        .map(|a| (a.move_state.world_x, a.move_state.world_z)),
                )
            }
            None => (0, 0, None),
        };
        // The page reports the lead's projected stage-Y off its own
        // view-projection each frame (`set_field_player_screen_y`); until it
        // has, the stand-in keeps the kernel's `None` meaning "staged load
        // pending" rather than "unknown".
        let projected_y = if suppressed {
            None
        } else {
            Some(
                self.field_hud_projected_y
                    .unwrap_or(ui::field_party_hud::NO_PROJECTION_STAND_IN),
            )
        };
        self.field_party_hud
            .tick(suppressed, view_mode, pad, player_pos, 1, projected_y);
    }

    /// Resolve this frame's **passive-ability badge column** into
    /// [`LegaiaRuntime::passive_hud_icons`] - the field overlay's
    /// `FUN_801d095c`, the icons floated over the player's head while an
    /// accessory passive is active.
    ///
    /// Split the way the native window's `passive_hud_draws` is: `World`
    /// answers the three head-relative world points and the icon list, and
    /// this host projects between them. Unlike the party readout above, the
    /// projection **is** available here - `LegaiaRuntime::play_camera_vp` is
    /// this frame's view-projection off the engine's own camera frame - it
    /// just needs a `&mut self`, which the draw pass does not have, so the
    /// answer is resolved on the tick and cached.
    ///
    /// Retail takes the anchor's X from the **first** projected point and its
    /// Y from the **third**; a single point's pair is not the same thing.
    pub(crate) fn tick_passive_hud(&mut self) {
        self.passive_hud_icons.clear();
        if self.field_party_hud_suppressed() {
            return;
        }
        let Some(points) = self
            .scene_host
            .as_ref()
            .map(|h| &h.world)
            .filter(|w| w.passive_hud_active())
            .and_then(|w| w.passive_hud_points())
        else {
            return;
        };
        // The retail stage, so the projected pair lands in the same
        // 320x240 space the icon seats are authored in.
        let vp = self.play_camera_vp(320.0, 240.0);
        if vp.len() != 16 {
            return;
        }
        let project = |p: [f32; 3]| -> Option<(i32, i32)> {
            let v = [p[0], p[1], p[2], 1.0];
            let mut clip = [0.0f32; 4];
            for (i, c) in clip.iter_mut().enumerate() {
                *c = (0..4).map(|j| vp[j * 4 + i] * v[j]).sum();
            }
            if clip[3] <= 0.01 {
                return None;
            }
            Some((
                ((clip[0] / clip[3] * 0.5 + 0.5) * 320.0) as i32,
                ((0.5 - clip[1] / clip[3] * 0.5) * 240.0) as i32,
            ))
        };
        let (Some(first), Some(third)) = (project(points[0]), project(points[2])) else {
            return;
        };
        let Some(world) = self.scene_host.as_ref().map(|h| &h.world) else {
            return;
        };
        self.passive_hud_icons = world.passive_hud_icons((first.0, third.1));
    }

    /// The cached passive-ability badges as surface-pixel text draws.
    ///
    /// The icon ids `0x47..=0x4D` name cells of the field pictogram bank
    /// (`FUN_8002C488`), which this host has no atlas for, so each draws as
    /// the same stand-in glyph the native window uses.
    pub(crate) fn passive_hud_draws(&self, surface_w: u32, surface_h: u32) -> Vec<TextDraw> {
        if self.passive_hud_icons.is_empty() {
            return Vec::new();
        }
        let Some(assets) = self.menu_assets.as_ref() else {
            return Vec::new();
        };
        let font = assets.font_ref();
        let (origin, scale) = crate::play_menu::stage_transform(surface_w.max(1), surface_h.max(1));
        let mut out = Vec::new();
        for i in &self.passive_hud_icons {
            let mut draws =
                ui::text_draws_for(&font.layout_ascii("*"), (i.x, i.y), ui::MENU_TEXT_GOLD);
            ui::scale_stage_text_draws(&mut draws, origin, scale);
            out.extend(draws);
        }
        out
    }

    /// This frame's HUD, split the way the page's overlay JSON wants it:
    /// atlas sprites (the translucent plate, then the `LV`/`HP`/`MP` label
    /// cells, the `/` and the numerals) and font-atlas text (the names),
    /// both already in surface pixels.
    pub(crate) fn field_party_hud_draws(
        &self,
        surface_w: u32,
        surface_h: u32,
    ) -> (Vec<SpriteDraw>, Vec<TextDraw>) {
        use ui::field_party_hud as fp;
        let empty = (Vec::new(), Vec::new());
        let Some(legaia_engine_vm::world_map_panel_actors::HudDecision::Draw { y }) =
            self.field_party_hud.decision()
        else {
            return empty;
        };
        let Some(assets) = self.menu_assets.as_ref() else {
            return empty;
        };
        let Some(host) = self.scene_host.as_ref() else {
            return empty;
        };
        let rows = legaia_engine_core::world_map_panel_host::field_party_hud_members(&host.world);
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
        let font = assets.font_ref();
        let (origin, scale) = crate::play_menu::stage_transform(surface_w.max(1), surface_h.max(1));
        let draws = fp::field_party_hud_draws_for(
            font,
            &fp::FieldPartyHudFrame {
                members: &members,
                y: i32::from(y),
                chrome: assets.chrome_rects(),
                scrim_src: assets.chrome_solid_texel(),
                solid_src: ui::font_solid_src(font),
                origin,
                scale: scale as i32,
            },
        );
        (draws.sprites, draws.text)
    }
}
