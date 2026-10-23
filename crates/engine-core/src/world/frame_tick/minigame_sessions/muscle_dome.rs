//! The Muscle Dome contest: arena entry and exit, leg reporting and settlement,
//! the special-battle words, the arena tick, the playback tally and the hub
//! between legs.

use super::*;
use legaia_engine_mode::mode::SceneMode;

impl World {
    /// Enter the Muscle Dome contest on `session`, suspending the current
    /// scene mode (restored by [`World::exit_muscle_dome`]). Same suspend
    /// contract as the other minigames / the pause menu.
    ///
    /// A contest that has just been opened also runs its **start restore**
    /// here: the arena refills the lead fighter's HP / MP / SP to their
    /// maxima, and on every course above Beginner strips the four gear slots
    /// first. Retail does this in the arena entry's first-entry arm, so it
    /// fires once per contest and not at a leg boundary
    /// ([`legaia_engine_minigames::muscle_dome::DomeContest::take_start_restore`]).
    ///
    /// PORT: FUN_801d0ed8 (the apply site; the body is
    /// `muscle_dome::apply_contest_start_restore`)
    pub fn enter_muscle_dome(
        &mut self,
        session: legaia_engine_battle_menus::muscle_dome_session::MuscleDomeSession,
    ) {
        if self.mode != SceneMode::MuscleDome {
            self.minigames.muscle_return_mode = self.mode;
        }
        if let Some(restore) = self
            .minigames
            .muscle_contest
            .as_mut()
            .and_then(|c| c.take_start_restore())
            && let Some(rec) = self.party.roster.members.first_mut()
        {
            legaia_engine_minigames::muscle_dome::apply_contest_start_restore(rec, restore);
        }
        let mut session = session;
        session.arm_intro();
        self.minigames.muscle_dome = Some(session);
        self.minigames.muscle_hub_between_legs = false;
        self.mode = SceneMode::MuscleDome;
        // The hub's announcer lines, staged ahead of the first visit's arms.
        self.queue_xa_prestage(legaia_engine_minigames::muscle_ringside::hub_xa_prestage());
    }

    /// Leave the arena **at the player's request** - the escape both hosts
    /// share (`Start` through [`Self::poll_minigame_escape`], and the native
    /// window's `M` hotkey).
    ///
    /// Leaving is not only [`Self::exit_muscle_dome`]: the leg has to be
    /// reported to the open contest, or the ladder carries on as if the leg
    /// never happened. A leg left undecided is the arena's run / give-up path
    /// ([`legaia_engine_minigames::muscle_dome::LEG_OUTCOME_RAN`], retail's `_DAT_80084448 = 4`
    /// arm), which ends the contest and voids the tally; a decided leg
    /// reports its own result, exactly as the Won / Lost confirm does. A
    /// contest that has run out then settles on the spot.
    ///
    /// Only the native hotkey used to report; the shared escape exited
    /// without a report, so on the browser play page a left leg kept the
    /// contest open with its tally intact.
    pub fn leave_muscle_dome(
        &mut self,
    ) -> Option<legaia_engine_battle_menus::muscle_dome_session::MuscleDomeSession> {
        use legaia_engine_battle_menus::muscle_dome_session::MusclePhase;
        use legaia_engine_minigames::muscle_dome::{LEG_OUTCOME_RAN, LegReport};
        if self.minigames.muscle_hub_between_legs && self.minigames.muscle_dome.is_none() {
            // Between legs: the last leg is already reported, so leaving
            // here is giving up the next one - the run / give-up arm, which
            // ends the contest and voids the tally.
            self.leave_muscle_arena();
            self.report_muscle_leg(LegReport {
                survived: true,
                outcome: LEG_OUTCOME_RAN,
                turns_taken: 0,
            });
            self.settle_muscle_contest();
            return None;
        }
        let s = self.exit_muscle_dome()?;
        let phase = s.phase();
        let decided = matches!(phase, MusclePhase::Won | MusclePhase::Lost);
        self.report_muscle_leg(LegReport {
            survived: phase != MusclePhase::Lost,
            outcome: if decided { 0 } else { LEG_OUTCOME_RAN },
            turns_taken: s.turn(),
        });
        self.settle_muscle_contest();
        Some(s)
    }

