//! Pre-rolled cast damage: the enemy move, the capture-bypass and capture-
//! respect paths, the player summon, and the enemy move power.

use super::*;

impl World {
    /// Roll a monster special-attack's damage through the faithful
    /// arts/physical kernel ([`legaia_engine_battle_vm::battle_formulas::arts_physical_predamage`],
    /// the non-summon branch of `FUN_801dd0ac`) seeded with the move's real
    /// per-move power (the move-power table's whole `+0` word). Returns the
    /// pre-finisher damage clamped to `1..=9999`, or `None` when the table
    /// isn't loaded or `move_id` has no power record - in which case the caller
    /// keeps the MP-scaled spell placeholder.
    ///
    /// Stat bridge (all read live off the actor arrays, faithful to the retail
    /// fields the kernel reads): attacker/target AGL = `battle_accuracy`
    /// (`+0x168`, the AGL-derived stat); HP = `battle.hp` (`+0x14c`); the two
    /// defender defense terms (`+0x15c`/`+0x160`) = the [`crate::world::BattleState::defense_split`]
    /// (UDF, LDF) pair, falling back to the single [`crate::world::BattleState::defense`].
    /// Element affinity comes from [`Self::enemy_affinity_pct`] when the
    /// affinity tables are installed (`matrix[enemy_element][party_element]`,
    /// `FUN_801dd864`), else 100 (neutral); status-weaken (`+0x16e`) and the
    /// guard byte (`+0x1de`) default to none. The affinity scale is applied
    /// *during* the roll (inside [`arts_physical_predamage_lazy`], before the
    /// conditional bonus-arm threshold, matching retail's scale→bonus order),
    /// so a non-neutral affinity can change whether the lazy bonus pair is
    /// drawn. What is invariant is the *gating*: an uninstalled table resolves
    /// to 100% (no scaling), reproducing the no-affinity baseline - magnitude
    /// and RNG stream bit-identical - so disc-free / synthetic battles are
    /// unperturbed.
    ///
    /// The `rand()` draws are taken in retail call order: attacker ×2, defender
    /// ×1, then the bonus pair ×2 **lazily** - drawn only when the conditional
    /// bonus arm fires ([`arts_physical_predamage_lazy`]). So the global RNG
    /// cursor advances by exactly three draws on the no-bonus path and five on
    /// the bonus path, matching `FUN_801dd0ac`'s call order.
    ///
    /// The roll then runs the closed-form finisher stages (`FUN_801ddb30`):
    /// the party defender's equipment elemental-resistance ladder (the
    /// elemental-guard / All-Guard accessory bits via [`Self::defender_resist`],
    /// tested against the attacking monster's record element `+0x1D`), the
    /// Spirit guard halve, the `rand()%9 + 8` no-damage floor (drawn lazily,
    /// exactly when mitigation zeroes the hit - retail's only finisher draw),
    /// and the 9999 cap.
    ///
    /// A capture-class boss cast ([`CAPTURE_BYPASS_MOVE_IDS`]) does **not**
    /// take this path: those hits run the resist-bypass wrapper
    /// `FUN_801DD6B4` instead of the shared kernel, which is a different roll
    /// as well as a different finisher flag - see
    /// [`Self::capture_bypass_predamage`].
    ///
    /// REF: FUN_801dd0ac (arts/physical branch)
    /// REF: FUN_801ddb30 (finisher stages on the enemy-special path)
    pub(super) fn enemy_move_predamage(
        &mut self,
        attacker: u8,
        target: u8,
        power: i32,
        move_id: u8,
    ) -> Option<u16> {
        use legaia_engine_battle_vm::battle_formulas::{
            DamageFinish, SummonRollActor, arts_physical_predamage_lazy, damage_finish_lazy,
        };

        // PROT 0961's party sweep rolls its wrapper only in the evolved-Cort
        // fight; anywhere else it is a scripted wipe with no roll at all.
        if self.dead_end_crisis_wipes(move_id) {
            return Some(legaia_engine_cast::cast_module_ticks::DEAD_END_CRISIS_WIPE_DAMAGE);
        }
        if CAPTURE_BYPASS_MOVE_IDS.contains(&move_id) {
            let power = self.baked_module_power(move_id).unwrap_or(power);
            // The jewel fix (`WorldToggles::jewel_fix`, the `--jewel-fix`
            // disc patch's twin) retargets these casts at the respecting
            // wrapper, as the patch retargets their `jal FUN_801DD6B4` words
            // at `FUN_801DD4B0`: the whole roll becomes that wrapper's, not
            // only the finisher flag.
            if self.toggles.jewel_fix {
                return self.capture_respect_predamage(attacker, target, power);
            }
            return self.capture_bypass_predamage(attacker, target, power);
        }
        if self.is_capture_class_move(move_id) {
            let power = self.baked_module_power(move_id).unwrap_or(power);
            return self.capture_respect_predamage(attacker, target, power);
        }

        let element_affinity_pct = self.enemy_affinity_pct(attacker, target);
        let a = self.actors.get(attacker as usize)?;
        let attacker_roll = SummonRollActor {
            hp: a.battle.hp,
            agl: self
                .battle
                .accuracy
                .get(attacker as usize)
                .copied()
                .unwrap_or(0),
            // `FUN_801DD864` scales the attacker roll by its `+0x16E` Venom /
            // Toxic bits (`0x801DD974..0x801DD9DC`).
            status: self.raw_status_word(attacker),
            ..Default::default()
        };
        let target_roll = self.summon_roll_defender(target)?;
        // Retail rand() is 15-bit; mask to match its range. The attacker (×2) and
        // defender (×1) draws are always taken; the bonus pair is drawn lazily by
        // the closure below, only when the bonus arm fires, so the shared RNG
        // cursor advances exactly as retail's does (three or five draws).
        let rng3 = [
            self.next_rand() as u16,
            self.next_rand() as u16,
            self.next_rand() as u16,
        ];
        let (atk, def) = arts_physical_predamage_lazy(
            power,
            &attacker_roll,
            &target_roll,
            element_affinity_pct,
            rng3,
            || [self.next_rand() as u16, self.next_rand() as u16],
        );
        // Closed-form finisher stages (FUN_801ddb30). The attacker element is
        // the monster record's `+0x1D` byte, read record-direct the way the
        // finisher's resist ladder does; unresolved (synthetic catalogs) it
        // falls back to 7 = non-elemental, which no resist bit matches, so
        // disc-free battles keep magnitude and RNG stream unchanged (a >= 1
        // roll never trips the floor draw without mitigation).
        let attacker_element = self.monster_seat_element(attacker as usize).unwrap_or(7);
        let target_is_party = target < self.party.party_count;
        let finish = DamageFinish {
            predamage: atk.saturating_sub(def).clamp(1, 9999),
            attacker_slot: 3,
            defender_slot: if target_is_party { 0 } else { 3 },
            attacker_element,
            defender_resist: self.defender_resist(target),
            defender_guarding: self
                .battle
                .guarding
                .get(target as usize)
                .copied()
                .unwrap_or(false),
            enemy_defender_halve: self.mystic_shield_up(),
            // The bypass casts are routed away above; every move that reaches
            // the shared kernel passes `param_5 = 0`.
            bypass_party_resist: false,
            summon_power_pct: 100,
            floor_rand: 0,
        };
        let over = damage_finish_lazy(&finish, || self.next_rand() as u16);
        Some(over.min(9999) as u16)
    }

