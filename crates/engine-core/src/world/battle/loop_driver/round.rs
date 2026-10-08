//! The battle round: turn cycling, round begin / end and execution, party
//! command commit and confirm, turn dispatch, dead-target redirects, the
//! round-open prompt and the basic attack.
//! Split out of `loop_driver.rs`; no logic change.

use super::*;

impl World {
    /// Turn cycling for the live loop - retail's round machine, keyed on the
    /// action SM idling at `EndOfAction`.
    ///
    /// The round has two bands ([`crate::battle_round::RoundPhase`]) that
    /// never overlap. In the **command band** the flow SM owns the frame and
    /// this does nothing: the command tick walks the party through their
    /// rings and [`Self::begin_round_execution`] hands the round over once the
    /// last member commits. In the **execution band** every idle is a pick:
    /// the highest unspent initiative key acts next, party or monster, a party
    /// member dispatching the command it committed
    /// ([`Self::dispatch_pending_party_action`]) and a monster its AI pick.
    /// When no key is left the round ends (`0xFF`: the mode-counter bump +
    /// the `0x400` waker) and the next one opens (`0x14`: the actor sweep,
    /// the key re-seed, the DoT tick, `Begin | Run`).
    ///
    /// Extracted so every site that PARKS the SM at `EndOfAction` mid-tick
    /// (the spell / Spirit arms and the monster cast fold, which run in a
    /// tick that returns before the step) can claim the turn in the same
    /// tick. The SM's own `end_of_action` handler otherwise steps
    /// `EndOfAction -> PreActionWait -> ActionSeed` on the NEXT tick and
    /// re-seeds the same actor's **stale** action bytes - the shape that
    /// made every Spirit guard and every spell cast grant its actor a free
    /// bonus attack off the battle-entry queue (caught by the
    /// `seru_cast_magic_xp_ladder` test).
    ///
    /// REF: FUN_801D0748 (states `0x14` / `0x6E` / `0xFE`)
    /// REF: FUN_801E295C (the `0x5A` re-pick and the `0xFF` round end at
    /// `0x801E67E8`)
    pub(in crate::world) fn cycle_battle_turn(&mut self) {
        use crate::battle_round::RoundPhase;
        use vm::battle_action::ActionState;
        if self.battle_ctx.action_state != ActionState::EndOfAction.as_byte() {
            return;
        }
        // Only cycle while BOTH sides still have a living member - if either
        // side is wiped we leave the SM at EndOfAction so its liveness scan
        // resolves the wipe into BattleComplete next step. A petrified actor
        // counts as defeated (Stone), so it doesn't keep its side "alive" - a
        // fully-petrified party is a wipe, not a stuck loop.
        if !self.battle_both_sides_alive() {
            return;
        }
        match self.battle.round_flow.phase {
            // The flow SM owns the frame; the command tick advances it.
            RoundPhase::Command => {}
            // Battle entry without the formation path (`World::enter_battle`
            // alone, or a host that staged the SM by hand): the first idle is
            // the first round start.
            RoundPhase::Open => self.begin_battle_round(),
            RoundPhase::Execute => {
                let pick = if std::mem::take(&mut self.battle.round_flow.leader_first) {
                    self.leader_first_combatant()
                } else {
                    self.next_combatant_by_initiative()
                };
                if let Some(next) = pick {
                    self.dispatch_battle_turn(next);
                } else {
                    self.end_battle_round();
                    // A DoT can down the last member of a side; the round
                    // start re-checks before it opens a prompt.
                    self.begin_battle_round();
                }
            }
        }
    }

    /// The first dispatch of a special battle's Run round: the leader, as the
    /// `0xFE` arm's `ctx[+0x274] = 0` leaves it (`0x801D3284`).
    ///
    /// Retail made the initiative pick at the round start (`0x14`, whose
    /// seeder ends in `FUN_801DABA4`) and the arm overwrites its result, so
    /// the pick's draws are still taken and the winner keeps its key - only
    /// the acting slot's key is spent, at the SM's `0x0C` seed
    /// (`sh zero,0x16c` at `0x801E2CDC`). A leader who cannot act (dead, or
    /// already spent) leaves the pick standing.
    pub(in crate::world) fn leader_first_combatant(&mut self) -> Option<u8> {
        let keys: Vec<u16> = self.actors.iter().map(|a| a.battle.init_key).collect();
        let walk = self.battle.round_flow.flat_walk_last;
        let picked = self.next_combatant_by_initiative()?;
        let leader_can_act = self
            .actors
            .first()
            .is_some_and(|a| a.battle.liveness != 0 && keys[0] != 0);
        if picked == 0 || !leader_can_act {
            return Some(picked);
        }
        // Give the pick back the key the engine spent on it (the sweep's
        // dead-slot zeroing is kept), then spend the leader's.
        if let (Some(a), Some(&k)) = (
            self.actors.get_mut(usize::from(picked)),
            keys.get(usize::from(picked)),
        ) {
            a.battle.init_key = k;
        }
        self.battle.round_flow.flat_walk_last = walk;
        self.actors[0].battle.init_key = 0;
        Some(0)
    }

