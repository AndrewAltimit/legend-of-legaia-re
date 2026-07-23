//! Muscle Dome card-battle methods of [`LegaiaMinigames`] - the browser twin
//! of the play-window's `start_muscle_minigame` (`window/minigames.rs`).
//!
//! The rules are the ported [`legaia_engine_core::muscle_dome`] engine: the
//! four-slot hand deal, the point-budget card commit into the fighter's action
//! queue, the HP-ratio score readout and the win/lose bookkeeping, driven by
//! the deck command-id table decoded from the visitor's disc (PROT 0898 rodata,
//! [`legaia_asset::muscle_dome`]). This file is the thin JSON shell over it.
//!
//! Two things are host models, not disc data, exactly as in the native launcher
//! (documented at their sites in `docs/subsystems/minigame-muscle-dome.md`):
//! the per-card **cost** (the browser has no player battle file to read the
//! per-command `+0x74` swing bytes from, so it uses the native fallback's flat
//! favored cost), and the per-card **damage** resolution (a battle-path
//! stand-in - the same `(atk - def)` stand-in the native `tick_muscle_dome`
//! uses, since the full battle-action playback is a host concern).

use super::*;

use legaia_asset::muscle_dome as md;
use legaia_engine_core::muscle_dome::{MuscleCard, MuscleDomeSession, MusclePhase};

/// Flat per-card cost (the native launcher's `FAVORED_COST` fallback, used when
/// the lead's per-command swing costs can't be read).
const WEB_CARD_COST: u16 = 0x1E;
/// Per-fighter round-budget pool (`+0x154` stand-in). The player pool is set
/// below the four-card total so the point budget is a real choice; the opponent
/// gets a shorter pool.
const WEB_BUDGET_POOL: [u16; 2] = [90, 70];
/// Per-fighter starting HP (`+0x14c`/`+0x14e` stand-in). Also the score-bar
/// max, surfaced to the page as `hp_max`.
const WEB_HP: [i32; 2] = [500, 400];
/// The Seru index awarded on a win (`ctx+0x269`); reward spell id is
/// `REWARD_SPELL_ID_BASE + index`.
const WEB_REWARD_SERU: u8 = 1;

/// The card-damage stand-in, matching the native `tick_muscle_dome`
/// constants: player deals `PLAYER_ATK - OPPONENT_DEF`, opponent deals
/// `OPPONENT_ATK - PLAYER_DEF`, each floored at 1.
fn web_card_damage(attacker: usize) -> i32 {
    const PLAYER_ATK: i32 = 60;
    const OPPONENT_ATK: i32 = 50;
    const PLAYER_DEF: i32 = 20;
    const OPPONENT_DEF: i32 = 15;
    if attacker == 0 {
        (PLAYER_ATK - OPPONENT_DEF).max(1)
    } else {
        (OPPONENT_ATK - PLAYER_DEF).max(1)
    }
}

impl LegaiaMinigames {
    /// Decode the battle overlay (PROT 0898) into the cached hand command-id
    /// table, returning the status object `load_disc` folds into its report.
    pub(super) fn load_muscle_tables(&mut self) -> String {
        self.muscle = None;
        self.muscle_hand = None;
        let hand = overlay_image(
            &self.prot,
            &self.entries,
            md::MUSCLE_OVERLAY_PROT_INDEX as u32,
        )
        .and_then(|img| md::hand_command_ids(&img));
        match hand {
            Some(cmds) => {
                self.muscle_hand = Some(cmds);
                let list = cmds
                    .iter()
                    .map(|c| c.to_string())
                    .collect::<Vec<_>>()
                    .join(",");
                format!(r#"{{"ok":true,"cards":[{list}]}}"#)
            }
            None => format!(
                r#"{{"ok":false,"why":{}}}"#,
                jstr("Muscle Dome battle overlay (PROT 0898) or its hand table did not decode")
            ),
        }
    }
}

#[wasm_bindgen]
impl LegaiaMinigames {
    /// Start a Muscle Dome contest on the disc's dealt hand, beginning in the
    /// selection phase. Returns `false` when the hand table didn't decode.
    pub fn muscle_start(&mut self) -> bool {
        let Some(cmds) = self.muscle_hand else {
            return false;
        };
        let player_hand = std::array::from_fn(|i| MuscleCard {
            command_id: cmds[i],
            cost: WEB_CARD_COST,
        });
        let opp_hand = player_hand;
        self.muscle = Some(MuscleDomeSession::new(
            player_hand,
            opp_hand,
            WEB_BUDGET_POOL,
            WEB_HP,
            WEB_REWARD_SERU,
        ));
        true
    }

