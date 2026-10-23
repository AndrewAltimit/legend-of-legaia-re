//! Fishing-minigame host for the browser **play page**.
//!
//! The play page runs a live [`legaia_engine_core::world::World`]; this module
//! gives that world the three things a fishing HUD needs to read and had no
//! source for:
//!
//! 1. **A session.** [`Self::play_fishing_start`] lifts the fishing overlay
//!    (PROT 0972) through the static-overlay map and hands it to
//!    `SceneHost::enter_fishing_from_overlay` - the entry the mode-24 door warp
//!    and the native window's `L` launcher take too. That decodes the species,
//!    spawn and cadence tables, runs the bring-up's rod and lure ownership
//!    scans, seeds a [`PondSession`] from the persistent save-block words on
//!    `World::minigames`, attaches the scene's `.MAP` as the venue the lure
//!    lands in, and suspends the field scene underneath.
//! 2. **A cast / reel input path.** None is added here, and that is the point:
//!    the driver is `World::tick_fishing`, which reads the *pad*
//!    ([`legaia_engine_system::input::PadButton`]) the page already routes through
//!    `LegaiaRuntime::set_pad` every frame. Circle casts and locks the power
//!    meter, Cross reels (reel A), Square reels harder (reel B). The ported
//!    reel decoder `ReelInput::from_pad_mask` classifies the two held bits, so
//!    holding both resolves the way retail does.
//! 3. **A persistent record.** `World::minigames` keeps the save-block words
//!    (retail `_DAT_8008444C..0x8008446C`); the session seeds from them and
//!    `exit_fishing` banks every one back, so points, casts and the lure
//!    survive leaving and re-entering.
//!
//! The HUD itself is not re-implemented: [`Self::play_fishing_hud_json`] builds
//! the retail draw list from the shared builders
//! ([`legaia_engine_ui::persistent_hud_draws`],
//! [`legaia_engine_ui::catch_hud_draws`] over `PondSession::catch_hud`, the
//! [`legaia_engine_ui::FishingBanners`] one-shots) and projects it through
//! [`legaia_engine_ui::fishing_hud_draws_for`] - the same consumer the native
//! window's `window/hud.rs` calls, with the same blind sprite atlas, because
//! the fishing sprite page is the one undecoded asset in the chain.
//!
//! # The gauge fills
//!
//! With the sprite table decoded, the fills are screen primitives of the
//! shared sprite pass ([`legaia_engine_ui::fishing::sprite::fishing_hud_sprite_prims`],
//! at the `FUN_801d1870` / `FUN_801d1a90` fill rect and drawn over the frames
//! as retail's ordering table does), on this host and the native window
//! alike. Only without the table does this host emit the resolved frames as a
//! `bars` channel for the page to fill as rects.

use crate::runtime::LegaiaRuntime;
use legaia_engine_fishing::fishing::{PondPhase, PondSession, TENSION_MAX};
use legaia_engine_ui::{self as ui, BarAxis, FishingCaptions, FishingHudAtlas, HudDraw, TextDraw};
use wasm_bindgen::prelude::*;

/// The empty payload. Kept as one literal so every early return agrees.
const CLOSED: &str = r#"{"open":false,"sprites":[],"texts":[],"bars":[]}"#;

/// The page-facing name of a session phase - the same strings the minigames
/// page's `fishing_pond_state_json` uses.
pub(crate) fn pond_phase_name(p: PondPhase) -> &'static str {
    match p {
        PondPhase::Idle => "idle",
        PondPhase::WindUp => "windup",
        PondPhase::Power => "power",
        PondPhase::Flight => "flight",
        PondPhase::Waiting => "waiting",
        PondPhase::Hooked => "hooked",
        PondPhase::Landed => "landed",
        PondPhase::Snapped => "snapped",
    }
}