    /// `true` while each side still has a member who is not defeated.
    pub(in crate::world) fn battle_both_sides_alive(&self) -> bool {
        let party_count = self.party.party_count.max(1);
        let n = self.actors.len() as u8;
        let party_alive = (0..party_count).any(|i| !self.actor_effectively_defeated(i));
        let monsters_alive = (party_count..n).any(|i| !self.actor_effectively_defeated(i));
        party_alive && monsters_alive
    }

    /// Retail's round end - the action SM's `ctx[+0x07] == 0xFF` arm
    /// (`0x801E67E8`), reached once the per-round action cursor has passed
    /// every living actor: bump the round counter `ctx[+0x28A]` and run the
    /// `0x400` waker `FUN_801F45A4`. The flow byte it parks at `0x14` is the
    /// next [`Self::begin_battle_round`].
    ///
    /// PORT: FUN_801E295C (state `0xFF`, `0x801E67E8..0x801E6810`)
    pub(in crate::world) fn end_battle_round(&mut self) {
        self.advance_battle_mode();
        // The next round's `0xFE` re-enters state `0x00` (`0x801D3224`).
        self.battle_ctx.formation_armed = false;
        self.tick_status_0x400_wakes();
    }

    /// Retail's round start - `FUN_801D0748` state `0x14` (`0x801D0EC4`):
    /// the actor sweep `FUN_801D88CC`, the initiative seeder `FUN_801DA780`,
    /// the per-round DoT ticker `FUN_801E752C` (round index `!= 0`), and the
    /// unconditional `ctx[+0x06] = 0x1E` that opens `Begin | Run` for the
    /// round's first party command. Every later member's ring is reached
    /// through that prompt, and nothing executes until the last commit.
    ///
    /// The keys are re-seeded only when none is live: the battle-open path
    /// ([`World::enter_battle_from_formation`]) seeds them itself, reading the
    /// unlatched `ctx+0x290` for the side lockout before any latch; every later
    /// round finds them all spent and re-rolls.
    ///
    /// A **back attack** on the opening round takes retail's `0x0B -> 0xFE`
    /// jump instead of the prompt: the party enters no command, and with its
    /// keys zeroed by the lockout only the monsters dispatch.
    ///
    /// PORT: FUN_801D0748 (state `0x14`, `0x801D0EC4..0x801D0F0C`; the `0x0B`
    /// back-attack arm at `0x801D0E68..0x801D0EB0`)
    pub(in crate::world) fn begin_battle_round(&mut self) {
        use crate::battle_flow::BattleFlowState;
        use crate::battle_round::RoundPhase;
        // The side-band holds the round start back in two fights. The
        // sparring fight's opening caption: retail's side-band tick sees
        // `0x14` stored, raises the caption and sets `ctx[+0x6B0]`, and
        // `FUN_801D0748` returns on it before its state switch (`0x801D0BDC`)
        // - so none of the sweep / seed / prompt below runs until the caption
        // has gone. And the Cort fight, whose flow sits at `0x0C` until the
        // arrival module hands it back. The side-band reopens the round in
        // both cases (`world/battle/sideband.rs`).
        // REF: FUN_80056208, FUN_801D0748 (`0x801D0BDC`)
        if self.battle_sideband_holds_round() {
            return;
        }
        self.battle.round_flow.flat_walk_last = None;
        // The actor sweep (`FUN_801D88CC`): action-gauge restore, the
        // `+0x1DF` action-stream clear, and the party band's stale-target
        // re-pick + category clear. Retail runs it *before* the initiative
        // seed and before the DoT tick (`801d0ec4..801d0ed8`). It draws no
        // RNG, so the seeder's stream is unchanged.
        crate::battle_round::BattleRound::boundary(self);
        // The Spirit stance is the `+0x1DE == 4` category the sweep just
        // cleared - it lasts exactly one round.
        self.battle.guarding = [false; 3];
        if !self.any_living_initiative_key() {
            self.reseed_initiative();
        }
        // `FUN_801D388C(0, 0)` (`0x801D0EE4`): the formation squash +
        // recentre, between the seeder and the DoT tick. RNG-free.
        self.normalize_battle_formation();
        // The seeder's tail clears the round-skip count `ctx[+0x25]` every
        // round (`sb zero,0x25(v0)` at `0x801DAB84`, the delay slot of the
        // pick's `jal`), keys re-rolled or not.
        self.battle_ctx.round_skip = 0;
        self.battle.round_flow.clear_pending();
        self.battle.round_flow.cursor = 0;
        // `FUN_801E752C` - the per-round status DoT ticker, skipped on round
        // 0 (`beq v0,zero` on `ctx[+0x28A]` at `0x801D0EFC`). RNG-free.
        if self.battle_mode() != 0 {
            self.tick_status_effects();
            if !self.battle_both_sides_alive() {
                return;
            }
        }
        let first_round = self.battle_mode() == 0;
        // `0x0B` reads the unlatched `+0x290` (round one's latch runs at its
        // `0xFE`, after this). A battle staged by `enter_battle` alone stepped
        // its parked Begin first, so its roll already sits in `+0x291`.
        let back = vm::battle_formulas::FormationAdvantage::BackAttack;
        let ambushed = first_round
            && (self.battle_formation() == back || self.battle_formation_latched() == back);
        if ambushed {
            // `0x0B`'s `ctx[+0x290] == 1` arm stores `0xFE` outright.
            self.begin_round_execution();
            return;
        }
        self.battle.round_flow.phase = RoundPhase::Command;
        // `0x14 -> 0x1E`, unconditional.
        self.set_battle_flow(BattleFlowState::TurnPrompt);
        if !self.battle.player_driven {
            // No pad drives the rings: every member strikes at its dispatch
            // (the auto-fight arm `FUN_801EED1C` seeds), so the round is
            // armed at once.
            self.begin_round_execution();
            return;
        }
        // Nobody can act: retail still raises `Begin | Run` (on the selector's
        // seed, member 0), and its `Begin` finds `FUN_801DBA04` equal to the
        // count and stores the commit confirm `0x6E` directly (`0x801D10A0`,
        // step `0x27`) - `tick_battle_command` takes that arm. `Reselect`
        // from there finds nobody behind the cursor and returns to `0x1E`
        // (`0x801D30D0..0x801D30E0`), the step-back's own empty case.
        let first = self.next_member_owing_command(None).unwrap_or(0);
        self.open_battle_command(first);
    }

