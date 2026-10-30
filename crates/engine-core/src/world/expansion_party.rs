//! A party night played alone on the `World`
//! ([`legaia_engine_expansion::party::LocalNight`]): the player against
//! the night's AI rivals, entered like any mode instance
//! ([`World::enter_pack_mode`] with a night's id - a map trigger's `mode`
//! event, a dialogue choice) and ticked under [`SceneMode::Expansion`].
//!
//! Between rounds the mode seat is empty and the screen is the night's
//! party card (intro, scoreboard, the last-place pick, the podium). A
//! round plays on the seat like any instance; its results stand a beat,
//! then the night scores them and the seat closes. The round itself grants
//! nothing (a poker round's chips are only points); a completed night
//! grants the player the prize for their final place. A Grid Duel match
//! between two rivals is played out at once, AI against AI.
//!
//! Start on a party card ends the night early (no prize); any button on
//! the finished podium closes it.

use legaia_engine_expansion::modes::{ModeStage, PackModeSeat};
use legaia_engine_expansion::party::LocalNight;
use legaia_engine_mode::mode::SceneMode;
use legaia_engine_modes::active::{ActiveMode, ModeKind};
use legaia_engine_modes::party::{
    PartyCue, PartyNight, PartyPhase, PlayerKey, round_section, solo_players,
};
use legaia_engine_modes::session::{Placement, SeatKind};
use legaia_engine_system::pad::PadButton;

use super::World;

/// Frames a finished round's results stand before the night scores them.
pub const PARTY_RESULT_HOLD: u32 = 3 * 60;
/// Frames an AI-against-AI Grid Duel match may run before it is called a
/// draw.
const AI_DUEL_FRAMES: u32 = 60 * 60 * 5;

/// What one frame of the night did with the frame.
enum Step {
    /// The night handled the frame (a party card, results standing).
    Handled,
    /// A round plays on the seat: the seat ticks as usual.
    PassThrough,
    /// The night is done: close it.
    Close,
}

/// Rank the night players' places among themselves (an AI seat with no
/// player that beats them takes no place of theirs).
pub fn rank_players(rows: &[(PlayerKey, u8)]) -> Vec<(PlayerKey, u8)> {
    rows.iter()
        .map(|&(k, place)| {
            let better = rows.iter().filter(|(_, p)| *p < place).count();
            (k, better as u8 + 1)
        })
        .collect()
}

impl World {
    /// Whether `id` names a party night of the mode section in use.
    pub fn is_party_night(&self, id: &str) -> bool {
        self.pack_mode_sections().0.party_night(id).is_some()
    }

    /// Open party night `id` played alone: the player and the night's AI
    /// rivals. `false` (with a log line) when the night will not build.
    pub fn enter_party_night(&mut self, id: &str) -> bool {
        let (modes, _) = self.pack_mode_sections();
        let Some(def) = modes.party_night(id) else {
            return false;
        };
        let players = solo_players(def, "You");
        let seed = self.pack_mode_seed(id);
        let night = match PartyNight::new(def, modes, &players, seed) {
            Ok(n) => n,
            Err(e) => {
                log::warn!("expansion: party night `{id}`: {e}");
                return false;
            }
        };
        let return_mode = self.pack_mode_return();
        self.expansion.mode_seat = None;
        self.expansion.party = Some(Box::new(LocalNight::new(night, return_mode)));
        self.mode = SceneMode::Expansion;
        true
    }

    /// The night played alone, while one runs.
    pub fn local_party(&self) -> Option<&LocalNight> {
        self.expansion.party.as_deref()
    }

    /// One frame of a night played alone. `true` when it took the frame;
    /// `false` while a round plays on the seat (the seat ticks it).
    pub(in crate::world) fn tick_local_party(&mut self) -> bool {
        let Some(mut lp) = self.expansion.party.take() else {
            return false;
        };
        lp.frame = lp.frame.wrapping_add(1);
        match self.step_local_party(&mut lp) {
            Step::Handled => {
                self.expansion.party = Some(lp);
                true
            }
            Step::PassThrough => {
                self.expansion.party = Some(lp);
                false
            }
            Step::Close => {
                self.close_party_seat();
                self.mode = lp.return_mode;
                true
            }
        }
    }

    /// Close a round's seat without leaving `SceneMode::Expansion`.
    fn close_party_seat(&mut self) {
        if let Some(seat) = self.expansion.mode_seat.take() {
            self.restore_pack_mode_player_from(seat.player_return);
            self.restore_pack_mode_bodies(&seat.body_idle);
            self.restore_pack_mode_headings(&seat.body_heading);
        }
    }