    /// Leave the Muscle Dome and restore the interrupted mode.
    ///
    /// **A leg pays nothing.** The finished leg is reported to the open
    /// contest ([`World::report_muscle_leg`]), which is what decides whether
    /// the ladder carries on and what the run is eventually worth; a contest
    /// that has reached its end settles here and pays into the coin bank
    /// ([`World::settle_muscle_contest`]).
    ///
    /// It used to credit a Seru capture on a won leg, keyed off the victory
    /// caption's spell id. That was a misattribution: the caption table the
    /// id indexes is the shared battle-family cast-caption table, reached by
    /// any cast in any battle overlay, and the arena grants no Seru at all.
    ///
    /// Returns the session so the host can read the final state.
    pub fn exit_muscle_dome(
        &mut self,
    ) -> Option<legaia_engine_battle_menus::muscle_dome_session::MuscleDomeSession> {
        self.leave_muscle_arena();
        self.end_muscle_leg()
    }

    /// Hand the frame back from the arena: the interrupted mode and the
    /// venue's own music, and no between-legs hub left open.
    pub(in crate::world::frame_tick) fn leave_muscle_arena(&mut self) {
        self.minigames.muscle_hub_between_legs = false;
        if self.mode == SceneMode::MuscleDome {
            self.mode = self.minigames.muscle_return_mode;
        }
        // Give the venue its own music back when the arena's battle theme
        // displaced it (no-op when it did not).
        self.restore_minigame_bgm();
    }

    /// The battle end of one leg, which leaves the mode alone: take the
    /// session, write the fighter's HP back and pick the ringside still.
    pub(in crate::world::frame_tick) fn end_muscle_leg(
        &mut self,
    ) -> Option<legaia_engine_battle_menus::muscle_dome_session::MuscleDomeSession> {
        let session = self.minigames.muscle_dome.take();
        // The battle end writes the fighter's HP back into the lead record
        // (`+0x106`), and the background read that follows streams one of the
        // two ringside stills into VRAM, picked off that record: live HP
        // `+0x106` against the base maximum `+0x11C`, not the effective
        // `+0x104` (`FUN_801F6B24`, `0x801F6B8C..0x801F6BAC`).
        if let Some(s) = session.as_ref() {
            let fought = s.hp(0).clamp(0, i32::from(u16::MAX)) as u16;
            let (hp_cur, hp_max) = match self.party.roster.members.first_mut() {
                Some(rec) => {
                    // The two saved command strings live in the record:
                    // the review confirm's `FUN_801DA59C` wrote the leg's
                    // into them (`MuscleDomeSession::auto_command_strings`).
                    let (primary, secondary) = s.auto_command_strings();
                    rec.set_auto_command_string(legaia_save::AutoCommandBand::Primary, primary);
                    rec.set_auto_command_string(legaia_save::AutoCommandBand::Secondary, secondary);
                    let mut hms = rec.hp_mp_sp();
                    // A hand-built record with no maximum has nothing to
                    // write back into; the pick reads the fight's HP.
                    if hms.hp_max > 0 {
                        hms.hp_cur = fought.min(hms.hp_max);
                        rec.set_hp_mp_sp(hms);
                    } else {
                        hms.hp_cur = fought;
                    }
                    (hms.hp_cur, rec.record_stats().hp_max)
                }
                None => (fought, 0),
            };
            self.minigames.muscle_ringside_still = Some(
                legaia_engine_minigames::muscle_ringside::still_prot_index(hp_cur, hp_max),
            );
        }
        session
    }

