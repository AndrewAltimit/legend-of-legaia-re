//! Monster-turn decisioning: the action picker, confuse retargeting, physical /
//! cast arming, and monster-side target-class resolution. Split out of
//! `battle.rs` as additional `impl World` blocks; no logic change from the
//! original inline definitions.

use super::*;

/// Accessory-passive index `0x18` = **Rot Guard** (Forest Amulet), i.e. bit
/// `0x0100_0000` of the character record's `+0xF4` ability bitfield - the
/// first of the two words the Rot arm tests
/// (`lui v0,0x100; and; bne` at `0x801E16D0..0x801E16D8`).
/// See [`docs/formats/accessory-passive-table.md`](../../../../../docs/formats/accessory-passive-table.md).
const ROT_GUARD_BIT: u32 = 1 << 0x18;
/// Accessory-passive index `0x1C` = **Master Guard** (Wonder Amulet), bit
/// `0x1000_0000`, the second word the Rot arm tests
/// (`lui v0,0x1000; and; bne` at `0x801E16E0..0x801E16E8`).
const MASTER_GUARD_BIT: u32 = 1 << 0x1C;

/// What the impact status proc rolled - see [`enemy_impact_status_proc`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::world) struct ImpactStatusProc {
    /// The status byte to feed
    /// [`legaia_engine_vm::status_effects::StatusEffectTracker::apply_from_enemy_effect`].
    pub effect: legaia_art::record::EnemyEffect,
    /// The rolled limb (`rand % 3`) when the selector was the Rot arm.
    pub rot_limb: Option<u8>,
}

/// The four-way branch `FUN_801E09F8` takes on a move-power record's `+0x0A`
/// impact-effect selector once the strike arm reaches the impact phase.
/// Disassembly, `0x801E15F8..0x801E1788`:
///
/// | selector | retail | port |
/// |---|---|---|
/// | `0` | `beq v1,zero,0x801E1788` - whole block skipped | `None` |
/// | `3` | `jal rand`, `andi v0,v0,0x7`, on `0` `ori 0x1` (`0x801E1630..0x801E165C`) | 1-in-8 Venom |
/// | `4` | `jal rand`, `andi v0,v0,0x7`, on `0` `ori 0x2` (`0x801E1660..0x801E168C`) | 1-in-8 Toxic |
/// | `5` | target slot `< 3` (`sltiu v0,a1,0x3`) and neither guard bit set, then `ori 1 << (rand%3 + 3)` (`0x801E1690..0x801E1740`) | Rot with the rolled limb |
/// | else | falls through | `None` |
///
/// The selector-`3` arm's comparison constant is the register `a2`, which
/// still holds the impact-phase byte `3` loaded at `0x801E156C` and proved
/// equal to `3` by the `bne` at `0x801E1574` - a register-economy trick, not a
/// separate constant.
///
/// **Selector `6` (Curse) is deliberately absent.** The sibling arts/melee
/// kernel `FUN_801EC3E4` has a fifth arm (`li v0,0x6` / `beq` at
/// `0x801EE478..0x801EE47C` -> `andi v0,v0,0x3` / `ori 0x1000` at
/// `0x801EE698..0x801EE6C8`, a 1-in-4 roll), but this routine's ladder ends at
/// `5`: `0x801E1620` tests only `5` and otherwise jumps to the join at
/// `0x801E178C`. An enemy *special* therefore cannot Curse; only the
/// physical/arts path can.
///
/// **RNG draw counts are faithful.** Arms `3`/`4` draw exactly one `rand()`.
/// The Rot arm reads the guard bitfield *before* its draw
/// (`lw v1,0x6bc(v0)` at `0x801E16CC`, both `bne`s taken out before the
/// `jal 0x80056798` at `0x801E16FC`), so a guarded - or non-party - target
/// draws nothing at all.
///
/// PORT: FUN_801E09F8 (interior: the `+0x0A` selector ladder)
pub(in crate::world) fn enemy_impact_status_proc(
    selector: u8,
    target_is_party: bool,
    target_ability_bits: u32,
    rng: &mut dyn FnMut() -> u32,
) -> Option<ImpactStatusProc> {
    let effect = legaia_art::record::EnemyEffect::from_byte(selector);
    match selector {
        // Weak DoT (Venom) and strong DoT (Toxic): the same `rand & 7 == 0`
        // gate, one draw each.
        3 | 4 => (rng() & 7 == 0).then_some(ImpactStatusProc {
            effect,
            rot_limb: None,
        }),
        // Random limb disable (Rot). Party seats only - the guard bitfield the
        // arm reads is a *character record* field, so retail's `< 3` test is
        // both a seat test and the precondition for the read.
        5 => {
            if !target_is_party {
                return None;
            }
            if target_ability_bits & (ROT_GUARD_BIT | MASTER_GUARD_BIT) != 0 {
                return None;
            }
            Some(ImpactStatusProc {
                effect,
                rot_limb: Some((rng() % 3) as u8),
            })
        }
        _ => None,
    }
}

