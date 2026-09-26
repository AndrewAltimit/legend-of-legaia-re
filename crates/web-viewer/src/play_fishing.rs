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
//!    ([`legaia_engine_core::input::PadButton`]) the page already routes through
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
//! # The gauge fills: one geometry, two carriers
//!
//! With a blind atlas and no solid texel, `fishing_hud_draws_for` drops every
//! glyph and every bar fill. The gauges are the functional part of the tension
//! tug-of-war, so both play hosts fill them from the same resolved frames
//! ([`legaia_engine_ui::HudDraw::resolve_bar`], the `FUN_801d1870` /
//! `FUN_801d1a90` geometry): the native window hands the shared consumer its
//! font's solid texel as `solid_src`, this host emits the frames as a third
//! `bars` channel for the page to fill as rects. Text comes from the shared
//! consumer on both.

use crate::runtime::LegaiaRuntime;
use legaia_engine_core::fishing::{PondEvent, PondPhase, PondSession, PrizeExchange, TENSION_MAX};
use legaia_engine_ui::{
    self as ui, BarAxis, CatchHudState, FishingCaptions, FishingHudAtlas, HudDraw, TextDraw,
};
use wasm_bindgen::prelude::*;

/// Stage-pixel pen for the phase / prompt status line, matching the native
/// window's fishing line at `(8, 62)`.
const STATUS_PEN: (i32, i32) = (8, 62);
/// Second status row (the native window's `(8, 80)` hint line).
const HINT_PEN: (i32, i32) = (8, 80);

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
    /// draws the HUD will emit. The browser twin of the native window's
    /// `tick_fishing_banners`: the session's events this tick
    /// (`World::minigames.fishing_events`) seed the timers, and each timer
    /// retires itself.
    ///
    /// Called from `tick_frame`, i.e. on the sim clock, so a page rendering
    /// below 60 Hz does not slow the banner animations down.
    pub(crate) fn tick_fishing_banners(&mut self) {
        let Some(world) = self
            .scene_host
            .as_ref()
            .map(|h| &h.world)
            .filter(|w| w.minigames.fishing.is_some())
        else {
            self.fishing_banners = Default::default();
            self.fishing_banner_draws.clear();
            return;
        };
        for e in &world.minigames.fishing_events {
            match e {
                PondEvent::Splash => self.fishing_banners.splash.start(),
                PondEvent::Hooked(_) => self.fishing_banners.on_hook(),
                PondEvent::Landed(_) => self.fishing_banners.on_landed(),
                PondEvent::Snapped => self.fishing_banners.on_snapped(),
                PondEvent::Recast => self.fishing_banners.on_recast(),
            }
        }
        self.fishing_banner_draws = self.fishing_banners.service_frame(1);
    }

    /// The fishing venue's actor-side frame - the browser twin of the native
    /// window's `tick_fishing_actors`, through the same engine kernel
    /// ([`legaia_engine_core::fishing_venue::tick_fishing_venue_on_host`]):
    /// the wander fish and its retarget ripple, the floor solve, the reeling
    /// line and its catch bursts, the sub-screen sway, and the venue camera
    /// writes, applied here to this page's engine camera. The ripples and
    /// bursts land in `World::minigames.fx`, which this page already draws.
    pub(crate) fn tick_fishing_actors(&mut self) {
        let Some(host) = self.scene_host.as_mut() else {
            return;
        };
        let writes = legaia_engine_core::fishing_venue::tick_fishing_venue_on_host(host);
        writes.apply(&mut self.camera);
    }

    /// The live fishing session, when one is installed on the scene host's
    /// world.
    fn fishing_session(&self) -> Option<&PondSession> {
        self.scene_host.as_ref()?.world.minigames.fishing.as_ref()
    }

    /// The phase / prompt status rows the native window prints above the retail
    /// HUD, so a player can tell which phase the session is in before the
    /// sprite page exists. The text is the engine's (`PondSession::status_rows`);
    /// the key names are this page's default bindings for Circle / Cross /
    /// Square.
    fn fishing_status_draws(&self, font: &legaia_font::Font) -> Vec<TextDraw> {
        let Some(s) = self.fishing_session() else {
            return Vec::new();
        };
        let white = [1.0, 1.0, 1.0, 1.0];
        let dim = [0.65, 0.72, 0.8, 1.0];
        let (line, hint) = s.status_rows("X", "Z", "V");
        let mut out = ui::text_draws_for(&font.layout_ascii(&line), STATUS_PEN, white);
        out.extend(ui::text_draws_for(&font.layout_ascii(&hint), HINT_PEN, dim));
        out
    }

    /// This frame's retail HUD draw list: the persistent rows
    /// (`FUN_801d13f0`), the catch HUD once a cast is out (`FUN_801d1580`, its
    /// gauge block only while the fish is on) and the live banner one-shots.
    fn fishing_hud_items(&self) -> Vec<HudDraw> {
        let Some(s) = self.fishing_session() else {
            return Vec::new();
        };
        let inventory = self
            .scene_host
            .as_ref()
            .map(|h| &h.world.party.inventory)
            .expect("fishing_session() proved the host exists");
        // The lure index is the session's; the entry's ownership gate already
        // re-pointed it at an owned lure, exactly as the native window reads it.
        let lure = s.lure;
        let lures_left = *inventory
            .get(&(legaia_engine_core::fishing::lure_item_id(lure) as u8))
            .unwrap_or(&0) as i32;
        let mut items =
            ui::persistent_hud_draws(s.record.points, s.record.best_points, lure, lures_left);
        // One derivation for the catch HUD on every host
        // (`PondSession::catch_hud`). The cast line-projection term
        // `DAT_801d9178` has no engine analogue and stays zero.
        let c = s.catch_hud();
        if c.visible {
            items.extend(ui::catch_hud_draws(&CatchHudState {
                record: c.record,
                line_extent: 0,
                cast_power: c.cast_power,
                depth: c.depth,
                tension: c.tension,
                gauges_visible: c.gauges_visible,
            }));
        }
        items.extend(self.fishing_banner_draws.iter().copied());
        items
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
        use legaia_asset::{fishing_species, static_overlay};
        let Some(host) = self.scene_host.as_mut() else {
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
        // The two point-exchange venue pages ride the same overlay image; row
        // labels resolve through the SCUS item table the page already parsed.
        let names = self.item_names.as_ref();
        self.fishing_venues = legaia_asset::fishing_exchange::parse(&loaded).map(|ex| {
            [0usize, 1].map(|venue| PrizeExchange::from_asset(venue, &ex.venues[venue], names))
        });
        self.fishing_banners = Default::default();
        self.fishing_banner_draws.clear();
        true
    }

    /// Leave the fishing session and restore the suspended scene mode, banking
    /// the session's persistent words into the world. Returns the banked
    /// point total (`-1` when no session was live).
    pub fn play_fishing_stop(&mut self) -> i32 {
        let Some(host) = self.scene_host.as_mut() else {
            return -1;
        };
        if host.world.exit_fishing().is_none() {
            return -1;
        }
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
    ///   "line": false, "fx_parts": 0 }
    /// ```
    ///
    /// `wander` / `line` are the venue actors the shared step
    /// ([`Self::tick_fishing_actors`]) runs, and `fx_parts` the live parts in
    /// the world's effect pool their ripples and bursts land in.
    pub fn play_fishing_state_json(&self) -> String {
        let Some(s) = self.fishing_session() else {
            return r#"{"live":false}"#.to_string();
        };
        let (wander, line, fx_parts) =
            self.scene_host
                .as_ref()
                .map(|h| {
                    let v = &h.world.minigames.fishing_venue;
                    let wander = v.wander.as_ref().map(
                        |w| serde_json::json!({"x": w.x, "y": w.y, "z": w.z, "facing": w.facing}),
                    );
                    (wander, v.line.is_some(), h.world.minigames.fx.len())
                })
                .unwrap_or((None, false, 0));
        serde_json::json!({
            "live": true,
            "phase": pond_phase_name(s.phase()),
            "cast_power": s.cast_power(),
            "cast_max": legaia_engine_core::fishing::CAST_POWER_MAX,
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
        })
        .to_string()
    }

    /// This frame's fishing HUD as page quads, in the same
    /// `{ open, sprites, texts, bars }` shape the other overlay payloads use.
    ///
    /// `texts` and `sprites` come from
    /// [`legaia_engine_ui::fishing_hud_draws_for`] - the shared consumer the
    /// native window calls, with the same blind [`FishingHudAtlas`] (the
    /// fishing sprite page is undecoded, so glyph ids resolve to nothing and
    /// only the digit / caption rows survive). `bars` carries the resolved
    /// gauge frames the blind atlas cannot fill; see the module note.
    pub fn play_fishing_hud_json(&mut self, surface_w: u32, surface_h: u32) -> String {
        if !self.play_fishing_active() {
            return CLOSED.to_string();
        }
        if !self.ensure_menu_assets() {
            return CLOSED.to_string();
        }
        let items = self.fishing_hud_items();
        let bars = bar_json(&items);
        let Some(assets) = self.menu_assets.as_ref() else {
            return CLOSED.to_string();
        };
        let font = assets.font_ref();
        // The retail persistent + catch rows through the shared consumer. No
        // fishing sprite page is uploaded on either host, so the glyph ids
        // resolve to nothing on both. The fills differ in carrier only: the
        // native window stretches its font's solid texel through
        // `solid_src`, this page leaves it `None` and fills the same resolved
        // frames from `bars` in JS.
        let atlas = FishingHudAtlas {
            solid_src: None,
            glyph_src: &|_| None,
            bar_thickness: 8,
        };
        let mut texts = ui::fishing_hud_draws_for(
            font,
            &items,
            &FishingCaptions::placeholder(),
            &atlas,
            (0, 0),
        );
        texts.extend(self.fishing_status_draws(font));
        // The venue's point-exchange sub-screen, when one is open. This page
        // used to answer the rows as a JSON side-channel only
        // (`play_fishing_prizes_json`), so the screen the native window
        // draws over the pond existed on one host and a data feed on the
        // other. Both now compose it through
        // `legaia_engine_ui::ui_fishing_exchange`.
        texts.extend(self.fishing_exchange_draws(font));
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
        let Some(venues) = self.fishing_venues.as_ref() else {
            return "null".to_string();
        };
        let Some(world) = self.scene_host.as_ref().map(|h| &h.world) else {
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

    /// The open point-exchange sub-screen's draws, through the shared
    /// composition.
    ///
    /// The panel anchor is the same one the native window resolves - retail's
    /// menu-picker rect (`FUN_801d74b0`) - offset by the venue's idle sway
    /// (`FUN_801d03b0`), which the shared venue step
    /// ([`Self::tick_fishing_actors`]) advances on the world for both hosts.
    fn fishing_exchange_draws(&self, font: &legaia_font::Font) -> Vec<TextDraw> {
        use legaia_engine_ui::ui_fishing_exchange as fx;
        let Some(world) = self.scene_host.as_ref().map(|h| &h.world) else {
            return Vec::new();
        };
        let Some(ex) = world.minigames.fishing_exchange.as_ref() else {
            return Vec::new();
        };
        let names: Vec<String> = ex
            .rows
            .iter()
            .map(|r| {
                r.name
                    .clone()
                    .unwrap_or_else(|| format!("item {:#04x}", r.item_id))
            })
            .collect();
        let rows: Vec<fx::ExchangeRowView<'_>> = ex
            .rows
            .iter()
            .enumerate()
            .map(|(i, r)| {
                let owned = *world.party.inventory.get(&r.item_id).unwrap_or(&0) as u32;
                fx::ExchangeRowView {
                    name: names[i].as_str(),
                    price: r.price,
                    owned,
                    available: ex.is_available(
                        i,
                        world.minigames.fishing_points,
                        owned,
                        world.minigames.fishing_prizes_purchased,
                    ),
                    one_time: r.is_one_time(),
                    latched: ex.is_latched(i, world.minigames.fishing_prizes_purchased),
                }
            })
            .collect();
        let view = fx::ExchangeView {
            venue: ex.venue as u8,
            points: world.minigames.fishing_points,
            cursor: ex.cursor,
            first_visible: ex.first_visible(world.minigames.fishing_points),
            rows: &rows,
        };
        let sway = world.minigames.fishing_venue.sway_offset;
        let pen = legaia_engine_core::fishing_chrome::centred_panel(0xA0, 0x50, 0x68, 0x50)
            .map(|p| (p.x as i32 + sway.0 as i32, p.y as i32 + sway.1 as i32))
            .unwrap_or((8, 98));
        fx::exchange_screen_draws_for(
            font,
            &view,
            "   (Enter = trade, Left/Right = venue, P = close)",
            pen,
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
        let Some(venues) = self.fishing_venues.as_ref() else {
            return -1;
        };
        let Some(host) = self.scene_host.as_mut() else {
            return -1;
        };
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
            .as_ref()
            .and_then(|h| h.world.minigames.fishing_exchange.as_ref());
        match ex {
            Some(e) => serde_json::json!({ "open": true, "venue": e.venue, "cursor": e.cursor }),
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
        let Some(venues) = self.fishing_venues.as_ref() else {
            return -1;
        };
        let Some(host) = self.scene_host.as_mut() else {
            return -1;
        };
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