impl LegaiaRuntime {
    /// Service the fishing banner one-shots for this sim tick and cache the
    /// draws the HUD will emit - the shared step
    /// (`legaia_engine_screens::fishing_banner_frame`, the native window's
    /// `tick_fishing_banners` too): the session's events this tick
    /// (`World::minigames.fishing_events`) seed the timers, and each timer
    /// retires itself.
    ///
    /// Called from `tick_frame`, i.e. on the sim clock, so a page rendering
    /// below 60 Hz does not slow the banner animations down.
    pub(crate) fn tick_fishing_banners(&mut self) {
        let Some(host) = self.scene_host.host() else {
            self.fishing_banners = Default::default();
            self.fishing_banner_draws.clear();
            return;
        };
        self.fishing_banner_draws =
            legaia_engine_screens::fishing_banner_frame(&host.world, &mut self.fishing_banners);
    }

    /// The fishing venue's actor-side frame - the browser twin of the native
    /// window's `tick_fishing_actors`, through the same engine kernel
    /// ([`legaia_engine_scene_host::fishing_venue::tick_fishing_venue_on_host`]):
    /// the wander fish and its retarget ripple, the floor solve, the reeling
    /// line and its catch bursts, the sub-screen sway, and the venue camera
    /// writes, applied here to this page's engine camera. The ripples and
    /// bursts land in `World::minigames.fx`, which this page already draws.
    pub(crate) fn tick_fishing_actors(&mut self) {
        let Some((host, camera)) = self.scene_host.host_cam_mut() else {
            return;
        };
        let writes = legaia_engine_scene_host::fishing_venue::tick_fishing_venue_on_host(host);
        writes.apply(camera);
    }

    /// The fishing rod and line as screen primitives for this tick
    /// (`legaia_engine_screens::fishing_line_screen_prims`, the native
    /// window's `fishing_line_screen_prims` twin), the line projected through
    /// the follow camera the page draws the venue with, into the page's
    /// screen-prim pass.
    pub(crate) fn fishing_line_prims(
        &mut self,
    ) -> Vec<legaia_render_kernels::screen_prim::ScreenPrim> {
        let centre = self.field_camera_centre();
        let Some((host, camera)) = self.scene_host.host_cam_mut() else {
            return Vec::new();
        };
        legaia_engine_screens::fishing_line_screen_prims(&mut host.world, camera, centre)
    }

    /// The fishing HUD's sprite half as screen primitives - the native
    /// window's `fishing_hud_screen_prims` twin: every plate, digit, gauge
    /// cap and banner cut from the overlay's sprite table (`FUN_801D63B0`,
    /// the shared `fishing::sprite` builder) out of the venue's HUD page,
    /// which the pond VRAM this page takes for the session already holds.
    /// Empty without a session or a decoded table.
    pub(crate) fn fishing_hud_sprite_prims(
        &self,
    ) -> Vec<legaia_render_kernels::screen_prim::ScreenPrim> {
        let Some(host) = self.scene_host.host() else {
            return Vec::new();
        };
        if host.world.mode != legaia_engine_mode::mode::SceneMode::Fishing {
            return Vec::new();
        }
        let Some(table) = host.world.minigames.fishing_sprites.as_deref() else {
            return Vec::new();
        };
        legaia_engine_ui::fishing::sprite::fishing_hud_sprite_prims(
            &self.fishing_hud_items(),
            table,
        )
    }

    /// The live fishing session, when one is installed on the scene host's
    /// world.
    fn fishing_session(&self) -> Option<&PondSession> {
        self.scene_host.host()?.world.minigames.fishing.as_ref()
    }

    /// The venue hub's draws (menu, help pages, tackle list) - empty while no
    /// hub is up.
    fn fishing_hub_draws(&self, font: &legaia_font::Font) -> Vec<TextDraw> {
        let Some(world) = self.scene_host.host().map(|h| &h.world) else {
            return Vec::new();
        };
        let lines = world.fishing_hub_lines();
        legaia_engine_ui::fishing::hub::fishing_hub_draws_for(
            font,
            lines
                .iter()
                .map(|l| (&l.text[..], i32::from(l.x), i32::from(l.y), l.marked)),
            [1.0, 1.0, 1.0, 1.0],
        )
    }

    /// This frame's retail HUD draw list
    /// (`legaia_engine_screens::fishing_hud_items`, the list the native
    /// window builds too) with the live banner one-shots.
    fn fishing_hud_items(&self) -> Vec<HudDraw> {
        let Some(host) = self.scene_host.host() else {
            return Vec::new();
        };
        legaia_engine_screens::fishing_hud_items(&host.world, &self.fishing_banner_draws)
    }
}