impl World {
    pub(in crate::world) fn take_monster_turn(&mut self, slot: u8) {
        use vm::battle_action::ActionState;

        self.battle_ctx.active_actor = slot;
        match self.pick_monster_action(slot) {
            // A silenced/petrified caster can't cast - fall back to a physical
            // strike (mirrors the affordability fallback below).
            MonsterAction::Cast { .. } if self.actor_blocked_from_magic(slot) => {
                self.arm_monster_physical(slot);
            }
            MonsterAction::Cast {
                spell_id,
                mut targets,
            } => {
                // A confused caster's spell lands on the opposite side.
                self.confuse_retarget_cast(slot, &mut targets);
                let def = self.spell_catalog.get(spell_id).cloned();
                if let Some(def) = def {
                    // Mark where this cast's damage popups start, so the
                    // status applier below can tell which targets the move
                    // actually reached impact on (see
                    // [`Self::apply_enemy_move_status`]).
                    let hit_fx_start = self.battle_hit_fx.len();
                    if self.cast_spell_on_slots(slot, &def, &targets) {
                        self.apply_enemy_move_status(slot, def.id, hit_fx_start);
                        self.apply_enemy_agl_status(slot, def.id, &targets);
                        self.battle_ctx.action_state = ActionState::EndOfAction.as_byte();
                        // NOT cycled here: take_monster_turn is itself called
                        // from `cycle_battle_turn`'s re-arm. But the SM's
                        // 0x5A self-advance WILL re-seed this actor's staged
                        // action on the next tick, and a category-2 re-seed
                        // runs the magic band's `MagicCastBegin` - a second
                        // MP debit. Neutralise the category (0 seeds the
                        // inert TacticalArts arm); the staged spell id in
                        // `params[0]` is left in place - it is the observable
                        // the AI-pick oracles read, and the cat-0 arm never
                        // consumes it.
                        if let Some(a) = self.actors.get_mut(slot as usize) {
                            a.battle.action_category = 0;
                        }
                        return;
                    }
                }
                // Cast didn't fold (no catalog entry / unaffordable after the
                // pick) - fall through to a physical strike.
                self.arm_monster_physical(slot);
            }
            MonsterAction::Physical { target } => {
                // AGL-driven multi-action budget: how many swings this monster
                // lands this turn (single swing when it has no AGL / swing data).
                self.arm_monster_strike_budget(slot);
                self.clear_action_stream(slot);
                self.battle_ctx.queued_action = 3;
                self.battle_ctx.action_state = ActionState::Begin.as_byte();
                if let Some(a) = self.actors.get_mut(slot as usize) {
                    a.battle.active_target = target;
                    a.battle.action_category = 3;
                }
                self.maybe_confuse_retarget(slot);
            }
            MonsterAction::Flee => {
                // The picker's flee checkpoint fired: arm the Run band
                // (category 5). The action SM's seed routes a monster-slot
                // category-5 actor to the state-0x68 leave-battle arm
                // (`ActionState::CaptureStart` and siblings), which plays the
                // break-off and removes the monster from the pool.
                self.battle_ctx.queued_action = 5;
                self.battle_ctx.action_state = ActionState::Begin.as_byte();
            }
        }
    }

    /// Roll the **enemy special-attack impact status proc** onto every target
    /// this cast reached, and push what lands into the status tracker.
    ///
    /// This is the monster -> party half of the status system, and the source
    /// is the move-power record, not an art record. Retail's chain, read off
    /// the instruction stream:
    ///
    /// 1. `FUN_801DEA50` (action setup) maps the acting actor's queued move id
    ///    at `actor[+0x1DF]` through the id -> index map at `0x801F4E63` and
    ///    stashes the resulting 26-byte move-power record pointer in the battle
    ///    context: `sw v0,0x1014(a0)` at `0x801DF284`, with the `x26` stride
    ///    built as `13a << 1` at `0x801DF264..0x801DF274`.
    /// 2. `FUN_801E09F8` (the per-frame action tick) waits for the arm's phase
    ///    byte to reach the impact value 3 (`lbu a2,0x24e(v0)` / `li v0,0x3` /
    ///    `bne` at `0x801E156C..0x801E1574`), then reads the record's
    ///    **`+0x0A` impact-effect selector** (`lbu v1,0xa(v0)` at
    ///    `0x801E1584`). A zero selector skips the whole block.
    /// 3. The non-zero selector latches on the *target* as the lingering status
    ///    visual (`sb v1,0x21f(v0)` at `0x801E15AC`, tint word from
    ///    `0x801F53D4[(sel-1)]` into `target+0x04`), then branches four ways -
    ///    ported in [`enemy_impact_status_proc`].
    ///
    /// The map is **special-attack-only** (see
    /// [`docs/formats/move-power.md`](../../../../../docs/formats/move-power.md)):
    /// a monster's *basic* attack id resolves to the all-zero record 0, whose
    /// `+0x0A` is `0`, so routing every monster move through here automatically
    /// gives basic attacks no status - no extra guard needed.
    ///
    /// **Which targets.** Retail rolls once per arm that reaches the impact
    /// phase. The engine's cast folds a `SpellOutcome` per target and pushes
    /// exactly one damage-coloured [`BattleHitFx`] for each one that resolved
    /// as damage, so the popups appended since `hit_fx_start` are this cast's
    /// impact list. Magnitude is not consulted: retail's arm is upstream of the
    /// damage subtraction and a fully-mitigated (or Stone-absorbed) hit still
    /// reaches impact.
    ///
    /// PORT: FUN_801E09F8 (status-proc arm, `0x801E1584..0x801E1788`)
    /// REF: FUN_801DEA50 (the `ctx+0x1014` move-power record the arm reads)
    /// REF: FUN_801EC3E4 (the sibling arts/melee arm, whose selector comes from
    /// the art record's `+0x7A` instead - the party -> monster direction)
    pub(in crate::world) fn apply_enemy_move_status(
        &mut self,
        caster: u8,
        move_id: u8,
        hit_fx_start: usize,
    ) {
        // The move-power table is the *enemy* special-attack table; a party
        // caster's art power comes from the art record instead, and its status
        // byte rides the `ApplyArtStrike` event (`World::fold_battle_event`).
        if (caster as usize) < self.party_count as usize {
            return;
        }
        let Some(selector) = self
            .move_power
            .as_ref()
            .and_then(|c| c.record_for_move_id(move_id))
            .map(|r| r.impact_effect())
            .filter(|&s| s != 0)
        else {
            return;
        };
        let targets: Vec<u8> = self
            .battle_hit_fx
            .get(hit_fx_start..)
            .unwrap_or(&[])
            .iter()
            .filter(|fx| !fx.is_heal)
            .map(|fx| fx.target_slot)
            .collect();
        let party_count = self.party_count;
        for target in targets {
            // `sb v1,0x21f(...)` - the lingering status visual latches on the
            // selector itself, before and independent of the proc roll.
            if let Some(a) = self.actors.get_mut(target as usize) {
                a.pending_status = Some(legaia_art::record::EnemyEffect::from_byte(selector));
            }
            let target_is_party = target < party_count;
            let ability_bits = if target_is_party {
                self.character_ability_bits
                    .get(target as usize)
                    .copied()
                    .unwrap_or(0)
            } else {
                0
            };
            let rolled = {
                let rng = &mut || self.next_rng();
                enemy_impact_status_proc(selector, target_is_party, ability_bits, rng)
            };
            let Some(proc) = rolled else {
                continue;
            };
            let applied = self
                .status_effects
                .apply_from_enemy_effect(target, proc.effect);
            if let (Some(vm::status_effects::StatusKind::Rot), Some(limb)) =
                (applied, proc.rot_limb)
            {
                self.status_effects.set_rot_limb(target, limb);
            }
        }
    }

