//! Spell catalog and pure-functional cast resolver.
//!
//! Tactical Arts are character-driven attack chains; spells are the
//! magic-side of the same command menu (ActionConstant `Magic = 0x02`,
//! followed by a per-character spell list). This module mirrors the shape
//! of [`crate::items`]: a typed catalog keyed by spell id, plus a pure
//! [`cast_spell`] function that consumes a [`SpellSnapshot`] and returns
//! a [`SpellOutcome`].
//!
//! The retail engine threads spell casts through:
//!   - `BattleActionHost::spell_mp_cost(id)` - MP gate (already exposed).
//!   - `BattleActionHost::is_capture_spell(id)` - Capture branch flag.
//!   - `BattleActionHost::spell_anim_trigger(party_slot, spell_id)` -
//!     fires `BattleEvent::SpellAnimTrigger`.
//!   - `BattleActionHost::apply_damage(...)` - for offensive spells.
//!
//! Engines that already drive their own per-spell anim list don't need
//! this catalog; it's a *minimum-viable* spell system that can be wired
//! end-to-end without an overlay capture for the per-spell base power
//! tables. Engines populate the catalog from disc data when the
//! per-character spell list is known.

use legaia_engine_vm::status_effects::StatusKind;
use std::collections::HashMap;

/// Magic element. Matches the eight retail elemental columns plus a
/// neutral catch-all for spells that aren't explicitly elemental.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum SpellElement {
    #[default]
    Neutral,
    Fire,
    Water,
    Wind,
    Thunder,
    Ice,
    Earth,
    Light,
    Dark,
}

impl SpellElement {
    /// Per-element type-effectiveness multiplier against a target with the
    /// listed weakness flags. The retail engine maps each element to a
    /// 4-bit mask; matching elements amplify damage by 1.5x. Engines that
    /// don't model weaknesses pass [`ElementMask::empty`] for a 1.0x
    /// passthrough.
    pub fn multiplier_against(self, weak: ElementMask) -> f32 {
        if weak.contains(self) { 1.5 } else { 1.0 }
    }
}

/// Bitmask of element weaknesses on a target. Engines populate from the
/// monster record's elemental flags.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ElementMask(pub u16);

impl ElementMask {
    pub const fn empty() -> Self {
        Self(0)
    }

    pub fn contains(self, element: SpellElement) -> bool {
        let bit = match element {
            SpellElement::Neutral => return false,
            SpellElement::Fire => 1 << 0,
            SpellElement::Water => 1 << 1,
            SpellElement::Wind => 1 << 2,
            SpellElement::Thunder => 1 << 3,
            SpellElement::Ice => 1 << 4,
            SpellElement::Earth => 1 << 5,
            SpellElement::Light => 1 << 6,
            SpellElement::Dark => 1 << 7,
        };
        (self.0 & bit) != 0
    }

    pub fn with(mut self, element: SpellElement) -> Self {
        let bit = match element {
            SpellElement::Neutral => 0,
            SpellElement::Fire => 1 << 0,
            SpellElement::Water => 1 << 1,
            SpellElement::Wind => 1 << 2,
            SpellElement::Thunder => 1 << 3,
            SpellElement::Ice => 1 << 4,
            SpellElement::Earth => 1 << 5,
            SpellElement::Light => 1 << 6,
            SpellElement::Dark => 1 << 7,
        };
        self.0 |= bit;
        self
    }
}

/// What slot the spell hits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SpellTarget {
    /// Single ally (party-side, including caster).
    OneAlly,
    /// Every ally.
    AllAllies,
    /// Single enemy. **Default** because most spells in retail have this
    /// shape - engines can override per spell.
    #[default]
    OneEnemy,
    /// Every enemy.
    AllEnemies,
    /// Caster targets themselves (most buff spells).
    SelfOnly,
}

/// Buffable / debuffable stat. The retail engine modifies a small set of
/// per-actor scalars during a magic phase; this enum mirrors them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BuffStat {
    Attack,
    Defense,
    Accuracy,
    Evasion,
    /// Speed / initiative - folds into turn-order recompute.
    Speed,
    /// Magic attack scalar.
    MagicAttack,
    /// Magic defense scalar.
    MagicDefense,
}