    /// Hand the round to the action SM - retail's `0x6E` begin arm storing
    /// `0xFE` (`0x801D31AC`) and `0xFE` storing `ctx[+0x07] = 0`
    /// (`0x801D3224`). The first pick happens here: retail's seeder made it
    /// at `0x14` (`FUN_801DA780` ends in `jal 0x801DABA4`) and the SM's
    /// `0x0C` dispatches whatever `ctx[+0x274]` names; the engine picks and
    /// dispatches on one call, and the command band draws no RNG in between,
    /// so the stream is retail's.
    ///
    /// PORT: FUN_801D0748 (state `0xFE`, `0x801D31E8..0x801D3224`)
    pub(in crate::world) fn begin_round_execution(&mut self) {
        use crate::battle_flow::BattleFlowState;
        use crate::battle_round::RoundPhase;
        use vm::battle_action::ActionState;
        self.battle.round_flow.phase = RoundPhase::Execute;
        self.battle.round_flow.flat_walk_last = None;
        self.battle.commit_log_launch = None;
        // Retail's Begin moves the flow to `0xFE`, whose arm restarts the
        // action SM at `0x00`, and `0x00` opens with `FUN_801F0450`: the pool
        // arm runs here, once per round, for every Auto-flagged Attack.
        self.run_auto_attack_pool_arms();
        // ...and the rest of that state-`0x00` pass: the delegated auto-fill
        // leg and the formation arm, once per round, before the first action
        // dispatches (a first-round Run rolls its escape at dispatch).
        self.run_round_state_zero();
        // The committed spells' cast voices, listed for hosts that decode
        // clips asynchronously - before the first dispatch takes `pending`.
        self.list_round_cast_voices();
        // `0xFE`'s special-battle run arm (`0x801D3228..0x801D328C`): a Run
        // round in a special battle hands the leader the first turn. Its
        // other store, the leg outcome `_DAT_80084448 = 4`, has no reader
        // here - the arena's legs run on the dome session, whose own leave
        // path reports the run (`World::leave_muscle_dome`).
        //
        // REF: FUN_801D0748 (kernel `battle_formulas::special_battle_run_forfeit`)
        let first_monster = self
            .battle
            .active_formation
            .as_ref()
            .and_then(|f| f.slots.first())
            .map_or(0, |s| s.monster_id as u8);
        let run_round = self
            .battle
            .round_flow
            .pending
            .iter()
            .any(|p| matches!(p, Some(crate::battle_round::PendingPartyAction::Run)));
        self.battle.round_flow.leader_first = vm::battle_formulas::special_battle_run_forfeit(
            self.special_battle_word(),
            if run_round { 5 } else { 0 },
            first_monster,
        );
        self.battle.command = None;
        self.set_battle_flow(BattleFlowState::Idle);
        // A round entered without its start (a host or test that opened a
        // command surface on a hand-built battle) has no keys yet; retail
        // never reaches `0xFE` without `0x14`'s seed, so seed here rather
        // than let the first pick read an empty round and drop every
        // commit. A round that came through `begin_battle_round` finds its
        // keys live and this is a no-op.
        if !self.any_living_initiative_key() {
            self.reseed_initiative();
        }
        self.battle_ctx.action_state = ActionState::EndOfAction.as_byte();
        self.cycle_battle_turn();
    }

