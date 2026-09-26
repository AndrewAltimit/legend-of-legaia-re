//! Turn order: opponent scan, round-robin fallback, and SPD-seeded initiative
//! keys. Split out of `battle.rs` as additional `impl World` blocks; no logic
//! change from the original inline definitions.

use super::*;

impl World {
    /// `ctx+0x290` - the formation advantage `FUN_80051D84` rolls at battle
    /// setup, read straight off the battle context the action SM owns.
    ///
    /// There is exactly one of these in retail and exactly one here. The
    /// initiative seeder is the only consumer of the *unlatched* copy (it
    /// zeroes the disadvantaged side's keys, `0x801DAA40`); the action SM's
    /// state `0x00` consumes it and clears it
    /// ([`vm::battle_action::begin_formation_arm`]).
    ///
    /// REF: FUN_80051D84
    pub fn battle_formation(&self) -> vm::battle_formulas::FormationAdvantage {
        vm::battle_formulas::FormationAdvantage::from_byte(self.battle_ctx.formation_advantage)
    }

    /// Write `ctx+0x290`. The roll ([`Self::roll_battle_formation`]) is its
    /// only production writer; tests use it to stage an advantage.
    pub fn set_battle_formation(&mut self, advantage: vm::battle_formulas::FormationAdvantage) {
        self.battle_ctx.formation_advantage = advantage.to_byte();
    }

    /// `ctx+0x291` - the latched copy of [`Self::battle_formation`]. Retail's
    /// `FUN_801E295C` state `0x00` copies `+0x290` here (`0x801E2B38`, the
    /// byte's only writer) and then clears the original; this is the copy that
    /// survives the battle and the one [`World::roll_battle_escape`] reads
    /// (`0x801E7AD8`; `== 2` -> the escape compare cannot fail). Latching is
    /// what makes a pre-emptive strike affect escapes at all - clearing
    /// `+0x290` without copying it silently disables them.
    ///
    /// REF: FUN_801E791C
    pub fn battle_formation_latched(&self) -> vm::battle_formulas::FormationAdvantage {
        vm::battle_formulas::FormationAdvantage::from_byte(self.battle_ctx.formation_latched)
    }

    /// Write `ctx+0x291` directly, bypassing the latch. Tests only - production
    /// reaches it through [`Self::latch_battle_formation`].
    pub fn set_battle_formation_latched(
        &mut self,
        advantage: vm::battle_formulas::FormationAdvantage,
    ) {
        self.battle_ctx.formation_latched = advantage.to_byte();
    }

    /// First living actor on the side opposing `attacker`. Party slots
    /// (`< party_count`) oppose the monster band (`party_count..`); monster
    /// slots oppose the party. `None` if that side is wiped.
    pub(in crate::world) fn first_living_opponent_of(&self, attacker: u8) -> Option<u8> {
        let pc = self.party.party_count.max(1);
        let n = self.actors.len() as u8;
        let (lo, hi) = if attacker < pc { (pc, n) } else { (0, pc) };
        (lo..hi).find(|&i| {
            self.actors
                .get(i as usize)
                .is_some_and(|a| a.battle.liveness != 0)
        })
    }

    /// Next living combatant after `after` in round-robin slot order across
    /// the whole actor table (party then monsters, wrapping). The no-SPD arm
    /// of [`Self::next_combatant_by_initiative`] walks the same order over
    /// the round's unspent turn tokens; this is its keyless reference, kept
    /// for the test that pins the wrap.
    #[cfg(test)]
    pub(in crate::world) fn next_living_combatant(&self, after: u8) -> Option<u8> {
        let n = self.actors.len();
        if n == 0 {
            return None;
        }
        (1..=n).find_map(|step| {
            let idx = (after as usize + step) % n;
            (self.actors[idx].battle.liveness != 0).then_some(idx as u8)
        })
    }

    /// True when at least one living battle slot carries a non-zero SPD. Gates
    /// the SPD-seeded initiative turn order on real speed data; otherwise the
    /// battle stays on the round-robin [`Self::next_living_combatant`].
    pub(in crate::world) fn any_battle_speed(&self) -> bool {
        (0..BATTLE_SLOTS).any(|i| {
            self.battle.speed[i] != 0 && self.actors.get(i).is_some_and(|a| a.battle.liveness != 0)
        })
    }