    /// Report the finished leg to the open contest and step the between-leg
    /// hub, applying the HP the recovery lanes hand back to the lead fighter.
    ///
    /// This is the arena's own re-entry: the ladder advances one leg, the new
    /// `(course, round)` decodes out of the sub-id word, and the hub decides
    /// between staging another leg and settling. Returns the contest state it
    /// landed in, or `None` when no contest is open.
    ///
    /// PORT: FUN_801cea6c (contest re-entry) / FUN_801cf870 states 0x0A..0x0C
    pub fn report_muscle_leg(
        &mut self,
        report: legaia_engine_minigames::muscle_dome::LegReport,
    ) -> Option<legaia_engine_minigames::muscle_dome::ContestState> {
        use legaia_engine_minigames::muscle_dome::ContestState;
        let flags = self.muscle_contest_flags();
        let hp_max = self
            .party
            .roster
            .members
            .first()
            .map(|r| r.hp_mp_sp().hp_max)
            .filter(|&hp| hp > 0)
            .unwrap_or(500);
        let contest = self.minigames.muscle_contest.as_mut()?;
        contest.finish_leg(report, hp_max, &flags);
        // The three recovery lanes drain, then the restore state hands the
        // total back to the fighter - a dome contest costs no permanent HP.
        while matches!(
            contest.state(),
            ContestState::LegScore | ContestState::Tally | ContestState::Restore
        ) {
            let restoring = contest.state() == ContestState::Restore;
            contest.advance();
            if restoring {
                // The retail store is the game-state window's `+0x6CC` /
                // `+0x6CE` pair, which is the lead party record's own
                // `+0x104` / `+0x106` HP pair (`0x80084708 - 0x80084140 =
                // 0x5C8`).
                let mut hms = match self.party.roster.members.first() {
                    Some(r) => r.hp_mp_sp(),
                    None => break,
                };
                hms.hp_cur = self
                    .minigames
                    .muscle_contest
                    .as_mut()?
                    .take_hp_restore(hms.hp_cur, hp_max);
                if let Some(rec) = self.party.roster.members.first_mut() {
                    rec.set_hp_mp_sp(hms);
                }
                return Some(self.minigames.muscle_contest.as_ref()?.state());
            }
        }
        Some(contest.state())
    }

    /// The story-flag reads the contest rules need, sampled off the system
    /// flag bank.
    pub fn muscle_contest_flags(&self) -> legaia_engine_minigames::muscle_dome::ContestFlags {
        let mut flags = legaia_engine_minigames::muscle_dome::ContestFlags::default();
        for (i, &(id, _)) in legaia_engine_minigames::muscle_dome::COURSE_UNLOCK_FLAGS
            .iter()
            .enumerate()
        {
            flags.course_unlock[i] = self.system_flag_test(id);
        }
        for (i, &(_, id)) in legaia_engine_minigames::muscle_dome::MASTER_LENGTH_GATES
            .iter()
            .enumerate()
        {
            flags.master_gates[i] = self.system_flag_test(id);
        }
        flags.prize_awarded =
            self.system_flag_test(legaia_engine_minigames::muscle_dome::CONTEST_PRIZE_FLAG);
        flags
    }

    /// The **special-battle word** `0x8007BAC0` a dome leg opens on, as
    /// retail's arena entry seeds it: `1` with no course unlocked, then
    /// `0x101` / `0x111` / `0x321` for story flags `0x536` / `0x537` /
    /// `0x538`, the last set flag winning.
    ///
    /// The word is what crosses the ring's Item chip out on every unlocked
    /// course and the Ra-Seru chip on Master
    /// ([`legaia_engine_battle_menus::muscle_dome_session::SPECIAL_ITEM_FORBIDDEN`] /
    /// [`legaia_engine_battle_menus::muscle_dome_session::SPECIAL_MAGIC_FORBIDDEN`]). Both dome entry
    /// paths hand it to
    /// [`legaia_engine_battle_menus::muscle_dome_session::MuscleDomeSession::set_special_word`].
    ///
    /// An arena re-entered with the word already non-zero only advances the
    /// ladder cursor (`0x801CEC00`); the port keeps the cursor on
    /// [`legaia_engine_minigames::muscle_dome::DomeContest`] instead, so every leg re-derives
    /// the word from the same flags and the high bits are stable across the
    /// contest - which is the property retail's low-byte-only restamp has.
    ///
    /// REF: FUN_801cea6c (`0x801CEB44..0x801CEBC8`; this samples the flag
    /// bank for [`legaia_engine_minigames::muscle_dome::contest_entry_word`], which is the port)
    pub fn dome_special_word(&self) -> u32 {
        legaia_engine_minigames::muscle_dome::contest_entry_word(&self.muscle_contest_flags())
    }

