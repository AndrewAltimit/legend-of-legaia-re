//! Fishing-minigame host for the browser **play page**.
//!
//! The play page runs a live [`legaia_engine_core::world::World`]; this module
//! gives that world the three things a fishing HUD needs to read and had no
//! source for:
//!
//! 1. **A session.** [`Self::play_fishing_start`] lifts the fishing overlay
//!    (PROT 0972) through the static-overlay map, decodes its per-species table
//!    ([`legaia_asset::fishing_species`]) plus the two point-exchange venue
//!    pages, and installs a [`FishingSession`] with `World::enter_fishing` -
//!    the same suspend contract the native `play-window` uses, so the field
//!    scene stays intact underneath and resumes on exit.
//! 2. **A cast / reel input path.** None is added here, and that is the point:
//!    the driver is `World::tick_fishing`, which reads the *pad*
//!    ([`legaia_engine_core::input::PadButton`]) the page already routes through
//!    `LegaiaRuntime::set_pad` every frame. Cross locks the cast and reels
//!    (reel A), Square reels harder (reel B), Cross recasts when the cast is
//!    done. The ported reel decoder `ReelInput::from_pad_mask` classifies the
//!    two held bits, so holding both resolves the way retail does.
//! 3. **A point record.** `World::fishing_points` is the persistent pool
//!    (retail `_DAT_8008444C`); the session seeds from it and `exit_fishing`
//!    banks back into it, so points survive leaving and re-entering.
//!
//! The HUD itself is not re-implemented: [`Self::play_fishing_hud_json`] builds
//! the retail draw list from the shared builders
//! ([`legaia_engine_ui::persistent_hud_draws`],
//! [`legaia_engine_ui::catch_hud_draws`], the [`legaia_engine_ui::FishingBanners`]
//! one-shots) and projects it through
//! [`legaia_engine_ui::fishing_hud_draws_for`] - the same consumer the native
//! window's `window/hud.rs` calls, with the same blind sprite atlas, because
//! the fishing sprite page is the one undecoded asset in the chain.
//!
//! # The one place this host draws more than native
//!
//! With a blind atlas, `fishing_hud_draws_for` drops every glyph and every bar
//! fill - native's fishing HUD is therefore digits and captions only, with no
//! visible gauges. The gauges are the functional part of the tension
//! tug-of-war, so this host emits their resolved frames as a third `bars`
//! channel (through the ported [`legaia_engine_ui::HudDraw::resolve_bar`], the
//! same `FUN_801d1870` / `FUN_801d1a90` geometry) for the page to fill as
//! rects. Text still comes from the shared consumer; only the quads the shared
//! consumer cannot produce without a sprite page are re-routed.

use crate::runtime::LegaiaRuntime;
use legaia_engine_core::fishing::{
    FishingPhase, FishingRecord, FishingSession, PrizeExchange, TENSION_MAX,
};
use legaia_engine_ui::{
    self as ui, BarAxis, CatchHudState, FishingCaptions, FishingHudAtlas, HudDraw, TextDraw,
};
use wasm_bindgen::prelude::*;

/// Rod stat for the page's entry point. Matches the native window's
/// `DEV_ROD_STAT`: the save-block fishing record is not loaded on either host's
/// dev entry, so both start from the same mid rod.
const WEB_ROD_STAT: i32 = 4;

/// Stage-pixel pen for the phase / prompt status line, matching the native
/// window's fishing line at `(8, 62)`.
const STATUS_PEN: (i32, i32) = (8, 62);
/// Second status row (the native window's `(8, 80)` hint line).
const HINT_PEN: (i32, i32) = (8, 80);

/// The empty payload. Kept as one literal so every early return agrees.
const CLOSED: &str = r#"{"open":false,"sprites":[],"texts":[],"bars":[]}"#;

