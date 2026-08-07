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
//! # The one thing this host cannot supply
//!
//! Retail's kernel moves the readout to its low row when the player projects
//! above stage `y = 0x30`, and it takes that number from a GTE projection of
//! the player position. The play page's camera lives in the page's own
//! WebGL code, not in this crate, so there is no view-projection here to
//! transform against.
//!
//! `None` is **not** the right stand-in: in the kernel `None` is retail's
//! staged-load arm, which forces the low row - so passing it would park the
//! browser's readout across the bottom of every frame while the native
//! window keeps it at the top. This host declares the common case instead
//! (the player is below the dodge line), which is what the projection
//! answers on all but a handful of framings.

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
            || w.current_dialog.is_some()
            || w.inline_dialogue.is_some()
            || w.text_balloon.is_some()
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
        // See the module docstring: this host has no view-projection, and the
        // kernel's `None` means "staged load pending", not "unknown".
        let projected_y = Some(ui::field_party_hud::NO_PROJECTION_STAND_IN);
        self.field_party_hud
            .tick(suppressed, view_mode, pad, player_pos, 1, projected_y);
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