    /// The **Stone / Curse infliction arm** of a capture-class boss cast -
    /// the engine stand-in for the streamed module's
    /// `FUN_800402F4(9 | 10, ..)` calls (jump-table entries decoded off the
    /// disc: class 9 = `0x80041C70` `ori 0x4` Stone, class 10 = `0x80041E64`
    /// `ori 0x1000` Curse; roll kernel
    /// [`vm::status_effects::agl_status_inflict_roll`]).
    ///
    /// **Which moves** is the disclosed-inference half: the class byte is a
    /// literal in the streamed capture-class module code (no spell-table
    /// field carries it, and the modules reach the applier through runtime
    /// dispatch, so no static scan recovers the pairing). The list is
    /// therefore keyed on the four capture-class records whose effect is the
    /// status: **Glare** `0x3C` (Stone - capture-pinned: the `status_effects`
    /// module doc's Glare before/after save pair shows `+0x16E` `0 -> 4`),
    /// **Stone Circle** `0xB9` (Stone), **Curse** `0x40` and **Curse All**
    /// `0x53` (Curse) - the latter three graded inference from the records'
    /// names + published behaviour.
    ///
    /// Faithful shape per the disassembly (`800402f4.txt`
    /// `0x80041C70..0x80041FB0`): party seats only, one `rand()` draw per
    /// rolled target, `target_agl < rand % (attacker_agl + target_agl)`
    /// lands the bit. Two engine-model equivalences, both disclosed: the
    /// guard accessories gate at infliction (retail's applier writes the bit
    /// unconditionally and the per-frame guard sweep `FUN_8004CE2C` clears
    /// it next frame - same steady state); and the group arm's queued-action
    /// drop (`sb zero,0x1de`) rides the engine's status block gates (a
    /// petrified actor's turn is skipped), while its reserved-item refund
    /// (`FUN_800421D4(id, 1)`) is not modelled.
    ///
    /// PORT: FUN_800402F4 (class-9 / class-10 arms - live wiring; the roll
    /// kernel carries the instruction-level cite)
    pub(in crate::world) fn apply_enemy_agl_status(
        &mut self,
        caster: u8,
        move_id: u8,
        targets: &[u8],
    ) {
        use vm::status_effects::{StatusKind, agl_status_inflict_roll};
        /// Accessory-passive index `0x19` = **Curse Guard** (Magic Amulet).
        const CURSE_GUARD_BIT: u32 = 1 << 0x19;
        /// Accessory-passive index `0x1A` = **Stone Guard** (Stone Amulet).
        const STONE_GUARD_BIT: u32 = 1 << 0x1A;
        let kind = match move_id {
            0x3C | 0xB9 => StatusKind::Stone,
            0x40 | 0x53 => StatusKind::Curse,
            _ => return,
        };
        if (caster as usize) < self.party_count as usize {
            return;
        }
        let attacker_agl = self
            .battle_accuracy
            .get(caster as usize)
            .copied()
            .unwrap_or(0);
        let pc = self.party_count;
        for &t in targets {
            // Retail's `sltiu v0,s0,0x3` party gate (both arms).
            if t >= pc {
                continue;
            }
            let target_agl = self.battle_accuracy.get(t as usize).copied().unwrap_or(0);
            // One draw per rolled target, in retail call order.
            let rand = self.next_rng();
            if !agl_status_inflict_roll(attacker_agl, target_agl, rand) {
                continue;
            }
            let guard_bit = match kind {
                StatusKind::Stone => STONE_GUARD_BIT,
                _ => CURSE_GUARD_BIT,
            };
            let bits = self
                .character_ability_bits
                .get(t as usize)
                .copied()
                .unwrap_or(0);
            if bits & (guard_bit | MASTER_GUARD_BIT) != 0 {
                continue;
            }
            self.status_effects.apply(t, kind);
        }
    }

    /// Confuse retarget: a confused actor "acts uncontrollably", so once its
    /// single-target action's target is picked, flip it to a random living
    /// member of the *opposite* side via the ported `FUN_801E7320` resolver
    /// ([`Self::resolve_monster_target`]). Consumes battle RNG (reroll-while-
    /// dead), matching retail's structure; no-op when `slot` isn't confused.
    ///
    /// Retail triggers the resolver off the actor's `+0x16E` status word
    /// (`field_flags & 0x380`); the engine bridges directly from
    /// [`StatusKind::Confuse`] instead, since the bit-set site is the still-open
    /// capture thread and the engine tracks status by kind. Wired for both the
    /// monster physical strike ([`Self::take_monster_turn`]) and the party
    /// physical strike ([`Self::arm_party_physical`], which the live loop routes
    /// a confused party member through instead of opening the command menu).
    /// Confused monster *casts* flip via [`Self::confuse_retarget_cast`] (the
    /// cast path resolves a targets `Vec` rather than `active_target`).
    pub(in crate::world) fn maybe_confuse_retarget(&mut self, slot: u8) {
        if self.actor_is_confused(slot) {
            self.resolve_monster_target(slot);
        }
    }

