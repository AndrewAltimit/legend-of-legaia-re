//! Status effects an enemy move lands: the impact status proc, move and agility
//! statuses, the stone cancel and the confusion retarget.

use super::*;

/// Accessory-passive index `0x18` = **Rot Guard** (Forest Amulet), i.e. bit
/// `0x0100_0000` of the character record's `+0xF4` ability bitfield - the
/// first of the two words the Rot arm tests
/// (`lui v0,0x100; and; bne` at `0x801E16D0..0x801E16D8`).
/// See [`docs/formats/accessory-passive-table.md`](../../../../../docs/formats/accessory-passive-table.md).
pub(super) const ROT_GUARD_BIT: u32 = 1 << 0x18;
/// Accessory-passive index `0x1C` = **Master Guard** (Wonder Amulet), bit
/// `0x1000_0000`, the second word the Rot arm tests
/// (`lui v0,0x1000; and; bne` at `0x801E16E0..0x801E16E8`).
pub(super) const MASTER_GUARD_BIT: u32 = 1 << 0x1C;

/// What the impact status proc rolled - see [`enemy_impact_status_proc`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::world) struct ImpactStatusProc {
    /// The status byte to feed
    /// [`legaia_engine_battle_vm::status_effects::StatusEffectTracker::apply_from_enemy_effect`].
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
        if (caster as usize) < self.party.party_count as usize {
            return;
        }
        let Some(selector) = self
            .tables
            .move_power
            .as_ref()
            .and_then(|c| c.record_for_move_id(move_id))
            .map(|r| r.impact_effect())
            .filter(|&s| s != 0)
        else {
            return;
        };
        let targets: Vec<u8> = self
            .battle
            .hit_fx
            .get(hit_fx_start..)
            .unwrap_or(&[])
            .iter()
            .filter(|fx| !fx.is_heal)
            .map(|fx| fx.target_slot)
            .collect();
        let party_count = self.party.party_count;
        for target in targets {
            // `sb v1,0x21f(...)` - the lingering status visual latches on the
            // selector itself, before and independent of the proc roll.
            if let Some(a) = self.actors.get_mut(target as usize) {
                a.pending_status = Some(legaia_art::record::EnemyEffect::from_byte(selector));
            }
            // The same three stores are the impact-tint triple
            // (`0x801E15AC..0x801E15EC`: `+0x21F = sel`, `+0x04 =
            // 0x801F53D4[sel - 1]`, `+0x0C = 0x1000`) - Tail Fire's red
            // Vahn in the `battle_gimard_tail_fire_a` capture.
            self.arm_impact_tint(target as usize, selector);
            let target_is_party = target < party_count;
            let ability_bits = if target_is_party {
                self.party
                    .character_ability_bits
                    .get(target as usize)
                    .copied()
                    .unwrap_or(0)
            } else {
                0
            };
            let rolled = {
                let rng = &mut || self.next_rand();
                enemy_impact_status_proc(selector, target_is_party, ability_bits, rng)
            };
            let Some(proc) = rolled else {
                continue;
            };
            let applied = self
                .battle
                .status_effects
                .apply_from_enemy_effect(target, proc.effect);
            if let (Some(legaia_engine_battle_vm::status_effects::StatusKind::Rot), Some(limb)) =
                (applied, proc.rot_limb)
            {
                self.battle.status_effects.set_rot_limb(target, limb);
            }
        }
    }

    /// The **Stone / Curse infliction arm** of a capture-class boss cast -
    /// the engine stand-in for the streamed module's
    /// `FUN_800402F4(9 | 10, ..)` calls (jump-table entries decoded off the
    /// disc: class 9 = `0x80041C70` `ori 0x4` Stone, class 10 = `0x80041E64`
    /// `ori 0x1000` Curse; roll kernel
    /// [`legaia_engine_battle_vm::status_effects::agl_status_inflict_roll`]).
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
    /// lands the bit. One engine-model equivalence, disclosed: the guard
    /// accessories gate at **infliction** here, where retail's applier writes
    /// the bit unconditionally and the per-frame guard sweep `FUN_8004CE2C`
    /// clears it the next frame - the same steady state, one frame apart.
    ///
    /// **The Stone arm's tail is asymmetric with the Curse one, and both
    /// halves are ported.** Between the roll and the exit the class-9 arm
    /// makes exactly three stores (`0x80041CEC..0x80041D4C` for the single
    /// target, `0x80041DE0..0x80041E38` for the all-party loop, byte-identical
    /// bodies): `+0x16E |= 4`; then, **only** when the target had a queued
    /// **Item** action (`+0x1DE == 1`) that has not been spent yet (its
    /// initiative key `+0x16C != 0`), `FUN_800421D4(target[+0x1DF], 1)` puts
    /// the reserved item back in the bag; then `+0x1DE = 0` unconditionally,
    /// cancelling whatever the target had queued. The class-10 (Curse) arm
    /// (`0x80041EE0..0x80041EEC`) makes **one** store - the bit - and neither
    /// refunds nor cancels, which is why Curse only grays the Magic command
    /// while Stone eats the turn outright.
    ///
    /// PORT: FUN_800402F4 (class-9 / class-10 arms - live wiring; the roll
    /// kernel carries the instruction-level cite)
    /// REF: FUN_800421D4 (the bag-add primitive the refund calls; kernel
    /// mirrored in `legaia_save::retail_inventory`)
    pub(in crate::world) fn apply_enemy_agl_status(
        &mut self,
        caster: u8,
        move_id: u8,
        targets: &[u8],
    ) {
        use legaia_engine_battle_vm::status_effects::{StatusKind, agl_status_inflict_roll};
        /// Accessory-passive index `0x19` = **Curse Guard** (Magic Amulet).
        const CURSE_GUARD_BIT: u32 = 1 << 0x19;
        /// Accessory-passive index `0x1A` = **Stone Guard** (Stone Amulet).
        const STONE_GUARD_BIT: u32 = 1 << 0x1A;
        let kind = match move_id {
            0x3C | 0xB9 => StatusKind::Stone,
            0x40 | 0x53 => StatusKind::Curse,
            _ => return,
        };
        if (caster as usize) < self.party.party_count as usize {
            return;
        }
        let attacker_agl = self
            .battle
            .accuracy
            .get(caster as usize)
            .copied()
            .unwrap_or(0);
        let pc = self.party.party_count;
        // The party seat Glare turned to stone, for its caption.
        let mut petrified = None;
        for &t in targets {
            // Retail's `sltiu v0,s0,0x3` party gate (both arms).
            if t >= pc {
                continue;
            }
            let target_agl = self.battle.accuracy.get(t as usize).copied().unwrap_or(0);
            // One draw per rolled target, in retail call order.
            let rand = self.next_rand();
            if !agl_status_inflict_roll(attacker_agl, target_agl, rand) {
                continue;
            }
            let guard_bit = match kind {
                StatusKind::Stone => STONE_GUARD_BIT,
                _ => CURSE_GUARD_BIT,
            };
            let bits = self
                .party
                .character_ability_bits
                .get(t as usize)
                .copied()
                .unwrap_or(0);
            if bits & (guard_bit | MASTER_GUARD_BIT) != 0 {
                continue;
            }
            self.battle.status_effects.apply(t, kind);
            petrified = petrified.or((kind == StatusKind::Stone).then_some(t));
            if kind == StatusKind::Stone {
                self.stone_cancels_queued_action(t);
            }
        }
        if move_id == GLARE_ID {
            self.raise_glare_caption(petrified);
        }
    }

    /// Glare's bottom-bar caption (HUD element `0x5B`), raised by the PROT
    /// 0940 body `0x801F69F8` as it resolves: a party seat turned to stone
    /// gets its character's line - `0x801F85E8` / `0x801F8600` /
    /// `0x801F8618` for character ids 1 / 2 / 3 (`0x8007BD10[seat]`,
    /// `0x801F70F0..0x801F7170`), each `0xC1 k` " has turned to stone." -
    /// and anything else (a failed roll, a Stone or Master Guard bearer, a
    /// monster seat) gets `0x801F8630`, "Nothing happened." (`0x801F7174..
    /// 0x801F7184`). Both raise through `FUN_801D8DE8(0x5B, 0)` with the
    /// record's content word `0x800774AC` pointed at the string, and mark
    /// `ctx[+0x18] = 0x5B` (`0x801F7188..0x801F719C`). The line is read
    /// off the resident module image, its name escapes spliced with the
    /// roster's names; it rides the steal caption's seat and closes the
    /// same way.
    ///
    /// PORT: overlay_cast_glare_divide_0940_801f69f8 (PROT 0940; the caption raise)
    fn raise_glare_caption(&mut self, petrified: Option<u8>) {
        const NOTHING_HAPPENED: u32 = 0x801F_8630;
        const TURNED_TO_STONE: [u32; 3] = [0x801F_85E8, 0x801F_8600, 0x801F_8618];
        let va = match petrified {
            None => Some(NOTHING_HAPPENED),
            Some(seat) => TURNED_TO_STONE
                .get(self.party_roster_slot(usize::from(seat)))
                .copied(),
        };
        let Some(raw) = va.and_then(|va| {
            let module = self.casting.effect_pool.as_ref()?.module(GLARE_ENTRY)?;
            module_line(&module.bytes, va)
        }) else {
            return;
        };
        let text = self.expand_name_escapes(&raw);
        self.battle.steal_caption = Some(legaia_engine_battle::battle_steal::StealCaption {
            text,
            owner: self.battle_ctx.active_actor,
        });
    }

    /// The two stores the class-9 (Stone) arm makes **after** the bit, and
    /// that the class-10 (Curse) arm does not.
    ///
    /// `0x80041CFC..0x80041D4C`: reload the target, and if its action
    /// category is Item (`+0x1DE == 1`) *and* its initiative key is still
    /// live (`+0x16C != 0` - i.e. the queued turn has not been taken), hand
    /// the reserved item id (`+0x1DF`) back to the bag through
    /// `FUN_800421D4(id, 1)`; then clear `+0x1DE` whether or not the refund
    /// fired. Both stores are repeated verbatim per slot in the all-party
    /// loop at `0x80041DF0..0x80041E38`.
    ///
    /// The refund is what stops a petrify from eating the item as well as
    /// the turn. It is deliberately *not* gated on the item being a real
    /// catalog entry - retail adds whatever byte sits at `+0x1DF`, and the
    /// bag is a byte array - but id `0` is the empty-slot sentinel whose
    /// quantity bump the add primitive itself skips (`andi v0,t0,0xff` /
    /// `beq v0,zero` at `0x800422D4..0x800422D8`), so it adds nothing here
    /// either.
    ///
    /// PORT: FUN_800402F4 (`0x80041CFC..0x80041D4C`)
    pub(in crate::world) fn stone_cancels_queued_action(&mut self, target: u8) {
        use legaia_engine_battle_vm::battle_action::ActionCategory;
        let Some(actor) = self.actors.get_mut(target as usize) else {
            return;
        };
        let refund = (actor.battle.action_category == ActionCategory::Item.as_byte()
            && actor.battle.init_key != 0)
            .then_some(actor.battle.params[0])
            .filter(|&id| id != 0);
        // `sb zero,0x1de(v0)` at `0x80041D4C` - unconditional on the success
        // arm, so a petrified actor loses its turn even when nothing was
        // refundable.
        actor.battle.action_category = 0;
        if let Some(item_id) = refund {
            let slot = self.party.inventory.entry(item_id).or_insert(0);
            *slot = slot.saturating_add(1).min(legaia_save::STACK_CAP);
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
    /// monster physical strike ([`Self::take_monster_turn`]); the party-side
    /// callers (the plain strike, the AI companion) are no-ops here.
    /// Confused monster *casts* flip via [`Self::confuse_retarget_cast`] (the
    /// cast path resolves a targets `Vec` rather than `active_target`).
    ///
    /// Monster seats only: `FUN_801E7320` has one caller in the disc, the
    /// seed's `jal 0x801e7320` at `0x801E2CA0`, which a party seat never
    /// reaches (`sltiu v0,a0,0x3` at `0x801E2C70` sends seats `0..=2` to the
    /// builder instead). A confused party member keeps its target.
    pub(in crate::world) fn maybe_confuse_retarget(&mut self, slot: u8) {
        if slot < self.party.party_count {
            return;
        }
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
    /// Monster-only: a confused party member's committed cast keeps its
    /// targets.
    pub(in crate::world) fn confuse_retarget_cast(&mut self, caster: u8, targets: &mut Vec<u8>) {
        // Monster seats only, as `maybe_confuse_retarget`.
        if caster < self.party.party_count {
            return;
        }
        if !self.actor_is_confused(caster) || targets.is_empty() {
            return;
        }
        // A self-only cast (e.g. a self-buff) isn't a side-flip target.
        if targets.len() == 1 && targets[0] == caster {
            return;
        }
        let pc = self.party.party_count.max(1);
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
            let pick = living[(self.next_rand() as usize) % living.len()];
            *targets = vec![pick];
        } else {
            *targets = living;
        }
    }

    /// True if `slot` carries the Confuse status.
    pub(in crate::world) fn actor_is_confused(&self, slot: u8) -> bool {
        self.battle
            .status_effects
            .statuses(slot)
            .iter()
            .any(|s| s.kind == legaia_engine_battle_vm::status_effects::StatusKind::Confuse)
    }
}

/// Glare's action id.
const GLARE_ID: u8 = 0x3C;
/// The module Glare's body lives in (PROT 0940).
const GLARE_ENTRY: u32 = 940;

/// A zero-terminated line out of a slot-B module image at `va`, with the
/// dialog escapes `0xC0..=0xCF` taken as two bytes (their operand may be
/// `0`: `0xC1 0x00` names the first party member).
pub(in crate::world) fn module_line(image: &[u8], va: u32) -> Option<Vec<u8>> {
    let off = va.checked_sub(legaia_overlay_images::summon_overlay::SUMMON_OVERLAY_LINK_BASE)?;
    let tail = image.get(off as usize..)?;
    let mut i = 0;
    while i < tail.len() && tail[i] != 0 {
        i += if (0xC0..=0xCF).contains(&tail[i]) {
            2
        } else {
            1
        };
    }
    Some(tail[..i.min(tail.len())].to_vec())
}

#[cfg(test)]
mod glare_caption_tests {
    use super::module_line;

    /// `0xC1 0x00` is one escape, not a terminator.
    #[test]
    fn a_name_escape_with_a_zero_operand_does_not_end_the_line() {
        let base = legaia_overlay_images::summon_overlay::SUMMON_OVERLAY_LINK_BASE;
        let mut img = vec![0u8; 4];
        img.extend_from_slice(b"\xC1\x00 has turned to stone.\0tail");
        let line = module_line(&img, base + 4).unwrap();
        assert_eq!(line, b"\xC1\x00 has turned to stone.");
    }
}
