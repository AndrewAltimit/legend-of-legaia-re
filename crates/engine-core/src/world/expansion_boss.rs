//! The `World` side of expansion boss scripts
//! ([`legaia_engine_expansion::boss_script`]): a scripted pack monster's
//! turns, phase changes, summons, status immunities and last stand.
//!
//! Every hook here returns at once unless the current fight seats a monster
//! whose pack variant carries a script
//! ([`legaia_engine_expansion::battle::BattleRuntime::fight_scripts`] is
//! empty otherwise - always so with no pack), so a retail fight, and a pack
//! fight on unscripted monsters, never reaches the kernel.
//!
//! The hooks:
//!
//! - [`World::expansion_boss_pick`] - the top of the monster AI's pick
//!   (`World::pick_monster_action`): the script's move in place of the
//!   retail pick. A swing is the base monster's own archive entry, staged
//!   through the ordinary attack band ([`World::arm_monster_strike_budget`]
//!   reads it from `forced_strike`); a spell goes through the retail cast
//!   path. A telegraphed move's charge turn is [`MonsterAction::Wait`].
//! - [`World::expansion_boss_round_start`] - each round's start (beside the
//!   refill-wave step): phase checks, then the summons seat.
//! - [`World::expansion_survives_lethal`] - the HP writers' `hp == 0` edge.
//! - [`World::expansion_boss_frame`] - the battle frame's dead-marking
//!   sweep: immunities stripped, and the last stand's catch-all.

use std::sync::Arc;

use legaia_engine_battle::monster_catalog::FormationDef;
use legaia_engine_battle_vm::status_effects::StatusKind;
use legaia_engine_expansion::boss_script::{
    self as bs, BossRun, Candidate, Change, CompiledPhase, MoveKind, Observation, Pick,
};
use legaia_expansion_format::boss_script::StatusName;

use super::World;
use super::types::MonsterAction;

/// The ceiling a phase's stat change holds a live battle stat to: the
/// record cap doubled by the scripted fight's defence boost.
const LIVE_STAT_CAP: u16 = legaia_engine_expansion::battle::RECORD_STAT_CAP * 2;

/// How long a boss banner stands, in frames (the timed message's hold).
const BOSS_BANNER_HOLD: i32 = legaia_engine_battle_loop::message_banner::COUNTER_MESSAGE_HOLD;

fn status_kind(s: StatusName) -> StatusKind {
    match s {
        StatusName::Venom => StatusKind::Venom,
        StatusName::Toxic => StatusKind::Toxic,
        StatusName::Numb => StatusKind::Numb,
        StatusName::Sleep => StatusKind::Sleep,
        StatusName::Confuse => StatusKind::Confuse,
        StatusName::Rot => StatusKind::Rot,
        StatusName::Curse => StatusKind::Curse,
        StatusName::Stone => StatusKind::Stone,
    }
}

impl World {
    /// Arm the fight's boss scripts at its entry
    /// ([`Self::expansion_prepare_battle`]).
    pub(in crate::world) fn expansion_boss_prepare(&mut self, formation: &FormationDef) {
        let scripts = self
            .expansion
            .battle
            .compiled
            .as_ref()
            .and_then(|c| c.formation(formation.formation_id))
            .map(|f| f.scripts.clone())
            .unwrap_or_default();
        let b = &mut self.expansion.battle;
        b.fight_scripts = scripts;
        b.boss_runs.clear();
        b.pending_summons.clear();
        b.boss_banners.clear();
        b.phase_screen = Default::default();
        b.boss_round = 0;
        b.forced_strike = None;
        b.boss_layout = formation.slots.len().clamp(1, 4) as u8;
    }

    /// Drop the fight's script state ([`Self::expansion_end_battle`]).
    pub(in crate::world) fn expansion_boss_end(&mut self) {
        let b = &mut self.expansion.battle;
        b.fight_scripts.clear();
        b.boss_runs.clear();
        b.pending_summons.clear();
        b.boss_banners.clear();
        b.phase_screen = Default::default();
        b.boss_round = 0;
        b.forced_strike = None;
    }