    /// Confuse retarget for a *cast*: a confused caster's spell lands on the
    /// opposite side (uncontrollably), mirroring the physical retarget. The
    /// engine's monster cast resolves targets into a `Vec` (not the single
    /// `active_target` byte `FUN_801E7320` rewrites), so this flips the whole
    /// list: a single-target cast picks one random living member of the opposite
    /// side (one RNG draw); an area cast hits every living member there. A
    /// self-only cast is left as-is, and the flip is skipped when the opposite
    /// side has no living member. No-op when `caster` isn't confused.
    ///
    /// Monster-only in practice: a confused party member never reaches a cast -
    /// it auto-flails physically (see [`Self::arm_party_physical`]).
    pub(in crate::world) fn confuse_retarget_cast(&mut self, caster: u8, targets: &mut Vec<u8>) {
        if !self.actor_is_confused(caster) || targets.is_empty() {
            return;
        }
        // A self-only cast (e.g. a self-buff) isn't a side-flip target.
        if targets.len() == 1 && targets[0] == caster {
            return;
        }
        let pc = self.party_count.max(1);
        let n = self.actors.len() as u8;
        let opposite_is_monster = targets[0] < pc;
        let opp = if opposite_is_monster { pc..n } else { 0..pc };
        let living: Vec<u8> = opp
            .filter(|&s| {
                self.actors
                    .get(s as usize)
                    .is_some_and(|a| a.battle.liveness != 0)
            })
            .collect();
        if living.is_empty() {
            return;
        }
        if targets.len() == 1 {
            let pick = living[(self.next_rng() as usize) % living.len()];
            *targets = vec![pick];
        } else {
            *targets = living;
        }
    }

    /// True if `slot` carries the Confuse status.
    pub(in crate::world) fn actor_is_confused(&self, slot: u8) -> bool {
        self.status_effects
            .statuses(slot)
            .iter()
            .any(|s| s.kind == vm::status_effects::StatusKind::Confuse)
    }

    /// Arm a generic physical strike for party member `slot` against the first
    /// living opponent, then apply any [`Self::maybe_confuse_retarget`]. Shared
    /// by the non-player-driven party turn and the confused-party turn (a
    /// confused member can't be controlled, so it auto-acts and the retarget
    /// flips its strike to a random living ally). No-op retarget when the member
    /// isn't confused, so the auto-resolve path is RNG-unchanged.
    pub(in crate::world) fn arm_party_physical(&mut self, slot: u8) {
        use vm::battle_action::ActionState;
        let target = self.first_living_opponent_of(slot).unwrap_or(slot);
        self.battle_ctx.active_actor = slot;
        self.clear_action_stream(slot);
        self.battle_ctx.queued_action = 3;
        self.battle_ctx.action_state = ActionState::Begin.as_byte();
        if let Some(a) = self.actors.get_mut(slot as usize) {
            a.battle.active_target = target;
            a.battle.action_category = 3;
        }
        // Same swing stream the player-driven Attack confirm seeds - this is
        // the path retail's no-input queue arm was written for in the first
        // place (`FUN_801EED1C` selects it on the slot's AI control byte
        // `(&DAT_8007BD10)[slot] == 4`). Without it the attack band reads its
        // terminator on byte 0 and the turn passes without a swing.
        self.seed_basic_attack_queue(slot, target);
        self.maybe_confuse_retarget(slot);
    }

    /// Zero an actor's action-parameter stream (`+0x1DF..+0x1EE`) and rewind
    /// its strike cursor, the state a freshly-armed action starts from.
    ///
    /// Retail re-seeds the stream per action (`FUN_801EED1C` for a party
    /// action, the AI picker for a monster one) and clears it wholesale at the
    /// round boundary (`FUN_801D88CC` loop A, ported as
    /// [`crate::battle_round::BattleRound::boundary`]), so no action ever
    /// reads a byte another action left behind. The engine had no per-action
    /// clear, and the gap was a **soft-lock**: the monster-AI picker writes
    /// the chosen spell id into `params[0]` before the cast is folded, and a
    /// cast that cannot fold (no catalog entry for the id - the default on a
    /// host with no spell catalog) falls back to a physical strike that then
    /// walked the spell id as if it were a swing-anim byte. The attack chain
    /// staged it, the staged id happened to equal the actor's current anim id,
    /// the anim commit's `q == current_anim` early-out skipped the
    /// `ADVANCE_DONE` clear, and the chain's pacing gate held at `AttackChain`
    /// (`0x1E`) for the rest of the session.
    ///
    /// REF: FUN_801D88CC (the round-boundary clear this is the per-action
    /// sibling of)
    /// REF: FUN_801EED1C (the party-side per-action seeder whose absence is
    /// what leaves a stale stream behind)
    pub(in crate::world) fn clear_action_stream(&mut self, slot: u8) {
        let len = vm::battle_formulas::ACTION_STREAM_RANGE.len();
        if let Some(a) = self.actors.get_mut(slot as usize) {
            for b in a.battle.params.iter_mut().take(len) {
                *b = 0;
            }
            a.battle.strike_index = 0;
            // The staged Tactical-Arts profile belongs to the stream that is
            // being cleared - keeping it would re-key the next action's
            // strikes to the previous turn's art.
            a.battle.clear_art_profile();
        }
    }

    /// Arm a generic physical strike for monster `slot` against the first
    /// living party member (fallback when a picked cast can't fold).
    fn arm_monster_physical(&mut self, slot: u8) {
        use vm::battle_action::ActionState;
        self.arm_monster_strike_budget(slot);
        self.clear_action_stream(slot);
        let target = self.first_living_opponent_of(slot).unwrap_or(slot);
        self.battle_ctx.queued_action = 3;
        self.battle_ctx.action_state = ActionState::Begin.as_byte();
        if let Some(a) = self.actors.get_mut(slot as usize) {
            a.battle.active_target = target;
            a.battle.action_category = 3;
        }
        self.maybe_confuse_retarget(slot);
    }