/// Concrete effect a spell produces when cast.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpellEffect {
    /// Damage with elemental multiplier. Final damage is
    /// `(caster_mag * mult / 8) - target_mdef` clamped at 1.
    Damage {
        base_power: u16,
        element: SpellElement,
    },
    /// Heal a fixed HP amount (Heal-class spells in retail are scalar,
    /// not formula-driven).
    Heal { amount: u16 },
    /// Heal every party member by `amount`.
    HealAll { amount: u16 },
    /// Cure one named status.
    Cure(StatusKind),
    /// Cure every status on the target.
    CureAll,
    /// Revive with the listed HP percentage.
    Revive { hp_pct: u8 },
    /// Apply a stat buff for `turns` turns. Magnitude can be negative for
    /// debuffs.
    Buff {
        stat: BuffStat,
        magnitude: i16,
        turns: u8,
    },
    /// Capture spell. Engines roll capture independently; the resolver
    /// surfaces the spell's hit chance.
    Capture { hit_pct: u8 },
    /// Field-only escape spell. Always succeeds in non-boss battles.
    Escape,
}

impl Default for SpellEffect {
    fn default() -> Self {
        Self::Damage {
            base_power: 0,
            element: SpellElement::Neutral,
        }
    }
}

/// One spell definition. Engines populate the catalog at battle init from
/// disc data + the per-character learned-spell list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpellDef {
    pub id: u8,
    /// Display name. Default "Spell {id}" for unknown spells.
    pub name: String,
    pub mp_cost: u8,
    pub element: SpellElement,
    pub target: SpellTarget,
    pub effect: SpellEffect,
    /// Animation id wired to `BattleActionHost::spell_anim_trigger`.
    /// `0` = no animation.
    pub anim_id: u8,
}

impl Default for SpellDef {
    fn default() -> Self {
        Self {
            id: 0,
            name: "(unknown)".into(),
            mp_cost: 0,
            element: SpellElement::Neutral,
            target: SpellTarget::OneEnemy,
            effect: SpellEffect::default(),
            anim_id: 0,
        }
    }
}

/// Spell catalog. Engines populate at battle init; the
/// `BattleActionHost::spell_mp_cost` lookup falls back here if engines
/// install the catalog into the world.
#[derive(Debug, Clone, Default)]
pub struct SpellCatalog {
    spells: HashMap<u8, SpellDef>,
}

impl SpellCatalog {
    pub fn new() -> Self {
        Self {
            spells: HashMap::new(),
        }
    }