/// Resolve the bar / power-bar items into the page's `bars` JSON. Geometry is
/// the ported cap/body/cap frame ([`HudDraw::resolve_bar`]); the page fills
/// `fill` pixels along `axis` from the frame's start (horizontal) or bottom
/// (vertical) cap, in `rgb`.
fn bar_json(items: &[HudDraw]) -> Vec<serde_json::Value> {
    items
        .iter()
        .filter_map(|d| d.resolve_bar())
        .filter_map(|f| {
            let (r, g, b) = f.fill_rgb?;
            let (start, span) = match f.axis {
                // Left-to-right from just past the start cap.
                BarAxis::Horizontal => (f.positions[1], (f.fill_len.max(0), 8)),
                // Upward from the bottom cap.
                BarAxis::Vertical => (
                    (f.positions[2].0, f.positions[2].1 - f.fill_len.max(0)),
                    (8, f.fill_len.max(0)),
                ),
            };
            Some(serde_json::json!({
                "axis": match f.axis { BarAxis::Horizontal => "h", BarAxis::Vertical => "v" },
                "x": start.0, "y": start.1, "w": span.0, "h": span.1,
                "rgb": [r, g, b],
            }))
        })
        .collect()
}

#[wasm_bindgen]
impl LegaiaRuntime {
    /// Start a fishing session on the live world, suspending the current scene
    /// mode. Returns `false` (and leaves the world untouched) when no disc is
    /// loaded or the fishing overlay's tables do not decode.
    ///
    /// The session seeds from the world's persistent fishing words
    /// (`World::minigames.fishing_points` and siblings), so leaving and
    /// re-entering keeps the running total and the cast counter.
    pub fn play_fishing_start(&mut self) -> bool {
        use legaia_minigame_formats::fishing_species;
        use legaia_overlay_images::static_overlay;
        let Some(host) = self.scene_host.host_mut() else {
            return false;
        };
        let Some(rec) = static_overlay::overlay_map()
            .by_prot_index(fishing_species::FISHING_OVERLAY_PROT_INDEX as u32)
        else {
            return false;
        };
        let Ok(raw) = host.index.entry_bytes_extended(rec.prot_index) else {
            return false;
        };
        let Ok(loaded) = static_overlay::as_loaded(&raw, rec) else {
            return false;
        };
        if !host.enter_fishing_from_overlay(&loaded) {
            return false;
        }
        // The two point-exchange venue pages were decoded by the shared entry
        // above (`World::minigames.fishing_prize_venues`), the one the venue
        // door reaches too.
        self.fishing_banners = Default::default();
        self.fishing_banner_draws.clear();
        true
    }

    /// Leave the fishing session and restore the suspended scene mode, banking
    /// the session's persistent words into the world. Returns the banked
    /// point total (`-1` when no session was live).
    pub fn play_fishing_stop(&mut self) -> i32 {
        let Some(host) = self.scene_host.host_mut() else {
            return -1;
        };
        if host.world.exit_fishing().is_none() {
            return -1;
        }
        // A door-entered pond closes its mode-24 round trip here, as the
        // Start escape does (a no-op for a button-launched session).
        host.world.close_minigame_round_trip();
        self.fishing_banners = Default::default();
        self.fishing_banner_draws.clear();
        host.world.minigames.fishing_points
    }

    /// Is a fishing session live on the world this frame?
    pub fn play_fishing_active(&self) -> bool {
        self.fishing_session().is_some()
    }

