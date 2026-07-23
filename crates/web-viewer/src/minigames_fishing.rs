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
    self, FightOutcome, FishingPhase, FishingRecord, FishingSession, ReelInput,
};

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
                self.fishing_species = Some(species);
                self.fishing_overlay = img;
                format!(r#"{{"ok":true,"species":{n}}}"#)
            }
            None => format!(
                r#"{{"ok":false,"why":{}}}"#,
                jstr("fishing overlay (PROT 0972) or its species table did not decode")
            ),
        }
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