    /// The special-battle word `_DAT_8007BAC0` as the battle's `!= 0` readers
    /// see it: the open arena leg's word ORed with the regular battle's half
    /// ([`crate::world::BattleState::special_word`], whose `0x200` battle init
    /// and the formation roll raise for monster `0xAF` and the Rim Elm
    /// ambush). Retail has one word; the port keeps the two halves apart
    /// because the arena session owns its own.
    ///
    /// Every reader that tests the whole word reads this, and each is a
    /// "no spoils / no escape hatch" gate - the arena restriction bits and the
    /// Ra-Seru bit suppress the same things:
    ///
    /// | Reader | Site | Suppresses |
    /// |---|---|---|
    /// | `FUN_8004E568` | `0x8004F0AC` | the gold award (zeroed after the Golden Book bonus) |
    /// | `FUN_8004E568` | `0x8004F274` | the per-member EXP (`s6 = 0`) |
    /// | `FUN_8004E568` | `0x8004F480` | the victory drop roll's seat loop |
    /// | `FUN_8004AD80` | `0x8004B48C` | the steal attack |
    /// | `FUN_801E91E8` | `0x801E9224` | the Seru absorb (reports "already known") |
    /// | `FUN_801DDB30` | `0x801DE450` | the summon spell-XP accrual |
    /// | `FUN_801E9FD4` | `0x801EA994` | a monster flee the roll granted |
    ///
    /// REF: FUN_8004E568, FUN_8004AD80, FUN_801E91E8, FUN_801DDB30, FUN_801E9FD4
    pub fn special_battle_word(&self) -> u32 {
        self.minigames
            .muscle_dome
            .as_ref()
            .map_or(0, |s| s.special_word())
            | self.battle.special_word
    }

    /// Settle the open contest: pay the tally into the casino coin bank,
    /// apply the flags the settlement names, and hand over the one-shot
    /// Master-course prize when it is due.
    ///
    /// The tally is the contest's whole reward. Returns the settlement, or
    /// `None` when no contest is open or it has not reached its end.
    ///
    /// PORT: FUN_801d0f60 / FUN_80026018 (the coin credit)
    pub fn settle_muscle_contest(
        &mut self,
    ) -> Option<legaia_engine_minigames::muscle_dome::ContestSettlement> {
        let flags = self.muscle_contest_flags();
        let contest = self.minigames.muscle_contest.as_mut()?;
        if !contest.over() {
            return None;
        }
        let out = contest.settle(&flags);
        self.minigames.muscle_contest = None;
        self.minigames.muscle_settlement = Some(out);
        self.minigames.casino_coins = legaia_engine_minigames::muscle_dome::credit_casino_coins(
            self.minigames.casino_coins,
            out.score,
        );
        if out.set_continue_flag {
            self.system_flag_set(legaia_engine_minigames::muscle_dome::CONTEST_CONTINUE_FLAG);
        } else {
            self.system_flag_clear(legaia_engine_minigames::muscle_dome::CONTEST_CONTINUE_FLAG);
        }
        if out.set_gave_up_flag {
            self.system_flag_set(legaia_engine_minigames::muscle_dome::CONTEST_GAVE_UP_FLAG);
        } else {
            self.system_flag_clear(legaia_engine_minigames::muscle_dome::CONTEST_GAVE_UP_FLAG);
        }
        if let Some(id) = out.set_ran_first_flag {
            self.system_flag_set(id);
        }
        if out.award_prize {
            self.system_flag_set(legaia_engine_minigames::muscle_dome::CONTEST_PRIZE_FLAG);
            let slot = self
                .party
                .inventory
                .entry(legaia_engine_minigames::muscle_dome::CONTEST_PRIZE_ITEM_ID)
                .or_insert(0);
            *slot = slot.saturating_add(1).min(legaia_save::STACK_CAP);
        }
        Some(out)
    }