    /// `true` while any *living* actor still holds an unspent initiative key.
    /// When this goes false the round is over and the next
    /// [`Self::next_combatant_by_initiative`] reseeds - the live loop uses this
    /// as its once-per-round boundary for status ticking. Dead actors' stale
    /// keys are ignored (only living actors count), so it agrees with the
    /// reseed condition inside the selector.
    pub(super) fn any_living_initiative_key(&self) -> bool {
        (0..BATTLE_SLOTS).any(|i| {
            self.actors
                .get(i)
                .is_some_and(|a| a.battle.liveness != 0 && a.battle.init_key != 0)
        })
    }

    /// Seed every living battle slot's initiative key; dead slots get `0`.
    ///
    /// The per-actor arithmetic is the shared kernel
    /// [`vm::battle_formulas::seed_initiative`] - the port of `FUN_801DA780`'s
    /// scoring body. This used to inline a bare `speed + rand()%(speed/2+1) + 1`,
    /// which is the *aliased* `overlay_0897_801e23ec` reading of the seeder and
    /// drops three terms the battle-resident routine actually applies: the
    /// wounded-HP bonus, the Slow halving, and the `+0xF4` always-act-first /
    /// always-act-last ability arms. Everything now goes through the kernel so
    /// there is one implementation to be right.
    ///
    /// After the per-slot sweep the `ctx+0x290` side lockout
    /// ([`vm::battle_formulas::apply_side_lockout`]) zeroes the disadvantaged
    /// side's keys, so a back attack costs the party its whole first round and
    /// a pre-emptive strike costs the monsters theirs.
    ///
    /// PORT: FUN_801DA780 (the slot sweep + lockout; per-actor scoring in
    /// `battle_formulas::seed_initiative`)
    pub(in crate::world) fn reseed_initiative(&mut self) {
        use vm::battle_formulas::{InitiativeActor, initiative_roll_modulus, seed_initiative};
        let party_count = self.party.party_count as usize;
        if !self.any_battle_speed() {
            // No SPD anywhere (the synthetic catalog, the disc-free tests):
            // there is nothing to roll, so every living slot gets one flat
            // turn token and no RNG is drawn. The pick
            // ([`Self::next_combatant_by_initiative`]) walks these in slot
            // order, which keeps the historical round-robin cadence while
            // still giving the battle retail's round boundary.
            for i in 0..BATTLE_SLOTS {
                if let Some(a) = self.actors.get_mut(i) {
                    a.battle.init_key = u16::from(a.battle.liveness != 0);
                }
            }
            self.apply_engine_side_lockout();
            return;
        }
        for i in 0..BATTLE_SLOTS {
            let alive = self.actors.get(i).is_some_and(|a| a.battle.liveness != 0);
            if !alive {
                if let Some(a) = self.actors.get_mut(i) {
                    a.battle.init_key = 0;
                }
                continue;
            }
            let is_party = i < party_count;
            let actor = InitiativeActor {
                speed: self.battle.speed[i],
                hp: self.actors[i].battle.hp,
                max_hp: self.actors[i].battle.max_hp,
                is_party,
                // Retail reads the Slow status from `actor+0x16E == 0x1000`.
                // The engine's status model does not carry that word yet, so
                // no actor is ever flagged Slow here.
                slowed: false,
                // The `+0xF4` ability word only applies to party slots (retail
                // gates the whole arm on `slot < 3`); monsters pass 0.
                ability_bits: if is_party {
                    self.initiative_ability_bits(i)
                } else {
                    0
                },
            };
            let roll = (self.next_rand() % initiative_roll_modulus(actor.speed)) as u16;
            let key = seed_initiative(&actor, roll);
            if let Some(a) = self.actors.get_mut(i) {
                a.battle.init_key = key;
            }
        }
        self.apply_engine_side_lockout();
    }