    /// Consume an [`InflightCastSeed`]: close the command surfaces, enter the
    /// round's execution band and dispatch the seeded cast on the caster, as
    /// if its turn had come up in initiative order. Retail's `+0x1DD` target
    /// byte picks the picker row the spell's target resolution reads: `8` /
    /// `9` are the party / enemy group codes, anything else an absolute slot
    /// in **retail** numbering - party `0..3`, monsters from
    /// [`MONSTER_SLOT_FIRST`](legaia_engine_vm::battle_cue_group::MONSTER_SLOT_FIRST)
    /// whatever the party size, where the engine seats monsters at
    /// `party_count`.
    pub(in crate::world) fn dispatch_inflight_seed(&mut self, seed: InflightCastSeed) {
        use crate::battle_round::{PendingPartyAction, RoundPhase};
        use crate::target_picker::CursorRow;
        use legaia_engine_vm::battle_cue_group::MONSTER_SLOT_FIRST;
        let (target_row, target_slot) = match seed.target {
            8 => (CursorRow::Ally, 0),
            9 => (CursorRow::Enemy, 0),
            t if t < MONSTER_SLOT_FIRST => (CursorRow::Ally, t),
            t => (CursorRow::Enemy, t - MONSTER_SLOT_FIRST),
        };
        self.battle.command = None;
        self.battle.spell_menu = None;
        self.battle.round_flow.phase = RoundPhase::Execute;
        self.set_battle_flow(crate::battle_flow::BattleFlowState::Idle);
        // The capture's MP is already charged (the Magic band debits at
        // `0x28`, before the summon band); credit the catalog price back so
        // the band's own debit lands on the captured figure.
        for (slot, at) in seed.ground.iter().enumerate() {
            if let (Some([x, z]), Some(a)) = (*at, self.actors.get_mut(slot)) {
                a.move_state.world_x = x;
                a.move_state.world_z = z;
                if a.battle.seat.is_some() {
                    a.battle.seat = Some((x, z));
                }
            }
        }
        let price = u16::from(self.tables.spell_catalog.mp_cost(seed.spell_id));
        if let Some(a) = self.actors.get_mut(usize::from(seed.caster)) {
            a.battle.action_category = 2;
            a.battle.mp = a.battle.mp.saturating_add(price);
        }
        self.dispatch_pending_party_action(
            seed.caster,
            PendingPartyAction::Spell {
                spell_id: seed.spell_id,
                target_row,
                target_slot,
            },
        );
    }

