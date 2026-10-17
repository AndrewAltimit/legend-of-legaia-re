//! The fourteen trampoline arms: extended arm state, the sweep rolls, the cast
//! steal, the glare / divide split and the effect-child hit.

use super::*;

impl World {
    // --- the fourteen trampoline arms ---

    /// The five extra record fields
    /// [`legaia_engine_cast::cast_arm_ticks::CastArmExtState`] carries, lifted
    /// off one actor slot.
    ///
    /// Two of them have an engine home: `+0x150` is the actor's live MP and
    /// `+0x172` its max HP. The other three do not, and the reason is that no
    /// routine in PROT 0903..0966 reads them back - `+0x152` (the MP base) and
    /// `+0x178` (where PROT 0943's drain stashes the old working MP) are
    /// write-only in the band, and `+0x1F3` sits one byte past the end of the
    /// engine's `+0x1DF..+0x1F2` action-parameter window. They round-trip
    /// through the view for the tick's own arithmetic and are dropped here.
    pub(in crate::world::battle::cast_band) fn cast_arm_ext_state(
        &self,
        slot: u8,
    ) -> legaia_engine_cast::cast_arm_ticks::CastArmExtState {
        use legaia_engine_cast::cast_arm_ticks::CastArmExtState;
        let Some(a) = self.actors.get(slot as usize) else {
            return CastArmExtState::default();
        };
        CastArmExtState {
            mp: a.battle.mp,
            mp_base: a.battle.mp,
            mp_stash: 0,
            max_hp: a.battle.max_hp,
            reaction_extra: 0,
        }
    }

    /// Write back the two halves of [`Self::cast_arm_ext_state`] the engine
    /// actually carries.
    pub(in crate::world::battle::cast_band) fn write_cast_arm_ext_state(
        &mut self,
        slot: u8,
        st: &legaia_engine_cast::cast_arm_ticks::CastArmExtState,
    ) {
        let Some(a) = self.actors.get_mut(slot as usize) else {
            return;
        };
        a.battle.mp = st.mp;
        a.battle.max_hp = st.max_hp;
    }

    /// Pre-roll one whole-row sweep arm's per-seat damage, in the order retail
    /// visits its seats.
    ///
    /// Only the body's own sweep arm draws anything
    /// ([`legaia_engine_cast::cast_arm_ticks::arm_sweep_arm`]); rolling on every
    /// re-entry would run the shared RNG cursor forward on frames retail never
    /// draws. The sibling of [`Self::sweep_status_rolls`] for the arms
    /// `cast_arm_ticks` carries.
    pub(in crate::world::battle::cast_band) fn cast_arm_sweep_rolls(
        &mut self,
        ctx: &legaia_engine_cast::cast_module_ticks::CastModuleCtx,
        entry: u32,
        body: u32,
        caster: u8,
    ) -> Vec<(u8, i32)> {
        let Some(arm) = legaia_engine_cast::cast_arm_ticks::arm_sweep_arm(entry, body) else {
            return Vec::new();
        };
        if ctx.phase != arm {
            return Vec::new();
        }
        let Some(shape) = legaia_engine_cast::cast_arm_ticks::arm_damage_shape_for(entry, body)
        else {
            return Vec::new();
        };
        let seats: Vec<legaia_engine_cast::cast_module_ticks::CastActorState> = (0..ctx
            .party_count)
            .map(|s| self.cast_actor_state(s))
            .collect();
        let mut out = Vec::new();
        for (seat, s) in seats.iter().enumerate() {
            if !legaia_engine_cast::cast_module_ticks::aoe_seat_is_hittable(s) {
                continue;
            }
            let seat = seat as u8;
            out.push((
                seat,
                self.capture_module_roll(shape, caster, seat).unwrap_or(0),
            ));
        }
        out
    }