    /// Advance the Muscle Dome one frame, reading this frame's pad:
    ///
    /// - **Select**: [`Left`](input::PadButton::Left) /
    ///   [`Right`](input::PadButton::Right) / [`Up`](input::PadButton::Up) /
    ///   [`Down`](input::PadButton::Down) commit the four dealt directions
    ///   (the retail direction bits, in the `ctx+0x1114..+0x1120` slot
    ///   order); [`Cross`](input::PadButton::Cross) confirms the queue. The
    ///   opponent commits through the shared selection logic when the player
    ///   confirms.
    /// - **Resolve**: each side's whole queued string plays out through the
    ///   session's installed [`DomeDamageModel`] - the *shared* retail damage
    ///   kernel (move-power record → predamage roll → element affinity →
    ///   finisher, on the contest's PsyQ `rand()` stream), the same one the
    ///   browser host resolves with. A session with no model installed
    ///   resolves to no damage rather than to invented constants.
    /// - **TurnOver / decided**: the next turn is taken automatically (retail
    ///   confirms nothing at a turn boundary), and [`Cross`] closes a
    ///   finished leg: it is reported to the contest, and a survived leg
    ///   with the course not exhausted stays in the arena for the hub's
    ///   INTERVAL / ROUND screens ([`World::muscle_hub_between_legs`]) while
    ///   every other leg settles and hands the field back. A leg finishes on a KO and on nothing else:
    ///   turns are counted, never budgeted. Retail agrees - the arena hands
    ///   the round to an ordinary battle (`FUN_801D1510` sets game mode
    ///   `0x14`), and the only battle-end signal comes from the `0x5A`
    ///   end-of-action KO scans.
    ///
    /// [`DomeDamageModel`]: legaia_engine_minigames::muscle_dome::DomeDamageModel
    ///
    /// `FUN_801D0748` is the **battle overlay's** round / flow SM (context
    /// pointer `_DAT_8007BD24`, phase byte `ctx+6`), not a dome-specific
    /// controller: its 2781 instructions form none of the dome's own tables
    /// (`0x801F4B8C` and friends appear nowhere in it). It is reached here
    /// because a dome leg *is* an ordinary battle, so the reuse is the retail
    /// chain rather than a shape match.
    ///
    /// PORT: FUN_801d0748 (that round driver's phase loop: pick / commit /
    /// resolve), with the presentation left to the host.
    pub(in crate::world::frame_tick) fn tick_muscle_dome(&mut self) {
        use legaia_engine_battle_menus::muscle_dome_session::MusclePhase;
        let Some(phase) = self.minigames.muscle_dome.as_ref().map(|s| s.phase()) else {
            // Between legs the arena hub owns the frame (its INTERVAL, the
            // ringside still and the ROUND card) until
            // [`Self::begin_next_muscle_leg`] stages the next fight.
            if !self.minigames.muscle_hub_between_legs {
                self.leave_muscle_arena();
            }
            return;
        };
        // The battle-open hold runs once the hub's own screens (the first
        // visit, the leg-open ROUND card) have handed the leg over - retail
        // starts the battle only past the hub's arm `0x16`.
        if !self.minigames.muscle_hub.covers_leg() {
            // `ctx[+0x6D6] -= 0x1F800393` per battle pass, one a vsync tick.
            let step = u16::from(super::super::battle::BATTLE_PASS_STEP_PER_TICK);
            if let Some(s) = self.minigames.muscle_dome.as_mut() {
                s.tick_intro(step);
            }
        }
        // The play-out choreography steps every tick: the bodies idle
        // between turns, act a resolved turn out, and hold a knockdown once
        // the leg is decided.
        if let Some(s) = self.minigames.muscle_dome.as_mut() {
            s.tick_choreo();
        }
        let confirm = self.input.just_pressed(input::PadButton::Cross);
        match phase {
            MusclePhase::Select => {
                // The whole selection - the command ring, the Auto |
                // Command prompt, the direction entry and its review, the
                // Ra-Seru list and the Begin | Reselect confirm - is the
                // session's command flow, which runs the battle's own
                // command and entry sessions (`muscle_dome::DomeMenu`).
                let pad = legaia_engine_battle_menus::muscle_dome_session::DomeSelectPad {
                    left: self.input.just_pressed(input::PadButton::Left),
                    right: self.input.just_pressed(input::PadButton::Right),
                    up: self.input.just_pressed(input::PadButton::Up),
                    down: self.input.just_pressed(input::PadButton::Down),
                    confirm,
                    cancel: self.input.just_pressed(input::PadButton::Circle),
                    triangle: self.input.just_pressed(input::PadButton::Triangle),
                    select_attack: self.toggles.select_attack,
                };
                let ev = self
                    .minigames
                    .muscle_dome
                    .as_mut()
                    .map(|s| s.select_input(pad));
                if ev == Some(legaia_engine_battle_menus::muscle_dome_session::DomeMenuEvent::Run) {
                    // Run on the round prompt: the leg is reported as ran,
                    // which settles the contest as a give-up - the shared
                    // escape path.
                    let _ = self.leave_muscle_dome();
                }
            }
            MusclePhase::Resolve => {
                if let Some(s) = self.minigames.muscle_dome.as_mut() {
                    // With no disc tables staged this closes the turn without
                    // damage rather than substituting invented numbers - and
                    // rather than parking the leg in `Resolve` forever. The
                    // damage rolls draw on the world stream (retail's one
                    // `rand()` seed).
                    s.resolve_turn_on_stream(&mut self.rng_state);
                    // The turn's plays now act out on the session's
                    // choreography (`muscle_dome_session::choreo`): the
                    // approach, the swings with their hits, the done tails.
                    // The leg holds at `TurnOver` until it has, as retail's
                    // action phases run before the round driver re-enters
                    // the command cluster.
                }
            }
            MusclePhase::TurnOver => {
                // Retail's turn boundary is automatic, not confirmed: the
                // battle-action SM writes `ctx[6] = 0x14` at `0x801E67F0` and
                // the round driver re-enters its own command cluster with no
                // press. The arena hub - the only thing that draws an
                // INTERVAL screen - runs in arena mode `0x18` and is not
                // executing during a leg, so a confirm gate here was a silent
                // one-press stall with nothing on screen to explain it.
                // REF: FUN_801e295c (turn-top arm)
                if let Some(s) = self.minigames.muscle_dome.as_mut()
                    && !s.playback_running()
                {
                    s.next_turn();
                }
            }
            MusclePhase::Won | MusclePhase::Lost => {
                if confirm {
                    let report = legaia_engine_minigames::muscle_dome::LegReport {
                        survived: phase == MusclePhase::Won,
                        outcome: 0,
                        turns_taken: self.minigames.muscle_dome.as_ref().map_or(0, |s| s.turn()),
                    };
                    self.end_muscle_leg();
                    self.report_muscle_leg(report);
                    // A contest that has run out settles on the spot: the
                    // payout is the contest's, not the leg's.
                    self.settle_muscle_contest();
                    // A survived leg with the course not exhausted re-enters
                    // the arena hub (state `0x0A`), which plays the INTERVAL
                    // tally and the ROUND card and then starts the next fight
                    // itself (`FUN_801D1510` past arm `0x16`): the player
                    // never leaves the arena between legs. Every other leg
                    // settles and hands the field back.
                    let continues =
                        legaia_engine_minigames::muscle_dome::leg_boundary_raises_interval(
                            self.minigames.muscle_contest.as_ref().map(|c| c.state()),
                        );
                    if continues {
                        self.minigames.muscle_hub_between_legs = true;
                    } else {
                        self.leave_muscle_arena();
                    }
                }
            }
        }
    }