impl LegaiaRuntime {
    /// Service the fishing banner one-shots for this sim tick and cache the
    /// draws the HUD will emit. The browser twin of the native window's
    /// `tick_fishing_banners`: the session's phase *edges* seed the timers
    /// (hook / landed / snapped / recast), and each timer retires itself.
    ///
    /// Called from `tick_frame`, i.e. on the sim clock, so a page rendering
    /// below 60 Hz does not slow the banner animations down.
    pub(crate) fn tick_fishing_banners(&mut self) {
        use legaia_engine_core::fishing::FightOutcome;
        let Some(session) = self
            .scene_host
            .as_ref()
            .and_then(|h| h.world.fishing.as_ref())
        else {
            self.fishing_banners = Default::default();
            self.fishing_banner_draws.clear();
            self.fishing_prev_phase = None;
            return;
        };
        let phase = session.phase();
        let outcome = session.last_outcome();
        match (self.fishing_prev_phase, phase) {
            (Some(FishingPhase::Casting), FishingPhase::Fighting) => {
                self.fishing_banners.on_hook();
            }
            (Some(FishingPhase::Fighting), FishingPhase::Done) => match outcome {
                Some(FightOutcome::Landed { .. }) => self.fishing_banners.on_landed(),
                Some(FightOutcome::Snapped) => self.fishing_banners.on_snapped(),
                // `Fighting` is the in-progress outcome, so it cannot describe
                // a fight the session has just left.
                Some(FightOutcome::Fighting) | None => {}
            },
            (Some(FishingPhase::Done), FishingPhase::Casting) => {
                self.fishing_banners.on_recast();
            }
            _ => {}
        }
        self.fishing_prev_phase = Some(phase);
        self.fishing_banner_draws = self.fishing_banners.service_frame(1);
    }

    /// The live fishing session, when one is installed on the scene host's
    /// world.
    fn fishing_session(&self) -> Option<&FishingSession> {
        self.scene_host.as_ref()?.world.fishing.as_ref()
    }

    /// The phase / prompt status rows the native window prints above the retail
    /// HUD, so a player can tell which phase the session is in before the
    /// sprite page exists.
    fn fishing_status_draws(&self, font: &legaia_font::Font) -> Vec<TextDraw> {
        use legaia_engine_core::fishing::FightOutcome;
        let Some(s) = self.fishing_session() else {
            return Vec::new();
        };
        let white = [1.0, 1.0, 1.0, 1.0];
        let dim = [0.65, 0.72, 0.8, 1.0];
        let line = match s.phase() {
            FishingPhase::Casting => {
                format!("FISHING  cast power {}  (Z = cast)", s.cast_power())
            }
            FishingPhase::Fighting => {
                let (tension, strength) = s
                    .fight()
                    .map(|f| (f.tension(), f.strength()))
                    .unwrap_or((0, 0));
                format!("FISHING  tension {tension}/{TENSION_MAX}  strength {strength}")
            }
            FishingPhase::Done => match s.last_outcome() {
                Some(FightOutcome::Landed { points }) => {
                    format!("FISHING  landed! +{points} points  (Z = recast)")
                }
                Some(FightOutcome::Snapped) => {
                    "FISHING  the line snapped!  (Z = recast)".to_string()
                }
                _ => "FISHING  (Z = recast)".to_string(),
            },
        };
        let hint = match s.phase() {
            FishingPhase::Fighting => "hold Z / V to reel",
            _ => "Z casts and reels, V reels harder",
        };
        let mut out = ui::text_draws_for(&font.layout_ascii(&line), STATUS_PEN, white);
        out.extend(ui::text_draws_for(&font.layout_ascii(hint), HINT_PEN, dim));
        out
    }

    /// This frame's retail HUD draw list: the persistent rows
    /// (`FUN_801d13f0`), the catch HUD once a cast is out (`FUN_801d1580`, its
    /// gauge block only while the fish is on) and the live banner one-shots.
    fn fishing_hud_items(&self) -> Vec<HudDraw> {
        let Some(s) = self.fishing_session() else {
            return Vec::new();
        };
        // The lure/rod index and its remaining count come from the retail
        // ownership gate, which re-points a stale selection at the next owned
        // lure - the same call the native window makes.
        use legaia_engine_core::fishing::{lure_item_id, select_owned_rod};
        let inventory = self
            .scene_host
            .as_ref()
            .map(|h| &h.world.inventory)
            .expect("fishing_session() proved the host exists");
        let count_of = |id: u32| *inventory.get(&(id as u8)).unwrap_or(&0) as i32;
        let mut rod_index = 0;
        let has_rod = select_owned_rod(&mut rod_index, count_of);
        let mut items = ui::persistent_hud_draws(
            s.record().points,
            s.record().best_points,
            rod_index,
            if has_rod {
                count_of(lure_item_id(rod_index))
            } else {
                0
            },
        );
        // Two retail globals have no engine analogue and stay zero, exactly as
        // on the native host: the cast line-projection term (`DAT_801d9178`)
        // and the line depth (`DAT_801d9298`).
        let fight = s.fight();
        items.extend(ui::catch_hud_draws(&CatchHudState {
            record: fight.map(|f| f.progress()).unwrap_or(0),
            line_extent: 0,
            cast_power: s.cast_power(),
            depth: 0,
            tension: fight.map(|f| f.tension()).unwrap_or(0),
            gauges_visible: s.phase() == FishingPhase::Fighting,
        }));
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
    /// loaded or the fishing overlay's species table does not decode.
    ///
    /// The session's point pool resumes [`World::fishing_points`], so leaving
    /// and re-entering keeps the running total.
    ///
    /// [`World::fishing_points`]: legaia_engine_core::world::World::fishing_points
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
        let Some(species) = fishing_species::parse(&loaded) else {
            return false;
        };
        // The two point-exchange venue pages ride the same overlay image; row
        // labels resolve through the SCUS item table the page already parsed.
        let names = self.item_names.as_ref();
        self.fishing_venues = legaia_asset::fishing_exchange::parse(&loaded).map(|ex| {
            [0usize, 1].map(|venue| PrizeExchange::from_asset(venue, &ex.venues[venue], names))
        });
        let record = FishingRecord {
            points: host.world.fishing_points,
            ..Default::default()
        };
        host.world
            .enter_fishing(FishingSession::new(species, WEB_ROD_STAT, record));
        self.fishing_banners = Default::default();
        self.fishing_banner_draws.clear();
        self.fishing_prev_phase = None;
        true
    }

