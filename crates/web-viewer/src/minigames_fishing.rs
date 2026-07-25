//! Fishing-minigame methods of [`LegaiaMinigames`] - the browser twin of the
//! play-window's `start_fishing_minigame` (`window/minigames.rs`).
//!
//! The rules are the ported [`legaia_engine_core::fishing`] engine: the
//! casting-power oscillator, the tension-gauge tug-of-war and the catch
//! scoring, all driven by the per-species table decoded from the visitor's own
//! disc (PROT 0972 rodata, [`legaia_asset::fishing_species`]). This file is the
//! thin JSON shell over it - it ticks the meter, applies reel input and hands
//! the state to the page. No table ships with the site.
//!
//! Interaction shape (mirroring the native fishing driver `tick_fishing`):
//! **Casting** oscillates the power meter until [`Self::fishing_lock_cast`]
//! hooks a fish (a longer cast reaches a rarer species); **Fighting** raises
//! tension while a reel button is held and bleeds it off when released - the
//! line snaps at max tension, the fish lands once enough progress is reeled in;
//! **Done** shows the outcome and [`Self::fishing_recast`] casts again.

use super::*;

use legaia_asset::fishing_species;
use legaia_engine_core::fishing::{
    self, FightOutcome, FishingPhase, FishingRecord, FishingSession, PondEvent, PondInput,
    PondPhase, PondSession, PrizeExchange, ReelInput,
};
// engine-ui re-exports its `ui_fishing` module's items at the crate root.
use legaia_engine_ui as ui_fishing;
use legaia_engine_ui::{BarAxis, CatchHudState, HudCaption, HudDraw};

/// Default rod stat for the browser entry point (the native dev launcher's
/// `DEV_ROD_STAT`; the save-block fishing record isn't loaded here).
const WEB_ROD_STAT: i32 = 4;

impl LegaiaMinigames {
    /// Decode the fishing overlay (PROT 0972) into the cached species table +
    /// overlay image (for species-name resolution), returning the status
    /// object `load_disc` folds into its report.
    pub(super) fn load_fishing_tables(&mut self) -> String {
        self.fishing = None;
        self.fishing_species = None;
        self.fishing_overlay = None;
        let img = overlay_image(
            &self.prot,
            &self.entries,
            fishing_species::FISHING_OVERLAY_PROT_INDEX as u32,
        );
        match img.as_ref().and_then(|o| fishing_species::parse(o)) {
            Some(species) => {
                let n = species.len();
                // The venue-faithful tables that ride the same overlay image:
                // the per-venue spawn pages, the reel-cadence gesture
                // templates, and the point-exchange prize pages. Each is
                // optional - a partial decode still plays the plain session.
                let ov = img.as_deref().unwrap_or_default();
                self.fishing_spawn = fishing_species::parse_spawn_tables(ov);
                self.fishing_cadence = fishing_species::parse_cadence_templates(ov);
                self.fishing_exchange = legaia_asset::fishing_exchange::parse(ov);
                let venue_ok = self.fishing_spawn.is_some() && self.fishing_cadence.is_some();
                self.fishing_species = Some(species);
                self.fishing_overlay = img;
                format!(
                    r#"{{"ok":true,"species":{n},"venue_tables":{venue_ok},"exchange":{}}}"#,
                    self.fishing_exchange.is_some()
                )
            }
            None => format!(
                r#"{{"ok":false,"why":{}}}"#,
                jstr("fishing overlay (PROT 0972) or its species table did not decode")
            ),
        }
    }

    /// Resolve a species id to its overlay name (`null` JSON when absent).
    fn fishing_species_name_json(&self, id: usize) -> String {
        self.fishing_species
            .as_ref()
            .and_then(|s| s.get(id))
            .and_then(|sp| self.fishing_overlay.as_deref().and_then(|o| sp.name(o)))
            .map(jstr)
            .unwrap_or_else(|| "null".to_string())
    }

    /// The engine prize-exchange view for `venue`, over the disc rows +
    /// the SCUS item names (full-disc loads).
    fn prize_exchange(&self, venue: usize) -> Option<PrizeExchange> {
        let ex = self.fishing_exchange.as_ref()?;
        let rows = ex.venues.get(venue.min(1))?;
        Some(PrizeExchange::from_asset(
            venue.min(1),
            rows,
            self.item_names.as_ref(),
        ))
    }
}