    /// Commit `action` as `actor`'s command for this round and walk the ring
    /// on - retail's ten-site commit idiom (`0x801D16AC` and siblings):
    /// advance to the next member that still owes a command, or - once none
    /// does - raise the party-wide `Begin | Reselect` screen (`0x6E`) that
    /// the same idiom stores when `FUN_801DB81C` comes back equal to the
    /// party count. The round begins from that screen's `Begin`. The `Run`
    /// commit is the exception retail makes at `0x32`
    /// (`0x801D1174..0x801D1184`): it stamps category `5` on every party actor
    /// and begins the round at once, with no confirm.
    ///
    /// PORT: FUN_801D0748 (the commit idiom; `0x32`'s run confirm)
    pub(in crate::world) fn commit_party_command(
        &mut self,
        actor: u8,
        action: crate::battle_round::PendingPartyAction,
    ) {
        use crate::battle_round::PendingPartyAction;
        let party_count = self.party.party_count.clamp(1, 3);
        let run = matches!(action, PendingPartyAction::Run);
        // The commit arms re-land the log at rest; a launch in flight ends.
        self.battle.commit_log_launch = None;
        // The Auto flag stands only for an Attack committed off an Auto pick;
        // every other commit leaves it clear (the ring clears it each frame).
        let auto =
            matches!(action, PendingPartyAction::Attack { .. }) && self.battle.auto_combo.pending;
        self.commit_auto_attack_flag(actor, auto);
        if let Some(slot) = self.battle.round_flow.pending.get_mut(usize::from(actor)) {
            *slot = Some(action);
        }
        self.battle.round_flow.cursor = actor;
        if run {
            for slot in 0..party_count {
                let alive = self
                    .actors
                    .get(usize::from(slot))
                    .is_some_and(|a| a.battle.liveness != 0);
                if alive {
                    self.battle.round_flow.pending[usize::from(slot)] =
                        Some(PendingPartyAction::Run);
                }
            }
            self.begin_round_execution();
            return;
        }
        match self.next_member_owing_command(Some(actor)) {
            Some(next) => self.open_battle_command(next),
            None => self.open_commit_confirm(actor),
        }
    }

    /// Raise the party-wide commit-confirm screen (retail `ctx[+0x06] =
    /// 0x6E`) over `actor`, the member whose commit completed the party. A
    /// battle no pad drives has nobody to press `Begin`, so it begins at once.
    ///
    /// REF: FUN_801D0748 (state `0x6E`, `0x801D3024..0x801D31E4`)
    pub(in crate::world) fn open_commit_confirm(&mut self, actor: u8) {
        use crate::battle_flow::BattleFlowState as Flow;
        use crate::battle_input::BattleCommandSession;
        if !self.battle.player_driven {
            self.begin_round_execution();
            return;
        }
        self.battle_ctx.active_actor = actor;
        self.battle.command = Some(BattleCommandSession::new_commit_confirm(actor, actor));
        self.set_battle_flow(Flow::CommitBegin);
    }

    /// Give `next` its turn in the execution band: age its buffs, then a
    /// blocked actor loses the turn, a monster runs its AI pick, and a party
    /// member dispatches the command it committed (a member with none - the
    /// auto-fight party, or one the member walk skipped - strikes, and a
    /// confused one strikes and re-targets).
    ///
    /// REF: FUN_801E295C (state `0x0C`: `FUN_801EED1C` for a party slot, the
    /// `0x380` re-target for a delegated one)
    pub(super) fn dispatch_battle_turn(&mut self, next: u8) {
        let party_count = self.party.party_count.max(1);
        // The turn picker's head drops any counter latch the last pick left
        // unconsumed (`sw zero,0x6970(v0)` at `0x801DABB4`).
        self.battle_ctx.counter_pending = 0;
        // Start-of-turn: age this actor's buffs / debuffs, reverting any
        // that expire this turn.
        self.tick_battle_buffs_on_turn(next);
        if self.actor_blocked_from_acting(next) {
            // Sleep / Stone / Faint: the actor loses its turn. Its
            // initiative key was already consumed by the picker, so the
            // next advance moves on; advancing `active_actor` also moves
            // the no-speed walk past it. The status duration ticks once per
            // round at the round start (`tick_status_effects`), so the
            // affliction still wears off. The SM stays at EndOfAction (no
            // action armed) - exactly the "skipped turn" outcome.
            self.battle_ctx.active_actor = next;
            return;
        }
        if next >= party_count {
            self.take_monster_turn(next);
            return;
        }
        if self.actor_is_confused(next) {
            // Confused party member: it "acts uncontrollably", so the player
            // never got the ring - auto-arm a physical strike, then flip the
            // target to a random living ally (the retarget runs inside
            // `arm_party_physical`).
            self.arm_party_physical(next);
            return;
        }
        let pending = self.battle.round_flow.pending[usize::from(next)]
            .take()
            .map(|action| self.redirect_pending_off_dead_target(action));
        match pending {
            Some(action) => self.dispatch_pending_party_action(next, action),
            None => self.arm_party_physical(next),
        }
    }