    /// Whether the resolved turn is still playing out: while it is the leg
    /// sits at `TurnOver` and the hosts show the turn's plays rather than the
    /// next command cluster. Retail's round driver reaches its command phase
    /// only after the action phases `0xFE` / `0xFF` have played every queued
    /// action.
    pub fn muscle_playback_running(&self) -> bool {
        self.minigames
            .muscle_dome
            .as_ref()
            .is_some_and(|s| s.playback_running())
    }

    /// The play the choreography is acting out this tick and its attacker's
    /// running damage total: `(attacker slot, total)`. The total sums that
    /// attacker's landed damage up to and including the current play, which
    /// is the tally retail's play-out counts up ("TOTAL n"). `None` outside a
    /// playback.
    pub fn muscle_playback_tally(&self) -> Option<(usize, i32)> {
        let s = self.minigames.muscle_dome.as_ref()?;
        let (beat, _) = s.choreo().beat()?;
        let plays = s.last_turn_plays();
        let i = beat.play.min(plays.len().checked_sub(1)?);
        let attacker = plays[i].attacker.min(1);
        if beat.kind == legaia_engine_battle_menus::muscle_dome_session::choreo::BeatKind::Approach
        {
            // The walk in is the acting side's action; nothing has landed.
            return Some((attacker, 0));
        }
        let sum = plays[..=i]
            .iter()
            .filter(|p| p.attacker.min(1) == attacker)
            .map(|p| p.damage.max(0))
            .sum();
        Some((attacker, sum))
    }