#[wasm_bindgen]
impl LegaiaMinigames {
    /// Start a fishing session over the disc's species table, beginning in the
    /// casting phase. Returns `false` when the table didn't decode.
    pub fn fishing_start(&mut self) -> bool {
        let Some(species) = self.fishing_species.clone() else {
            return false;
        };
        self.fishing = Some(FishingSession::new(
            species,
            WEB_ROD_STAT,
            FishingRecord::default(),
        ));
        true
    }

    /// Advance the cast-power oscillator by `step` (no-op outside casting). The
    /// native driver steps `0x80` per frame; the page passes its own rate.
    pub fn fishing_advance_cast(&mut self, step: i32) {
        if let Some(s) = self.fishing.as_mut() {
            s.advance_cast(step);
        }
    }

    /// Lock the cast and hook a fish, entering the fight (no-op outside
    /// casting). The locked power selects the species.
    pub fn fishing_lock_cast(&mut self) {
        if let Some(s) = self.fishing.as_mut() {
            s.lock_cast();
        }
    }

    /// Apply one fight frame's reel input, stepped by `frames`: `0` = idle
    /// (tension bleeds off), `1` = reel A (Cross, `rod*9 + 0x23` divisor),
    /// `2` = reel B (Square, `rod*6 + 0x19`). No-op outside the fighting phase.
    pub fn fishing_reel(&mut self, input: u8, frames: i32) {
        let reel = match input {
            1 => ReelInput::ReelA,
            2 => ReelInput::ReelB,
            _ => ReelInput::Idle,
        };
        if let Some(s) = self.fishing.as_mut() {
            s.reel(reel, frames.max(1));
        }
    }

    /// Recast after a resolved fight: reset the meter and clear the fight
    /// (no-op unless the fight is done).
    pub fn fishing_recast(&mut self) {
        if let Some(s) = self.fishing.as_mut() {
            s.recast();
        }
    }

    /// Live fishing state.
    ///
    /// ```json
    /// { "live": true, "phase": "casting"|"fighting"|"done",
    ///   "cast_power": 64, "cast_min": 32, "cast_max": 4096, "cast_seed": 64,
    ///   "tension": 0, "tension_max": 4096, "strength": 0, "land_target": 310,
    ///   "fish": { "index": 2, "name": "Legaia Bass", "score": 10000 },
    ///   "points": 0, "best_points": 0, "best_fish": 0,
    ///   "outcome": "landed"|"snapped"|null, "outcome_points": 0 }
    /// ```
    ///
    /// `strength` is the confirmed catch-score accumulator - it grows only
    /// while reeling, so it doubles as a "how worked-in is the fish" readout;
    /// `tension` climbing to `tension_max` snaps the line. `fish` is `null`
    /// while casting.
    pub fn fishing_state_json(&self) -> String {
        let Some(s) = self.fishing.as_ref() else {
            return r#"{"live":false}"#.to_string();
        };
        let phase = match s.phase() {
            FishingPhase::Casting => "casting",
            FishingPhase::Fighting => "fighting",
            FishingPhase::Done => "done",
        };
        let rec = s.record();
        let fish = match s.fight() {
            Some(f) => {
                let sp = f.species();
                let name = self
                    .fishing_overlay
                    .as_deref()
                    .and_then(|o| sp.name(o))
                    .map(jstr)
                    .unwrap_or_else(|| "null".to_string());
                format!(
                    r#"{{"index":{},"name":{},"score":{}}}"#,
                    sp.index, name, sp.score_value
                )
            }
            None => "null".to_string(),
        };
        let (tension, strength, land_target) = match s.fight() {
            Some(f) => (f.tension(), f.strength(), f.land_target()),
            None => (0, 0, 0),
        };
        let (outcome, outcome_points) = match s.last_outcome() {
            Some(FightOutcome::Landed { points }) => ("landed", points),
            Some(FightOutcome::Snapped) => ("snapped", 0),
            _ => ("null", 0),
        };
        let outcome = if outcome == "null" {
            "null".to_string()
        } else {
            jstr(outcome)
        };
        format!(
            concat!(
                r#"{{"live":true,"phase":{},"cast_power":{},"cast_min":{},"cast_max":{},"#,
                r#""cast_seed":{},"tension":{},"tension_max":{},"strength":{},"land_target":{},"#,
                r#""fish":{},"points":{},"best_points":{},"best_fish":{},"outcome":{},"#,
                r#""outcome_points":{}}}"#
            ),
            jstr(phase),
            s.cast_power(),
            fishing::CAST_POWER_MIN,
            fishing::CAST_POWER_MAX,
            fishing::CAST_POWER_SEED,
            tension,
            fishing::TENSION_MAX,
            strength,
            land_target,
            fish,
            rec.points,
            rec.best_points,
            rec.best_fish,
            outcome,
            outcome_points,
        )
    }

