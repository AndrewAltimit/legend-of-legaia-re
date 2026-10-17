//! Spell table class lookups, module and capture power, the summon roll
//! defender, the caster's magic power byte, and Seru heal amount and spell XP.

use super::*;

impl World {
    /// The `+0` **class byte** of `move_id`'s record in the disc spell table
    /// (`DAT_800754C8 + id*0xC`), or `None` when no disc table is installed.
    ///
    /// One accessor, two consumers, because retail keys two decisions on this
    /// one byte: the damage-kernel pick above ([`Self::is_capture_class_move`],
    /// class `'c'`) and the action-seed band pick
    /// (`legaia_engine_battle_vm::battle_action` `action_seed`'s Magic arm, which
    /// compares the byte against `0x14`). The battle-action host reaches it
    /// through `BattleActionHost::spell_class_byte`.
    pub(in crate::world) fn spell_table_class(&self, move_id: u8) -> Option<u8> {
        self.menu
            .text
            .as_ref()
            .and_then(|t| t.spell_names.as_ref())
            .and_then(|t| t.entry(move_id))
            .map(|e| e.class)
    }

    /// The `+1` sub-class of the same record - the tier half of the pair the
    /// action commit stamps into `actor[+0x1E8]` / `[+0x1E9]`, which selects
    /// which cue group an executing cast expands to.
    pub(in crate::world) fn spell_table_sub_class(&self, move_id: u8) -> Option<u8> {
        self.menu
            .text
            .as_ref()
            .and_then(|t| t.spell_names.as_ref())
            .and_then(|t| t.entry(move_id))
            .map(|e| e.sub_class)
    }

    /// Roll a **capture-class boss cast** whose streamed module calls the
    /// guard-**respecting** wrapper `FUN_801DD4B0`.
    ///
    /// This is the majority arm of the capture band: the census over every
    /// capture-class module finds the bypass wrapper in exactly six of them
    /// ([`CAPTURE_BYPASS_MOVE_IDS`]) and the respecting wrapper in all the
    /// rest - every Songi cast, Neo Star Slash, the enemy-side Evil Seru
    /// Magic, and the whole Earthquake / Chaos Breath / Blade Breath band
    /// (`docs/subsystems/battle-formulas.md`).
    ///
    /// `FUN_801DD4B0` is **not** the shared kernel. Its attacker and defender
    /// rolls are instruction-identical to `FUN_801DD0AC`'s, but its bonus arm
    /// is not, and that is the whole difference:
    ///
    /// | | `FUN_801DD0AC` (shared) | `FUN_801DD4B0` (this) |
    /// |---|---|---|
    /// | bonus threshold | `defender + (power >> 1) + (agl >> 1)` | `defender + power` |
    /// | bonus rebuild | `+ rand % ((power >> 3) + 1) + (agl >> 1) + rand % ((agl >> 3) + 1)` | `+ rand % ((power >> 2) + 1)` |
    /// | draws on the bonus path | two | one |
    ///
    /// So a capture-class respecting cast routed through the shared kernel
    /// lands on a different damage figure *and* advances the RNG cursor by a
    /// different amount. The finisher runs with `bypass_party_resist = false`
    /// ([`INT_WRAPPER_BYPASSES_PARTY_RESIST`]), so the party defender's jewel /
    /// All-Guard ladder applies - which is what community playtests observe
    /// for these casts.
    ///
    /// Stat bridge is [`Self::enemy_move_predamage`]'s: `+0x14C` live HP and
    /// the `+0x168` AGL-derived stat on the attacker, [`Self::summon_roll_defender`]
    /// on the defender. `power` is the move-power table's `+0` scalar; retail's
    /// module bakes a per-hit constant into the call site instead, so the
    /// magnitude stays the engine's own seed even though the arithmetic is the
    /// wrapper's.
    ///
    /// PORT: FUN_801DD4B0 (live wiring; pure kernel in
    /// `battle_damage_wrappers::int_wrapper_predamage`)
    /// The **baked** per-hit power the resident slot-B module hands its
    /// damage wrapper, when the cast's module is one of the six the band's
    /// PORT rows decode ([`legaia_engine_cast::cast_module_ticks`]).
    ///
    /// Retail's capture-class modules do not read the move-power table at
    /// all: each `jal` into `FUN_801DD4B0` / `FUN_801DD6B4` / `FUN_801DD0AC`
    /// sets `a0` from an immediate compiled into the module image
    /// (`addiu a0, zero, 0x1C0` at `0x801F8168` for Plasma Strike, and so on
    /// for the rest). So the table's scalar is not the magnitude the wrapper
    /// sees, and this is the byte-exact substitute; a module with no decoded
    /// shape keeps the table's number.
    ///
    /// REF: FUN_801F74E4, FUN_801F6DD8, FUN_801F6EDC, FUN_801F6A14,
    /// REF: FUN_801F85A8, FUN_801F8D64 (the sites the constants come from)
    pub(super) fn baked_module_power(&self, move_id: u8) -> Option<i32> {
        let entry = self.cast_module_for(move_id)?;
        // --- the fourteen trampoline arms ---
        // Seven more modules bake a power, and two of them bake a *different*
        // one per body - PROT 0941 pairs a wrapper-free Steal against `0xC0`,
        // PROT 0950 pairs `0x100` against `0x29A` - so the entry alone cannot
        // answer for them. Resolve the body through the module's trampoline
        // first and fall back to the entry-keyed table.
        if let Some(body) = legaia_engine_cast::cast_module_ticks::capture_tick_body(entry, move_id)
            && let Some(shape) =
                legaia_engine_cast::cast_arm_ticks::arm_damage_shape_for(entry, body)
            && let Some(power) = shape.powers.first()
        {
            return Some(i32::from(*power));
        }
        legaia_engine_cast::cast_module_ticks::baked_power_for(entry)
            .or_else(|| legaia_engine_cast::cast_module_ticks::capture_site_power(entry, move_id))
            .map(i32::from)
    }