    /// The `ctx+0x290` side lockout over the engine's compacted seating - the
    /// tail of [`Self::reseed_initiative`], shared by its SPD and no-SPD arms.
    fn apply_engine_side_lockout(&mut self) {
        // `ctx+0x290` side lockout - read the *unlatched* copy, as retail does.
        //
        // `battle_formulas::apply_side_lockout` splits the sides at the fixed
        // retail boundary (party `0..=2`, monsters `3..=6`), because retail
        // always reserves three party slots even for a one- or two-member
        // party. The engine **compacts** instead: `enter_battle` seats the
        // first monster at `party_count`, so slot 1 can be a monster. Applying
        // the fixed split here would lock out the wrong side for any party
        // smaller than three, so the side test is taken from `party_count`.
        // The kernel stays the retail-layout reference; this is the engine's
        // seating adapter, not a different rule.
        let lockout = self.battle_formation();
        let party_count = self.party.party_count as usize;
        for slot in 0..BATTLE_SLOTS {
            let locked = match lockout {
                vm::battle_formulas::FormationAdvantage::None => false,
                // Back attack: the monsters got the drop, the party sits out.
                vm::battle_formulas::FormationAdvantage::BackAttack => slot < party_count,
                // Pre-emptive strike: the party got the drop.
                vm::battle_formulas::FormationAdvantage::Preemptive => slot >= party_count,
            };
            if locked && let Some(a) = self.actors.get_mut(slot) {
                a.battle.init_key = 0;
            }
        }
    }

    /// The character record's `+0xF4` ability bitfield for a party battle slot -
    /// the word `FUN_801DA780` tests for the always-act-first / always-act-last
    /// passives ([`vm::battle_formulas::InitiativeAbility`]). Unresolvable slots
    /// carry no bits.
    fn initiative_ability_bits(&self, slot: usize) -> u32 {
        let Some(member) = self.party.roster.members.get(self.party_roster_slot(slot)) else {
            return 0;
        };
        let bits = member.ability_bits();
        u32::from_le_bytes([bits[0], bits[1], bits[2], bits[3]])
    }

    /// Seed the battle's initiative keys at setup: every living actor gets a
    /// key and **none is consumed**, so round 1's first turn is picked by the
    /// same max-key selector ([`Self::next_combatant_by_initiative`], the port
    /// of `FUN_801DABA4`) that picks every later turn. No-op (keys left at `0`)
    /// when no SPD is present, leaving the battle on the round-robin fallback.
    ///
    /// This used to zero slot 0's key here, which handed slot 0 the opening
    /// turn of every battle in the game regardless of SPD: the party's fastest
    /// member could not open ahead of Vahn, no monster could ever act first,
    /// and a rolled **back attack** - whose entire effect is that the monsters
    /// get the drop - still opened on the party, because the side lockout only
    /// zeroed keys that slot 0's hand-arm had already bypassed.
    pub fn seed_battle_initiative(&mut self) {
        if !self.any_battle_speed() {
            return;
        }
        self.reseed_initiative();
    }