    /// The whole decoded species table, for the "what's biting" panel:
    ///
    /// ```json
    /// [ { "index": 0, "name": "Legaia Bass", "score": 8000,
    ///     "pull": 250, "strike_gate": 8 }, ... ]
    /// ```
    ///
    /// `name` is `null` when the overlay's name pointer doesn't resolve.
    pub fn fishing_species_json(&self) -> String {
        let Some(species) = self.fishing_species.as_ref() else {
            return "[]".to_string();
        };
        let overlay = self.fishing_overlay.as_deref();
        let rows = species
            .iter()
            .map(|sp| {
                let name = overlay
                    .and_then(|o| sp.name(o))
                    .map(jstr)
                    .unwrap_or_else(|| "null".to_string());
                format!(
                    r#"{{"index":{},"name":{},"score":{},"pull":{},"strike_gate":{}}}"#,
                    sp.index, name, sp.score_value, sp.pull_factor, sp.strike_gate
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        format!("[{rows}]")
    }
}

// --------------------------------------------------------------------------
// The venue-faithful pond session (the retail cast/band/strike/fight loop)
// --------------------------------------------------------------------------

/// Serialize one [`HudDraw`] into the page's draw-item JSON. Numbers and
/// counts are expanded through the ported 8-slot digit field
/// ([`ui_fishing::number_digit_cells`]); bars resolve through the ported
/// cap/body/cap frame ([`HudDraw::resolve_bar`]); glyphs and captions carry
/// their ids for the page to map (the fishing sprite page itself is the one
/// undecoded asset - the page substitutes labelled text and says so).
fn hud_draw_json(d: &HudDraw, out: &mut Vec<String>) {
    match *d {
        HudDraw::Number {
            x,
            y,
            value,
            brightness,
        } => {
            for c in ui_fishing::number_digit_cells(0, x, y, value) {
                out.push(format!(
                    r#"{{"t":"digit","x":{},"y":{},"d":{},"b":{brightness}}}"#,
                    c.x, c.y, c.digit
                ));
            }
        }
        HudDraw::Count {
            value,
            digits: _,
            x,
            y,
        } => {
            for c in ui_fishing::number_digit_cells(0, x, y, value) {
                out.push(format!(
                    r#"{{"t":"digit","x":{},"y":{},"d":{},"b":128}}"#,
                    c.x, c.y, c.digit
                ));
            }
        }
        HudDraw::Glyph {
            layer,
            id,
            x,
            y,
            brightness,
        } => out.push(format!(
            r#"{{"t":"glyph","layer":{layer},"id":{id},"x":{x},"y":{y},"b":{brightness}}}"#
        )),
        HudDraw::Caption { text, x, y } => {
            let k = match text {
                HudCaption::RodName(i) => format!("lure{i}"),
                HudCaption::LuresLeft => "lures_left".to_string(),
                HudCaption::LureCountSuffix => "lure_suffix".to_string(),
            };
            out.push(format!(r#"{{"t":"cap","k":{},"x":{x},"y":{y}}}"#, jstr(&k)));
        }
        HudDraw::Bar { .. } | HudDraw::PowerBar { .. } => {
            if let Some(f) = d.resolve_bar() {
                let axis = match f.axis {
                    BarAxis::Horizontal => "h",
                    BarAxis::Vertical => "v",
                };
                let (x, y) = f.positions[0];
                let span =
                    f.positions[2].0.max(f.positions[2].1) - f.positions[0].0.min(f.positions[0].1);
                let (r, g, b) = f.fill_rgb.unwrap_or((0, 0, 0));
                out.push(format!(
                    concat!(
                        r#"{{"t":"bar","axis":"{}","x":{},"y":{},"end_x":{},"end_y":{},"#,
                        r#""span":{},"fill":{},"bright":{},"rgb":[{},{},{}]}}"#
                    ),
                    axis,
                    x,
                    y,
                    f.positions[2].0,
                    f.positions[2].1,
                    span,
                    f.fill_len,
                    f.fill_brightness,
                    r,
                    g,
                    b
                ));
            }
        }
    }
}

#[wasm_bindgen]
impl LegaiaMinigames {
    /// Whether the venue-faithful pond can start (species + spawn + cadence
    /// tables all decoded).
    pub fn fishing_pond_ready(&self) -> bool {
        self.fishing_species.is_some()
            && self.fishing_spawn.is_some()
            && self.fishing_cadence.is_some()
    }

    /// Start a pond session at `venue` (0 = Buma, 1 = Vidna) with the
    /// persistent save-block state: equipped `lure` (0..=2), `rod` stat
    /// (0..=2), lifetime `casts`, the point record triple, and the one-time
    /// prize `purchased_mask`. `seed` feeds the deterministic BIOS-rand
    /// stream. Returns `false` when the tables didn't decode.
    #[allow(clippy::too_many_arguments)]
    pub fn fishing_pond_start(
        &mut self,
        venue: u32,
        lure: u32,
        rod: i32,
        casts: i32,
        points: i32,
        best_points: i32,
        best_fish: u32,
        purchased_mask: u32,
        seed: u32,
    ) -> bool {
        let (Some(species), Some(spawn), Some(cadence)) = (
            self.fishing_species.clone(),
            self.fishing_spawn.as_ref(),
            self.fishing_cadence.clone(),
        ) else {
            return false;
        };
        let venue = (venue as usize).min(1);
        let record = FishingRecord {
            points,
            best_points,
            best_fish: best_fish as usize,
        };
        self.fishing_pond = Some(PondSession::new(
            species,
            spawn[venue].clone(),
            cadence,
            venue,
            lure,
            rod,
            casts,
            record,
            purchased_mask,
            seed,
        ));
        self.fishing_banners = Default::default();
        self.fishing_prizes.clear();
        true
    }

    /// Advance the pond one frame. `reel_mask` carries the held pad bits
    /// (`0x40` Cross / reel A, `0x80` Square / reel B), `cast_edge` the cast /
    /// confirm press, `edge_bonus` the count of fresh input edges this frame
    /// (each feeds the strike credit). Returns the events raised this frame:
    ///
    /// ```json
    /// [ {"e":"splash"}, {"e":"hooked","id":3,"name":"..."},
    ///   {"e":"landed","points":1234}, {"e":"snapped"} ]
    /// ```
    pub fn fishing_pond_tick(
        &mut self,
        reel_mask: u32,
        cast_edge: bool,
        edge_bonus: i32,
    ) -> String {
        let Some(p) = self.fishing_pond.as_mut() else {
            return "[]".to_string();
        };
        let was = p.phase();
        p.tick(
            PondInput {
                reel_mask,
                cast_edge,
                edge_bonus,
            },
            1,
            0x80,
        );
        if was != PondPhase::Idle && p.phase() == PondPhase::Idle {
            self.fishing_banners.on_recast();
        }
        let events = p.take_events();
        let mut out = Vec::new();
        for e in &events {
            match *e {
                PondEvent::Splash => {
                    self.fishing_banners.splash.start();
                    out.push(r#"{"e":"splash"}"#.to_string());
                }
                PondEvent::Hooked(id) => {
                    self.fishing_banners.on_hook();
                    let name = self.fishing_species_name_json(id);
                    out.push(format!(r#"{{"e":"hooked","id":{id},"name":{name}}}"#));
                }
                PondEvent::Landed(points) => {
                    self.fishing_banners.on_landed();
                    out.push(format!(r#"{{"e":"landed","points":{points}}}"#));
                }
                PondEvent::Snapped => {
                    self.fishing_banners.on_snapped();
                    out.push(r#"{"e":"snapped"}"#.to_string());
                }
            }
        }
        format!("[{}]", out.join(","))
    }

    /// Live pond state:
    ///
    /// ```json
    /// { "live": true, "phase": "idle"|"windup"|"power"|"flight"|"waiting"|
    ///   "hooked"|"landed"|"snapped",
    ///   "cast_power": 64, "cast_max": 4096, "tension": 0, "tension_max": 4096,
    ///   "record": 0, "readout": 0, "depth": 0, "lateral": 0,
    ///   "fish": {"index":3,"name":...,"score":8000}|null, "move": "run"|...,
    ///   "points": 0, "best": 0, "best_fish": 0, "casts": 51,
    ///   "last_award": 0, "venue": 0, "lure": 1, "rod": 2 }
    /// ```
    pub fn fishing_pond_state_json(&self) -> String {
        let Some(p) = self.fishing_pond.as_ref() else {
            return r#"{"live":false}"#.to_string();
        };
        let phase = match p.phase() {
            PondPhase::Idle => "idle",
            PondPhase::WindUp => "windup",
            PondPhase::Power => "power",
            PondPhase::Flight => "flight",
            PondPhase::Waiting => "waiting",
            PondPhase::Hooked => "hooked",
            PondPhase::Landed => "landed",
            PondPhase::Snapped => "snapped",
        };
        let fish = match p.hooked() {
            Some(sp) => format!(
                r#"{{"index":{},"name":{},"score":{}}}"#,
                sp.index,
                self.fishing_species_name_json(sp.index),
                sp.score_value
            ),
            None => "null".to_string(),
        };
        let mv = match p.fish_move() {
            Some(fishing::FishMove::Run) => r#""run""#,
            Some(fishing::FishMove::DartLeft) => r#""dart_left""#,
            Some(fishing::FishMove::DartRight) => r#""dart_right""#,
            Some(fishing::FishMove::Dive) => r#""dive""#,
            None => "null",
        };
        format!(
            concat!(
                r#"{{"live":true,"phase":{},"cast_power":{},"cast_max":{},"#,
                r#""tension":{},"tension_max":{},"record":{},"readout":{},"#,
                r#""depth":{},"lateral":{},"fish":{},"move":{},"#,
                r#""points":{},"best":{},"best_fish":{},"casts":{},"#,
                r#""last_award":{},"venue":{},"lure":{},"rod":{}}}"#
            ),
            jstr(phase),
            p.cast_power(),
            fishing::CAST_POWER_MAX,
            p.tension(),
            fishing::TENSION_MAX,
            p.line_record(),
            p.readout(),
            p.depth(),
            p.lateral(),
            fish,
            mv,
            p.record.points,
            p.record.best_points,
            p.record.best_fish,
            p.casts,
            p.last_award(),
            p.venue,
            p.lure,
            p.rod,
        )
    }

    /// The retail HUD draw list for this frame: the persistent rows
    /// (`FUN_801d13f0`), the catch HUD (`FUN_801d1580`, gauges once hooked)
    /// and the live banner animations, each resolved through the ported
    /// engine-ui builders. Coordinates are retail 320x240 screen space.
    /// Also services the banner timers, so call it exactly once per frame.
    pub fn fishing_pond_hud_json(&mut self) -> String {
        let Some(p) = self.fishing_pond.as_ref() else {
            return "[]".to_string();
        };
        let mut draws = ui_fishing::persistent_hud_draws(
            p.record.points,
            p.record.best_points,
            p.lure,
            99, // the browser session has no live inventory; lures aren't consumed
        );
        if p.phase() != PondPhase::Idle {
            draws.extend(ui_fishing::catch_hud_draws(&CatchHudState {
                record: p.line_record(),
                line_extent: 0,
                cast_power: p.cast_power(),
                depth: p.depth(),
                tension: p.tension(),
                gauges_visible: p.phase() == PondPhase::Hooked,
            }));
        }
        draws.extend(self.fishing_banners.service_frame(1));
        let mut out = Vec::new();
        for d in &draws {
            hud_draw_json(d, &mut out);
        }
        format!("[{}]", out.join(","))
    }

    /// The venue's species-spawn table, named:
    ///
    /// ```json
    /// { "venue": 0, "rows": [ { "lure": 0, "bands": [
    ///     {"id": 3, "name": "..."}, ... 5 ] }, ... 3 ] }
    /// ```
    pub fn fishing_spawn_json(&self, venue: u32) -> String {
        let Some(spawn) = self.fishing_spawn.as_ref() else {
            return "null".to_string();
        };
        let page = &spawn[(venue as usize).min(1)];
        let rows = page
            .iter()
            .take(3) // three lures exist; rows 3..8 are zero padding
            .enumerate()
            .map(|(lure, row)| {
                let bands = row
                    .iter()
                    .take(5)
                    .map(|&id| {
                        format!(
                            r#"{{"id":{id},"name":{}}}"#,
                            self.fishing_species_name_json(id as usize)
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(",");
                format!(r#"{{"lure":{lure},"bands":[{bands}]}}"#)
            })
            .collect::<Vec<_>>()
            .join(",");
        format!(r#"{{"venue":{},"rows":[{rows}]}}"#, (venue as usize).min(1))
    }

    // ----------------------------------------------------- point exchange

    /// The venue's point-exchange page, evaluated against the live session's
    /// points + purchased mask through the ported prize kernels:
    ///
    /// ```json
    /// { "venue": 0, "points": 1200, "first_visible": 1, "rows": [
    ///   { "row": 0, "limit": 1, "price": 50000, "item_id": 12,
    ///     "name": "War God Icon"|null, "owned": 0, "available": false,
    ///     "max_qty": 0, "one_time": true, "latched": false }, ... ] }
    /// ```
    pub fn fishing_exchange_json(&self, venue: u32) -> String {
        let Some(ex) = self.prize_exchange(venue as usize) else {
            return "null".to_string();
        };
        let (points, mask) = match self.fishing_pond.as_ref() {
            Some(p) => (p.record.points, p.purchased_mask),
            None => (0, 0),
        };
        let rows = ex
            .rows
            .iter()
            .map(|r| {
                let owned = *self.fishing_prizes.get(&(r.item_id as u32)).unwrap_or(&0);
                let latched = (mask >> ex.purchase_bit(r.row)) & 1 != 0;
                format!(
                    concat!(
                        r#"{{"row":{},"limit":{},"price":{},"item_id":{},"name":{},"#,
                        r#""owned":{},"available":{},"max_qty":{},"one_time":{},"latched":{}}}"#
                    ),
                    r.row,
                    r.limit,
                    r.price,
                    r.item_id,
                    r.name.as_deref().map(jstr).unwrap_or("null".to_string()),
                    owned,
                    ex.is_available(r.row, points, owned, mask),
                    ex.max_qty(r.row, points, owned, mask),
                    r.is_one_time(),
                    latched,
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        format!(
            r#"{{"venue":{},"points":{},"first_visible":{},"rows":[{rows}]}}"#,
            ex.venue,
            points,
            ex.first_visible(points)
        )
    }

    /// Buy `qty` of `row` at the venue's exchange, spending the pond
    /// session's point pool and latching one-time rows in the purchased mask
    /// (`FUN_801d06c8`'s Yes arm through the ported kernels). Returns the
    /// purchase (`{"item_id":..,"qty":..,"cost":..,"name":..}`) or `null`.
    pub fn fishing_exchange_buy(&mut self, venue: u32, row: u32, qty: u32) -> String {
        let Some(ex) = self.prize_exchange(venue as usize) else {
            return "null".to_string();
        };
        let Some(p) = self.fishing_pond.as_mut() else {
            return "null".to_string();
        };
        let owned = *self
            .fishing_prizes
            .get(
                &(ex.rows
                    .get(row as usize)
                    .map(|r| r.item_id as u32)
                    .unwrap_or(0)),
            )
            .unwrap_or(&0);
        let Some(purchase) = ex.buy(row as usize, qty, p.record.points, owned, p.purchased_mask)
        else {
            return "null".to_string();
        };
        p.record.points -= purchase.cost as i32;
        if let Some(bit) = purchase.latched_bit {
            p.purchased_mask |= 1 << bit;
        }
        *self
            .fishing_prizes
            .entry(purchase.item_id as u32)
            .or_insert(0) += purchase.qty;
        let name = ex
            .rows
            .get(row as usize)
            .and_then(|r| r.name.as_deref())
            .map(jstr)
            .unwrap_or("null".to_string());
        format!(
            r#"{{"item_id":{},"qty":{},"cost":{},"name":{}}}"#,
            purchase.item_id, purchase.qty, purchase.cost, name
        )
    }

    /// Prizes collected through the exchange this session:
    /// `[{"item_id":..,"qty":..,"name":..}, ...]`.
    pub fn fishing_prizes_json(&self) -> String {
        let mut rows: Vec<_> = self.fishing_prizes.iter().collect();
        rows.sort();
        let out = rows
            .into_iter()
            .map(|(&id, &qty)| {
                let name = self
                    .item_names
                    .as_ref()
                    .and_then(|t| t.name(id as u8))
                    .map(jstr)
                    .unwrap_or("null".to_string());
                format!(r#"{{"item_id":{id},"qty":{qty},"name":{name}}}"#)
            })
            .collect::<Vec<_>>()
            .join(",");
        format!("[{out}]")
    }
}

// --------------------------------------------------------------------------
// Save-block fishing record (retail memory-card import / export)
// --------------------------------------------------------------------------
//
// The persistent fishing block lives in the save's SC block under the linear
// map SC = 0x200 + (RAM - 0x80084340) (see `legaia_save::card`, pinned by the
// gold/coins offsets): points 0x8008444C, lure 0x80084450, rod 0x80084454,
// best 0x80084458, best-fish 0x8008445C, cast counter 0x80084460, one-time
// prize bitmask 0x8008446C.

/// SC-block byte offset of the fishing point total (`0x8008444C`).
const SC_FISHING_POINTS: usize = 0x30C;
/// SC-block byte offset of the equipped lure index (`0x80084450`).
const SC_FISHING_LURE: usize = 0x310;
/// SC-block byte offset of the rod stat (`0x80084454`).
const SC_FISHING_ROD: usize = 0x314;
/// SC-block byte offset of the best-catch value (`0x80084458`).
const SC_FISHING_BEST: usize = 0x318;
/// SC-block byte offset of the best-catch fish id (`0x8008445C`).
const SC_FISHING_BEST_FISH: usize = 0x31C;
/// SC-block byte offset of the lifetime cast counter (`0x80084460`).
const SC_FISHING_CASTS: usize = 0x320;
/// SC-block byte offset of the one-time prize bitmask (`0x8008446C`).
const SC_FISHING_PURCHASED: usize = 0x32C;

fn sc_read_u32(sc: &[u8], off: usize) -> Option<u32> {
    let b = sc.get(off..off + 4)?;
    Some(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

fn card_fishing_json_core(bytes: &[u8], block: u8) -> Result<String, String> {
    let view = legaia_save::emu::detect(bytes).map_err(|e| format!("{e}"))?;
    let sc = view
        .sc_block(bytes, block)
        .ok_or_else(|| format!("card_fishing: no block {block}"))?;
    let rd = |off| sc_read_u32(sc, off).ok_or_else(|| "card_fishing: block too small".to_string());
    Ok(format!(
        concat!(
            r#"{{"points":{},"lure":{},"rod":{},"best":{},"best_fish":{},"#,
            r#""casts":{},"purchased":{}}}"#
        ),
        rd(SC_FISHING_POINTS)? as i32,
        rd(SC_FISHING_LURE)?,
        rd(SC_FISHING_ROD)? as i32,
        rd(SC_FISHING_BEST)? as i32,
        rd(SC_FISHING_BEST_FISH)?,
        rd(SC_FISHING_CASTS)? as i32,
        rd(SC_FISHING_PURCHASED)?,
    ))
}

#[allow(clippy::too_many_arguments)]
fn card_patch_fishing_core(
    bytes: Vec<u8>,
    block: u8,
    points: i32,
    lure: u32,
    rod: i32,
    best: i32,
    best_fish: u32,
    casts: i32,
    purchased: u32,
) -> Result<Vec<u8>, String> {
    let mut out = bytes;
    let view = legaia_save::emu::detect(&out).map_err(|e| format!("{e}"))?;
    let sc = view
        .sc_block_mut(&mut out, block)
        .ok_or_else(|| format!("card_patch_fishing: no block {block}"))?;
    if sc.len() < SC_FISHING_PURCHASED + 4 {
        return Err("card_patch_fishing: block too small".to_string());
    }
    let mut wr = |off: usize, v: u32| sc[off..off + 4].copy_from_slice(&v.to_le_bytes());
    wr(SC_FISHING_POINTS, points.max(0) as u32);
    wr(SC_FISHING_LURE, lure.min(2));
    wr(SC_FISHING_ROD, rod.clamp(0, 2) as u32);
    wr(SC_FISHING_BEST, best.max(0) as u32);
    wr(SC_FISHING_BEST_FISH, best_fish);
    wr(SC_FISHING_CASTS, casts.max(0) as u32);
    wr(SC_FISHING_PURCHASED, purchased);
    Ok(out)
}

/// Read the persistent fishing block out of save block `block` of an
/// emulator card container: point total, equipped lure + rod, best catch,
/// the lifetime cast counter and the one-time prize bitmask.
#[wasm_bindgen]
pub fn card_fishing_json(bytes: Vec<u8>, block: u8) -> Result<String, JsValue> {
    card_fishing_json_core(&bytes, block).map_err(|e| JsValue::from_str(&e))
}

/// Write the fishing block back into save block `block`, returning the whole
/// container with only those seven dwords changed - the same in-place edit
/// shape as `card_patch_coins`, so the card still loads in an emulator.
#[wasm_bindgen]
#[allow(clippy::too_many_arguments)]
pub fn card_patch_fishing(
    bytes: Vec<u8>,
    block: u8,
    points: i32,
    lure: u32,
    rod: i32,
    best: i32,
    best_fish: u32,
    casts: i32,
    purchased: u32,
) -> Result<Vec<u8>, JsValue> {
    card_patch_fishing_core(
        bytes, block, points, lure, rod, best, best_fish, casts, purchased,
    )
    .map_err(|e| JsValue::from_str(&e))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A synthetic one-save card with a fishing block.
    fn card_with_fishing() -> Vec<u8> {
        let mut card = vec![0u8; legaia_save::CARD_SIZE];
        card[..2].copy_from_slice(&legaia_save::CARD_MAGIC);
        let f = 0x80;
        card[f..f + 4].copy_from_slice(&0x51u32.to_le_bytes());
        card[f + 8..f + 10].copy_from_slice(&0xFFFFu16.to_le_bytes());
        card[f + 10..f + 22].copy_from_slice(b"BASCUS-94254");
        let b = legaia_save::BLOCK_SIZE;
        card[b..b + 2].copy_from_slice(&legaia_save::SAVE_BLOCK_MAGIC);
        let wr = |card: &mut Vec<u8>, off: usize, v: u32| {
            card[b + off..b + off + 4].copy_from_slice(&v.to_le_bytes())
        };
        wr(&mut card, SC_FISHING_POINTS, 12345);
        wr(&mut card, SC_FISHING_LURE, 1);
        wr(&mut card, SC_FISHING_ROD, 2);
        wr(&mut card, SC_FISHING_BEST, 900);
        wr(&mut card, SC_FISHING_BEST_FISH, 5);
        wr(&mut card, SC_FISHING_CASTS, 77);
        wr(&mut card, SC_FISHING_PURCHASED, 0b101);
        card
    }

    #[test]
    fn card_fishing_reads_the_block() {
        let card = card_with_fishing();
        let json = card_fishing_json_core(&card, 1).expect("read");
        assert!(json.contains(r#""points":12345"#), "{json}");
        assert!(json.contains(r#""lure":1"#), "{json}");
        assert!(json.contains(r#""rod":2"#), "{json}");
        assert!(json.contains(r#""casts":77"#), "{json}");
        assert!(json.contains(r#""purchased":5"#), "{json}");
    }

    #[test]
    fn card_patch_fishing_touches_only_the_fishing_dwords() {
        let card = card_with_fishing();
        // A no-op patch is byte-identical.
        let same =
            card_patch_fishing_core(card.clone(), 1, 12345, 1, 2, 900, 5, 77, 0b101).expect("noop");
        assert_eq!(same, card);
        let patched = card_patch_fishing_core(card.clone(), 1, 99999, 2, 1, 5000, 9, 78, 0b111)
            .expect("patch");
        let json = card_fishing_json_core(&patched, 1).expect("read");
        assert!(json.contains(r#""points":99999"#), "{json}");
        assert!(json.contains(r#""casts":78"#), "{json}");
        // Only the fishing region changed.
        let base = legaia_save::BLOCK_SIZE;
        let diff: Vec<usize> = card
            .iter()
            .zip(patched.iter())
            .enumerate()
            .filter(|(_, (a, b))| a != b)
            .map(|(i, _)| i)
            .collect();
        assert!(!diff.is_empty());
        assert!(
            diff.iter()
                .all(|&i| (base + SC_FISHING_POINTS..base + SC_FISHING_PURCHASED + 4).contains(&i)),
            "only the fishing block may change: {diff:?}"
        );
    }
}