    /// Compute + store the AGL-driven multi-action budget for the physical swing
    /// monster `slot` is about to make - the enemy analogue of the party Arts AP
    /// gauge. Clean-room port of the AGL-gauge spending loop in the picker
    /// `FUN_801E9FD4`: the monster gets one swing per action its per-round AGL
    /// gauge ([`crate::monster_catalog::MonsterDef::agl`]) can afford from its
    /// physical swing costs (`action_costs`), capped at 15, via
    /// [`vm::battle_action::enemy_action_budget`]. Draws battle RNG (one roll per
    /// candidate pick) exactly as retail's picker does.
    ///
    /// Falls back to a single swing - drawing **no** RNG - when the monster has
    /// no AGL gauge or no costed swing actions (the disc-free / synthetic
    /// catalog), so unbudgeted battles keep their RNG stream and behaviour
    /// bit-identical. The result is consumed by [`Self::apply_basic_attack`].
    ///
    /// PORT: FUN_801E9FD4
    pub(in crate::world) fn arm_monster_strike_budget(&mut self, slot: u8) {
        let (catalog_agl, costs) = self
            .actors
            .get(slot as usize)
            .and_then(|a| a.battle_monster_id)
            .and_then(|id| self.monster_catalog.get(id))
            .map(|d| (d.agl, d.action_costs.clone()))
            .unwrap_or((0, Vec::new()));
        // The gauge retail spends is the actor's **live** `+0x154`, which the
        // round boundary (`BattleRound::boundary`, the port of `FUN_801D88CC`)
        // restores from `+0x156` once per round. A slot whose base `+0x156` was
        // never seeded is not on that maintenance path at all - a synthetic
        // battle assembled without the formation seeder - so it keeps reading
        // the catalog stat directly and behaves exactly as it did before the
        // gauge existed.
        let base_seeded = self
            .actors
            .get(slot as usize)
            .is_some_and(|a| a.battle.agl_base != 0);
        let agl = if base_seeded {
            self.actors[slot as usize].battle.agl
        } else {
            catalog_agl
        };
        self.monster_strike_budget = if agl > 0 && !costs.is_empty() {
            let stream =
                vm::battle_action::enemy_action_budget(agl, &costs, &mut || self.next_rng());
            // Retail's budget loop spends the gauge it walked. Only do so on
            // the SPD-seeded path: the round-robin fallback has no round
            // boundary to restore it, so an unrestorable gauge would drain to
            // a single swing and stay there.
            if base_seeded && self.any_battle_speed() {
                let spent: u16 = stream
                    .iter()
                    .map(|&pick| u16::from(costs.get(pick as usize).copied().unwrap_or(0)))
                    .sum();
                if let Some(a) = self.actors.get_mut(slot as usize) {
                    a.battle.agl = a.battle.agl.saturating_sub(spent);
                }
            }
            (stream.len() as u8).max(1)
        } else {
            1
        };
    }

