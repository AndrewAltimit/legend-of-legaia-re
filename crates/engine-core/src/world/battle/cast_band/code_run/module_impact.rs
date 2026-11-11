//! The slot-B modules' hit-tint stores, applied on the arm a module tick ran
//! (`legaia_engine_cast::cast_module_impact`).

use super::*;

impl World {
    /// Apply every store `entry`'s `body` makes on `arm` this tick. `passed`
    /// is whether the arm's gate let it through, `countdown` the module
    /// countdown the tick opened on.
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
    ) {
        use legaia_engine_cast::cast_module_impact::{ImpactSeat, module_impacts_due};
        let party = self.party.party_count.clamp(1, 3);
        for m in module_impacts_due(entry, body, arm, passed, countdown) {
            let seats: Vec<u8> = match m.seat {
                ImpactSeat::Victim => vec![victim],
                ImpactSeat::Caster => vec![caster],
                ImpactSeat::Creature => vec![creature],
                ImpactSeat::Party => (0..party).collect(),
                ImpactSeat::LiveParty => (0..party)
                    .filter(|&s| {
                        self.actors
                            .get(usize::from(s))
                            .is_some_and(|a| a.active && a.battle.hp != 0)
                    })
                    .collect(),
                ImpactSeat::LiveMonsters => (party..party + 4)
                    .filter(|&s| {
                        self.actors.get(usize::from(s)).is_some_and(|a| {
                            a.active && a.battle_monster_id.is_some() && a.battle.hp != 0
                        })
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
