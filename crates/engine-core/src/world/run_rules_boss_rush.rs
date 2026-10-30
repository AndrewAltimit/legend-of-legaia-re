//! Boss Rush on the `World`: the run state machine's world half
//! ([`legaia_engine_run_rules::boss_rush`]).
//!
//! The host enters each stage's scene the way it enters a New Game's opening
//! leg, then calls [`World::boss_rush_arm_stage`]; the fight runs as the
//! story's own (its row through the ordinary encounter transition, its
//! record's flag arm and music replayed as `--battle` replays them). The
//! battle's end moves the run on ([`World::boss_rush_battle_finished`]),
//! and the intermission's rows act here.

use legaia_engine_run_rules::boss_rush::{
    BossRushRun, Intermission, IntermissionRow, Phase, STAGE_PURSE, SUPPLIES, StageRef,
};

use super::World;

/// What a Boss Rush waits on the host for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BossRushDue {
    /// Enter this scene as an opening leg, then [`World::boss_rush_arm_stage`].
    EnterScene(&'static str),
    /// Arm the stage where the party stands.
    Arm,
}

impl World {
    /// Begin a Boss Rush on this (freshly New-Gamed) world: the picker party
    /// (Vahn, Noa, Gala in their starter gear), the run over every retail
    /// stage and `pack` stages (pack boss key, level). The host then enters
    /// the first stage's scene.
    pub fn begin_boss_rush(&mut self, pack: &[(String, u8)]) {
        if let Some(t) = self.tables.starting_party.clone() {
            self.seed_picker_party(&t, None);
        }
        self.toggles.live_gameplay_loop = true;
        self.run_rules.boss_rush = Some(BossRushRun::new(pack));
        self.run_rules.intermission = None;
    }

    /// What the run waits on the host for, while it waits on one
    /// (`Phase::Enter`).
    pub fn boss_rush_due(&self) -> Option<BossRushDue> {
        let run = self.run_rules.boss_rush.as_ref()?;
        if run.phase != Phase::Enter {
            return None;
        }
        Some(match run.current()? {
            s @ StageRef::Retail { .. } => BossRushDue::EnterScene(s.retail()?.scene),
            // A pack boss fights wherever the party stands.
            StageRef::Pack { .. } => BossRushDue::Arm,
        })
    }

    /// Raise every party member to `level` through the ordinary EXP path
    /// (growth, maxima rebuilt), then restore them.
    pub fn boss_rush_raise_party(&mut self, level: u8) {
        let n = self.party.roster.members.len().min(3);
        for slot in 0..n {
            for _ in 0..100 {
                let cur = self
                    .party
                    .level_up_tracker
                    .level
                    .get(slot)
                    .copied()
                    .unwrap_or(1);
                if cur >= level {
                    break;
                }
                let Some(next) = self.party.level_up_tracker.threshold_for(slot, cur) else {
                    break;
                };
                let have = self
                    .party
                    .level_up_tracker
                    .xp
                    .get(slot)
                    .copied()
                    .unwrap_or(0);
                if self
                    .credit_character_xp(slot as u8, next.saturating_sub(have).max(1))
                    .is_none()
                    && self.party.level_up_tracker.level.get(slot).copied() == Some(cur)
                {
                    break;
                }
            }
        }
        self.party.pending_level_up_banners.clear();
        self.party.current_level_up_banner = None;
        self.boss_rush_rest();
    }

    /// Restore every member's HP and MP.
    pub fn boss_rush_rest(&mut self) {
        for slot in 0..self.party.roster.members.len() {
            let rec = &mut self.party.roster.members[slot];
            let mut h = rec.hp_mp_sp();
            if h.hp_max == 0 {
                continue;
            }
            h.hp_cur = h.hp_max;
            h.mp_cur = h.mp_max;
            rec.set_hp_mp_sp(h);
            self.mirror_roster_hp_mp(slot);
        }
    }

    /// Arm the current stage's fight on the scene the host just entered.
    /// `false` (the run finishes, lost) when the scene does not hold the
    /// stage's row or the pack boss will not start.
    pub fn boss_rush_arm_stage(&mut self) -> bool {
        let Some(stage) = self
            .run_rules
            .boss_rush
            .as_ref()
            .and_then(|r| r.current().cloned())
        else {
            return false;
        };
        self.boss_rush_raise_party(stage.level());
        self.toggles.live_gameplay_loop = true;
        let started = match &stage {
            StageRef::Retail { .. } => {
                let Some(s) = stage.retail() else {
                    return false;
                };
                let row = u16::from(s.row);
                self.replay_scripted_battle_arm(row);
                self.replay_scripted_battle_score(row);
                self.force_encounter(row)
            }
            StageRef::Pack { boss, .. } => self.start_expansion_boss(boss),
        };
        let run = self.run_rules.boss_rush.as_mut().expect("checked above");
        if started {
            run.fight_started();
        } else {
            log::error!("boss rush: stage '{}' would not start", stage.name());
            run.phase = Phase::Fight;
            run.fight_ended(false);
            self.boss_rush_record_finish();
        }
        started
    }