    /// Monster-AI action picker - clean-room port of the **generic decision
    /// core** of `FUN_801E9FD4` (`overlay_battle_action_801e9fd4.txt`), the
    /// routine retail runs (from `recompute_battle_order` / `FUN_801DABA4`) to
    /// choose each monster's action.
    ///
    /// Faithful to the core: it rolls `rand % (1 + live_magic_count)` over the
    /// monster's own global magic-attack ids (record `+0x21..=+0x23`, carried on
    /// [`crate::monster_catalog::MonsterDef::magic_attacks`]); a roll of `0`
    /// picks a **physical** strike (target `rand % party_count`), otherwise it
    /// picks magic id `magic[roll-1]` and resolves the target by the spell's
    /// shape byte (`spell_table[id*0xC + 2] & 0x60`), modelled here through the
    /// catalog's [`crate::spells::SpellTarget`]: `OneEnemy` → a random living
    /// party member, `AllEnemies` → the whole living party, `AllAllies` → the
    /// whole living monster band, `OneAlly` → the most-weakened living ally (or
    /// self), `SelfOnly` → self. A cast the monster can't afford from its live
    /// MP (`actor+0x150`) falls back to a physical strike, matching retail's
    /// affordability gate (`actor[0x150] < spell.mp_cost`).
    ///
    /// The large per-monster-id scripted-cast `switch` that follows the core in
    /// retail keys on `DAT_8007BD0C[slot]`, which `FUN_801DA51C` fills from the
    /// encounter record's `[+4 + slot]` monster ids - i.e. the **monster id**,
    /// not an abstract AI-type, so each case is bespoke AI for a specific
    /// monster the engine already identifies via `battle_monster_id`. That
    /// switch is ported in [`crate::monster_ai`] ([`crate::monster_ai::decide`])
    /// and consulted here as an override, followed by the post-switch
    /// recent-target ring ([`crate::monster_ai::apply_recent_target_ring`]). The
    /// companion target resolver `FUN_801E7320` is ported as
    /// [`Self::resolve_monster_target`] (the `monster_setup` hook).
    ///
    /// PORT: FUN_801E9FD4
    /// REF: FUN_801DABA4, FUN_801DA51C
    pub(in crate::world) fn pick_monster_action(&mut self, slot: u8) -> MonsterAction {
        let pc = self.party_count.max(1);

        // --- generic decision core ---
        // The monster's own castable global magic ids (parser already drops the
        // empty `<= 1` slots, so every entry is "live").
        let magic: Vec<u8> = self
            .actors
            .get(slot as usize)
            .and_then(|a| a.battle_monster_id)
            .and_then(|id| self.monster_catalog.get(id))
            .map(|d| d.magic_attacks.clone())
            .unwrap_or_default();
        let mp = self
            .actors
            .get(slot as usize)
            .map(|a| a.battle.mp)
            .unwrap_or(0);

        // Roll over (1 + live_magic_count); 0 => physical. Always consumes one
        // RNG draw, exactly like retail.
        let denom = 1 + magic.len() as u32;
        let roll = self.next_rng() % denom;
        // Provisional choice (category 3 = physical strike, 2 = magic).
        let (mut category, mut spell_id) = (3u8, 0u8);
        let mut target_class;
        if roll != 0 {
            let id = magic[(roll - 1) as usize];
            if let Some(def) = self.spell_catalog.get(id).cloned()
                && mp >= def.mp_cost as u16
            {
                category = 2;
                spell_id = id;
                target_class = self.monster_cast_target_class(slot, &def);
            } else {
                target_class = self.random_living_party_member(pc).unwrap_or(slot);
            }
        } else {
            target_class = self.random_living_party_member(pc).unwrap_or(slot);
        }

        // --- per-monster-id scripted override (the FUN_801E9FD4 switch) + the
        // post-switch recent-target anti-repeat ring. Run in a borrow window
        // with the AI state owned locally so the RNG closure can take `self`.
        if let Some(monster_id) = self
            .actors
            .get(slot as usize)
            .and_then(|a| a.battle_monster_id)
        {
            let (hp, max_hp) = self
                .actors
                .get(slot as usize)
                .map(|a| (a.battle.hp, a.battle.max_hp))
                .unwrap_or((0, 0));
            let allies_with_mp = (0..pc)
                .filter(|&i| {
                    self.actors
                        .get(i as usize)
                        .is_some_and(|a| a.battle.liveness != 0 && a.battle.mp != 0)
                })
                .count() as u8;
            let n = self.actors.len() as u8;
            let ctx = crate::monster_ai::MonsterAiCtx {
                monster_id: (monster_id & 0xFF) as u8,
                monster_index: slot.saturating_sub(pc),
                caster_slot: slot,
                hp,
                max_hp,
                mp,
                party_count: pc,
                monster_count: n.saturating_sub(pc).max(1),
                field_flags: self
                    .actors
                    .get(slot as usize)
                    .map(|a| a.battle.field_flags)
                    .unwrap_or(0),
                allies_with_mp,
                spirit_gauge: self
                    .actors
                    .get(slot as usize)
                    .map(|a| a.battle.spirit_gauge)
                    .unwrap_or(0),
            };
            let mut ai = std::mem::take(&mut self.monster_ai_state);
            let mut spirit_writeback = None;
            if let Some(cast) = crate::monster_ai::decide(&ctx, &mut ai, &mut || self.next_rng()) {
                category = cast.category;
                spell_id = cast.spell_id;
                target_class = cast.target_class;
                spirit_writeback = cast.spirit_gauge_writeback;
            }
            // The 0x8A charge gate clamps the caster's own gauge as it fires
            // (`actor+0x170 = 0x32`). Applied after the RNG borrow window; it
            // draws no RNG, so the determinism stream is untouched.
            if let Some(g) = spirit_writeback
                && let Some(a) = self.actors.get_mut(slot as usize)
            {
                a.battle.spirit_gauge = g;
            }
            // Anti-repeat ring (applies to whichever single party target stands).
            target_class = crate::monster_ai::apply_recent_target_ring(
                target_class,
                spell_id,
                pc,
                &mut ai,
                &mut || self.next_rng(),
            );
            self.monster_ai_state = ai;
        }

        // --- the once-per-pass flee checkpoint (`FUN_801E9FD4` loop bottom,
        // `jal 0x801ec0dc` at 801ea980) ---
        // Retail attempts the enemy escape roll exactly once per picker pass,
        // after the current monster's pick (including the scripted switch) has
        // consumed its draws, and a success OVERRIDES the picked category with
        // 5 (`sb 5, 0x1de(s4)`). The `lw` gate on the battle-flag word
        // `0x8007BAC0` (roll only when it is zero) passes as unset here, the
        // same reading `roll_battle_escape` documents for its `forced` bit.
        if !self.battle_monster_flee_attempted {
            self.battle_monster_flee_attempted = true;
            if self.monster_flee_roll(slot) {
                if let Some(a) = self.actors.get_mut(slot as usize) {
                    a.battle.action_category = 5;
                }
                return MonsterAction::Flee;
            }
        }

        // Optional, gated, NON-FAITHFUL: smarter single-target selection. By
        // now every RNG draw of the faithful random pick (magic roll, target
        // roll + re-roll loop, scripted override, anti-repeat ring) is already
        // consumed, so overriding the chosen slot here does not move the RNG
        // stream. We only redirect a single living-party target (`class < pc`)
        // to the lowest-HP living member; all-party (8) / monster-band (9) /
        // self targets are left exactly as the faithful path resolved them.
        if self.smarter_monster_targeting
            && target_class < pc
            && let Some(low) = self.lowest_hp_living_party_member(pc)
        {
            target_class = low;
        }

        // --- build the action ---
        if category == 2 {
            let targets = self.resolve_class_to_slots(slot, target_class);
            if !targets.is_empty() {
                if let Some(a) = self.actors.get_mut(slot as usize) {
                    a.battle.action_category = 2;
                    a.battle.params[0] = spell_id;
                }
                return MonsterAction::Cast { spell_id, targets };
            }
        }
        // Physical strike (or a cast that resolved no targets).
        let target = if target_class < pc {
            target_class
        } else {
            self.random_living_party_member(pc)
                .or_else(|| self.first_living_opponent_of(slot))
                .unwrap_or(slot)
        };
        if let Some(a) = self.actors.get_mut(slot as usize) {
            a.battle.action_category = 3;
            a.battle.active_target = target;
        }
        MonsterAction::Physical { target }
    }

    /// The live battle-mode counter (`ctx+0x28A`, `_DAT_8007BD24[0x28A]`).
    ///
    /// This is the boss/scripted-mode gate the per-monster AI `switch` reads:
    /// multi-phase bosses (`0xA8`, `0xB4`, `0xB5`, `0xB6`, `0xA2..=0xA4`, …)
    /// change which spell they cast as it advances. `0` in a normal battle.
    pub fn battle_mode(&self) -> u8 {
        self.monster_ai_state.mode_flags
    }