    /// Roll this battle's formation advantage into
    /// [`Self::battle_formation`] (`ctx+0x290`), the port of `FUN_80051D84`'s
    /// caller side. Both sides' mean SPD is compared with a random spread and
    /// the winner still has to pass a rarity gate; see
    /// [`vm::battle_formulas::roll_formation_advantage`] for the arithmetic.
    ///
    /// The Guardian Ring / Sentinel-class `+0xF8` bits
    /// ([`vm::battle_formulas::FormationAbility`]) fold from the living party
    /// members, matching the escape roll's fold.
    ///
    /// **Partial**: retail's map-gated scripted ambush arm (monster ids
    /// `0x3D..=0x3F` on maps `0x0C` / `0x15`) is passed `map_id: 0` because the
    /// engine has no numeric map-id space at this layer, so that arm never
    /// fires. The unconditional `monster_id == 0xA7` ambush arm does.
    ///
    /// PORT: FUN_80051D84 (the caller side; arithmetic in
    /// `battle_formulas::roll_formation_advantage`)
    pub(in crate::world) fn roll_battle_formation(
        &mut self,
        formation: &crate::monster_catalog::FormationDef,
    ) {
        use vm::battle_formulas::{FormationInputs, roll_formation_advantage};
        let party_n = (self.party.party_count as usize).min(self.actors.len());
        let party_spd: Vec<u16> = (0..party_n)
            .filter(|&i| self.actors[i].battle.liveness != 0)
            .map(|i| self.battle.speed.get(i).copied().unwrap_or(0))
            .collect();
        let enemy_spd: Vec<u16> = (party_n..self.actors.len())
            .filter(|&i| self.actors[i].battle.liveness != 0)
            .map(|i| self.battle.speed.get(i).copied().unwrap_or(0))
            .collect();
        let mut ability_bits = 0u32;
        for slot in 0..party_n {
            if self.actors[slot].battle.liveness == 0 {
                continue;
            }
            if let Some(member) = self.party.roster.members.get(self.party_roster_slot(slot)) {
                let b = member.ability_bits();
                ability_bits |= u32::from_le_bytes([b[4], b[5], b[6], b[7]]);
            }
        }
        let inputs = FormationInputs {
            ability_bits,
            monster_id: formation
                .slots
                .first()
                .map(|s| s.monster_id as u8)
                .unwrap_or(0),
            map_id: 0,
        };
        // The score inputs are owned locals, so the RNG closure can hold the
        // only borrow of `self` - draws stay on the shared determinism stream.
        let mut rand = || self.next_rand();
        let rolled = roll_formation_advantage(&party_spd, &enemy_spd, &inputs, &mut rand);
        self.set_battle_formation(rolled);
    }

    /// Run the action SM's state-`0x00` **formation arm** on the live battle
    /// context - [`vm::battle_action::begin_formation_arm`], which seeds the
    /// turn cursor `ctx[+0x1A]` from `ctx[+0x290]` and then latches `+0x290`
    /// into `+0x291` and clears it.
    ///
    /// The engine calls it at battle open because that is where retail runs
    /// it: the flow SM's `0xFE` arm is the corpus' only writer of `ctx[7] = 0`
    /// (`FUN_801D0748`, `0x801D3224`), so `FUN_801E295C` reaches `0x00` on the
    /// first battle frame and then holds in `0x0B` while the command menu is
    /// up. The port parks its whole SM while a command session is open, so the
    /// arm has to be driven here or a first-turn Run would roll its escape
    /// against an unlatched `+0x291`.
    ///
    /// Must run **after** [`Self::seed_battle_initiative`]: the seeder is the
    /// only reader of the unlatched `+0x290` (`0x801DAA40`), so arming before
    /// it would silently disable the side lockout, and never arming at all
    /// silently disables pre-emptive-strike escapes
    /// ([`Self::roll_battle_escape`]).
    ///
    /// REF: FUN_801E295C (state 0x00; the kernel carries the `PORT:` tag)
    /// REF: FUN_801D0748 (the flow arm that writes `ctx[7] = 0` at battle open)
    pub(in crate::world) fn latch_battle_formation(&mut self) {
        // The banner is chosen from the **unlatched** copy, because the arm
        // below clears it - retail reads it in state `0x0A` (`FUN_801D9D3C`),
        // one state before the action SM's `0x00` latch runs.
        self.raise_battle_open_banner();
        let party = self.party.party_count;
        // `ctx[+0x01]` is the seated monster count, not the width of the
        // 8-slot table - the tail of it is empty in most formations.
        let monsters = ((party as usize)..self.actors.len())
            .filter(|&slot| self.actors[slot].battle.liveness != 0)
            .count() as u8;
        vm::battle_action::begin_formation_arm(party, monsters, &mut self.battle_ctx);
    }