    /// The scripted run of actor slot `slot`, started when the seat's
    /// monster fights under a scripted pack variant. `false` otherwise.
    fn ensure_boss_run(&mut self, slot: usize) -> bool {
        if self.expansion.battle.fight_scripts.is_empty() {
            return false;
        }
        let pc = usize::from(self.party.party_count.max(1));
        if slot < pc {
            return false;
        }
        let Some(id) = self.actors.get(slot).and_then(|a| a.battle_monster_id) else {
            return false;
        };
        let b = &mut self.expansion.battle;
        if b.boss_runs.get(&slot).is_some_and(|(i, _)| *i == id) {
            return true;
        }
        let Some(script) = b.fight_scripts.get(&id).cloned() else {
            b.boss_runs.remove(&slot);
            return false;
        };
        b.boss_runs.insert(slot, (id, BossRun::new(script)));
        true
    }

    /// The monster seats with a script run, started where due.
    fn boss_slots(&mut self) -> Vec<usize> {
        if self.expansion.battle.fight_scripts.is_empty() {
            return Vec::new();
        }
        let pc = usize::from(self.party.party_count.max(1));
        (pc..self.actors.len().min(8))
            .filter(|&s| self.ensure_boss_run(s))
            .collect()
    }

    /// What the script of `slot` sees now.
    fn boss_observation(&self, slot: usize) -> Observation {
        let (hp, max_hp) = self
            .actors
            .get(slot)
            .map(|a| (a.battle.hp, a.battle.max_hp))
            .unwrap_or((0, 0));
        // Monsters this fight has seated, less those standing (itself among
        // them): the allies it has lost.
        let seated = self
            .battle
            .active_formation
            .as_ref()
            .map_or(0, |f| f.slots.len());
        let standing = self
            .expansion_standing_seats()
            .iter()
            .filter(|&&s| s)
            .count();
        Observation {
            hp,
            max_hp,
            round: self.expansion.battle.boss_round.max(1),
            allies_down: seated.saturating_sub(standing).min(255) as u8,
        }
    }

    /// Run the phase checks of `slot` and apply what begins.
    fn boss_observe(&mut self, slot: usize) {
        let obs = self.boss_observation(slot);
        let Some((_, run)) = self.expansion.battle.boss_runs.get_mut(&slot) else {
            return;
        };
        let changes = run.observe(&obs);
        let phases: Vec<(Change, CompiledPhase)> = changes
            .into_iter()
            .filter_map(|c| Some((c, run.changed(c)?.clone())))
            .collect();
        for (c, p) in phases {
            self.apply_boss_change(slot, c, &p);
        }
    }

    /// The monster's display name this fight (its pack name).
    fn boss_name(&self, slot: usize) -> String {
        self.actors
            .get(slot)
            .and_then(|a| a.battle_monster_id)
            .and_then(|id| self.tables.monster_catalog.get(id))
            .map(|d| d.name.clone())
            .unwrap_or_default()
    }

    /// The banner text of pack dialog line `line`.
    fn boss_line_text(&self, line: &str) -> Option<String> {
        let pack = self.expansion.pack.as_ref()?;
        let key = legaia_expansion_format::battle::local_key(&pack.manifest.id, line)?;
        pack.dialog.lines.get(key).map(bs::banner_text)
    }

    /// A stock boss banner ([`bs::stock_banner`]): the pack's own dialog
    /// line `key` when it words one, else the engine's.
    fn boss_stock_banner(&self, key: &str, slot: usize) -> String {
        let dialog = self.expansion.pack.as_ref().map(|p| &p.dialog);
        bs::stock_banner(dialog, key, &self.boss_name(slot))
    }

    /// Raise a boss banner on the battle's timed-message line, or queue it
    /// behind the banner standing there ([`Self::expansion_boss_frame`]
    /// shows it once the line clears).
    fn boss_banner(&mut self, text: String) {
        if text.is_empty() {
            return;
        }
        log::info!("expansion boss: {text}");
        if self.battle.message_banner.is_some() {
            self.expansion.battle.boss_banners.push_back(text);
            return;
        }
        self.show_boss_banner(text);
    }

    fn show_boss_banner(&mut self, text: String) {
        self.battle.message_banner = Some(
            legaia_engine_battle_loop::message_banner::BattleMessageBanner {
                element: legaia_engine_battle_loop::message_banner::TIMED_MESSAGE_ELEMENT,
                text,
                hold: BOSS_BANNER_HOLD,
            },
        );
    }