    /// Commit one of the player's four hand cards (0..4) into the action queue,
    /// debiting the budget. Returns `false` when it can't be committed
    /// (overspend, queue full, or outside the selection phase).
    pub fn muscle_commit(&mut self, card_slot: usize) -> bool {
        self.muscle
            .as_mut()
            .is_some_and(|s| s.commit_card(0, card_slot))
    }

    /// Run the opponent's greedy in-order commit (the host AI model), then
    /// close the selection phase so the round is ready to resolve.
    pub fn muscle_end_selection(&mut self) {
        if let Some(s) = self.muscle.as_mut() {
            s.ai_commit_all(1);
            s.end_selection();
        }
    }

    /// Play the round out through the card-damage stand-in. No-op unless the
    /// round is in the resolve phase (i.e. after [`Self::muscle_end_selection`]).
    pub fn muscle_resolve(&mut self) {
        if let Some(s) = self.muscle.as_mut() {
            s.resolve_round(|attacker, _cmd| web_card_damage(attacker));
        }
    }

    /// Start the next round after a non-terminal resolution: reseed budgets,
    /// clear queues. No-op unless the contest is at a round break.
    pub fn muscle_next_round(&mut self) {
        if let Some(s) = self.muscle.as_mut() {
            s.next_round();
        }
    }

    /// Live contest state.
    ///
    /// ```json
    /// { "live": true, "phase": "select"|"resolve"|"round_over"|"won"|"lost",
    ///   "round": 0, "hp": [500, 400], "hp_max": [500, 400],
    ///   "budget": [90, 70], "spent": [0, 0], "score": [108, 108],
    ///   "queue": [[12], []], "last_damage": [0, 0],
    ///   "hand": [ { "cmd": 12, "cost": 30 }, ... ], "reward_spell": 129 }
    /// ```
    ///
    /// `score` is the retail `hp * 0x6c / max_hp` readout; `hand` is the
    /// player's four dealt cards; `reward_spell` is the spell id awarded on a
    /// win (an id into the shared spell-name table's player Seru-magic block).
    pub fn muscle_state_json(&self) -> String {
        let Some(s) = self.muscle.as_ref() else {
            return r#"{"live":false}"#.to_string();
        };
        let phase = match s.phase() {
            MusclePhase::Select => "select",
            MusclePhase::Resolve => "resolve",
            MusclePhase::RoundOver => "round_over",
            MusclePhase::Won => "won",
            MusclePhase::Lost => "lost",
        };
        let queue = |slot: usize| {
            s.queue(slot)
                .iter()
                .map(|c| c.to_string())
                .collect::<Vec<_>>()
                .join(",")
        };
        let hand = s.hand(0);
        let hand_json = hand
            .iter()
            .map(|c| format!(r#"{{"cmd":{},"cost":{}}}"#, c.command_id, c.cost))
            .collect::<Vec<_>>()
            .join(",");
        let dmg = s.last_round_damage();
        format!(
            concat!(
                r#"{{"live":true,"phase":{},"round":{},"hp":[{},{}],"hp_max":[{},{}],"#,
                r#""budget":[{},{}],"spent":[{},{}],"score":[{},{}],"queue":[[{}],[{}]],"#,
                r#""last_damage":[{},{}],"hand":[{}],"reward_spell":{}}}"#
            ),
            jstr(phase),
            s.round(),
            s.hp(0),
            s.hp(1),
            WEB_HP[0],
            WEB_HP[1],
            s.budget(0),
            s.budget(1),
            s.spent(0),
            s.spent(1),
            s.score_percent(0),
            s.score_percent(1),
            queue(0),
            queue(1),
            dmg[0],
            dmg[1],
            hand_json,
            s.reward_spell_id(),
        )
    }
}