    /// Leave the fishing session and restore the suspended scene mode, banking
    /// the session's points into the world's persistent pool. Returns the
    /// banked total (`-1` when no session was live).
    pub fn play_fishing_stop(&mut self) -> i32 {
        let Some(host) = self.scene_host.as_mut() else {
            return -1;
        };
        if host.world.exit_fishing().is_none() {
            return -1;
        }
        self.fishing_banners = Default::default();
        self.fishing_banner_draws.clear();
        self.fishing_prev_phase = None;
        host.world.fishing_points
    }

    /// Is a fishing session live on the world this frame?
    pub fn play_fishing_active(&self) -> bool {
        self.fishing_session().is_some()
    }

    /// The live session's state for the page's readout:
    ///
    /// ```json
    /// { "live": true, "phase": "casting", "cast_power": 0, "cast_max": 0,
    ///   "tension": 0, "tension_max": 0, "progress": 0, "points": 0,
    ///   "best": 0, "lure": 0 }
    /// ```
    pub fn play_fishing_state_json(&self) -> String {
        let Some(s) = self.fishing_session() else {
            return r#"{"live":false}"#.to_string();
        };
        let phase = match s.phase() {
            FishingPhase::Casting => "casting",
            FishingPhase::Fighting => "fighting",
            FishingPhase::Done => "done",
        };
        let fight = s.fight();
        serde_json::json!({
            "live": true,
            "phase": phase,
            "cast_power": s.cast_power(),
            "cast_max": legaia_engine_core::fishing::CAST_POWER_MAX,
            "tension": fight.map(|f| f.tension()).unwrap_or(0),
            "tension_max": TENSION_MAX,
            "progress": fight.map(|f| f.progress()).unwrap_or(0),
            "points": s.record().points,
            "best": s.record().best_points,
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
        // The retail persistent + catch rows through the shared consumer. The
        // atlas is blind on purpose: no fishing sprite page is uploaded on
        // either host, so this is byte-for-byte the native call.
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
    ///       "owned": 0 } ] }
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
                let owned = *world.inventory.get(&r.item_id).unwrap_or(&0) as u32;
                serde_json::json!({
                    "name": r.name.clone().unwrap_or_else(|| format!("item {:#04x}", r.item_id)),
                    "price": r.price,
                    "one_time": r.is_one_time(),
                    "owned": owned,
                    "available": ex.is_available(
                        i,
                        world.fishing_points,
                        owned,
                        world.fishing_prizes_purchased,
                    ),
                })
            })
            .collect();
        serde_json::json!({
            "venue": ex.venue,
            "points": world.fishing_points,
            "first_visible": ex.first_visible(world.fishing_points),
            "rows": rows,
        })
        .to_string()
    }

    /// Buy prize row `row` at `venue` with the live point pool. Returns the
    /// remaining points, or `-1` when the row is unavailable (too few points,
    /// a latched one-time prize, or a full stack).
    pub fn play_fishing_prize_buy(&mut self, venue: u32, row: usize) -> i32 {
        let Some(venues) = self.fishing_venues.as_ref() else {
            return -1;
        };
        let exchange = venues[(venue as usize).min(1)].clone();
        let Some(host) = self.scene_host.as_mut() else {
            return -1;
        };
        host.world.open_fishing_exchange(exchange);
        let ok = host.world.fishing_exchange_buy(row, 1).is_some();
        host.world.close_fishing_exchange();
        if ok { host.world.fishing_points } else { -1 }
    }
}