    /// Advance the battle-mode counter by one - the faithful port of the
    /// battle-action SM's `case 0xFF` (`_DAT_8007BD24[0x28A] += 1`), the
    /// boss-phase-transition pseudo-action. A boss script issues action `0xFF`
    /// when the fight crosses a scripted phase boundary; the next monster turn's
    /// `Self::pick_monster_action` then reads the bumped mode through
    /// [`crate::monster_ai::decide`], activating that phase's scripted casts.
    /// The retail counter is a byte, so it wraps at `0xFF`.
    ///
    /// PORT: FUN_801E295C
    pub fn advance_battle_mode(&mut self) {
        self.monster_ai_state.mode_flags = self.monster_ai_state.mode_flags.wrapping_add(1);
    }

    /// Target **class** the generic core picks for a monster casting `def`, by
    /// the spell's [`crate::spells::SpellTarget`] shape (monster's perspective:
    /// enemies = party band, allies = monster band). Single-enemy → a random
    /// living party slot; `AllEnemies` → class `8`; `AllAllies` → class `9`;
    /// `OneAlly` → the most-weakened living ally (or self); `SelfOnly` → self.
    fn monster_cast_target_class(&mut self, slot: u8, def: &crate::spells::SpellDef) -> u8 {
        use crate::spells::SpellTarget;
        let pc = self.party_count.max(1);
        let n = self.actors.len() as u8;
        match def.target {
            SpellTarget::OneEnemy => self.random_living_party_member(pc).unwrap_or(slot),
            SpellTarget::AllEnemies => 8,
            SpellTarget::AllAllies => 9,
            SpellTarget::SelfOnly => slot,
            SpellTarget::OneAlly => {
                let mut best: Option<(u8, u16)> = None;
                for i in pc..n {
                    if let Some(a) = self.actors.get(i as usize)
                        && a.battle.liveness != 0
                        && a.battle.hp < a.battle.max_hp / 2
                        && best.is_none_or(|(_, hp)| a.battle.hp < hp)
                    {
                        best = Some((i, a.battle.hp));
                    }
                }
                best.map(|(i, _)| i).unwrap_or(slot)
            }
        }
    }

    /// Resolve an absolute target list from a `+0x1DD` target class: `8` = all
    /// living party, `9` = all living monsters, `< party_count` = that single
    /// party slot, otherwise that single monster/self slot.
    fn resolve_class_to_slots(&self, slot: u8, class: u8) -> Vec<u8> {
        let pc = self.party_count.max(1);
        let n = self.actors.len() as u8;
        let alive = |i: u8| {
            self.actors
                .get(i as usize)
                .is_some_and(|a| a.battle.liveness != 0)
        };
        let _ = slot;
        match class {
            8 => (0..pc).filter(|&i| alive(i)).collect(),
            9 => (pc..n).filter(|&i| alive(i)).collect(),
            t if t < n => vec![t],
            // Out-of-range class: no targets (the caller falls back to physical).
            _ => Vec::new(),
        }
    }

    /// Pick a random living party member (`rand % party_count`, re-rolled until
    /// it lands on a living slot), mirroring the party-target roll shared by
    /// `FUN_801E9FD4` and `FUN_801E7320`. `None` only when the whole party is
    /// down. The deterministic LCG cycles every value, so the re-roll loop
    /// always terminates once one member is alive.
    fn random_living_party_member(&mut self, party_count: u8) -> Option<u8> {
        let pc = party_count.max(1);
        let any_alive = (0..pc).any(|i| {
            self.actors
                .get(i as usize)
                .is_some_and(|a| a.battle.liveness != 0)
        });
        if !any_alive {
            return None;
        }
        loop {
            let t = (self.next_rng() % pc as u32) as u8;
            if self
                .actors
                .get(t as usize)
                .is_some_and(|a| a.battle.liveness != 0)
            {
                return Some(t);
            }
        }
    }

    /// Lowest-HP living party member (slot `0..party_count`), ties broken by
    /// the lower slot index. `None` only when the whole party is down.
    /// Consumes no RNG - used solely by the opt-in
    /// [`World::smarter_monster_targeting`] override, which runs after the
    /// faithful random pick has already advanced the RNG stream.
    fn lowest_hp_living_party_member(&self, party_count: u8) -> Option<u8> {
        let pc = party_count.max(1);
        let mut best: Option<(u8, u16)> = None;
        for i in 0..pc {
            if let Some(a) = self.actors.get(i as usize)
                && a.battle.liveness != 0
                && best.is_none_or(|(_, hp)| a.battle.hp < hp)
            {
                best = Some((i, a.battle.hp));
            }
        }
        best.map(|(i, _)| i)
    }