    /// Roll a **capture-class boss cast** whose streamed module calls the
    /// resist-bypass wrapper `FUN_801DD6B4` ([`CAPTURE_BYPASS_MOVE_IDS`]).
    ///
    /// The wrapper is not the shared kernel with a flag flipped - it is its
    /// own roll, and the two differ in three places the port keeps apart
    /// ([`legaia_engine_battle_vm::battle_damage_wrappers`]):
    ///
    /// * the attacker roll mixes the flat ATK-working stat (`+0x158`) in place
    ///   of the shared kernel's doubled `+0x168` term, and draws **one**
    ///   `rand()` where the kernel draws two;
    /// * the defender roll ignores `+0x168` entirely and weights the two
    ///   defence terms (`+0x15C` / `+0x160`) eight times as heavily
    ///   (`>> 1` instead of `>> 4`);
    /// * the bonus arm's threshold is `defender + power` and its re-roll takes
    ///   **one** draw, against the kernel's AGL-widened threshold and pair.
    ///
    /// So the RNG cursor advances by exactly two draws on the no-bonus path
    /// and three on the bonus path, then a fourth only when the finisher's
    /// no-damage floor fires - matching `FUN_801DD6B4`'s call order.
    ///
    /// The finisher runs with `bypass_party_resist =
    /// `[`ATK_WRAPPER_BYPASSES_PARTY_RESIST`], the `param_5 = 1` the wrapper pushes
    /// at `0x801DD828`: the party defender's elemental-jewel / All-Guard
    /// ladder is skipped outright. The affinity scale still reads the caster's
    /// slot element, so only the *defender's* resist stage is dropped.
    ///
    /// Stat bridge is the sibling paths' ([`Self::summon_roll_defender`] for
    /// the defender, [`crate::world::BattleState::attack`] for the attacker's `+0x158`).
    /// `power` is the move-power table's `+0` scalar; retail's module bakes
    /// the per-hit constant into the call site instead, so the magnitude is
    /// the engine's own seed even though the arithmetic is the wrapper's.
    ///
    /// PORT: FUN_801DD6B4 (live wiring; pure kernel in
    /// `battle_damage_wrappers::atk_wrapper_predamage`)
    fn capture_bypass_predamage(&mut self, attacker: u8, target: u8, power: i32) -> Option<u16> {
        use legaia_engine_battle_vm::battle_damage_wrappers::{
            ATK_WRAPPER_BYPASSES_PARTY_RESIST, WrapperAttacker, WrapperDefender,
            atk_wrapper_predamage,
        };
        use legaia_engine_battle_vm::battle_formulas::{DamageFinish, damage_finish_lazy};

        let element_affinity_pct = self.enemy_affinity_pct(attacker, target);
        let attacker_hp = self.actors.get(attacker as usize)?.battle.hp;
        let defender = self.summon_roll_defender(target)?;
        let a = WrapperAttacker {
            hp: attacker_hp,
            // `+0x168` is not read on this path at all.
            agl: 0,
            spell_power: self
                .battle
                .attack
                .get(attacker as usize)
                .copied()
                .unwrap_or(0),
            status: self.raw_status_word(attacker),
        };
        // The bypass wrapper scales through `FUN_801DD864` too (`jal` at
        // `0x801DD7C4`, `a0` = attacker, `a1` = defender): the defender's
        // Spirit double and both sides' Venom / Toxic scales apply.
        let d = WrapperDefender {
            hp: defender.hp,
            agl: 0,
            stat_a: defender.stat_a,
            stat_b: defender.stat_b,
            status: defender.status,
            guard: defender.guard,
        };
        let power = power.max(0) as u32;
        let rng2 = [self.next_rand() as u16, self.next_rand() as u16];
        let (atk, def) = atk_wrapper_predamage(power, &a, &d, element_affinity_pct, rng2, || {
            self.next_rand() as u16
        });

        let attacker_element = self.monster_seat_element(attacker as usize).unwrap_or(7);
        let target_is_party = target < self.party.party_count;
        let finish = DamageFinish {
            predamage: atk.saturating_sub(def).clamp(1, 9999),
            attacker_slot: 3,
            defender_slot: if target_is_party { 0 } else { 3 },
            attacker_element,
            defender_resist: self.defender_resist(target),
            defender_guarding: self
                .battle
                .guarding
                .get(target as usize)
                .copied()
                .unwrap_or(false),
            enemy_defender_halve: self.mystic_shield_up(),
            bypass_party_resist: ATK_WRAPPER_BYPASSES_PARTY_RESIST,
            summon_power_pct: 100,
            floor_rand: 0,
        };
        let over = damage_finish_lazy(&finish, || self.next_rand() as u16);
        Some(over.min(9999) as u16)
    }