    /// Queue the battle-open formation banner - `Ambushed!` or the
    /// `surprised the enemy` line - onto the shared battle message box.
    ///
    /// Retail draws it in flow state `0x0A` from `FUN_801D9D3C`'s
    /// `ctx[+0x290]` arm (`0x801DA234`), holds it for the `ctx[+0x6D6]` intro
    /// timer, and then lets the round start. The port re-hosts that as one
    /// self-dismissing box on the battle message queue, which is retail's own
    /// single-box surface (`ctx[+0x6B2]`); the loop parks on it exactly as it
    /// parks on the intro timer.
    ///
    /// An ordinary formation queues nothing, which is retail's own skip.
    ///
    /// PORT: FUN_801D9D3C (the `ctx+0x290` banner arm; the party-plate layout
    /// stays with the HUD)
    pub fn raise_battle_open_banner(&mut self) {
        use crate::battle_open::{BANNER_BOX_STYLE, BANNER_FRAMES, FormationBanner};
        let Some(banner) =
            FormationBanner::for_formation(self.battle_formation(), self.party.party_count)
        else {
            return;
        };
        // Retail substitutes the *leader's* name, not the acting member's:
        // the operand it writes after the `0xC1` token is
        // `DAT_8007BD10[0] - 1`.
        let leader = self
            .party
            .roster
            .members
            .get(self.party_roster_slot(0))
            .map(|m| m.name())
            .filter(|n| !n.trim().is_empty())
            .unwrap_or_else(|| "The party".to_string());
        let template = self
            .battle
            .ui_strings
            .get(banner.disc_label())
            .map(str::to_string);
        let group = self.next_battle_tutorial_group();
        self.battle
            .tutorial_boxes
            .push_back(crate::battle_flow::ActiveTutorialBox {
                text: banner.line(&leader, template.as_deref()),
                style: BANNER_BOX_STYLE,
                waits_for_input: false,
                frames_remaining: BANNER_FRAMES,
                group,
                any_press_dismisses: false,
            });
    }

    /// Next combatant by SPD-seeded initiative - the port of
    /// `recompute_battle_order` (`FUN_801daba4`). Returns the actor with the
    /// highest current initiative key, consuming that actor's key so the next
    /// pick moves on - retail consumes it at the action SM's `0x0C` dispatch
    /// (`sh zero,0x16c(s3)` at `0x801E2CDC`), and the engine dispatches on the
    /// same call, so the two are one seam.
    ///
    /// **The dead-slot sweep** (`0x801DABD0..0x801DAC74`) runs first, over
    /// every seat: a combatant with no HP that still holds an unspent key
    /// (`lhu 0x14c` / `lhu 0x16c` tests at `0x801DABD8` / `0x801DABE8`) has
    /// the key zeroed (`0x801DABF8`), its Spirit gauge `+0x170` clamped to
    /// `100` (`sltiu v0,v0,0x65` at `0x801DAC0C`), and - if it had committed an
    /// item (`+0x1DE == 1`) - the item handed back to the bag
    /// (`FUN_800421D4(+0x1DF, 1)` at `0x801DAC54..0x801DAC58`) and the category
    /// byte cleared (`0x801DAC68`). A member who used an item and died before
    /// acting keeps the item. The same arm bumps the round-skip count
    /// `ctx[+0x25]` (`0x801DAC2C..0x801DAC38`, `battle_ctx.round_skip`), which
    /// the action SM's end-of-action bound subtracts from the seated count.
    ///
    /// **The pick** (`0x801DAC7C..0x801DAD60`) builds retail's tie list, which
    /// is not a plain list of the tied seats: it starts as `[0]` with the
    /// running maximum at seat 0's key, and each later seat that **raises** the
    /// maximum resets the list to `[s]` *and then* matches it, appending `s`
    /// again. So a maximum first set by a seat above 0 sits in the list twice
    /// and wins `2 / (ties + 2)` of the `rand % (count + 1)` draw rather than an
    /// even share, while a maximum held by seat 0 draws evenly. `None` once
    /// every key is spent: that is the round's end, and it is the caller's to
    /// close (retail's `0x5A` bound test -> `0xFF` -> `0x14`); the keys are
    /// re-seeded by the *round start* (`FUN_801DA780` from `FUN_801D0748`'s
    /// `0x14` arm, `0x801D0ED8`), never by the pick itself. Retail draws its
    /// `rand` before it learns the maximum was zero; the port's generator is
    /// its own LCG, so the round-end draw is not reproduced. A battle with no
    /// SPD walks its flat turn tokens in slot order after the last acting
    /// actor - the historical round-robin.
    ///
    /// PORT: FUN_801DABA4
    pub(in crate::world) fn next_combatant_by_initiative(&mut self) -> Option<u8> {
        use crate::battle_round::PendingPartyAction;
        // The dead-slot sweep.
        for i in 0..BATTLE_SLOTS {
            let Some(a) = self.actors.get_mut(i) else {
                continue;
            };
            if a.battle.liveness != 0 || a.battle.init_key == 0 {
                continue;
            }
            a.battle.init_key = 0;
            a.battle.spirit_gauge = a.battle.spirit_gauge.min(100);
            // `ctx[+0x25]`: one more combatant out of this round unacted.
            self.battle_ctx.round_skip = self.battle_ctx.round_skip.saturating_add(1);
            if a.battle.action_category == vm::battle_action::ActionCategory::Item.as_byte() {
                a.battle.action_category = 0;
                if let Some(Some(PendingPartyAction::Item { item_id, .. })) =
                    self.battle.round_flow.pending.get_mut(i).map(Option::take)
                {
                    let _ = self.party.inventory.add(item_id, 1);
                }
            }
        }
        if !self.any_battle_speed() {
            // Flat turn tokens, walked in slot order from the round's start
            // (party first, then monsters) - see `RoundFlow::flat_walk_last`.
            let n = self.actors.len().min(BATTLE_SLOTS);
            let start = self
                .battle
                .round_flow
                .flat_walk_last
                .map_or(0, |last| usize::from(last) + 1);
            let pick = (start..n).find(|&i| {
                let a = &self.actors[i].battle;
                a.liveness != 0 && a.init_key != 0
            })?;
            self.actors[pick].battle.init_key = 0;
            self.battle.round_flow.flat_walk_last = Some(pick as u8);
            return Some(pick as u8);
        }
        let keys: Vec<u16> = (0..BATTLE_SLOTS)
            .map(|i| self.actors.get(i).map_or(0, |a| a.battle.init_key))
            .collect();
        let pick = initiative_tie_pick(|i| keys[i], || self.next_rand())?;
        if let Some(a) = self.actors.get_mut(pick as usize) {
            a.battle.init_key = 0; // consume this turn
        }
        Some(pick)
    }
}