    /// `true` when `move_id` runs PROT 0961's body (Dead End Crisis `0xA1`,
    /// Final Crisis `0xB4`) in a fight whose first formation monster is not
    /// the evolved Cort (`0x8007BD0C != 0xB5`).
    ///
    /// Arm 3 of that body (`0x801F7098`) loads `s0 = 9999` (`li s0,0x270f`
    /// in the `bne` delay slot at `0x801F7398..0x801F739C`) and calls
    /// `FUN_801DD4B0(0x880, ..)` to replace it only when the formation cell
    /// reads `0xB5`. So Koru's round-4 finisher - the one other caster -
    /// writes a flat 9999 per party seat, clamped to its HP, with no wrapper
    /// and no RNG draw: the timed fight's game over.
    ///
    /// REF: FUN_801F69D8 (PROT 0961 arm 3, `0x801F738C..0x801F740C`)
    pub(in crate::world) fn dead_end_crisis_wipes(&self, move_id: u8) -> bool {
        if self.cast_module_for(move_id)
            != Some(legaia_engine_cast::cast_module_ticks::DEAD_END_CRISIS_ENTRY)
        {
            return false;
        }
        let lead = self
            .battle_monster_slots()
            .into_iter()
            .find(|&(_, _, slot)| slot == 0)
            .map(|(_, id, _)| id);
        lead != Some(u16::from(
            legaia_engine_cast::cast_module_ticks::DEAD_END_CRISIS_ROLL_FORMATION,
        ))
    }

    /// The magnitude seed for a capture-class special a **monster** casts:
    /// the module's baked `a0` ([`Self::baked_module_power`]). The move-power
    /// table carries no record for most of these ids, and retail's module
    /// never reads it, so without this the fold had nothing to roll with.
    /// `None` for a party caster, a non-capture id, or a body with no damage
    /// site (a status-only special such as Glare).
    pub(in crate::world) fn monster_capture_power(&self, caster: u8, move_id: u8) -> Option<i32> {
        if (caster as usize) < self.party.party_count as usize
            || !self.is_capture_class_move(move_id)
        {
            return None;
        }
        self.baked_module_power(move_id)
    }