    /// Pre-populate with a small "vanilla" catalog covering the canonical
    /// retail spell list. Useful for tests and the `legaia-engine battle`
    /// subcommand. The base powers are placeholders pending overlay
    /// capture of the real values.
    pub fn vanilla() -> Self {
        let mut c = Self::new();
        // Healing spells.
        c.insert(SpellDef {
            id: 0x10,
            name: "Heal".into(),
            mp_cost: 4,
            target: SpellTarget::OneAlly,
            effect: SpellEffect::Heal { amount: 60 },
            anim_id: 0x20,
            ..Default::default()
        });
        c.insert(SpellDef {
            id: 0x11,
            name: "Heal All".into(),
            mp_cost: 8,
            target: SpellTarget::AllAllies,
            effect: SpellEffect::HealAll { amount: 60 },
            anim_id: 0x21,
            ..Default::default()
        });
        c.insert(SpellDef {
            id: 0x12,
            name: "Mega Heal".into(),
            mp_cost: 12,
            target: SpellTarget::OneAlly,
            effect: SpellEffect::Heal { amount: 200 },
            anim_id: 0x22,
            ..Default::default()
        });
        c.insert(SpellDef {
            id: 0x13,
            name: "Refresh".into(),
            mp_cost: 6,
            target: SpellTarget::OneAlly,
            effect: SpellEffect::CureAll,
            anim_id: 0x23,
            ..Default::default()
        });
        c.insert(SpellDef {
            id: 0x14,
            name: "Vital".into(),
            mp_cost: 16,
            target: SpellTarget::OneAlly,
            effect: SpellEffect::Revive { hp_pct: 50 },
            anim_id: 0x24,
            ..Default::default()
        });
        // Offensive elemental spells.
        c.insert(SpellDef {
            id: 0x20,
            name: "Flame".into(),
            mp_cost: 5,
            element: SpellElement::Fire,
            target: SpellTarget::OneEnemy,
            effect: SpellEffect::Damage {
                base_power: 80,
                element: SpellElement::Fire,
            },
            anim_id: 0x40,
        });
        c.insert(SpellDef {
            id: 0x21,
            name: "Burning Heat".into(),
            mp_cost: 10,
            element: SpellElement::Fire,
            target: SpellTarget::AllEnemies,
            effect: SpellEffect::Damage {
                base_power: 60,
                element: SpellElement::Fire,
            },
            anim_id: 0x41,
        });
        c.insert(SpellDef {
            id: 0x22,
            name: "Aqua".into(),
            mp_cost: 5,
            element: SpellElement::Water,
            target: SpellTarget::OneEnemy,
            effect: SpellEffect::Damage {
                base_power: 80,
                element: SpellElement::Water,
            },
            anim_id: 0x42,
        });
        c.insert(SpellDef {
            id: 0x23,
            name: "Thunder Bolt".into(),
            mp_cost: 8,
            element: SpellElement::Thunder,
            target: SpellTarget::OneEnemy,
            effect: SpellEffect::Damage {
                base_power: 100,
                element: SpellElement::Thunder,
            },
            anim_id: 0x43,
        });
        c.insert(SpellDef {
            id: 0x24,
            name: "Wind".into(),
            mp_cost: 6,
            element: SpellElement::Wind,
            target: SpellTarget::OneEnemy,
            effect: SpellEffect::Damage {
                base_power: 90,
                element: SpellElement::Wind,
            },
            anim_id: 0x44,
        });
        c.insert(SpellDef {
            id: 0x25,
            name: "Ice".into(),
            mp_cost: 7,
            element: SpellElement::Ice,
            target: SpellTarget::OneEnemy,
            effect: SpellEffect::Damage {
                base_power: 95,
                element: SpellElement::Ice,
            },
            anim_id: 0x45,
        });
        c.insert(SpellDef {
            id: 0x26,
            name: "Crash".into(),
            mp_cost: 9,
            element: SpellElement::Earth,
            target: SpellTarget::OneEnemy,
            effect: SpellEffect::Damage {
                base_power: 110,
                element: SpellElement::Earth,
            },
            anim_id: 0x46,
        });
        // Buff / debuff spells.
        c.insert(SpellDef {
            id: 0x30,
            name: "Power Up".into(),
            mp_cost: 5,
            target: SpellTarget::OneAlly,
            effect: SpellEffect::Buff {
                stat: BuffStat::Attack,
                magnitude: 20,
                turns: 4,
            },
            anim_id: 0x50,
            ..Default::default()
        });
        c.insert(SpellDef {
            id: 0x31,
            name: "Defense Up".into(),
            mp_cost: 5,
            target: SpellTarget::OneAlly,
            effect: SpellEffect::Buff {
                stat: BuffStat::Defense,
                magnitude: 20,
                turns: 4,
            },
            anim_id: 0x51,
            ..Default::default()
        });
        c.insert(SpellDef {
            id: 0x32,
            name: "Speed Up".into(),
            mp_cost: 6,
            target: SpellTarget::OneAlly,
            effect: SpellEffect::Buff {
                stat: BuffStat::Speed,
                magnitude: 15,
                turns: 4,
            },
            anim_id: 0x52,
            ..Default::default()
        });
        c.insert(SpellDef {
            id: 0x33,
            name: "Power Down".into(),
            mp_cost: 5,
            target: SpellTarget::OneEnemy,
            effect: SpellEffect::Buff {
                stat: BuffStat::Attack,
                magnitude: -20,
                turns: 4,
            },
            anim_id: 0x53,
            ..Default::default()
        });
        // Capture / escape.
        c.insert(SpellDef {
            id: 0x40,
            name: "Reseal".into(),
            mp_cost: 12,
            target: SpellTarget::OneEnemy,
            effect: SpellEffect::Capture { hit_pct: 60 },
            anim_id: 0x60,
            ..Default::default()
        });
        c.insert(SpellDef {
            id: 0x41,
            name: "Warp".into(),
            mp_cost: 8,
            target: SpellTarget::SelfOnly,
            effect: SpellEffect::Escape,
            anim_id: 0x61,
            ..Default::default()
        });
        c
    }

    pub fn insert(&mut self, def: SpellDef) {
        self.spells.insert(def.id, def);
    }

    pub fn get(&self, id: u8) -> Option<&SpellDef> {
        self.spells.get(&id)
    }

    pub fn len(&self) -> usize {
        self.spells.len()
    }