    fn step_local_party(&mut self, lp: &mut LocalNight) -> Step {
        // A round on the seat.
        if let Some(seat) = self.expansion.mode_seat.as_deref() {
            match &seat.stage {
                ModeStage::Results {
                    completed: true,
                    rows,
                    ..
                } => {
                    lp.finished_for += 1;
                    if lp.finished_for < PARTY_RESULT_HOLD {
                        return Step::Handled;
                    }
                    let rows = rows.clone();
                    self.close_party_seat();
                    record_round(lp, &rows);
                    return Step::Handled;
                }
                ModeStage::Results { .. } => {
                    // Quit through the quit prompt: the round is void.
                    self.close_party_seat();
                    lp.night.void_round();
                    return Step::Handled;
                }
                _ => return Step::PassThrough,
            }
        }
        let pressed = self.input.pad() & !self.input.pad_prev();
        let hit = |b: PadButton| pressed & b.mask() != 0;
        if lp.night.is_over() {
            if hit(PadButton::Cross) || hit(PadButton::Circle) || hit(PadButton::Start) {
                return Step::Close;
            }
            return Step::Handled;
        }
        if hit(PadButton::Start) && !lp.night.completed() {
            lp.night.end_early();
            return Step::Handled;
        }
        if let PartyPhase::Pick { picker: 0, .. } = lp.night.phase {
            let step = i32::from(hit(PadButton::Down) || hit(PadButton::Right))
                - i32::from(hit(PadButton::Up) || hit(PadButton::Left));
            lp.night.pick(0, step, hit(PadButton::Cross));
        }
        match lp.night.tick() {
            PartyCue::Start { slot } => self.start_local_round(lp, slot),
            PartyCue::Podium => self.grant_local_prize(lp),
            PartyCue::Over | PartyCue::None => {}
        }
        Step::Handled
    }

    /// Build round `slot` and seat it (a rivals-only Grid Duel match plays
    /// out at once instead).
    fn start_local_round(&mut self, lp: &mut LocalNight, slot: usize) {
        let info = lp.night.infos[slot].clone();
        let players = lp.night.players.len();
        let built = {
            let (modes, cards) = self.pack_mode_sections();
            round_section(modes, &lp.night.rounds[slot], players)
                .map(|s| (s, cards.clone()))
                .map_err(|e| e.to_string())
        };
        let (section, cards) = match built {
            Ok(b) => b,
            Err(e) => {
                log::warn!("expansion: party round `{}`: {e}", info.mode);
                lp.night.void_round();
                return;
            }
        };
        let mut opts = self.pack_mode_build_options();
        opts.golf_ai = vec![lp.night.ai_tier.into(); players.saturating_sub(1)];
        let seed = self.pack_mode_seed(&info.mode) ^ (lp.night.history.len() as u64) << 32;
        let maps = self.pack_mode_maps();
        let mut mode =
            match ActiveMode::build_with_maps(&section, &cards, &maps, &info.mode, seed, &opts) {
                Ok(m) => m,
                Err(e) => {
                    log::warn!("expansion: party round `{}`: {e}", info.mode);
                    lp.night.void_round();
                    return;
                }
            };
        lp.finished_for = 0;
        if info.kind == ModeKind::CardDuel {
            let Some((a, b)) = lp.night.current_match() else {
                lp.night.void_round();
                return;
            };
            if a != 0 && b != 0 {
                // Two rivals: play it out now.
                mode.set_seat_kind(0, SeatKind::Ai(lp.night.ai_tier.into()));
                for _ in 0..AI_DUEL_FRAMES {
                    if mode.is_finished() {
                        break;
                    }
                    mode.tick(&[0, 0]);
                }
                let winner = mode
                    .results()
                    .and_then(|r| duel_winner(&r.placements, &[a, b]));
                lp.night.record_duel(winner);
                return;
            }
            lp.seat_keys = vec![0, if a == 0 { b } else { a }];
        } else {
            lp.seat_keys = (0..players as PlayerKey).collect();
        }
        let source = self.mode_sections().0;
        let seat = PackModeSeat::playing(lp.return_mode, source, mode).with_maps(maps);
        self.expansion.mode_seat = Some(Box::new(seat));
        self.remember_pack_mode_player();
    }

    fn grant_local_prize(&mut self, lp: &mut LocalNight) {
        if lp.granted.is_some() {
            return;
        }
        let won = lp
            .night
            .standings()
            .first()
            .is_some_and(|s| s.key == 0 && s.place == 1);
        let lines = match lp.night.prize_for(0).cloned() {
            Some(prize) => self.grant_party_prize(&lp.night.id, &prize, won),
            None => Vec::new(),
        };
        lp.granted = Some(lines);
    }
}

/// The unique first-placed seat's player, by `keys` (seat -> key).
fn duel_winner(rows: &[Placement], keys: &[PlayerKey]) -> Option<PlayerKey> {
    let firsts: Vec<&Placement> = rows.iter().filter(|r| r.place == 1).collect();
    match firsts.as_slice() {
        [one] => keys.get(one.seat).copied(),
        _ => None,
    }
}

/// Score a finished round's results into the night.
fn record_round(lp: &mut LocalNight, rows: &[Placement]) {
    let duel = lp
        .night
        .current_slot()
        .is_some_and(|s| lp.night.infos[s].kind == ModeKind::CardDuel);
    if duel {
        let winner = duel_winner(rows, &lp.seat_keys);
        lp.night.record_duel(winner);
        return;
    }
    let mine: Vec<(PlayerKey, u8)> = rows
        .iter()
        .filter_map(|r| Some((*lp.seat_keys.get(r.seat)?, r.place)))
        .collect();
    lp.night.record(&rank_players(&mine));
}