    /// Drive one of the two table-dispatched sweep arms end to end: roll,
    /// tick, write the seats back.
    pub(in crate::world::battle::cast_band) fn run_cast_arm_sweep(
        &mut self,
        ctx: &mut legaia_engine_cast::cast_module_ticks::CastModuleCtx,
        entry: u32,
        body: u32,
        caster_slot: u8,
        _caster: &mut legaia_engine_cast::cast_module_ticks::CastActorState,
    ) -> (
        legaia_engine_cast::cast_module_ticks::CastTickStep,
        Vec<legaia_engine_cast::cast_module_ticks::AoeHit>,
    ) {
        let sweep =
            Some(ctx.phase) == legaia_engine_cast::cast_arm_ticks::arm_sweep_arm(entry, body);
        let rolls = self.cast_arm_sweep_rolls(ctx, entry, body, caster_slot);
        let mut seats: Vec<legaia_engine_cast::cast_module_ticks::CastActorState> =
            self.cast_seat_row();
        let take = |seat: u8| {
            rolls
                .iter()
                .find(|(s, _)| *s == seat)
                .map(|(_, d)| *d)
                .unwrap_or(0)
        };
        let (step, hits) =
            legaia_engine_cast::cast_arm_ticks::steal_sweep_tick(ctx, &mut seats, sweep, take);
        self.write_cast_seat_row(&seats);
        (
            step,
            self.cast_hits_to_engine(hits.iter().map(|h| {
                legaia_engine_cast::cast_module_ticks::AoeHit {
                    seat: h.seat,
                    applied: h.applied as i32,
                }
            })),
        )
    }

    /// PROT 0941's Steal resolution, both legs.
    ///
    /// **Monster seat** (`victim_slot >= party_count`, the engine's row): the static
    /// `SCUS_942.54` steal table `0x80077828 + monster_id * 2`, fields
    /// `[chance, item]` - the same table the player-side steal reads, and NOT
    /// a field of the PROT 867 monster record (`docs/formats/steal-table.md`).
    /// `rand() % 100 < chance` decides. `None` when no table is installed (a
    /// disc-free host) or the victim carries no monster id, which keeps a
    /// synthetic battle from inventing a steal and from drawing the roll.
    ///
    /// **Party seat**: the bag draw plus the consume. The rejection rule
    /// (`id != 0 && count != 0 && the item table knows the id`, up to `0x400`
    /// draws, one RNG draw per rejection so the shared cursor advances the way
    /// retail's does) is the module's, byte for byte.
    ///
    /// The **array** is retail's own: [`legaia_engine_menus::item_bag::ItemBag`] holds the
    /// physical 256 slots, so the draw rejects its way past a played-through
    /// bag's holes exactly as retail's does, and the third acceptance leg is
    /// the item record's **shop price** halfword (`0x80074368 + id*0xC + 2`,
    /// `0x801F789C`) rather than a tautology over the ids the bag already
    /// holds - the quest and found-only items carry a zero price and are
    /// unstealable.
    ///
    /// The re-roll floor is applied too. `0x801F77E8` arms it on
    /// `DAT_8007BD10[1] == 4` (the second present-party member's character id)
    /// and re-draws while `slot < *(i16*)0x8007B5EA`, which is `gp[+0x2D2]` -
    /// the active window's **start** (`gp = 0x8007B318`), so the arm confines
    /// the draw to the window's own half.
    ///
    /// The removal is asymmetric with the draw, and deliberately so: the draw
    /// is over the whole array while `FUN_80042310` scans only
    /// `[gp[+0x2D2], gp[+0x2D4])` and returns `0x100` for an id outside it,
    /// touching nothing. A steal that lands on the other half's slot therefore
    /// announces an item the party keeps.
    pub(in crate::world) fn roll_cast_steal(
        &mut self,
        victim_slot: u8,
    ) -> Option<legaia_engine_cast::cast_arm_ticks::StealOutcome> {
        use legaia_engine_cast::cast_arm_ticks::StealOutcome;
        // A monster seat is the engine's compacted row, not retail's fixed
        // `>= 3` (see [`Self::engine_slot_for_retail_pool`]).
        if victim_slot >= self.party.party_count.clamp(1, 3) {
            let entry = self
                .actors
                .get(victim_slot as usize)
                .and_then(|a| a.battle_monster_id)
                .and_then(|id| self.tables.steal_table.as_ref()?.entry(id))?;
            let roll = (self.next_rand() % 100) as u8;
            return Some(StealOutcome::FromMonster {
                chance: entry.chance_pct,
                item: entry.item_id,
                roll,
                hit: roll < entry.chance_pct,
            });
        }
        let bag: Vec<(u8, u8)> = self.party.inventory.slots().to_vec();
        // The price leg, precomputed so the draw closure can borrow `self`
        // for the RNG. Without a disc image there is no item table to read a
        // price from, so the leg cannot be evaluated and every id passes -
        // a disc-free host still spends retail's draws and rejects on the
        // two legs it can see.
        let mut priced = [true; 256];
        if let Some(data) = self.shops.item_shop_data.as_ref() {
            for (id, cell) in priced.iter_mut().enumerate() {
                *cell = data.price(id as u8) != 0;
            }
        }
        // `DAT_8007BD10[1] == 4`: the second present-party member's character
        // id, which the engine mirrors as `party_roster_slot(1) + 1`. A party
        // of one has no second member, and the port's identity mapping would
        // fabricate one, so the arm needs both.
        let floor = (self.party.party_count > 1 && self.party_roster_slot(1) as u8 + 1 == 4)
            .then(|| self.party.inventory.window_bounds().0.min(0xFF) as u8);
        // One `next_rand` per rejected slot, not a pre-drawn batch: retail
        // advances the shared cursor once per draw, so over-drawing would
        // desynchronise every later roll in the battle.
        let slot = legaia_engine_cast::cast_arm_ticks::steal_pick_bag_slot(
            &bag,
            floor,
            || self.next_rand(),
            |id| priced[id as usize],
        );
        match slot.and_then(|s| bag.get(s as usize).copied()) {
            Some((item, _)) => Some(StealOutcome::FromBag { item }),
            None => Some(StealOutcome::BagEmpty),
        }
    }