    /// The turn picker's dead-target redirect for the party member it just
    /// picked: `FUN_801DABA4` calls `FUN_801DB124` at `0x801DAF14`, gated on
    /// the command-flow byte `ctx[+0x06] == 0xFF` - the round's execution
    /// band, which is the only band the engine dispatches from. A physical
    /// command (category `3`: a plain strike or an arts string) whose target
    /// died earlier in the round re-rolls a living slot on the dead target's
    /// own side (`rand % party_count`, or `rand % monster_count + 3`),
    /// drawing until one lives. Without it the member walks at a corpse the
    /// range law never brings into reach, and the attack short step `0x19`
    /// (which has no timeout) holds the round forever.
    ///
    /// The engine seats its first monster at `party_count` rather than at
    /// retail's fixed slot 3, so the kernel runs in retail's slot space and
    /// the result maps back. Magic and item commands carry their own target
    /// resolution at dispatch and are left alone here.
    ///
    /// REF: FUN_801DABA4 (party arm, `0x801DAEC4..0x801DAF1C`)
    /// REF: FUN_801DB124 (`vm::battle_action::redirect_dead_target`)
    pub(super) fn redirect_pending_off_dead_target(
        &mut self,
        action: crate::battle_round::PendingPartyAction,
    ) -> crate::battle_round::PendingPartyAction {
        use crate::battle_round::PendingPartyAction as Pending;
        match action {
            Pending::Attack { target } => Pending::Attack {
                target: self.redirect_dead_battle_target(target, 3, 0),
            },
            Pending::Art {
                sequence,
                target_row,
                target_slot,
            } => {
                // The arts target is row-relative; the redirect keeps the
                // side, so it maps back into the same row.
                use crate::target_picker::CursorRow;
                let party_count = self.party.party_count.max(1);
                let target_slot = match target_row {
                    CursorRow::Enemy => {
                        self.redirect_dead_battle_target(party_count + target_slot, 3, 0)
                            - party_count
                    }
                    CursorRow::Ally => self.redirect_dead_battle_target(target_slot, 3, 0),
                };
                Pending::Art {
                    sequence,
                    target_row,
                    target_slot,
                }
            }
            other => other,
        }
    }

    /// [`vm::battle_action::redirect_dead_target`] over the engine's
    /// compacted seating: `target` is an engine slot; the return is the
    /// engine slot to use (unchanged when no redirect applies).
    pub(in crate::world) fn redirect_dead_battle_target(
        &mut self,
        target: u8,
        category: u8,
        param0: u8,
    ) -> u8 {
        const RETAIL_FIRST_MONSTER: u8 = 3;
        let party_count = self.party.party_count.max(1);
        let seated = self.actors.len().min(BATTLE_SLOTS) as u8;
        let monster_count = seated.saturating_sub(party_count);
        let to_retail = |s: u8| {
            if s < party_count {
                s
            } else {
                s - party_count + RETAIL_FIRST_MONSTER
            }
        };
        let to_engine = |s: u8| {
            if s < RETAIL_FIRST_MONSTER {
                s
            } else {
                s - RETAIL_FIRST_MONSTER + party_count
            }
        };
        if target >= seated || monster_count == 0 {
            return target;
        }
        let alive: Vec<bool> = (0..seated)
            .map(|s| self.actors[usize::from(s)].battle.hp != 0)
            .collect();
        // The kernel retries until it lands on a living slot; a side with
        // nobody left alive has already ended the battle, but never loop on it.
        let side_alive = if target < party_count {
            alive[..usize::from(party_count)].iter().any(|&a| a)
        } else {
            alive[usize::from(party_count)..].iter().any(|&a| a)
        };
        if !side_alive {
            return target;
        }
        let is_alive = |retail: u8| {
            let s = to_engine(retail);
            // A retail slot past the seated band (party slots `party_count..3`
            // when fewer than three are seated) is never alive.
            (retail >= RETAIL_FIRST_MONSTER || retail < party_count)
                && alive.get(usize::from(s)).copied().unwrap_or(false)
        };
        // The Magic arm keys on the cast's spell-table class byte `+0`
        // (`0x800754C8[param0 * 12]`, `sltiu v0,v0,0xa` at `0x801DB1C8`).
        // Without the table (a disc-free fixture) the cast reads as a plain
        // `0x14` cast: every record a monster can pick (`0x25..=0x7F` and the
        // `'c'` capture class) carries a class at or above `0x0A`.
        let cast_class = if category == 2 {
            self.spell_table_class(param0).unwrap_or(0x14)
        } else {
            0
        };
        let mut rng = || (self.next_rand() & 0x7FFF) as i32;
        vm::battle_action::redirect_dead_target(
            vm::battle_action::RedirectQuery {
                target_slot: to_retail(target),
                category,
                param0,
            },
            party_count,
            monster_count,
            &mut rng,
            is_alive,
            |_| cast_class,
        )
        .map_or(target, to_engine)
    }