    /// The live session's state for the page's readout - the phase names the
    /// minigames page's `fishing_pond_state_json` uses:
    ///
    /// ```json
    /// { "live": true, "phase": "idle", "cast_power": 0, "cast_max": 0,
    ///   "tension": 0, "tension_max": 0, "record": 0, "points": 0,
    ///   "best": 0, "casts": 0, "lure": 0, "rod": 0, "venue": 0,
    ///   "wander": {"x":1024,"y":0,"z":1024,"facing":2048}|null,
    ///   "line": false, "fx_parts": 0, "ripple_parts": 0 }
    /// ```
    ///
    /// `wander` / `line` are the venue actors the shared step
    /// ([`Self::tick_fishing_actors`]) runs, `fx_parts` the live parts in
    /// the world's effect pool the catch bursts land in, and `ripple_parts`
    /// the live ripple parts the wander's retarget roll seats (the overlay's
    /// own move-VM ring records, `FishingVenue::ripple_fx`).
    pub fn play_fishing_state_json(&self) -> String {
        let Some(s) = self.fishing_session() else {
            return r#"{"live":false}"#.to_string();
        };
        let (wander, line, fx_parts, ripple_parts) =
            self.scene_host
                .host()
                .map(|h| {
                    let v = &h.world.minigames.fishing_venue;
                    let wander = v.wander.as_ref().map(
                        |w| serde_json::json!({"x": w.x, "y": w.y, "z": w.z, "facing": w.facing}),
                    );
                    let ripples = v.ripple_fx.as_ref().map_or(0, |s| s.parts.len());
                    (
                        wander,
                        v.line.is_some(),
                        h.world.minigames.fx.len(),
                        ripples,
                    )
                })
                .unwrap_or((None, false, 0, 0));
        serde_json::json!({
            "live": true,
            "phase": pond_phase_name(s.phase()),
            "cast_power": s.cast_power(),
            "cast_max": legaia_engine_fishing::fishing::CAST_POWER_MAX,
            "tension": s.tension(),
            "tension_max": TENSION_MAX,
            "record": s.line_record(),
            "points": s.record.points,
            "best": s.record.best_points,
            "casts": s.casts,
            "lure": s.lure,
            "rod": s.rod,
            "venue": s.venue,
            "wander": wander,
            "line": line,
            "fx_parts": fx_parts,
            "ripple_parts": ripple_parts,
        })
        .to_string()
    }

    /// This frame's fishing HUD as page quads, in the same
    /// `{ open, sprites, texts, bars }` shape the other overlay payloads use.
    ///
    /// `texts` and `sprites` come from
    /// [`legaia_engine_ui::fishing_hud_draws_for`] - the shared consumer the
    /// native window calls. With the sprite table decoded the plates, digits
    /// and gauge caps are screen primitives ([`Self::fishing_hud_sprite_prims`])
    /// and this carries the captions and counts. `bars` carries the resolved
    /// gauge frames only when there is no sprite pass; see the module note.
    pub fn play_fishing_hud_json(&mut self, surface_w: u32, surface_h: u32) -> String {
        if !self.play_fishing_active() {
            return CLOSED.to_string();
        }
        if !self.ensure_menu_assets() {
            return CLOSED.to_string();
        }
        let items = self.fishing_hud_items();
        let Some(assets) = self.menu_assets.as_ref() else {
            return CLOSED.to_string();
        };
        let font = assets.font_ref();
        // The retail persistent + catch rows through the shared consumer.
        // With the sprite table decoded, the plates, digits, gauge caps and
        // gauge fills are screen primitives (`fishing_hud_sprite_prims`) and
        // this keeps the captions and counts. Without it the fills fall back
        // to `bars`, filled in JS.
        let sprites_drawn = self
            .scene_host
            .host()
            .is_some_and(|h| h.world.minigames.fishing_sprites.is_some());
        // The sprite pass draws the fills itself, over the frames, so `bars`
        // is only the fallback for a page without the sprite table.
        let bars = if sprites_drawn {
            Vec::new()
        } else {
            bar_json(&items)
        };
        let atlas = FishingHudAtlas {
            solid_src: None,
            glyph_src: &|_| None,
            bar_thickness: 8,
            sprites_drawn,
        };
        // The lure row's captions off the disc, as the native window draws
        // them.
        let disc_captions = self
            .scene_host
            .host()
            .and_then(|h| h.world.minigames.fishing_captions.clone());
        let captions = match disc_captions.as_ref() {
            Some(t) => FishingCaptions::from_disc(&t.lure_names, &t.lures_left, &t.suffix)
                .with_fish_names(&t.species_names),
            None => FishingCaptions::placeholder(),
        };
        let mut texts = ui::fishing_hud_draws_for(font, &items, &captions, &atlas, (0, 0));
        // The phase line, the reel hint and the session affordances are not
        // text in this frame: they are the caption
        // (`play_mg_caption_json`, `minigame_status::fishing_status_rows`).
        // The venue's point-exchange sub-screen, when one is open. This page
        // used to answer the rows as a JSON side-channel only
        // (`play_fishing_prizes_json`), so the screen the native window
        // draws over the pond existed on one host and a data feed on the
        // other. Both now compose it through
        // `legaia_engine_ui::fishing::exchange`.
        texts.extend(self.fishing_exchange_draws(font));
        // The venue's hub menu / help pages / tackle list, laid out by the
        // engine (`World::fishing_hub_lines`) and composed by the one
        // `legaia_engine_ui::fishing::hub` draw the native window uses.
        texts.extend(self.fishing_hub_draws(font));
        let (origin, scale) = crate::play_menu::stage_transform(surface_w.max(1), surface_h.max(1));
        ui::scale_stage_text_draws(&mut texts, origin, scale);
        serde_json::json!({
            "open": true,
            "sprites": Vec::<serde_json::Value>::new(),
            "texts": texts.iter().map(crate::play_menu::quad_json).collect::<Vec<_>>(),
            "bars": bars,
            "stage": [origin.0, origin.1, scale],
        })
        .to_string()
    }