    /// Apply the changes phase (or enrage) `p` makes as it begins on `slot`.
    fn apply_boss_change(&mut self, slot: usize, c: Change, p: &CompiledPhase) {
        let st = p.stats;
        let b = &mut self.battle;
        if st.attack.is_some() {
            b.attack[slot] = bs::apply_stat(b.attack[slot], st.attack, LIVE_STAT_CAP);
            b.attack_base[slot] = bs::apply_stat(b.attack_base[slot], st.attack, LIVE_STAT_CAP);
        }
        if st.defence.is_some() {
            let sc = |v: u16| bs::apply_stat(v, st.defence, LIVE_STAT_CAP);
            b.defense_split[slot] = b.defense_split[slot].map(|(u, l)| (sc(u), sc(l)));
            b.defense_base[slot] = b.defense_base[slot].map(|(u, l)| (sc(u), sc(l)));
            b.defense[slot] = sc(b.defense[slot]);
        }
        if st.speed.is_some() {
            b.speed[slot] = bs::apply_stat(b.speed[slot], st.speed, LIVE_STAT_CAP);
            b.speed_base[slot] = bs::apply_stat(b.speed_base[slot], st.speed, LIVE_STAT_CAP);
        }
        if st.intelligence.is_some() {
            let sc = |v: u16| bs::apply_stat(v, st.intelligence, LIVE_STAT_CAP);
            b.accuracy[slot] = sc(b.accuracy[slot]);
            b.evasion[slot] = sc(b.evasion[slot]);
            b.accuracy_base[slot] = sc(b.accuracy_base[slot]);
        }
        if let Some(a) = self.actors.get_mut(slot) {
            if st.agility.is_some() {
                a.battle.agl_base = bs::apply_stat(a.battle.agl_base, st.agility, LIVE_STAT_CAP);
                a.battle.agl = bs::apply_stat(a.battle.agl, st.agility, LIVE_STAT_CAP);
            }
            if let Some(e) = p.element {
                a.battle_element = Some(e);
            }
        }
        if let Some(pct) = p.heal_pct {
            let max = self.actors.get(slot).map_or(0, |a| a.battle.max_hp);
            let heal = (u32::from(max) * u32::from(pct) / 100).max(1);
            self.apply_battle_hp_delta(slot, -(heal.min(i32::MAX as u32) as i32));
        }
        self.expansion
            .battle
            .pending_summons
            .extend(p.summon.iter().copied());
        let text = match (&p.line, c) {
            (Some(l), _) => self.boss_line_text(l),
            (None, Change::Enrage) => Some(self.boss_stock_banner(bs::BANNER_ENRAGED, slot)),
            (None, Change::Phase(_)) => None,
        };
        if let Some(t) = text {
            self.boss_banner(t);
        }
        self.expansion_phase_audio(p.music.as_ref(), p.sound.as_ref());
        let screen = &mut self.expansion.battle.phase_screen;
        if let Some(t) = p.tint {
            screen.start_tint(t.color, t.frames, t.mode);
        }
        if let Some(f) = p.flash {
            screen.start_flash(f.color, f.frames);
        }
    }

    /// A round opens: the round count moves on, every scripted seat runs
    /// its phase checks, and the summons they made take free seats (ahead
    /// of the initiative seed, so they act this round). Nothing without a
    /// scripted monster in the fight.
    pub(in crate::world) fn expansion_boss_round_start(&mut self) {
        if self.expansion.battle.fight_scripts.is_empty() {
            return;
        }
        self.expansion.battle.boss_round = self.expansion.battle.boss_round.saturating_add(1);
        for slot in self.boss_slots() {
            if self.actors[slot].battle.hp > 0 {
                self.boss_observe(slot);
            }
        }
        self.expansion_seat_summons();
    }