/// `FUN_801DABA4`'s maximum-and-tie pick over the seats' initiative keys
/// (see [`World::next_combatant_by_initiative`]): retail's list construction,
/// duplicate included, then `rand % (count + 1)`. `None` when every key is
/// zero. The keys are compared as the unsigned halfwords they are stored as;
/// retail sign-extends the running maximum (`sra` at `0x801DACAC`), which
/// agrees for every key below `0x8000`.
fn initiative_tie_pick(key: impl Fn(usize) -> u16, mut rand: impl FnMut() -> u32) -> Option<u8> {
    let mut best = key(0);
    let mut list = [0u8; BATTLE_SLOTS + 1];
    let mut count = 0usize;
    for s in 1..BATTLE_SLOTS {
        let k = key(s);
        if best < k {
            best = k;
            count = 0;
            list[0] = s as u8;
        }
        if best == k {
            count += 1;
            list[count] = s as u8;
        }
    }
    if best == 0 {
        return None;
    }
    Some(list[rand() as usize % (count + 1)])
}

#[cfg(test)]
mod tie_pick_tests {
    use super::*;

    fn keys(k: &[u16]) -> impl Fn(usize) -> u16 + '_ {
        move |i| k.get(i).copied().unwrap_or(0)
    }

    #[test]
    fn a_maximum_raised_above_seat_zero_sits_in_the_list_twice() {
        // Seats 2 and 5 tie at 9: the list is [2, 2, 5] and the draw is mod 3.
        let k = [1, 3, 9, 0, 4, 9];
        let picks: Vec<u8> = (0..3)
            .map(|r| initiative_tie_pick(keys(&k), || r).unwrap())
            .collect();
        assert_eq!(picks, [2, 2, 5]);
    }

    #[test]
    fn a_maximum_held_by_seat_zero_draws_evenly() {
        let k = [9, 3, 9];
        let picks: Vec<u8> = (0..2)
            .map(|r| initiative_tie_pick(keys(&k), || r).unwrap())
            .collect();
        assert_eq!(picks, [0, 2]);
    }

    #[test]
    fn every_key_spent_ends_the_round() {
        assert_eq!(initiative_tie_pick(keys(&[0, 0, 0]), || 0), None);
    }
}