    /// Build the defender-side [`SummonRollActor`] for an actor slot - the
    /// stat bridge shared by the monster special-attack roll
    /// ([`Self::enemy_move_predamage`]) and the player summon roll
    /// ([`Self::player_summon_predamage`]). INT = `battle_accuracy` (the
    /// `+0x168` halfword); HP = `battle.hp` (`+0x14c`); the two
    /// defense terms (`+0x15c`/`+0x160`) = the [`crate::world::BattleState::defense_split`]
    /// (UDF, LDF) pair, falling back to the single [`crate::world::BattleState::defense`];
    /// the guard byte (`+0x1de`) reads `4` for a party member in the Spirit
    /// stance, which the scale stage `FUN_801dd864` doubles the defender roll
    /// on (`0x801DD8F4`) ahead of the finisher's own guard halve; status-weaken
    /// (`+0x16e`) defaults to none.
    pub(in crate::world::battle) fn summon_roll_defender(
        &self,
        slot: u8,
    ) -> Option<legaia_engine_battle_vm::battle_formulas::SummonRollActor> {
        let t = self.actors.get(slot as usize)?;
        let (stat_a, stat_b) = self
            .battle
            .defense_split
            .get(slot as usize)
            .copied()
            .flatten()
            .unwrap_or_else(|| {
                (
                    self.battle.defense.get(slot as usize).copied().unwrap_or(0),
                    0,
                )
            });
        Some(legaia_engine_battle_vm::battle_formulas::SummonRollActor {
            hp: t.battle.hp,
            agl: self
                .battle
                .accuracy
                .get(slot as usize)
                .copied()
                .unwrap_or(0),
            stat_a,
            stat_b,
            // `FUN_801DD864` reads the defender's `+0x16E` Venom / Toxic bits.
            status: self.raw_status_word(slot),
            guard: if self
                .battle
                .guarding
                .get(slot as usize)
                .copied()
                .unwrap_or(false)
            {
                legaia_engine_battle_vm::battle_formulas::GUARD_SPIRIT_STANCE
            } else {
                0
            },
        })
    }

    /// The caster's per-spell magic-power byte, read the way the scale stage
    /// `FUN_801dd864` does: search the character record's 32-entry spell-id
    /// list (record `+0x13D`, live `0x80084845 + char*0x414`) for the cast
    /// spell id and take the parallel level byte (`+0x161`, live
    /// `0x80084869`). The retail loop bounds the search at `0x20` entries even
    /// though the record field holds 36; mirrored here. Returns `1` (the
    /// [`vm::battle_formulas::apply_magic_power`] identity) when the roster
    /// doesn't carry the spell - a fresh cast with no recorded level.
    pub(in crate::world) fn caster_magic_power_byte(&self, caster: u8, spell_id: u8) -> u8 {
        const RETAIL_SEARCH_BOUND: usize = 0x20;
        // Battle ordinal -> the occupying character's record.
        let Some(record) = self
            .party
            .roster
            .members
            .get(self.party_roster_slot(caster as usize))
        else {
            return 1;
        };
        let list = record.spell_list();
        list.ids
            .iter()
            .take(RETAIL_SEARCH_BOUND)
            .position(|&id| id == spell_id)
            .map(|i| list.levels[i])
            .unwrap_or(1)
    }