    /// Seat the pending summons into free monster seats, widening the
    /// monster block when the seats laid out are full. A summon with no
    /// free seat is dropped.
    pub(in crate::world) fn expansion_seat_summons(&mut self) {
        let pending = std::mem::take(&mut self.expansion.battle.pending_summons);
        if pending.is_empty() {
            return;
        }
        let pc = usize::from(self.party.party_count.clamp(1, 3));
        let standing = self.expansion_standing_seats();
        let free: Vec<usize> = (0..standing.len())
            .filter(|&k| !standing[k] && pc + k < self.actors.len().min(7))
            .collect();
        if pending.len() > free.len() {
            log::info!(
                "expansion boss: {} summon(s) find no free seat",
                pending.len() - free.len()
            );
        }
        let placed: Vec<(usize, u16)> = free.into_iter().zip(pending).collect();
        if placed.is_empty() {
            return;
        }
        let layout = self
            .expansion
            .battle
            .waves
            .as_ref()
            .map_or(self.expansion.battle.boss_layout, |r| r.layout());
        let needed = placed.iter().map(|&(k, _)| k + 1).max().unwrap_or(1) as u8;
        let layout = if needed > layout {
            self.expansion_relayout_monsters(needed);
            if let Some(r) = self.expansion.battle.waves.as_mut() {
                r.note_layout(needed);
            }
            needed
        } else {
            layout
        };
        self.expansion.battle.boss_layout = layout;
        let scripted = self.battle.scripted_fight;
        for (k, id) in placed {
            let slot = pc + k;
            let seat = legaia_engine_battle::battle_seats::monster_seat(layout, k, scripted);
            self.seat_wave_monster(slot, k, id, seat, scripted);
            if let Some(f) = self.battle.active_formation.as_mut() {
                f.slots
                    .push(legaia_engine_battle::monster_catalog::FormationSlot::new(
                        id,
                    ));
            }
            self.ensure_boss_run(slot);
        }
    }

    /// The living party as the targeting rules read it.
    fn boss_candidates(&self) -> Vec<Candidate> {
        let pc = self.party.party_count.max(1);
        (0..pc)
            .filter(|&s| {
                self.actors
                    .get(usize::from(s))
                    .is_some_and(|a| a.battle.liveness != 0 && a.battle.hp > 0)
            })
            .map(|s| {
                let a = &self.actors[usize::from(s)];
                Candidate {
                    slot: s,
                    hp: a.battle.hp,
                    attack: self.battle.attack[usize::from(s)],
                    mp: a.battle.mp,
                    heals: self.party_member_heals(s),
                }
            })
            .collect()
    }

    /// Whether party member `slot` knows an ally-side spell.
    fn party_member_heals(&self, slot: u8) -> bool {
        use legaia_engine_battle::spells::SpellTarget;
        let char_slot = self.party_roster_slot(usize::from(slot));
        let Some(member) = self.party.roster.members.get(char_slot) else {
            return false;
        };
        let list = member.spell_list();
        let n = usize::from(list.count).min(list.ids.len());
        list.ids[..n]
            .iter()
            .chain(self.seru.log.learned_spells(char_slot as u8))
            .any(|&id| {
                self.tables.spell_catalog.get(id).is_some_and(|d| {
                    matches!(d.target, SpellTarget::OneAlly | SpellTarget::AllAllies)
                })
            })
    }

