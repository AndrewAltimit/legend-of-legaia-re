//! Browser **field party-status HUD** - the name / `LV` / `HP` / `MP` readout
//! retail keeps in the top-left of every walkable frame.
//!
//! Pure wiring, like the rest of the play page's overlays: the decision is
//! [`legaia_engine_field::world_map_panel_actors::field_hud_tick`] driven through
//! [`legaia_engine_field::world_map_panel_host::FieldPartyHud`], the rows come
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
//!
//! Once the page **has** reported, a lead that does not project (no live
//! player actor, or behind the camera) reaches the kernel as `None`, exactly
//! as the native window's `field_hud_projected_player_y` hands it over. The
//! two cases used to share the stand-in, so on those framings the page kept
//! the readout on the top row while the window dropped it to the low one.

use crate::runtime::LegaiaRuntime;
use legaia_engine_ui::{self as ui, SpriteDraw, TextDraw};
use legaia_engine_world_views::passive_hud::WorldPassiveHud;

impl LegaiaRuntime {
    /// Retail's `_DAT_8007B868` suppress gate, asked of the shared kernel
    /// [`legaia_engine_world_views::world_map_panel_host::field_hud_suppressed`].
    /// This host answers only the term the world cannot: whether a page-side
    /// panel owns the frame. The enumeration used to be spelled out here and
    /// again in the native window, and the two copies had drifted.
    pub(crate) fn field_party_hud_suppressed(&self) -> bool {
        let Some(h) = self.scene_host.host() else {
            return true;
        };
        // A movie owning the screen is the native window's `cutscene` term:
        // the readout must not sit over an in-game FMV.
        legaia_engine_world_views::world_map_panel_host::field_hud_suppressed(
            &h.world,
            self.menu.is_open() || self.fmv.is_open(),
        )
    }

    /// Advance the HUD's idle countdown one frame.
    ///
    /// Reached only from `tick_frame`'s session arm, so a scene host is
    /// always up; with none there is no field to read a HUD over.
    pub(crate) fn tick_field_party_hud(&mut self) {
        // Retail's scene-change and player-engaged rearm terms, the step the
        // native window runs too.
        let Some(h) = self.scene_host.host() else {
            return;
        };
        legaia_engine_screens::field_screens::field_party_hud_rearm(
            &mut self.field_party_hud,
            &mut self.field_party_hud_scene,
            h,
        );
        let suppressed = self.field_party_hud_suppressed();
        // The page reports the lead's projected stage-Y off its own
        // view-projection each frame (`set_field_player_screen_y`); until it
        // has, the stand-in keeps the kernel's `None` meaning "staged load
        // pending" rather than "unknown". A reported "not projectable" is
        // handed over as `None`, as the native window hands it.
        let projected_y = if suppressed {
            None
        } else if self.field_hud_projection_reported {
            self.field_hud_projected_y
        } else {
            Some(ui::field_party_hud::NO_PROJECTION_STAND_IN)
        };
        if let Some(h) = self.scene_host.host() {
            legaia_engine_screens::field_screens::field_party_hud_tick(
                &mut self.field_party_hud,
                &h.world,
                suppressed,
                projected_y,
            );
        }
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
        // Build the view-projection only when there is a column to seat:
        // `play_camera_vp` also caches the frame it resolves.
        let has_column = self.scene_host.host().is_some_and(|h| {
            h.world.passive_hud_active() && h.world.passive_hud_points().is_some()
        });
        if !has_column {
            return;
        }
        // The retail stage, so the projected pair lands in the same
        // 320x240 space the icon seats are authored in.
        let vp = self.play_camera_vp(320.0, 240.0);
        let Ok(vp) = <[f32; 16]>::try_from(vp.as_slice()) else {
            return;
        };
        let Some(world) = self.scene_host.host().map(|h| &h.world) else {
            return;
        };
        self.passive_hud_icons =
            legaia_engine_screens::field_screens::passive_hud_icons(world, &vp);
    }

    /// The cached passive-ability badges as surface-pixel text draws.
    ///
    /// The icon ids `0x47..=0x4D` name cells of the field pictogram bank
    /// (`FUN_8002C488`), which this host has no atlas for, so each draws as
    /// the same stand-in glyph the native window uses.
    pub(crate) fn passive_hud_draws(&self, surface_w: u32, surface_h: u32) -> Vec<TextDraw> {
        // Same reason as the readout above: the cached icon list is cleared
        // on the tick, and the page's frame loop skips the tick on the very
        // frames a panel owns the screen.
        if self.field_party_hud_suppressed() {
            return Vec::new();
        }
        if self.passive_hud_icons.is_empty() {
            return Vec::new();
        }
        let Some(assets) = self.menu_assets.as_ref() else {
            return Vec::new();
        };
        let font = assets.font_ref();
        let (origin, scale) = crate::play_menu::stage_transform(surface_w.max(1), surface_h.max(1));
        legaia_engine_screens::field_screens::passive_hud_text_draws(
            font,
            &self.passive_hud_icons,
            origin,
            scale,
        )
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
        let empty = (Vec::new(), Vec::new());
        // Ask the suppress gate on the DRAW path too. The page's frame loop
        // (`site/js/play-app.js::_frame`) skips `tick_frame` entirely while a
        // field shop, the naming prompt or the pause menu is up, so the tick
        // that would have suppressed this readout does not run on those
        // frames - and the shop arm goes on to call
        // `play_overlay_draws_json`, which is where this builder sits. The
        // decision alone is therefore only as fresh as the last ticked frame.
        if self.field_party_hud_suppressed() {
            return empty;
        }
        let Some(legaia_engine_field::world_map_panel_actors::HudDecision::Draw { y }) =
            self.field_party_hud.decision()
        else {
            return empty;
        };
        let Some(assets) = self.menu_assets.as_ref() else {
            return empty;
        };
        let Some(host) = self.scene_host.host() else {
            return empty;
        };
        let font = assets.font_ref();
        let draws = legaia_engine_screens::field_screens::field_party_hud_draws(
            &host.world,
            font,
            legaia_engine_screens::field_screens::FieldHudSources {
                chrome: assets.chrome_rects(),
                scrim_src: assets.chrome_solid_texel(),
                solid_src: ui::font_solid_src(font),
            },
            y,
            surface_w.max(1),
            surface_h.max(1),
        );
        (draws.sprites, draws.text)
    }
}