    pub fn is_empty(&self) -> bool {
        self.spells.is_empty()
    }

    /// Iterate all defined spells in arbitrary order.
    pub fn iter(&self) -> impl Iterator<Item = &SpellDef> {
        self.spells.values()
    }

    /// Aggregate MP-cost lookup. Mirrors the
    /// `BattleActionHost::spell_mp_cost` callback so engines can install
    /// the catalog as the canonical source.
    pub fn mp_cost(&self, id: u8) -> u8 {
        self.spells.get(&id).map(|s| s.mp_cost).unwrap_or(0)
    }

    /// `true` when the spell is the Capture / Reseal class. Mirrors
    /// `BattleActionHost::is_capture_spell`.
    pub fn is_capture(&self, id: u8) -> bool {
        matches!(
            self.spells.get(&id).map(|s| &s.effect),
            Some(SpellEffect::Capture { .. })
        )
    }
}

/// Per-cast snapshot. Engines pull these scalars from the live actor
/// table at the moment the cast resolves.
#[derive(Debug, Clone, Default)]
pub struct SpellSnapshot {
    /// Caster's Magic stat (the retail aggregator's `mag` column).
    pub caster_mag: u16,
    pub caster_hp: u16,
    pub caster_max_hp: u16,
    pub caster_mp: u16,
    /// Target's Magic Defense scalar.
    pub target_mdef: u16,
    pub target_hp: u16,
    pub target_hp_max: u16,
    pub target_mp: u16,
    pub target_alive: bool,
    /// Element-weakness mask for the target (empty for non-elemental).
    pub target_weakness: ElementMask,
}

/// Why a cast failed when no in-game effect resolved.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpellFailReason {
    /// MP gauge was below `mp_cost`.
    NotEnoughMp,
    /// Cast a single-target offensive spell on a dead actor.
    DeadTarget,
    /// Cast a healing or revive spell on a target whose state didn't
    /// change (revive on alive, heal on already-full HP).
    NoChange,
    /// Tried to cast a single-target ally spell on a non-ally slot.
    InvalidTarget,
    /// Catalog had no entry for this id.
    UnknownSpell,
}

/// Effect a successful cast applied. Engines fold these into world state
/// (HP / MP / status / buff timers).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpellOutcome {
    /// Damage spell. `amount` is the post-formula damage; `mult` is the
    /// elemental multiplier engines can show in the HUD.
    Damage {
        target: u8,
        amount: u16,
        element: SpellElement,
        weakness: bool,
    },
    /// Multi-target damage. Per-target entries.
    MultiDamage {
        targets: Vec<(u8, u16)>,
        element: SpellElement,
    },
    /// Heal - single target.
    Heal { target: u8, amount: u16 },
    /// Heal - multi target.
    MultiHeal { targets: Vec<(u8, u16)> },
    /// Cure - `removed` is the count of statuses cleared.
    Cure { target: u8, removed: u8 },
    /// Revive with the resolved HP value.
    Revive { target: u8, hp: u16 },
    /// Buff applied. `magnitude` may be negative.
    Buff {
        target: u8,
        stat: BuffStat,
        magnitude: i16,
        turns: u8,
    },
    /// Capture-spell hit. Engines roll the actual capture vs. monster HP.
    CaptureRoll { target: u8, hit_pct: u8 },
    /// Escape spell - caster fled the encounter.
    Escape,
    /// Cast was rolled but produced no effect.
    Failed { reason: SpellFailReason },
}