    /// `true` when `move_id`'s spell-table record is **capture-class** (class
    /// byte `'c'` at `DAT_800754C8 + id*0xC`).
    ///
    /// A capture-class cast never reaches the shared kernel `FUN_801DD0AC`:
    /// the action SM's `0x63` arm pages the cast's own streamed module
    /// (`FUN_8003EC70(record[+1] + 0x28)` -> PROT `935..966`) and that
    /// module's ticks call one of the two per-move wrappers instead. So the
    /// class byte is the routing question, and it is disc data - read here off
    /// the SCUS spell table installed at boot ([`Self::install_menu_text`]).
    /// Without a disc image the answer is `false`, which keeps disc-free
    /// battles on the shared kernel exactly as before.
    pub(super) fn is_capture_class_move(&self, move_id: u8) -> bool {
        self.spell_table_class(move_id) == Some(legaia_game_tables::spell_names::CAPTURE_CLASS)
    }

    fn capture_respect_predamage(&mut self, attacker: u8, target: u8, power: i32) -> Option<u16> {
        use legaia_engine_battle_vm::battle_damage_wrappers::{
            INT_WRAPPER_BYPASSES_PARTY_RESIST, WrapperAttacker, WrapperDefender,
            int_wrapper_predamage,
        };
        use legaia_engine_battle_vm::battle_formulas::{DamageFinish, damage_finish_lazy};

        let element_affinity_pct = self.enemy_affinity_pct(attacker, target);
        let attacker_hp = self.actors.get(attacker as usize)?.battle.hp;
        let defender = self.summon_roll_defender(target)?;
        let a = WrapperAttacker {
            hp: attacker_hp,
            agl: self
                .battle
                .accuracy
                .get(attacker as usize)
                .copied()
                .unwrap_or(0),
            // `+0x158` is not read on this path at all.
            spell_power: 0,
            status: self.raw_status_word(attacker),
        };
        let d = WrapperDefender {
            hp: defender.hp,
            agl: defender.agl,
            stat_a: defender.stat_a,
            stat_b: defender.stat_b,
            status: defender.status,
            guard: defender.guard,
        };
        let power_u = power.max(0) as u32;
        let rng3 = [
            self.next_rand() as u16,
            self.next_rand() as u16,
            self.next_rand() as u16,
        ];
        let (atk, def) = int_wrapper_predamage(power_u, &a, &d, element_affinity_pct, rng3, || {
            self.next_rand() as u16
        });

        let attacker_element = self.monster_seat_element(attacker as usize).unwrap_or(7);
        let target_is_party = target < self.party.party_count;
        let finish = DamageFinish {
            predamage: atk.saturating_sub(def).clamp(1, 9999),
            attacker_slot: 3,
            defender_slot: if target_is_party { 0 } else { 3 },
            attacker_element,
            defender_resist: self.defender_resist(target),
            defender_guarding: self
                .battle
                .guarding
                .get(target as usize)
                .copied()
                .unwrap_or(false),
            enemy_defender_halve: self.mystic_shield_up(),
            bypass_party_resist: INT_WRAPPER_BYPASSES_PARTY_RESIST,
            summon_power_pct: 100,
            floor_rand: 0,
        };
        let over = damage_finish_lazy(&finish, || self.next_rand() as u16);
        Some(over.min(9999) as u16)
    }