    /// Whether the arena hub is between two legs of an open contest: the
    /// mode is still [`SceneMode::MuscleDome`], no leg is open, and the hub's
    /// INTERVAL / ROUND screens own the frame.
    pub fn muscle_hub_between_legs(&self) -> bool {
        self.minigames.muscle_hub_between_legs
            && self.minigames.muscle_dome.is_none()
            && self.mode == SceneMode::MuscleDome
    }

    /// Advance the arena hub's screen timers one tick
    /// ([`legaia_engine_minigames::muscle_ringside::HubTimers`], `FUN_801CF870`'s screen arms),
    /// off this tick's leg / contest edges and pad. Runs every tick in every
    /// mode - the INTERVAL screen plays after the leg has closed - and is
    /// called by the shared scene host right after [`Self::tick`], so the
    /// two play hosts and a headless harness all run one hub. The hub's
    /// hand-off past arm `0x16` stages the next fight here
    /// ([`Self::begin_next_muscle_leg`]); the sounds it fires queue for the
    /// host ([`Self::take_muscle_hub_sounds`]).
    pub fn tick_muscle_hub(&mut self) {
        let pad = self.input.retail_pad().pressed as u16;
        // `_DAT_80084580`, the voice/SFX volume each tally cue halves.
        let volume_word = self.audio.levels.voice_volume as u32;
        let mut timers = std::mem::take(&mut self.minigames.muscle_hub);
        let frame = timers.tick(
            legaia_engine_minigames::muscle_ringside::HubTickInputs {
                leg_open: self.minigames.muscle_dome.is_some(),
                contest: self.minigames.muscle_contest.as_ref(),
                ringside_still: self.minigames.muscle_ringside_still,
                between_legs: self.muscle_hub_between_legs(),
            },
            pad,
            volume_word,
        );
        self.minigames.muscle_hub = timers;
        if frame.next_leg {
            self.begin_next_muscle_leg();
        }
        // The INTERVAL arm's tally cues go through the SFX ring like every
        // other cue: the drainer resolves them against the arena bundle.
        for &(slot, id, delay) in &frame.ring_cues {
            self.audio
                .sfx_ring_ops
                .push(legaia_engine_system::sound_state::SfxRingOp::ArmSlot(
                    slot, id, delay,
                ));
        }
        let sounds = &mut self.minigames.muscle_hub_sounds;
        if frame.xa.is_some() {
            sounds.xa = frame.xa;
        }
        sounds.voice_cues.extend(frame.voice_cues);
        // A host that never drains (a headless harness) keeps only the
        // latest roll's keys.
        let excess = sounds.voice_cues.len().saturating_sub(32);
        sounds.voice_cues.drain(..excess);
    }

    /// Drain the CD-XA line and the tally voice keys the hub fired since the
    /// last drain, for the host to sound.
    pub fn take_muscle_hub_sounds(
        &mut self,
    ) -> legaia_engine_minigames::muscle_ringside::HubTimersFrame {
        std::mem::take(&mut self.minigames.muscle_hub_sounds)
    }

    /// Stage the contest's next fight once the between-legs hub has played
    /// out - the hub's hand-off past arm `0x16` (`FUN_801D1510`). The fight
    /// opens through the same mode-24 drain the arena door uses, without
    /// re-arming the round trip (the departure scene stays backed up from
    /// the door). A no-op when the hub is not between legs.
    ///
    /// [`Self::tick_muscle_hub`] calls it on [`legaia_engine_minigames::muscle_ringside::HubTimersFrame::next_leg`].
    pub fn begin_next_muscle_leg(&mut self) {
        if !self.muscle_hub_between_legs() {
            return;
        }
        // The flag stays up until the leg is installed
        // ([`Self::enter_muscle_dome`]), so the arena keeps the frame on the
        // ticks before the scene host drains the request.
        self.minigames.pending_warp =
            Some(legaia_engine_mode::minigame_entry::MinigameSubId::MuscleDome.sub_id());
    }
}