    /// The inventory consume PROT 0941's Steal performs (`FUN_80042310`),
    /// through the window-bounded helper.
    ///
    /// Returns whether a slot was actually emptied: the helper's `0x100`
    /// sentinel says the id is outside the active window, and retail's own
    /// steal ignores the return, so the message has already been staged by the
    /// time the removal declines. The engine keeps the same order and reports
    /// the difference instead of hiding it.
    pub(in crate::world) fn take_one_from_bag(&mut self, item: u8) -> bool {
        self.party.inventory.consume_returning_slot(item, 1)
            != legaia_save::retail_inventory::NOT_IN_WINDOW
    }

    /// Materialise the seat PROT 0940's split allocated.
    ///
    /// Retail builds a whole actor: it copies the caster's monster-record
    /// pointer into `0x801C9348[seat]`, allocates a display object through
    /// `FUN_80054CB0` / `FUN_80024C88`, and unaligned-copies the caster's
    /// pose. The engine's equivalent is the caster's own actor record cloned
    /// into the seat, and the five simulation writes the arm makes on top of
    /// that (`+0x16C`, `+0x1DE`, `+0x1DF`, `+0x1DD`, `+0x14C` / `+0x172`) are
    /// what this applies.
    ///
    /// Returns `false` when the table has no seat there, which is retail's own
    /// bound: `ctx[+1]` indexes `actor_table` and the engine caps both counts
    /// at [`BATTLE_TABLE_SLOTS`].
    pub(in crate::world::battle::cast_band) fn apply_glare_divide_split(
        &mut self,
        caster_slot: u8,
        split: &legaia_engine_cast::cast_arm_ticks::GlareDivideSplit,
    ) -> bool {
        use legaia_engine_cast::cast_arm_ticks::{
            SPLIT_CLONE_ACTION, SPLIT_CLONE_CATEGORY, SPLIT_WEAK_AGL, SplitWeakened,
        };
        // `clone_seat` is a retail pool slot (`3 + ctx[+1]`): retail seats
        // monster `k` at `3 + k` whatever the party size, and the engine
        // compacts the monster row down to `party_count + k`
        // (`formation_span`'s module docs). So the clone takes the seat right
        // after the engine's seated monsters. Placing it at the retail index
        // left a small party's clones past the five-seat enemy row the
        // target picker walks (`World::battle_target_rows`): a lone Vahn on
        // `map01` faced a Divide clone in slot 6 that no command could
        // target, and the fight never ended. The bound is retail's own -
        // `ctx[+1] < 5` on both sides.
        let _ = split.clone_seat;
        let pc = usize::from(self.party.party_count.clamp(1, 3));
        let table = self.actors.len().min(BATTLE_TABLE_SLOTS);
        let seated = (pc..table)
            .filter(|&s| self.actors[s].battle_monster_id.is_some())
            .count();
        let seat = pc + seated;
        if seat >= table || seated >= 5 {
            return false;
        }
        let Some(src) = self.actors.get(caster_slot as usize).cloned() else {
            return false;
        };
        let mut clone = src;
        clone.active = true;
        clone.battle.init_key = 0;
        clone.battle.action_category = SPLIT_CLONE_CATEGORY;
        if let Some(p) = clone.battle.params.first_mut() {
            *p = SPLIT_CLONE_ACTION;
        }
        clone.battle.active_target = legaia_engine_cast::cast_module_ticks::TARGET_CODE_ENEMY_ROW;
        clone.battle.hp = split.clone_hp;
        clone.battle.max_hp = split.clone_hp;
        if split.weakened == Some(SplitWeakened::Clone) {
            clone.battle.hp = 1;
            clone.battle.mp = 0;
            clone.battle.atk_working = 1;
            clone.battle.agl = SPLIT_WEAK_AGL;
            clone.battle.agl_base = SPLIT_WEAK_AGL;
        }
        self.actors[seat] = clone;
        true
    }

