//! The dome opponent's turn pick: the ordinary battle's **monster AI**.
//!
//! A dome leg is an ordinary battle, so its opponent never reaches the deal /
//! commit paths the fighter's command flow runs. The action SM's turn-order
//! pass `FUN_801DABA4` calls the picker `FUN_801E9FD4(ctx[+0x274] - 3)` for
//! every actor slot at or past `3` (`sltiu v0,v0,0x3` at `0x801DAF2C`,
//! `jal 0x801e9fd4` at `0x801DAF48`). The picker:
//!
//! 1. rolls `rand() % (1 + live magic count)` over the record's
//!    `+0x21..=+0x23` magic ids - `0` is a physical strike, otherwise the
//!    pick is a cast when the monster's MP covers it, else a strike;
//! 2. rolls a party target (`rand() % party_count`, re-rolled onto a living
//!    member) for a strike or a one-enemy cast;
//! 3. runs its per-monster-id `switch` (`legaia_engine_battle::monster_ai::decide`)
//!    and the recent-target ring;
//! 4. on a strike, fills the monster's `+0x1DF` stream out of its AGL gauge
//!    from its own candidate swing entries
//!    (`legaia_engine_battle_vm::battle_action::enemy_action_budget`).
//!
//! The kernels are the regular battle's (`World::pick_monster_action` +
//! `World::arm_monster_strike_budget` run the same ones); this module is their
//! dome seat, over the session's one-member party and one-monster band, on
//! the damage model's `rand()` stream. The queued bytes are the archive
//! **entry indices** the picker writes into the stream, and a cast goes
//! through the session's own cast path with the monster's spells installed as
//! slot 1's loadout.

use super::*;
use legaia_engine_battle::spells::{SpellDef, SpellTarget};

/// The opponent's picker inputs: its monster definition (stats, magic ids,
/// AGL gauge and candidate swings) and the cast records its magic ids name.
#[derive(Debug, Clone)]
pub struct DomeOpponentAi {
    /// The monster's catalog definition
    /// (`legaia_engine_battle::monster_catalog::monster_def_from_record`).
    pub def: legaia_engine_battle::monster_catalog::MonsterDef,
    /// The cast definitions of [`Self::def`]'s magic ids, as the host's spell
    /// catalog resolves them; an id with no definition rolls as a strike, as
    /// an unaffordable one does.
    pub spells: Vec<SpellDef>,
    /// The per-monster-id `switch`'s cooldown / phase state, carried across
    /// turns.
    pub(super) state: legaia_engine_battle::monster_ai::MonsterAiState,
}

impl DomeOpponentAi {
    /// Picker inputs for `def` with the cast records `spells`.
    pub fn new(
        def: legaia_engine_battle::monster_catalog::MonsterDef,
        spells: Vec<SpellDef>,
    ) -> Self {
        Self {
            def,
            spells,
            state: Default::default(),
        }
    }
}

/// The dome's actor slots in the battle's convention: the fighter is party
/// slot `0` of a one-member party, the opponent the monster band's first slot.
const PARTY_COUNT: u8 = 1;
const OPPONENT_SLOT: u8 = 1;

impl MuscleDomeSession {
    /// Install the opponent's picker inputs. Its magic ids' cast records go
    /// in as slot 1's loadout (MP from the record), so a cast resolves through
    /// the session's shared cast rule; a monster has no Ra-Seru chip to gate.
    pub fn install_opponent_ai(&mut self, ai: DomeOpponentAi) {
        self.magic[1] = Some(DomeMagic {
            ring: DomeRing::default(),
            mp: ai.def.mp,
            mp_max: ai.def.mp,
            ability_bits: 0,
            magic_power: ai.def.intel,
            spells: ai.spells.clone(),
        });
        self.opponent_ai = Some(ai);
    }

    /// Whether the opponent picks through the monster AI
    /// ([`Self::install_opponent_ai`]).
    pub fn has_opponent_ai(&self) -> bool {
        self.opponent_ai.is_some()
    }

    /// The opponent's turn: the monster AI's pick, or - with no picker inputs
    /// installed - the greedy stand-in over the player's directions.
    pub(super) fn opponent_commit(&mut self) {
        if self.opponent_ai.is_some() {
            self.pick_opponent_action();
        } else {
            self.ai_commit_all(1);
        }
    }