    /// Clean-room port of `FUN_801E7320` - the monster-AI **target resolver**,
    /// invoked by the battle SM (`FUN_801E295C`) at `ActionSeed` as the
    /// `monster_setup` hook for monster actors whose `field_flags & 0x380` is
    /// set. It reads the targeting-class byte the action picker left in
    /// `actor.active_target` (`+0x1DD`) and expands it into a concrete target,
    /// re-rolling the deterministic RNG until it lands on a living actor on the
    /// matching side:
    ///
    /// - **class `0..2`** → a living **monster** slot (`rand % monster_count +
    ///   party_count`); if it lands on self, clears `action_category` and keeps
    ///   self as the target.
    /// - **class `3..6`** → a living **party** slot (`rand % party_count`).
    /// - **class `8`** → 1-in-3 keeps the all-target code `9`, else self.
    /// - **class `7` / other** → 1-in-3 sets the all-target code `8`, else self.
    ///
    /// Retail ctx fields: `ctx[+0]` = party count, `ctx[+1]` = monster count,
    /// `ctx[+0x13]` = active slot - here read from `party_count` / the actor
    /// table / `slot`. See `ghidra/scripts/funcs/overlay_battle_action_801e7320.txt`.
    ///
    /// Retail invokes this from the SM when the actor's `+0x16E` status word has
    /// `field_flags & 0x380` set (the confuse-class statuses). The engine doesn't
    /// model that bitfield, so [`Self::maybe_confuse_retarget`] bridges directly
    /// from [`StatusKind::Confuse`] and calls this on the monster physical-strike
    /// path. (Side detection assumes the retail 3-slot party layout - correct for
    /// a full party; a reduced party + confused monster is a pre-existing edge.)
    ///
    /// PORT: FUN_801E7320
    /// REF: FUN_801E295C
    pub(in crate::world) fn resolve_monster_target(&mut self, slot: u8) {
        let pc = self.party_count.max(1);
        let mc = (self.actors.len() as u8).saturating_sub(pc).max(1);
        let class = match self.actors.get(slot as usize) {
            Some(a) => a.battle.active_target,
            None => return,
        };
        let set_target = |w: &mut Self, t: u8| {
            if let Some(a) = w.actors.get_mut(slot as usize) {
                a.battle.active_target = t;
            }
        };
        let clear_category_self = |w: &mut Self| {
            if let Some(a) = w.actors.get_mut(slot as usize) {
                a.battle.action_category = 0;
                a.battle.active_target = slot;
            }
        };
        if class < 3 {
            // Target a living monster (the caster's own band).
            loop {
                let t = (self.next_rng() % mc as u32) as u8 + pc;
                set_target(self, t);
                if self
                    .actors
                    .get(t as usize)
                    .is_some_and(|a| a.battle.liveness != 0)
                {
                    if t == slot {
                        clear_category_self(self);
                    }
                    return;
                }
            }
        } else if class < 7 {
            // Target a living party member.
            loop {
                let t = (self.next_rng() % pc as u32) as u8;
                set_target(self, t);
                if self
                    .actors
                    .get(t as usize)
                    .is_some_and(|a| a.battle.liveness != 0)
                {
                    return;
                }
            }
        } else if class == 8 {
            if self.next_rng().is_multiple_of(3) {
                set_target(self, 9);
            } else {
                clear_category_self(self);
            }
        } else if self.next_rng().is_multiple_of(3) {
            set_target(self, 8);
        } else {
            clear_category_self(self);
        }
    }
}

#[cfg(test)]
mod impact_proc_tests {
    use super::{ImpactStatusProc, enemy_impact_status_proc};
    use legaia_art::record::EnemyEffect;

    /// `FUN_801E09F8`'s selector ladder covers exactly `3` / `4` / `5`. The
    /// Curse arm (`selector 6`, a 1-in-4 `ori 0x1000`) exists only in the
    /// sibling arts/melee kernel `FUN_801EC3E4`, so an enemy **special** cannot
    /// Curse. Driven with an all-zero RNG, i.e. every probability gate passes -
    /// so a `None` here is the ladder's shape, not a failed roll.
    #[test]
    fn ladder_covers_three_four_five_only() {
        let mut rng = || 0u32;
        assert_eq!(enemy_impact_status_proc(0, true, 0, &mut rng), None);
        assert_eq!(enemy_impact_status_proc(1, true, 0, &mut rng), None);
        assert_eq!(enemy_impact_status_proc(2, true, 0, &mut rng), None);
        assert_eq!(
            enemy_impact_status_proc(3, true, 0, &mut rng),
            Some(ImpactStatusProc {
                effect: EnemyEffect::Other(3),
                rot_limb: None
            })
        );
        assert_eq!(
            enemy_impact_status_proc(4, true, 0, &mut rng),
            Some(ImpactStatusProc {
                effect: EnemyEffect::Other(4),
                rot_limb: None
            })
        );
        assert_eq!(
            enemy_impact_status_proc(5, true, 0, &mut rng),
            Some(ImpactStatusProc {
                effect: EnemyEffect::Other(5),
                rot_limb: Some(0)
            })
        );
        assert_eq!(
            enemy_impact_status_proc(6, true, 0, &mut rng),
            None,
            "no Curse arm in FUN_801E09F8 - that arm is FUN_801EC3E4's"
        );
        assert_eq!(enemy_impact_status_proc(7, true, 0, &mut rng), None);
    }

    /// The DoT arms gate on `rand & 7 == 0`: exactly one of any eight
    /// consecutive RNG values passes, and each arm draws exactly one value.
    #[test]
    fn dot_arms_gate_on_rand_and_seven() {
        for selector in [3u8, 4] {
            let mut draws = 0usize;
            let landed = (0u32..8)
                .filter(|&v| {
                    let mut rng = || {
                        draws += 1;
                        v
                    };
                    enemy_impact_status_proc(selector, true, 0, &mut rng).is_some()
                })
                .count();
            assert_eq!(
                landed, 1,
                "selector {selector} passes on exactly `rand & 7 == 0`"
            );
            assert_eq!(draws, 8, "one draw per call, no more");
        }
    }

    /// The Rot arm's limb is `rand % 3`, and the guard bitfield is read
    /// *before* the draw - a guarded (or non-party) target consumes no RNG at
    /// all, which is what keeps the shared cursor faithful.
    #[test]
    fn rot_arm_rolls_a_limb_and_guards_draw_nothing() {
        for (v, limb) in [(0u32, 0u8), (1, 1), (2, 2), (3, 0), (7, 1)] {
            let mut rng = || v;
            assert_eq!(
                enemy_impact_status_proc(5, true, 0, &mut rng)
                    .and_then(|p| p.rot_limb)
                    .unwrap(),
                limb
            );
        }
        for bits in [super::ROT_GUARD_BIT, super::MASTER_GUARD_BIT] {
            let mut draws = 0usize;
            let mut rng = || {
                draws += 1;
                0u32
            };
            assert_eq!(enemy_impact_status_proc(5, true, bits, &mut rng), None);
            assert_eq!(draws, 0, "a guarded target draws no RNG");
        }
        let mut draws = 0usize;
        let mut rng = || {
            draws += 1;
            0u32
        };
        assert_eq!(
            enemy_impact_status_proc(5, false, 0, &mut rng),
            None,
            "selector 5 on a monster target does nothing (retail's `sltiu a1,0x3`)"
        );
        assert_eq!(draws, 0, "a non-party target draws no RNG");
    }
}