    /// The restore a player Seru **heal** cast lands, straight off the owning
    /// module's own tick kernel, or `None` for a cast whose module computes no
    /// restore.
    ///
    /// Three player Seru modules heal - two in the base block, Spoon in the
    /// evolved one - and none reads a spell-table magnitude:
    ///
    /// * **Vera** (`0x83`, PROT 0905 arm 9): `magic_level * 0x20 + 0xE0`,
    ///   clamped to the target's missing HP with retail's own signed compare
    ///   ([`legaia_engine_battle_vm::battle_formulas::vera_heal_amount`]).
    /// * **Orb** (`0x89`, PROT 0911 arm 9): `(magic_level << 6) + 0x1C0` -
    ///   exactly twice `spell-table.md`'s band-wide `(power << 5) + 0xE0`, so
    ///   the formula is per module
    ///   ([`legaia_engine_battle_vm::battle_formulas::orb_heal_amount`]).
    /// * **Spoon** (`0x91`, PROT 0919 arm 7): `(magic_level << 7) + 0x380`,
    ///   unsigned clamp
    ///   ([`legaia_engine_battle_vm::battle_formulas::spoon_heal_amount`]).
    ///
    /// The magic level is the caster's per-spell byte
    /// ([`Self::caster_magic_power_byte`]), which is the record field both
    /// modules scan for.
    ///
    /// # Why the magnitude comes here rather than out of the tick
    ///
    /// The engine folds a cast's HP outcome exactly once, at
    /// [`Self::cast_spell_on_slots_prepaid`], and the band's tick bodies run
    /// with their damage / heal inputs neutral so a driven band cannot apply
    /// an outcome twice. Keeping that and routing the module's own number into
    /// the fold gives retail's magnitude without a second owner - and it is
    /// what the damage half already does
    /// ([`Self::player_summon_predamage`] feeds the summon roll into the same
    /// fold). The ticks stay the kernels; the fold stays the applier.
    ///
    /// REF: FUN_801F69D8 (PROT 0905 arm 9 restore, PROT 0911 arm 9 heal sweep)
    pub(super) fn seru_tick_heal_amount(
        &self,
        caster: u8,
        spell_id: u8,
        target_hp: u16,
        target_hp_max: u16,
    ) -> Option<u16> {
        let level = self.caster_magic_power_byte(caster, spell_id);
        match spell_id {
            VERA_SPELL_ID => Some(legaia_engine_battle_vm::battle_formulas::vera_heal_amount(
                level,
                target_hp,
                target_hp_max,
            )),
            ORB_SPELL_ID => {
                let want = legaia_engine_battle_vm::battle_formulas::orb_heal_amount(level);
                // Retail's sweep clamps each seat to its own missing HP before
                // storing; the fold caps at max HP anyway, so clamping here
                // only makes the reported amount the restored one.
                let missing = u32::from(target_hp_max.saturating_sub(target_hp));
                Some(want.min(missing).min(u32::from(u16::MAX)) as u16)
            }
            // PROT 0919's arm 7: the same unsigned clamp over its own amount
            // (`sltu v0,a0,a3` at `0x801F8078`).
            SPOON_SPELL_ID => {
                let want = legaia_engine_battle_vm::battle_formulas::spoon_heal_amount(level);
                let missing = u32::from(target_hp_max.saturating_sub(target_hp));
                Some(want.min(missing).min(u32::from(u16::MAX)) as u16)
            }
            _ => None,
        }
    }

    /// The spell XP a striking-free high-block module credits in its own arm,
    /// read off the battle before the cast folds.
    ///
    /// - **Horn** walks every party seat and credits `+3` for a seat missing
    ///   HP and `+1` for a seat carrying a status
    ///   ([`legaia_engine_battle::magic_xp::horn_seat_xp_gain`]).
    /// - **Jedo** outside a scripted fight (`jedo_banish`) credits
    ///   [`legaia_engine_battle::magic_xp::JEDO_XP_PER_LIVING_MONSTER`] per monster seat
    ///   whose HP is non-zero.
    ///
    /// Every other id returns `0` - its XP comes from the per-hit tail.
    ///
    /// REF: FUN_801F6A74 (PROT 0930), FUN_801F6A58 (PROT 0931)
    pub(super) fn inline_module_spell_xp(&self, spell_id: u8, jedo_banish: bool) -> u32 {
        let party_count = self.party.party_count as usize;
        match spell_id {
            HORN_SPELL_ID => (0..party_count)
                .filter_map(|seat| self.actors.get(seat).map(|a| (seat, a)))
                .map(|(seat, a)| {
                    legaia_engine_battle::magic_xp::horn_seat_xp_gain(
                        a.battle.hp,
                        a.battle.max_hp,
                        self.battle.status_effects.display_flags(seat as u8) != 0,
                    )
                })
                .sum(),
            JEDO_SPELL_ID if jedo_banish => {
                let living = self
                    .actors
                    .iter()
                    .take(BATTLE_SLOTS)
                    .skip(party_count)
                    .filter(|a| a.battle.max_hp != 0 && a.battle.hp != 0)
                    .count() as u32;
                living * legaia_engine_battle::magic_xp::JEDO_XP_PER_LIVING_MONSTER
            }
            _ => 0,
        }
    }

    /// The spell XP a heal module's arm credits for one healed seat
    /// ([`legaia_engine_battle::magic_xp::module_heal_xp_gain`]) at the caster's level.
    ///
    /// REF: FUN_801F69D8 (PROT 0905 / 0911 / 0919 heal arms)
    pub(super) fn seru_tick_heal_xp(
        &self,
        caster: u8,
        spell_id: u8,
        target_hp: u16,
        target_hp_max: u16,
    ) -> u32 {
        let level = self.caster_magic_power_byte(caster, spell_id);
        legaia_engine_battle::magic_xp::module_heal_xp_gain(
            spell_id,
            level,
            target_hp,
            target_hp_max,
        )
    }
}