    /// The script's pick for monster seat `slot`, in place of the retail AI:
    /// `None` - the retail pick runs - for a seat with no script, a script
    /// with no moves standing, or a move the disc data cannot back (a swing
    /// index past the base monster's attack list, a spell id the catalog
    /// does not hold).
    pub(in crate::world) fn expansion_boss_pick(&mut self, slot: u8) -> Option<MonsterAction> {
        let s = usize::from(slot);
        if !self.ensure_boss_run(s) {
            return None;
        }
        self.boss_observe(s);
        let (id, mut run) = self.expansion.battle.boss_runs.remove(&s)?;
        let pick = run.pick(&mut || self.next_rand());
        self.expansion.battle.boss_runs.insert(s, (id, run));
        let m = match pick {
            Pick::Retail => return None,
            Pick::Charge(m) => {
                if let Some(s) = &m.charge_sound {
                    self.expansion_play_sound(s);
                }
                let text = m
                    .charge_line
                    .as_deref()
                    .and_then(|l| self.boss_line_text(l))
                    .unwrap_or_else(|| self.boss_stock_banner(bs::BANNER_CHARGING, s));
                self.boss_banner(text);
                return Some(MonsterAction::Wait);
            }
            Pick::Move(m) => m,
        };
        let cands = self.boss_candidates();
        match m.kind {
            MoveKind::Attack { index, hits } => {
                let entry = self
                    .tables
                    .monster_catalog
                    .get(id)
                    .and_then(|d| d.action_entries.get(usize::from(index)).copied());
                let Some(entry) = entry else {
                    log::warn!(
                        "expansion boss: monster {id} has no swing {index}; the retail pick runs"
                    );
                    return None;
                };
                let target = bs::choose_target(m.target, &cands, &mut || self.next_rand())
                    .or_else(|| self.first_living_opponent_of(slot))
                    .unwrap_or(slot);
                self.expansion.battle.forced_strike = Some(vec![entry; usize::from(hits)]);
                if let Some(a) = self.actors.get_mut(s) {
                    a.battle.action_category = 3;
                    a.battle.active_target = target;
                }
                Some(MonsterAction::Physical { target })
            }
            MoveKind::Spell(spell_id) => {
                let Some(def) = self.monster_cast_def(spell_id) else {
                    log::warn!(
                        "expansion boss: spell 0x{spell_id:02X} is not in the spell table; the \
                         retail pick runs"
                    );
                    return None;
                };
                let class = if def.target == legaia_engine_battle::spells::SpellTarget::OneEnemy {
                    bs::choose_target(m.target, &cands, &mut || self.next_rand()).unwrap_or(slot)
                } else {
                    self.monster_cast_target_class(slot, &def)
                };
                let targets = self.resolve_class_to_slots(slot, class);
                if targets.is_empty() {
                    return None;
                }
                if let Some(a) = self.actors.get_mut(s) {
                    a.battle.action_category = 2;
                    a.battle.params[0] = spell_id;
                    // `ignore_mp`: the band's `0x28` debit takes back what
                    // this adds, so the cast always goes and spends nothing
                    // (the retail-compare cast seed's own top-up).
                    if m.ignore_mp {
                        a.battle.mp = a.battle.mp.saturating_add(u16::from(def.mp_cost));
                    }
                }
                Some(MonsterAction::Cast { spell_id, targets })
            }
        }
    }

    /// A lethal blow lands on `slot`: `true` when its script's last stand
    /// holds it at 1 HP (once a fight; the banner rises). The caller writes
    /// the 1 HP. `false` - always so without a scripted monster - lets the
    /// blow kill.
    pub(in crate::world) fn expansion_survives_lethal(&mut self, slot: usize) -> bool {
        if !self.ensure_boss_run(slot) {
            return false;
        }
        let Some((_, run)) = self.expansion.battle.boss_runs.get_mut(&slot) else {
            return false;
        };
        let Some(line) = run.spend_last_stand() else {
            return false;
        };
        let text = line
            .as_deref()
            .and_then(|l| self.boss_line_text(l))
            .unwrap_or_else(|| self.boss_stock_banner(bs::BANNER_LAST_STAND, slot));
        self.boss_banner(text);
        true
    }

    /// The battle frame's boss sweep, ahead of the dead-marking pass:
    /// statuses a script is immune to are stripped (the tracker entry and
    /// the status word's bits), and a scripted monster driven to 0 HP by a
    /// writer outside the HP-delta path gets its last stand.
    pub(in crate::world) fn expansion_boss_frame(&mut self) {
        let frames = u32::from(self.clock.display_frame_step.max(1));
        self.expansion.battle.phase_screen.tick(frames);
        if self.expansion.battle.boss_runs.is_empty() {
            return;
        }
        if self.battle.message_banner.is_none()
            && let Some(text) = self.expansion.battle.boss_banners.pop_front()
        {
            self.show_boss_banner(text);
        }
        let runs: Vec<(
            usize,
            Arc<legaia_engine_expansion::boss_script::CompiledScript>,
        )> = self
            .expansion
            .battle
            .boss_runs
            .iter()
            .map(|(&s, (_, r))| (s, r.script.clone()))
            .collect();
        for (slot, script) in runs {
            for &st in &script.immune {
                let kind = status_kind(st);
                self.battle.status_effects.strip(slot as u8, kind);
                if let Some(a) = self.actors.get_mut(slot) {
                    a.battle.field_flags &= !kind.display_bit();
                }
            }
            let lethal = self
                .actors
                .get(slot)
                .is_some_and(|a| a.battle.max_hp > 0 && a.battle.hp == 0);
            if lethal && self.expansion_survives_lethal(slot) {
                let a = &mut self.actors[slot];
                a.battle.hp = 1;
                a.battle.liveness = 1;
            }
        }
    }
}