    /// The three accessory ids PROT 0955's Void Accessories rolls between -
    /// `record[+0x19B + slot]` for the character seated at `slot`.
    pub(in crate::world::battle::cast_band) fn cast_victim_accessories(&self, slot: u8) -> [u8; 3] {
        let mut out = [0u8; 3];
        let Some(rec) = self
            .party
            .roster
            .members
            .get(self.party_roster_slot(slot as usize))
        else {
            return out;
        };
        // `+0x196` is equipment slot 0, so `+0x19B` - the module's
        // `+ 0x75E + 5` - is index 5, and the three accessory slots are
        // 5, 6, 7 (`legaia_save::character::EquipmentSlots`).
        let eq = rec.equipment();
        for (i, o) in out.iter_mut().enumerate() {
            *o = eq
                .slots
                .get(ACCESSORY_EQUIP_SLOT_0 + i)
                .copied()
                .unwrap_or(0);
        }
        out
    }

    /// Apply one enemy-cast hit through retail's **safe** applier - the hit
    /// arm of `FUN_801E09F8`'s per-slot effect-child driver
    /// (`legaia_engine_battle_vm::battle_cast_census::effect_child_hit`).
    ///
    /// This is the path a monster's cast takes in retail, and it differs from
    /// the action band's accumulating seed in the one way that matters: the
    /// roll is clamped against live HP **once** and that single value reaches
    /// both the readout accumulator `+0x10` and live HP `+0x14C`, so the bar
    /// can never be asked to travel further than HP moved. The action band's
    /// seed can, which is the `0x51` settle park
    /// `legaia_engine_battle_vm::battle_hp_bar` documents.
    ///
    /// Also carried: the reaction-clip pick (`+0x1F2` gates `+0x1F1` against
    /// `+0x1EF` / `+0x1F0`, and a dead victim always takes `+0x1F1`), the
    /// `+0x1DC` **bit** ORs - retail ORs here, it does not bump - and the
    /// readout cursor `ctx[+0x262]`.
    ///
    /// Returns the damage actually applied.
    pub(in crate::world) fn apply_effect_child_hit(&mut self, slot: usize, damage: i32) -> i32 {
        use legaia_engine_battle_vm::battle_cast_census::{EffectChildVictim, effect_child_hit};
        let mut cursor = self.battle_ctx.cast_readout_cursor;
        let Some(a) = self.actors.get_mut(slot) else {
            return 0;
        };
        a.battle.arm_hp_bar();
        let p = |i: usize| a.battle.params.get(i - 0x1DF).copied().unwrap_or(0);
        let mut victim = EffectChildVictim {
            hp: a.battle.hp,
            hp_bar_delta: a.battle.hp_bar_pending,
            flags: a.battle.field_flags,
            staged_anim: a.battle.queued_anim,
            restage: 0,
            reaction_alt: p(0x1EF),
            reaction_alt2: p(0x1F0),
            knockdown_anim: p(0x1F1),
            reaction_gate: p(0x1F2),
        };
        let hit = effect_child_hit(&mut victim, damage, &mut cursor);
        a.battle.hp = victim.hp;
        a.battle.hp_bar_pending = victim.hp_bar_delta;
        a.battle.queued_anim = victim.staged_anim;
        // `hp == 0 -> liveness = 0` holds for **present** actors only, the
        // same `max_hp > 0` guard `apply_battle_hp_delta` applies.
        if a.battle.max_hp > 0 && a.battle.hp == 0 {
            a.battle.liveness = 0;
        }
        self.battle_ctx.cast_readout_cursor = cursor;
        hit.applied
    }
}