    /// `FUN_801E9FD4` for the dome opponent: the generic core, the
    /// per-monster `switch`, the recent-target ring and, on a strike, the AGL
    /// budget fill. Leaves a cast in `cast[1]` or the queued entry indices in
    /// the opponent's queue.
    ///
    /// REF: FUN_801DABA4 (`0x801DAF48`, the call for a monster slot)
    /// REF: FUN_801E9FD4 (the kernels carry the port tags)
    fn pick_opponent_action(&mut self) {
        let Some(mut ai) = self.opponent_ai.take() else {
            return;
        };
        let mut rng = match self.damage.as_ref() {
            Some(m) => m.rng_seed(),
            None => self.auto.rng,
        };
        let mut draw = || legaia_engine_system::rand::world_rand(&mut rng);
        let mp = self.mp(1);
        let player_alive = self.f[0].hp > 0;
        // A party target roll: `rand() % party_count`, re-rolled until it
        // lands on a living member (`World::random_living_party_member`).
        let party_target = |draw: &mut dyn FnMut() -> u32| -> u8 {
            if player_alive {
                (draw() % u32::from(PARTY_COUNT)) as u8
            } else {
                OPPONENT_SLOT
            }
        };
        // --- the generic core: one roll over (1 + live magic count).
        let magic = ai.def.magic_attacks.clone();
        let roll = draw() % (1 + magic.len() as u32);
        let (mut category, mut spell_id) = (3u8, 0u8);
        let mut target_class;
        let picked = (roll != 0)
            .then(|| magic[(roll - 1) as usize])
            .and_then(|id| ai.spells.iter().find(|s| s.id == id).cloned())
            .filter(|def| mp >= u16::from(def.mp_cost));
        match picked {
            Some(def) => {
                category = 2;
                spell_id = def.id;
                target_class = match def.target {
                    SpellTarget::OneEnemy => party_target(&mut draw),
                    SpellTarget::AllEnemies => 8,
                    SpellTarget::AllAllies => 9,
                    // The band's only member is the caster; the
                    // most-weakened-ally scan finds no other.
                    SpellTarget::SelfOnly | SpellTarget::OneAlly => OPPONENT_SLOT,
                };
            }
            None => target_class = party_target(&mut draw),
        }
        // --- the per-monster-id `switch` and the recent-target ring.
        let ctx = legaia_engine_battle::monster_ai::MonsterAiCtx {
            monster_id: (ai.def.id & 0xFF) as u8,
            monster_index: 0,
            caster_slot: OPPONENT_SLOT,
            hp: self.f[1].hp.clamp(0, i32::from(u16::MAX)) as u16,
            max_hp: self.f[1].max_hp.clamp(0, i32::from(u16::MAX)) as u16,
            mp,
            party_count: PARTY_COUNT,
            monster_count: 1,
            field_flags: 0,
            allies_with_mp: u8::from(player_alive && self.mp(0) != 0),
            spirit_gauge: self.spirit(1),
            core_category: category,
        };
        let mut spirit_writeback = None;
        if let Some(cast) = legaia_engine_battle::monster_ai::decide(&ctx, &mut ai.state, &mut draw)
        {
            category = cast.category;
            spell_id = cast.spell_id;
            if !cast.keep_target {
                target_class = cast.target_class;
            }
            spirit_writeback = cast.spirit_gauge_writeback;
        }
        let _ = legaia_engine_battle::monster_ai::apply_recent_target_ring(
            target_class,
            spell_id,
            PARTY_COUNT,
            &mut ai.state,
            &mut draw,
        );
        // --- build the action: a cast the session can resolve, else the
        // physical branch's AGL budget over the candidate swing entries.
        let mut queue = Vec::new();
        let cast = (category == 2)
            .then_some(spell_id)
            .filter(|id| ai.spells.iter().any(|s| s.id == *id));
        if cast.is_none() && ai.def.agl > 0 && !ai.def.action_costs.is_empty() {
            let picks = legaia_engine_battle_vm::battle_action::enemy_action_budget(
                ai.def.agl,
                &ai.def.action_costs,
                &mut draw,
            );
            queue = picks
                .iter()
                .filter_map(|&p| ai.def.action_entries.get(usize::from(p)).copied())
                .collect();
            if ai.state.cut_strike_stream {
                // The `0xB3` arm's cut keeps the stream's first entry.
                queue.truncate(1);
                ai.state.cut_strike_stream = false;
            }
        }
        if cast.is_none() && queue.is_empty() {
            // No AGL gauge or no costed swing: one swing on the first
            // candidate entry, as the battle's single-swing fallback.
            queue.extend(ai.def.action_entries.first().copied());
        }
        match self.damage.as_mut() {
            Some(m) => m.set_rng_seed(rng),
            None => self.auto.rng = rng,
        }
        if let Some(g) = spirit_writeback
            && let Some(m) = self.damage.as_mut()
        {
            m.set_spirit(1, g);
        }
        let f = &mut self.f[1];
        f.queue.clear();
        f.spent = 0;
        f.budget = f.budget_pool;
        match cast {
            Some(id) => self.cast[1] = Some(id),
            None => {
                queue.truncate(QUEUE_CAP);
                self.f[1].queue = queue;
            }
        }
        self.opponent_ai = Some(ai);
    }
}

#[cfg(test)]
impl MuscleDomeSession {
    /// Seat the stand-in `rand()` cursor a session without a damage model
    /// draws from.
    pub(super) fn auto_rng_seed_for_tests(&mut self, seed: u32) {
        self.auto.rng = seed;
    }
}