    /// The battle's end: a won stage pays the purse and opens the
    /// intermission, a lost one ends the run. Called from the battle
    /// teardown; nothing outside a Boss Rush fight.
    pub(in crate::world) fn boss_rush_battle_finished(&mut self, won: bool) {
        let Some(run) = self.run_rules.boss_rush.as_mut() else {
            return;
        };
        if run.phase != Phase::Fight {
            return;
        }
        run.fight_ended(won);
        if won {
            let money = i64::from(self.party.money) + i64::from(STAGE_PURSE);
            self.party.money = money.min(i64::from(super::BATTLE_GOLD_CAP)) as i32;
        }
        match self.run_rules.boss_rush.as_ref().map(|r| r.phase.clone()) {
            Some(Phase::Intermission) => {
                self.run_rules.intermission = Some(Intermission::default());
            }
            Some(Phase::Finished { .. }) => self.boss_rush_record_finish(),
            _ => {}
        }
    }

    fn boss_rush_record_finish(&mut self) {
        if let Some(run) = self.run_rules.boss_rush.as_ref() {
            self.run_rules.record.record(run);
        }
        self.run_rules.intermission = None;
    }

    /// One frame of the Boss Rush clock (the host's frame step).
    pub fn boss_rush_tick(&mut self) {
        if let Some(run) = self.run_rules.boss_rush.as_mut() {
            run.tick();
        }
    }

    /// The finished run's result, `(cleared, stages won, frames)`.
    pub fn boss_rush_result(&self) -> Option<(bool, usize, u64)> {
        let run = self.run_rules.boss_rush.as_ref()?;
        match run.phase {
            Phase::Finished { cleared } => Some((cleared, run.won(), run.frames)),
            _ => None,
        }
    }

    /// Drop the Boss Rush (the host returns to the title after its result).
    pub fn end_boss_rush(&mut self) {
        self.run_rules.boss_rush = None;
        self.run_rules.intermission = None;
    }

    /// The item id a supply name resolves to on this disc.
    fn supply_item_id(&self, name: &str) -> Option<u8> {
        let scus = self.tables.scus.as_deref()?;
        let t = legaia_game_tables::item_names::ItemNameTable::from_scus(scus)?;
        (1..=u8::MAX).find(|&id| t.name(id) == Some(name))
    }

    /// Act on the intermission's selected row (the host's confirm). Returns
    /// whether the intermission closed (Next or Give up).
    pub fn boss_rush_intermission_confirm(&mut self) -> bool {
        let Some(row) = self.run_rules.intermission.as_ref().map(|m| m.selected()) else {
            return false;
        };
        let message = match row {
            IntermissionRow::Next => {
                if let Some(r) = self.run_rules.boss_rush.as_mut() {
                    r.continue_run();
                }
                self.run_rules.intermission = None;
                return true;
            }
            IntermissionRow::GiveUp => {
                if let Some(r) = self.run_rules.boss_rush.as_mut() {
                    r.give_up();
                }
                self.boss_rush_record_finish();
                return true;
            }
            IntermissionRow::Rest => {
                self.boss_rush_rest();
                "The party is rested.".to_string()
            }
            IntermissionRow::Buy(i) => {
                let (name, price) = SUPPLIES[i];
                match self.supply_item_id(name) {
                    _ if self.party.money < price as i32 => "Not enough gold.".to_string(),
                    None => format!("{name} is not on this disc."),
                    Some(id) => {
                        if self.party.inventory.add(id, 1) {
                            self.party.money -= price as i32;
                            format!("Bought {name}.")
                        } else {
                            "The bag is full.".to_string()
                        }
                    }
                }
            }
        };
        if let Some(m) = self.run_rules.intermission.as_mut() {
            m.message = Some(message);
        }
        false
    }

    /// Move the intermission cursor.
    pub fn boss_rush_intermission_move(&mut self, dir: i32) {
        if let Some(m) = self.run_rules.intermission.as_mut() {
            m.move_cursor(dir);
        }
    }
}