    /// The fishing point-exchange rows for `venue` (`0` Buma, `1` Vidna), with
    /// the retail availability gating applied against the live point pool and
    /// bag:
    ///
    /// ```json
    /// { "venue": 0, "points": 0, "rows": [
    ///     { "name": "...", "price": 0, "one_time": false, "available": true,
    ///       "owned": 0, "latched": false } ] }
    /// ```
    ///
    /// `null` when the venue pages did not decode.
    pub fn play_fishing_prizes_json(&self, venue: u32) -> String {
        let Some(world) = self.scene_host.host().map(|h| &h.world) else {
            return "null".to_string();
        };
        let Some(venues) = world.minigames.fishing_prize_venues.as_ref() else {
            return "null".to_string();
        };
        let ex = &venues[(venue as usize).min(1)];
        let rows: Vec<serde_json::Value> = ex
            .rows
            .iter()
            .enumerate()
            .map(|(i, r)| {
                let owned = *world.party.inventory.get(&r.item_id).unwrap_or(&0) as u32;
                serde_json::json!({
                    "name": r.name.clone().unwrap_or_else(|| format!("item {:#04x}", r.item_id)),
                    "price": r.price,
                    "one_time": r.is_one_time(),
                    "owned": owned,
                    "available": ex.is_available(
                        i,
                        world.minigames.fishing_points,
                        owned,
                        world.minigames.fishing_prizes_purchased,
                    ),
                    // The one-time latch, on its own. `available` folds the
                    // price and owned-cap refusals in with it, so this page
                    // - which exposed only `available` - could not tell a
                    // prize already taken from one the player cannot yet
                    // afford, and the JS had no way to label either.
                    "latched": ex.is_latched(i, world.minigames.fishing_prizes_purchased),
                })
            })
            .collect();
        serde_json::json!({
            "venue": ex.venue,
            "points": world.minigames.fishing_points,
            "first_visible": ex.first_visible(world.minigames.fishing_points),
            "rows": rows,
        })
        .to_string()
    }

    /// The open point-exchange sub-screen's draws, through the composition
    /// both hosts share (`legaia_engine_screens::fishing_exchange_draws`: the
    /// swaying venue panel, the rows and their live bag counts).
    fn fishing_exchange_draws(&self, font: &legaia_font::Font) -> Vec<TextDraw> {
        let Some(world) = self.scene_host.host().map(|h| &h.world) else {
            return Vec::new();
        };
        legaia_engine_screens::fishing_exchange_draws(
            world,
            font,
            // The page drives this screen from its click panel (Buy, the
            // venue tabs, Close) - it binds no key to it, so the footer
            // names the panel rather than the native window's keys.
            "   (Buy / venue tabs / Close in the panel)",
            [1.0, 1.0, 1.0, 1.0],
            [0.65, 0.72, 0.8, 1.0],
        )
    }