/// Resolve a spell cast.
///
/// Pure function. Engines pass per-target snapshots because the catalog
/// itself doesn't carry actor pointers; multi-target spells require a
/// matching shape (`AllAllies` -> 3 snapshots, `AllEnemies` -> N enemy
/// snapshots).
pub fn cast_spell(spell: &SpellDef, target_slot: u8, snap: &SpellSnapshot) -> SpellOutcome {
    if snap.caster_mp < spell.mp_cost as u16 {
        return SpellOutcome::Failed {
            reason: SpellFailReason::NotEnoughMp,
        };
    }
    match &spell.effect {
        SpellEffect::Damage {
            base_power,
            element,
        } => {
            if !snap.target_alive {
                return SpellOutcome::Failed {
                    reason: SpellFailReason::DeadTarget,
                };
            }
            let mult = element.multiplier_against(snap.target_weakness);
            let raw = (snap.caster_mag as u32 * (*base_power as u32) / 8)
                .saturating_sub(snap.target_mdef as u32);
            let scaled = ((raw as f32) * mult).round() as u32;
            let dmg = scaled.max(1).min(u16::MAX as u32) as u16;
            SpellOutcome::Damage {
                target: target_slot,
                amount: dmg,
                element: *element,
                weakness: snap.target_weakness.contains(*element),
            }
        }
        SpellEffect::Heal { amount } => {
            if !snap.target_alive {
                return SpellOutcome::Failed {
                    reason: SpellFailReason::DeadTarget,
                };
            }
            let cap = snap.target_hp_max.saturating_sub(snap.target_hp);
            if cap == 0 {
                return SpellOutcome::Failed {
                    reason: SpellFailReason::NoChange,
                };
            }
            let actual = (*amount).min(cap);
            SpellOutcome::Heal {
                target: target_slot,
                amount: actual,
            }
        }
        SpellEffect::HealAll { amount } => {
            // Caller resolves multi-heal by calling `cast_spell` with each
            // ally snapshot - surfacing the heal amount the formula uses.
            if !snap.target_alive {
                return SpellOutcome::Failed {
                    reason: SpellFailReason::DeadTarget,
                };
            }
            let cap = snap.target_hp_max.saturating_sub(snap.target_hp);
            let actual = (*amount).min(cap);
            SpellOutcome::Heal {
                target: target_slot,
                amount: actual,
            }
        }
        SpellEffect::Cure(_) | SpellEffect::CureAll => SpellOutcome::Cure {
            target: target_slot,
            // Engines query the StatusEffectTracker for the actual count;
            // the resolver returns a zero placeholder.
            removed: 0,
        },
        SpellEffect::Revive { hp_pct } => {
            if snap.target_alive {
                return SpellOutcome::Failed {
                    reason: SpellFailReason::NoChange,
                };
            }
            let hp = (snap.target_hp_max as u32 * (*hp_pct as u32) / 100)
                .max(1)
                .min(snap.target_hp_max as u32) as u16;
            SpellOutcome::Revive {
                target: target_slot,
                hp,
            }
        }
        SpellEffect::Buff {
            stat,
            magnitude,
            turns,
        } => SpellOutcome::Buff {
            target: target_slot,
            stat: *stat,
            magnitude: *magnitude,
            turns: *turns,
        },
        SpellEffect::Capture { hit_pct } => {
            if !snap.target_alive {
                return SpellOutcome::Failed {
                    reason: SpellFailReason::DeadTarget,
                };
            }
            SpellOutcome::CaptureRoll {
                target: target_slot,
                hit_pct: *hit_pct,
            }
        }
        SpellEffect::Escape => SpellOutcome::Escape,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn alive_enemy(mag_def: u16, hp: u16) -> SpellSnapshot {
        SpellSnapshot {
            caster_mag: 100,
            caster_hp: 100,
            caster_max_hp: 100,
            caster_mp: 50,
            target_mdef: mag_def,
            target_hp: hp,
            target_hp_max: hp,
            target_mp: 0,
            target_alive: hp > 0,
            target_weakness: ElementMask::empty(),
        }
    }

    fn vanilla_spell(id: u8) -> SpellDef {
        SpellCatalog::vanilla().get(id).cloned().unwrap()
    }

    #[test]
    fn vanilla_catalog_has_expected_count() {
        let cat = SpellCatalog::vanilla();
        // 5 healing + 7 damage + 4 buff + 2 utility = 18.
        assert!(cat.len() >= 16, "vanilla catalog has {} spells", cat.len());
    }

    #[test]
    fn mp_cost_returns_zero_for_unknown_id() {
        let cat = SpellCatalog::vanilla();
        assert_eq!(cat.mp_cost(0xFF), 0);
        assert_eq!(cat.mp_cost(0x10), 4);
    }

    #[test]
    fn is_capture_marks_only_capture_spells() {
        let cat = SpellCatalog::vanilla();
        assert!(cat.is_capture(0x40));
        assert!(!cat.is_capture(0x20));
        assert!(!cat.is_capture(0x10));
    }

    #[test]
    fn cast_damage_spell_fails_when_target_dead() {
        let snap = alive_enemy(10, 0);
        let s = vanilla_spell(0x20); // Flame
        let outcome = cast_spell(&s, 3, &snap);
        assert!(matches!(
            outcome,
            SpellOutcome::Failed {
                reason: SpellFailReason::DeadTarget
            }
        ));
    }

    #[test]
    fn cast_damage_spell_fails_when_caster_low_mp() {
        let mut snap = alive_enemy(0, 100);
        snap.caster_mp = 0;
        let s = vanilla_spell(0x20);
        let outcome = cast_spell(&s, 3, &snap);
        assert!(matches!(
            outcome,
            SpellOutcome::Failed {
                reason: SpellFailReason::NotEnoughMp
            }
        ));
    }

    #[test]
    fn cast_damage_spell_returns_positive_amount() {
        let snap = alive_enemy(5, 100);
        let s = vanilla_spell(0x20); // Flame, base 80, fire
        let outcome = cast_spell(&s, 3, &snap);
        match outcome {
            SpellOutcome::Damage {
                target,
                amount,
                element,
                weakness,
            } => {
                assert_eq!(target, 3);
                // 100 * 80 / 8 - 5 = 995. Clamped to u16 (995).
                assert_eq!(amount, 995);
                assert_eq!(element, SpellElement::Fire);
                assert!(!weakness);
            }
            _ => panic!("expected damage outcome"),
        }
    }

    #[test]
    fn cast_damage_spell_respects_element_weakness() {
        let mut snap = alive_enemy(5, 100);
        snap.target_weakness = ElementMask::empty().with(SpellElement::Fire);
        let s = vanilla_spell(0x20);
        let outcome = cast_spell(&s, 3, &snap);
        match outcome {
            SpellOutcome::Damage {
                amount, weakness, ..
            } => {
                // 1.5x multiplier means roughly 1492 damage.
                assert!(amount > 1400 && amount < 1600);
                assert!(weakness);
            }
            _ => panic!("expected damage outcome"),
        }
    }

    #[test]
    fn cast_damage_spell_clamps_floor_at_one() {
        // Target has more defense than caster has magic - formula goes
        // negative; the floor clamps it to 1.
        let snap = SpellSnapshot {
            caster_mag: 1,
            caster_mp: 50,
            target_mdef: 9999,
            target_hp: 100,
            target_hp_max: 100,
            target_alive: true,
            ..Default::default()
        };
        let s = vanilla_spell(0x20);
        let outcome = cast_spell(&s, 3, &snap);
        match outcome {
            SpellOutcome::Damage { amount, .. } => assert_eq!(amount, 1),
            _ => panic!("expected damage outcome"),
        }
    }

    #[test]
    fn cast_heal_spell_caps_at_hp_max() {
        // HP at 95, max 100; Heal would have given 60 but only 5 effective.
        let snap = SpellSnapshot {
            caster_mp: 50,
            target_alive: true,
            target_hp: 95,
            target_hp_max: 100,
            ..Default::default()
        };
        let s = vanilla_spell(0x10); // Heal
        let outcome = cast_spell(&s, 1, &snap);
        match outcome {
            SpellOutcome::Heal { target, amount } => {
                assert_eq!(target, 1);
                assert_eq!(amount, 5);
            }
            _ => panic!("expected heal outcome"),
        }
    }

    #[test]
    fn cast_heal_spell_returns_no_change_when_full_hp() {
        let snap = SpellSnapshot {
            caster_mp: 50,
            target_alive: true,
            target_hp: 100,
            target_hp_max: 100,
            ..Default::default()
        };
        let s = vanilla_spell(0x10);
        let outcome = cast_spell(&s, 1, &snap);
        assert!(matches!(
            outcome,
            SpellOutcome::Failed {
                reason: SpellFailReason::NoChange
            }
        ));
    }

    #[test]
    fn cast_revive_returns_no_change_for_alive_target() {
        let snap = SpellSnapshot {
            caster_mp: 50,
            target_alive: true,
            target_hp: 50,
            target_hp_max: 100,
            ..Default::default()
        };
        let s = vanilla_spell(0x14);
        let outcome = cast_spell(&s, 1, &snap);
        assert!(matches!(
            outcome,
            SpellOutcome::Failed {
                reason: SpellFailReason::NoChange
            }
        ));
    }

    #[test]
    fn cast_revive_returns_correct_hp_for_dead_target() {
        let snap = SpellSnapshot {
            caster_mp: 50,
            target_alive: false,
            target_hp: 0,
            target_hp_max: 100,
            ..Default::default()
        };
        let s = vanilla_spell(0x14); // Vital - hp_pct: 50
        let outcome = cast_spell(&s, 1, &snap);
        match outcome {
            SpellOutcome::Revive { target, hp } => {
                assert_eq!(target, 1);
                assert_eq!(hp, 50);
            }
            _ => panic!("expected revive outcome"),
        }
    }

    #[test]
    fn cast_buff_spell_returns_buff_outcome() {
        let snap = SpellSnapshot {
            caster_mp: 50,
            target_alive: true,
            ..Default::default()
        };
        let s = vanilla_spell(0x30); // Power Up
        let outcome = cast_spell(&s, 0, &snap);
        match outcome {
            SpellOutcome::Buff {
                target,
                stat,
                magnitude,
                turns,
            } => {
                assert_eq!(target, 0);
                assert_eq!(stat, BuffStat::Attack);
                assert_eq!(magnitude, 20);
                assert_eq!(turns, 4);
            }
            _ => panic!("expected buff outcome"),
        }
    }

    #[test]
    fn cast_debuff_returns_negative_magnitude() {
        let snap = SpellSnapshot {
            caster_mp: 50,
            target_alive: true,
            ..Default::default()
        };
        let s = vanilla_spell(0x33); // Power Down
        let outcome = cast_spell(&s, 3, &snap);
        match outcome {
            SpellOutcome::Buff { magnitude, .. } => assert_eq!(magnitude, -20),
            _ => panic!("expected buff outcome"),
        }
    }

    #[test]
    fn cast_capture_spell_returns_capture_roll() {
        let snap = alive_enemy(5, 50);
        let s = vanilla_spell(0x40); // Reseal
        let outcome = cast_spell(&s, 3, &snap);
        match outcome {
            SpellOutcome::CaptureRoll { target, hit_pct } => {
                assert_eq!(target, 3);
                assert_eq!(hit_pct, 60);
            }
            _ => panic!("expected capture outcome"),
        }
    }

    #[test]
    fn cast_escape_spell_returns_escape() {
        let snap = SpellSnapshot {
            caster_mp: 50,
            ..Default::default()
        };
        let s = vanilla_spell(0x41); // Warp
        let outcome = cast_spell(&s, 0, &snap);
        assert!(matches!(outcome, SpellOutcome::Escape));
    }

    #[test]
    fn element_mask_with_and_contains_round_trip() {
        let m = ElementMask::empty()
            .with(SpellElement::Fire)
            .with(SpellElement::Ice);
        assert!(m.contains(SpellElement::Fire));
        assert!(m.contains(SpellElement::Ice));
        assert!(!m.contains(SpellElement::Water));
    }

    #[test]
    fn element_neutral_never_matches_weakness() {
        let m = ElementMask::empty().with(SpellElement::Fire);
        // Neutral element passthrough means multiplier = 1.0.
        assert_eq!(SpellElement::Neutral.multiplier_against(m), 1.0);
    }

    #[test]
    fn cure_outcome_count_returns_placeholder_zero() {
        let snap = SpellSnapshot {
            caster_mp: 50,
            target_alive: true,
            ..Default::default()
        };
        let s = vanilla_spell(0x13);
        let outcome = cast_spell(&s, 0, &snap);
        match outcome {
            SpellOutcome::Cure { removed, .. } => assert_eq!(removed, 0),
            _ => panic!("expected cure outcome"),
        }
    }

    #[test]
    fn catalog_iter_yields_every_inserted_spell() {
        let mut cat = SpellCatalog::new();
        cat.insert(SpellDef {
            id: 0x01,
            name: "Spark".into(),
            mp_cost: 3,
            ..Default::default()
        });
        cat.insert(SpellDef {
            id: 0x02,
            name: "Mist".into(),
            mp_cost: 4,
            ..Default::default()
        });
        let names: Vec<&str> = cat.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"Spark"));
        assert!(names.contains(&"Mist"));
    }

    #[test]
    fn unknown_spell_id_returns_none() {
        let cat = SpellCatalog::vanilla();
        assert!(cat.get(0xFE).is_none());
    }

    #[test]
    fn spell_def_default_is_neutral_one_enemy_zero_cost() {
        let s = SpellDef::default();
        assert_eq!(s.id, 0);
        assert_eq!(s.mp_cost, 0);
        assert_eq!(s.element, SpellElement::Neutral);
        assert_eq!(s.target, SpellTarget::OneEnemy);
    }
}
