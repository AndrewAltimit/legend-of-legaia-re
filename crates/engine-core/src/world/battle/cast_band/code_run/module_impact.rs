//! The slot-B modules' hit-tint stores, applied on the arm a module tick ran
//! (`legaia_engine_cast::cast_module_impact`).

use super::*;

/// The seat state a hit loop tests, read as the tick opens.
#[derive(Debug, Clone, Copy, Default)]
pub(in crate::world::battle::cast_band) struct ImpactOpen {
    /// The latched victim stood in Stone.
    pub victim_stoned: bool,
    /// Each monster seat `party + k` stood living, shown (`+0x21C == 0`) and
    /// out of Stone.
    pub monster_shown: [bool; 4],
}

impl World {
    /// [`ImpactOpen`] as the tick opens.
    pub(in crate::world::battle::cast_band) fn impact_open(&self, victim: u8) -> ImpactOpen {
        let party = usize::from(self.party.party_count.clamp(1, 3));
        ImpactOpen {
            victim_stoned: self.battle.actor_is_petrified(victim),
            monster_shown: std::array::from_fn(|k| {
                let s = party + k;
                self.actors.get(s).is_some_and(|a| {
                    a.active
                        && a.battle_monster_id.is_some()
                        && a.battle.hp != 0
                        && a.battle.render_flag == 0
                }) && !self.battle.actor_is_petrified(s as u8)
            }),
        }
    }

    /// Apply every store `entry`'s `body` makes on `arm` this tick. `passed`
    /// is whether the arm's gate let it through, `countdown` the module
    /// countdown after the tick's drain: PROT 0950's arm 3 tests the word it
    /// has just stored (`sw` at `0x801F7ED8`, `lw` at `0x801F7F10` /
    /// `0x801F7F30`), so a store read off the word the tick opened on lands a
    /// tick late.
    #[allow(clippy::too_many_arguments)]
    pub(in crate::world::battle::cast_band) fn apply_module_impacts(
        &mut self,
        entry: u32,
        body: u32,
        arm: u8,
        passed: bool,
        countdown: i32,
        caster: u8,
        victim: u8,
        creature: u8,
        party_hittable: [bool; 3],
        opened: ImpactOpen,
    ) {
        use legaia_engine_cast::cast_module_impact::{ImpactSeat, module_impacts_due};
        // PROT 0916's arm `0x12` store sits on its damage path
        // (`0x801F8644..0x801F868C`), which a kill verdict branches past
        // (`beqz` on `0x801F9010` at `0x801F8598`).
        if entry == legaia_engine_cast::cast_module_camera::ALURU_ENTRY
            && arm == legaia_engine_cast::cast_module_camera::ALURU_HIT_ARM
            && self.casting.module_cam.kill == Some(true)
        {
            return;
        }
        let party = self.party.party_count.clamp(1, 3);
        for m in module_impacts_due(entry, body, arm, passed, countdown) {
            let seats: Vec<u8> = match m.seat {
                ImpactSeat::Victim => vec![victim],
                ImpactSeat::UnstonedVictim => {
                    if opened.victim_stoned {
                        Vec::new()
                    } else {
                        vec![victim]
                    }
                }
                ImpactSeat::ShownOtherMonsters => (party..party + 4)
                    .filter(|&s| s != victim && opened.monster_shown[usize::from(s - party)])
                    .collect(),
                ImpactSeat::Caster => vec![caster],
                ImpactSeat::Creature => vec![creature],
                ImpactSeat::Party => (0..party).collect(),
                ImpactSeat::LiveParty => (0..party)
                    .filter(|&s| party_hittable[usize::from(s)])
                    .collect(),
                ImpactSeat::LiveMonsters | ImpactSeat::LiveUnstonedMonsters => (party..party + 4)
                    .filter(|&s| {
                        self.actors.get(usize::from(s)).is_some_and(|a| {
                            a.active && a.battle_monster_id.is_some() && a.battle.hp != 0
                        })
                    })
                    .filter(|&s| {
                        m.seat != ImpactSeat::LiveUnstonedMonsters
                            || self.raw_status_word(s)
                                & legaia_engine_battle_vm::status_effects::display_flags::STONE
                                == 0
                    })
                    .collect(),
            };
            for s in seats {
                let Some(a) = self.actors.get_mut(usize::from(s)) else {
                    continue;
                };
                if let Some(c) = m.colour {
                    a.battle.render_color = c;
                }
                if m.full_blend {
                    a.battle.render_blend =
                        legaia_engine_battle_vm::battle_formulas::TINT_BLEND_FULL;
                }
                if let Some(state) = m.tint_state {
                    a.battle.render_flag = state;
                }
                if let Some(selector) = m.selector {
                    a.battle.impact_state = selector;
                }
            }
        }
    }
}