    /// Roll a player Seru-magic summon's damage through the faithful summon
    /// branch of the shared battle kernel (`FUN_801dd0ac` `attacker_slot ==
    /// 7`) plus the closed-form finisher stages (`FUN_801ddb30`), replacing
    /// the MP-scaled spell placeholder. Returns `None` when `spell_id` isn't
    /// a summon or the monster catalog doesn't resolve the summon creature -
    /// disc-free / synthetic battles keep the placeholder path with an
    /// unchanged RNG stream.
    ///
    /// Faithful seeds:
    /// - **Summon body** (the retail slot-7 actor): the cast's own
    ///   `summon.dat` record HP (`+0x0C`) and INT (`+0x18`) - the seat arm
    ///   copies the streamed record onto slot 7 (`FUN_80054CB0(record, 4)`
    ///   from `FUN_801F19EC`), for the big summons as for the Seru
    ///   ([`Self::summon_creature_def`]).
    /// - **Caster AGL** (`DAT_801C9370[ctx+0x13]` `+0x168`): the casting
    ///   party member's `battle_accuracy`, doubled into the roll.
    /// - **Scale stage** (`FUN_801dd864`): element affinity
    ///   `matrix[summon element][target element]`, then the caster's
    ///   per-spell magic-power byte ([`Self::caster_magic_power_byte`]).
    /// - **Finisher** (`FUN_801ddb30`): no-damage floor (`rand()%9 + 8`,
    ///   drawn lazily), the per-caster summon power-percent
    ///   (`0x801F5468[(char_id-1)*8 + summon_element]`,
    ///   [`legaia_game_tables::element_affinity::ElementAffinity::summon_power_pct`]),
    ///   and the 9999 cap. The party-resist / guard stages don't apply to an
    ///   enemy defender; the state-mutating tail (popup accumulator, MP
    ///   drain, stat debuffs) stays in the live fold.
    ///
    /// RNG: draws attacker + defender eagerly, the bonus-arm and floor draws
    /// lazily - the shared cursor advances exactly as retail's does (two,
    /// three, or four draws).
    ///
    /// PORT: FUN_801dd0ac (summon branch, live wiring; pure kernel in
    /// battle_formulas::summon_predamage_lazy)
    pub(in crate::world) fn player_summon_predamage(
        &mut self,
        caster: u8,
        target: u8,
        spell_id: u8,
    ) -> Option<u16> {
        use legaia_engine_battle_vm::battle_formulas::{
            DamageFinish, DefenderResist, SummonRollActor, damage_finish_lazy,
            summon_predamage_lazy,
        };

        if (caster as usize) >= self.party.party_count as usize {
            return None;
        }
        let creature = self.summon_creature_def(spell_id)?;
        let summon = SummonRollActor {
            hp: creature.hp,
            // The summon-roll's "agl" slot is the actor's `+0x168` stat, which
            // for a monster is record `+0x18` = INT, installed under this
            // fight's boost profile: the seat arm's `FUN_80054CB0` picks it by
            // `ctx[+0x287]` as for a monster seat. Pinned live: a random-
            // encounter Gimard cast rolls a body of INT 60 off the record's 48
            // (`autorun_summon_body_stats.lua`).
            agl: creature.installed_stats(self.battle.scripted_fight)[4],
            ..Default::default()
        };
        let summon_element = creature.element;
        let caster_agl = self
            .battle
            .accuracy
            .get(caster as usize)
            .copied()
            .unwrap_or(0);
        let target_roll = self.summon_roll_defender(target)?;
        let element_affinity_pct = self.cast_affinity_pct(spell_id, target);
        let magic_power_byte = self.caster_magic_power_byte(caster, spell_id);

        let rng2 = [self.next_rand() as u16, self.next_rand() as u16];
        let (atk, def) = summon_predamage_lazy(
            &summon,
            caster_agl,
            &target_roll,
            element_affinity_pct,
            magic_power_byte,
            rng2,
            || self.next_rand() as u16,
        );

        // Closed-form finisher stages (FUN_801ddb30). The defender is an
        // enemy (no party-resist stage); kernel slot convention is retail's
        // (party 0..2, enemies 3+). The summon power-percent comes from the
        // per-caster 0x801F5468 table when the affinity tables are installed
        // (the caster seat's roster id, as the element lookups), else 100.
        let summon_power_pct = self
            .tables
            .element_affinity
            .as_ref()
            .and_then(|aff| aff.summon_power_pct(self.battle_party_char_id(caster), summon_element))
            .unwrap_or(100);
        let predamage = atk.saturating_sub(def);
        let finish = DamageFinish {
            predamage,
            attacker_slot: 7,
            defender_slot: 3 + target.saturating_sub(self.party.party_count),
            attacker_element: summon_element,
            defender_resist: DefenderResist::default(),
            defender_guarding: false,
            enemy_defender_halve: self.mystic_shield_up(),
            bypass_party_resist: false,
            summon_power_pct,
            floor_rand: 0,
        };
        let over = damage_finish_lazy(&finish, || self.next_rand() as u16);
        Some(over.min(9999) as u16)
    }

    /// Whether a monster caster's chosen move id resolves to a real per-move
    /// power record (so its damage should roll through [`Self::enemy_move_predamage`]
    /// rather than the MP-scaled spell placeholder). Only fires when the
    /// move-power table is installed (disc-real battles), keeping disc-free /
    /// synthetic battles on the placeholder path with an unchanged RNG stream.
    pub(super) fn enemy_move_power(&self, caster: u8, move_id: u8) -> Option<i32> {
        if (caster as usize) < self.party.party_count as usize {
            return None;
        }
        self.tables.move_power.as_ref()?.power_for_move_id(move_id)
    }
}
