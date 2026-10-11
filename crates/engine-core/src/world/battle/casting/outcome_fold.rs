//! Folding a resolved spell outcome into battle state.

use super::*;

impl World {
    /// Fold a single-target [`legaia_engine_battle::spells::SpellOutcome`] into live actor
    /// state and surface a HUD popup. Damage subtracts HP (and downs the
    /// target at zero); heals / revives add HP (capped); cures clear the
    /// target's status; buffs adjust a per-slot scalar with a turn timer
    /// ([`Self::apply_battle_buff`]); capture rolls vs the monster's weakened
    /// state ([`Self::resolve_capture`]); escape flags a return to the field
    /// ([`crate::world::BattleState::escaped`]). `Failed` is a no-op (MP already spent).
    pub(in crate::world) fn fold_spell_outcome(
        &mut self,
        outcome: legaia_engine_battle::spells::SpellOutcome,
    ) {
        use legaia_engine_battle::spells::SpellOutcome as O;
        match outcome {
            O::Damage { target, amount, .. } => {
                // The shared damage finisher fills the defender's spirit-art
                // gauge from any hit (magic included), and it does so on the
                // pre-nullify amount - retail charges the gauge before the
                // absorb stage, so a Stone target's absorbed cast still charges.
                self.accrue_spirit_gauge(target, amount);
                // A petrified target (Stone) absorbs the hit - no HP loss, like
                // the basic-attack path; Stone is invulnerable at every damage
                // entry point.
                let applied = if self.actor_is_petrified(target) {
                    0
                } else {
                    amount
                };
                // Retail applies a cast's HP through `FUN_801E09F8`'s
                // effect-child hit arm, whose clamp is the **safe** one: one
                // value clamped against live HP reaches both `+0x10` and
                // `+0x14C`, so the bar can never outrun HP. That arm also
                // picks the victim's reaction clip off `+0x1F2`.
                self.apply_effect_child_hit(target as usize, i32::from(applied));
                self.battle.hit_fx.push(BattleHitFx {
                    target_slot: target,
                    amount: applied,
                    is_heal: false,
                    is_crit: false,
                });
                if applied > 0 {
                    let survives = self
                        .actors
                        .get(target as usize)
                        .map(|a| a.battle.hp > 0)
                        .unwrap_or(false);
                    self.queue_battle_reaction(target as usize, survives);
                }
            }
            O::Heal { target, amount } => {
                self.apply_battle_hp_delta(target as usize, -i32::from(amount));
                if amount > 0 {
                    self.battle.hit_fx.push(BattleHitFx {
                        target_slot: target,
                        amount,
                        is_heal: true,
                        is_crit: false,
                    });
                }
            }
            O::Cure { target, .. } => {
                self.battle.status_effects.cure_all(target);
            }
            O::Revive { target, hp } => {
                if let Some(a) = self.actors.get_mut(target as usize) {
                    let before = a.battle.hp;
                    a.battle.hp = hp.min(a.battle.max_hp);
                    if a.battle.hp > 0 {
                        a.battle.liveness = 1;
                    }
                    // Retail's revive is `FUN_800402F4` selector 4, and that
                    // routine seeds the readout accumulator alongside its stat
                    // write. Skipping the seed leaves the pair `hp !=
                    // hp_display` with a zero accumulator, which the ramp's
                    // `+0x10 != 0` guard makes absorbing - and the action SM's
                    // `0x51` gate then parks on the revived member forever.
                    // REF: FUN_800402F4
                    let delta = i32::from(a.battle.hp) - i32::from(before);
                    if delta != 0 {
                        a.battle.assign_hp_bar(
                            delta.clamp(i32::from(i16::MIN), i32::from(i16::MAX)) as i16,
                        );
                    }
                    if before == 0 {
                        self.stand_revived_party_member(target as usize);
                    }
                }
                self.battle.hit_fx.push(BattleHitFx {
                    target_slot: target,
                    amount: hp,
                    is_heal: true,
                    is_crit: false,
                });
            }
            O::Buff {
                target,
                stat,
                magnitude,
                turns,
            } => {
                self.apply_battle_buff(target, stat, magnitude, turns);
            }
            O::CaptureRoll { target, hit_pct } => {
                self.resolve_capture(target, hit_pct);
            }
            O::Escape => {
                self.battle.escaped = true;
            }
            // Multi-target variants aren't produced by per-slot casts; Failed
            // is a no-op (MP was already spent up front).
            _ => {}
        }
    }
}