    /// Drive the point-exchange sub-screen through the shared engine kernel
    /// ([`World::fishing_exchange_input`]): `code` `0` toggle, `1` up, `2`
    /// down, `3` switch venue, `4` buy at the cursor. The screen stays open
    /// on the world between calls, which is what lets this page's HUD compose
    /// ([`Self::fishing_exchange_draws`]) draw it every frame the way the
    /// native window does. Returns the remaining points after a buy, `-1`
    /// for a refused input, `0` otherwise.
    ///
    /// [`World::fishing_exchange_input`]: legaia_engine_core::world::World::fishing_exchange_input
    pub fn play_fishing_exchange_input(&mut self, code: u32) -> i32 {
        use legaia_engine_core::fishing_exchange_input::{ExchangeInput, ExchangeOutcome};
        let Some(input) = ExchangeInput::from_code(code) else {
            return -1;
        };
        let Some(host) = self.scene_host.host_mut() else {
            return -1;
        };
        let Some(venues) = host.world.minigames.fishing_prize_venues.clone() else {
            return -1;
        };
        let venues = &venues;
        match host.world.fishing_exchange_input(venues, input) {
            ExchangeOutcome::Bought(_) => host.world.minigames.fishing_points,
            ExchangeOutcome::Refused => -1,
            _ => 0,
        }
    }

    /// The point-exchange sub-screen's live state as JSON - `{ "open": bool,
    /// "venue": n, "cursor": n }` - so the page's side panel can mirror what
    /// the canvas draws.
    pub fn play_fishing_exchange_state_json(&self) -> String {
        let ex = self
            .scene_host
            .host()
            .and_then(|h| h.world.minigames.fishing_exchange.as_ref());
        match ex {
            Some(e) => serde_json::json!({ "open": true, "venue": e.venue, "cursor": e.cursor }),
            None => serde_json::json!({ "open": false }),
        }
        .to_string()
    }

    /// The venue hub's live state - `{ "open": bool, "screen": "menu" |
    /// "help0" | "help1" | "tackle" | "exchange" }` - so the page can tell a
    /// player which keys the canvas is listening to.
    pub fn play_fishing_hub_state_json(&self) -> String {
        use legaia_engine_fishing::fishing_hub::HubScreen;
        let hub = self.fishing_session().and_then(|s| s.hub());
        match hub {
            Some(h) => serde_json::json!({
                "open": true,
                "screen": match h.screen {
                    HubScreen::Menu => "menu",
                    HubScreen::Help(0) => "help0",
                    HubScreen::Help(_) => "help1",
                    HubScreen::Tackle => "tackle",
                    HubScreen::Exchange => "exchange",
                },
            }),
            None => serde_json::json!({ "open": false }),
        }
        .to_string()
    }

    /// Buy prize row `row` at `venue` with the live point pool, through the
    /// same kernel: opens the sub-screen on `venue` when it is not already
    /// showing it, puts the cursor on `row`, buys, and **leaves the screen
    /// open** - it used to open, buy and close inside this one call, so the
    /// screen the HUD compose draws was never open when a frame composed.
    /// Returns the remaining points, or `-1` when the row is unavailable (too
    /// few points, a latched one-time prize, or a full stack).
    pub fn play_fishing_prize_buy(&mut self, venue: u32, row: usize) -> i32 {
        use legaia_engine_core::fishing_exchange_input::{ExchangeInput, ExchangeOutcome};
        let Some(host) = self.scene_host.host_mut() else {
            return -1;
        };
        let Some(venues) = host.world.minigames.fishing_prize_venues.clone() else {
            return -1;
        };
        let venues = &venues;
        let world = &mut host.world;
        let want = (venue as usize).min(1);
        if world.minigames.fishing_exchange.is_none() {
            world.fishing_exchange_input(venues, ExchangeInput::Toggle);
        }
        if world.minigames.fishing_exchange.as_ref().map(|e| e.venue) != Some(want) {
            world.fishing_exchange_input(venues, ExchangeInput::SwitchVenue);
        }
        if let Some(ex) = &mut world.minigames.fishing_exchange {
            ex.cursor = row.min(ex.rows.len().saturating_sub(1));
        }
        match world.fishing_exchange_input(venues, ExchangeInput::Buy) {
            ExchangeOutcome::Bought(_) => world.minigames.fishing_points,
            _ => -1,
        }
    }
}