    /// Backstop for a session that was already open when the flow byte moved
    /// onto the round-open `Begin | Run` prompt.
    ///
    /// Retail's `0x14` arm sets `ctx[+0x06] = 0x1E` before any member picks,
    /// and the ring (`0x28`) is only reached through it - so the prompt is a
    /// property of the **round**, not of the turn. The port reads that off
    /// [`crate::battle_flow::BattleFlowState::TurnPrompt`]: the round boundary
    /// parks the flow there and battle entry leaves it at `Idle`, and
    /// [`World::open_battle_command`] now builds the session **already on the
    /// prompt** in both cases. What is left for this pass is the one ordering
    /// it cannot cover - a session opened while the flow was elsewhere and
    /// still open when the boundary parks it here. A session reopened
    /// mid-round (a submenu backed out of) finds the flow on a window state
    /// and is left alone, which is where retail's own cancel arms land.
    ///
    /// An **ambushed** party never reaches here on its lost round: the
    /// `ctx[+0x290]` side lockout ([`World::reseed_initiative`]) zeroes every
    /// party key, so no party turn opens - retail's `0x0B -> 0xFE` jump in the
    /// port's own seating.
    ///
    /// REF: FUN_801D0748 (states 0x14 / 0x1E)
    pub(super) fn arm_round_open_prompt(&mut self) {
        use crate::battle_flow::BattleFlowState;
        use crate::battle_input::CommandPhase;
        if self.battle.flow != BattleFlowState::TurnPrompt {
            return;
        }
        let no_escape = self.battle.no_escape;
        if let Some(session) = self.battle.command.as_mut()
            && matches!(session.phase, CommandPhase::Menu { .. })
        {
            session.no_escape = no_escape;
            session.phase = CommandPhase::RoundPrompt { cursor: 0 };
        }
    }

    /// Apply one generic physical attack from the active attacker to its
    /// resolved target as an **immediate** combo: every swing of the AGL
    /// budget rolls through the retail melee kernel
    /// ([`legaia_engine_vm::battle_formulas::physical_predamage`], the body
    /// of `FUN_801EC3E4`) and accumulates, then the total lands on live HP
    /// once - the retail accumulate / apply shape without a clip to pace it.
    ///
    /// This is the path for an attacker whose stream carries no bytes: a
    /// monster with no attack entries in its catalog (the synthetic
    /// catalog), whose swing count is the AGL budget
    /// ([`Self::arm_monster_strike_budget`]). Every actor with a seeded
    /// stream is paced by the hit-event driver instead.
    ///
    /// REF: FUN_801EC3E4
    pub(in crate::world) fn apply_basic_attack(&mut self) {
        let attacker = self.battle_ctx.active_actor;
        let party_count = self.party.party_count.max(1);
        let strikes = if attacker >= party_count {
            self.battle.monster_strike_budget.max(1)
        } else {
            1
        };
        let committed = self
            .actors
            .get(attacker as usize)
            .map(|a| a.battle.current_anim)
            .unwrap_or(0);
        let mut target = None;
        for n in 0..strikes {
            let Some(t) = self.resolve_attack_target(attacker) else {
                break;
            };
            target = Some(t);
            // The total lands after the last strike, which is the one hit the
            // kill check sees.
            let last = n + 1 == strikes;
            self.land_melee_hit(attacker, t, BASIC_ATTACK_COMMAND, committed, false, last);
        }
        if let Some(t) = target {
            self.apply_combo_total(t);
        }
    }
}
