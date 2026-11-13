//! Battle **end presentation** - what retail draws between the last combatant
//! falling and the field reloading. The results sequencer `FUN_8004E568`
//! runs every frame the battle-end signal `DAT_8007BD71 == 0xFE` is up
//! (`FUN_80046A20` `0x800470D0..0x800470E8`), the action SM does not
//! (`0x80047040`), and the battle only exits once the sequencer's phase
//! halfword `ctx[+0x6CE]` reaches `0x43` (`0x80046DAC`).
//!
//! PORT: FUN_8004e568 (the presentation half - the phase walk, the pose
//! tier + pose pick, the results hold, the exit fade; the reward arithmetic
//! it also carries is `battle_formulas::victory` / `World::apply_battle_loot`
//! and the level-up applier is `levelup`).
//! REF: FUN_80046a20 (the `ctx[+0x6CE] >= 0x43` exit gate)
//!
//! # The retail timeline (disassembly + a PCSX-Redux poll, N=1)
//!
//! `_DAT_8007BD2C` is both the wipe cause the `0x5A` gate writes and the
//! sequencer's phase word: a **victory** (`0`) walks the jump table at
//! `0x800152FC` as `0 -> 2 -> 4 -> 5` - phase 0 picks the pose tier and
//! kicks the hero's `monster.snd` voice clip into VAB slot 7, phase 2 waits
//! for the CD then streams PROT 0889 (the level-up jingle bank) toward slot
//! 11, phase 4 waits again and installs it, setting `DAT_8007BD60 |= 0x80`;
//! a **party wipe** (`5`) lands on phase 5 at once with that bit clear,
//! which is what selects the annihilated arm (`0x8004F8C0`). Through phases
//! `0..=4` the pose actor is framed at `FUN_801D5854(seat, 8)`; on the
//! `rim_elm_gimard_victory` state that load window measures **80 vsyncs**
//! (signal at v322, results frame at v402; `autorun_victory_timeline.lua`).
//!
//! The results frame (`ctx[+0x6CE]` `0 -> 1`, `0x8004EEB4..0x8004F73C`)
//! sets story flag `0x35`, bumps the round counter, stages the chosen pose
//! id into the pose actor's `+0x1DA` with `+0x1DC |= 2`, seeds the hold
//! timer `0x8007BD6C = 0`, floors every downed member's HP at 1, credits
//! XP / gold / the drop, runs the level-up applier, opens the result window
//! (`FUN_801D8DE8(0x41)`), and - when any member levelled - fires cue `0x50`
//! and the level-up window `0x44 + mask`. The hold then counts one per
//! vsync; at `0x100` (with the field preload `ctx[+0xB]` settled) the
//! exit fade template (kind 2, `0x40` frames, black -> white; kind 2 is the
//! `B - F` blend, so the scene fades **to black**) is spawned and the
//! phase halfword starts counting from 2 (`0x8004FC6C`); the exit gate
//! fires at `0x43`. Measured: fade at v657 (= results + 255), exit at v723
//! (= fade + 66).
//!
//! The pose actor is `ctx[+0x13]` (`lbu s8,0x13(a0)` at `0x8004E5A4`, `a0` the
//! battle context `*0x8007BD24`), the acting slot: the member who landed the
//! last action poses, not the leader. The action SM's victory arm writes it
//! last (`FUN_801E295C` runs with `s5 = ctx + 0x11`, so its `sb v1,0x2(s5)`
//! stores are `ctx[+0x13]`): it keeps a living acting actor (`0x801E6690`),
//! re-picks a dead one with `rand() % party_count` (`0x801E66EC`), and forces
//! seat `2` when the formation's first monster is `0xB3` and seat `1` when it
//! is `0xB4` (`0x801E6740` / `0x801E6760`) - Songi's fights - before it arms
//! that seat's win-pose archive (`FUN_80055B4C(char * 3 - 1)`, `0x801E678C`).
//! The engine runs that arm as `battle_action::end_of_action`'s
//! `victory_pose_fixup`, which leaves the seat in `World::battle_ctx`'s
//! `active_actor`; [`World::begin_battle_end_sequence`] seats the pose
//! there. The three-member capture of the results frame
//! (`noa_levelup_banner`: Noa levelled, seat 0 carries the staged pose
//! `0x14`, `ctx[+0x13] == 0`) is a fight Vahn finished.
//!
//! # What the port does with it
//!
//! [`World::begin_battle_end_sequence`] arms a `VictorySequence` (with the
//! pose kernels, `legaia_engine_battle::victory`) where the
//! SM's `BattleComplete` used to call `finish_battle` on the spot, and
//! [`World::tick_battle_end_sequence`] walks it one retail frame per tick
//! while the scene stays in [`SceneMode::Battle`]. The two CD-bound waits are
//! one constant, `VICTORY_LOAD_FRAMES` - the engine streams nothing, so the
//! measured span is kept as a hold rather than modelled as a drive. The hero
//! voice clip is the pose's `monster.snd` bank
//! ([`VictorySequence::voice_bank`]), which both hosts stage over slot 7
//! from the sequence's arming (`World::battle_monster_sound_banks`); the pose
//! clip's own cue track keys it (cue `0xA1`, category `7`).
//!
//! An **escape** (`0x66` -> `0x67`) takes the sequencer's `0x67` arm instead
//! (`lbu v1,0x7(a0); li v0,0x67` at `0x8004E63C`): no results, the phase
//! halfword counts up from its battle-start zero by the vsync delta
//! (`0x8004E70C..0x8004E724`) behind the fade the SM's `0x66` arm
//! spawned, and the same `0x43` gate exits. A **party wipe** takes the
//! annihilated arm: the loss window, the same `0x100` hold and fade,
//! every member's HP floored at 1, then the MAIN INIT game-over gate that
//! [`World::finish_battle`] folds. Its framing is the shared tail's
//! `FUN_801D5854(ctx[+0x13], 6)` on whoever acted last - the monster that
//! landed the wiping blow, which stages its `0x22` knockout taunt - with the
//! yaw counter at `0x7C0`, and a boss formation barks once
//! ([`party_wipe_bark`]).

use super::*;
use legaia_engine_battle::battle_events::BattleSfxCue;
use legaia_engine_battle::victory::*;

/// The yaw counter `ctx[+0x6DA]` the annihilated arm's results frame stores
/// (`li v0,0x7c0` at `0x8004F920`).
const WIPE_YAW_BASE: i32 = 0x7C0;

impl World {
    /// Re-exported for hosts / tests that pace the sequence.
    pub const VICTORY_LOAD_FRAMES: u16 = VICTORY_LOAD_FRAMES;
    pub const VICTORY_RESULTS_HOLD_FRAMES: u16 = VICTORY_RESULTS_HOLD_FRAMES;
    pub const VICTORY_EXIT_PHASE: u16 = VICTORY_EXIT_PHASE;
    pub const VICTORY_FADE_PHASE_SEED: u16 = VICTORY_FADE_PHASE_SEED;

    /// Arm the end-of-battle presentation for the cause the action SM just
    /// raised. Replaces the immediate `finish_battle` at `BattleComplete`.
    ///
    /// A monster wipe enters the load phases; a wipe lands on the results
    /// frame directly (retail phase 5 with the survived bit clear); an escape
    /// enters the exit hold (the SM's `0x66` arm already spawned the fade).
    /// While the game-over hold is up the repeat `BattleComplete`s the parked
    /// wipe scan keeps raising are consumed exactly as before.
    pub(in crate::world) fn begin_battle_end_sequence(&mut self) {
        if self.game_over_hold {
            self.battle.end = None;
            return;
        }
        if self.battle.victory.is_some() {
            return;
        }
        let cause = self.battle.end.unwrap_or(BattleEndCause::MonsterWipe);
        let phase = match cause {
            BattleEndCause::MonsterWipe => VictoryPhase::Loading {
                frames_left: VICTORY_LOAD_FRAMES,
            },
            BattleEndCause::PartyWipe => VictoryPhase::Results { hold: 0 },
            // The `0x67` arm counts `ctx[+0x6CE]` from its battle-start
            // zero; only the victory / wipe fade frame seeds it at 2.
            BattleEndCause::Escaped => VictoryPhase::Exit { phase: 0 },
        };
        let mut seq = VictorySequence {
            cause,
            phase,
            pose_actor: self.victory_pose_seat(cause),
            pose_id: None,
            voice_bank: None,
            window_opened: false,
        };
        match cause {
            BattleEndCause::MonsterWipe => {
                // Phase 0: the pose pick. Retail reads the round count
                // BEFORE the results frame bumps it.
                seq.pose_id = self.pick_victory_pose(seq.pose_actor);
                // ... and streams the pose's voice into VAB slot 7.
                seq.voice_bank = self.victory_voice_bank(seq.pose_actor, seq.pose_id);
            }
            BattleEndCause::PartyWipe => {
                self.open_battle_results_frame(&mut seq);
            }
            BattleEndCause::Escaped => {
                // Run-away EXP (`WorldToggles::flee_exp`): banked once, here,
                // where every escape path converges. Off is retail's empty
                // flee.
                let banked = self.bank_flee_exp();
                if banked > 0 {
                    log::info!("battle end: escape banked {banked} XP per member");
                }
                // The command-flow escape paths (spell / item) reach here
                // without the SM's `0x66` arm; give them its fade.
                if self.presentation.fade.is_none() {
                    self.presentation.fade = Some(legaia_engine_system::fade::FadeState::load(
                        &legaia_engine_system::fade::escape_fade_template(),
                    ));
                }
            }
        }
        self.battle.victory = Some(seq);
    }

    /// The seat the results sequencer frames and poses: `ctx[+0x13]`, the
    /// acting slot - the port's `battle_ctx.active_actor`.
    ///
    /// - A **victory** runs the action SM's victory arm first, which leaves
    ///   the acting party member there (kept while alive, re-picked when
    ///   dead, forced by the Songi formations); a seat outside the party (a
    ///   formation override on a party too small for it) falls back to the
    ///   leader rather than posing a monster.
    /// - A **party wipe** stores nothing there (the wipe arm of `FUN_801E295C`
    ///   has no `ctx[+0x13]` store), so the sequencer's shared tail
    ///   (`0x8004FC80..0x8004FC90`, `FUN_801D5854(s8, 6)`) frames whoever
    ///   acted last - usually the monster that landed the wiping blow, whose
    ///   knockout taunt the annihilated arm then stages
    ///   ([`Self::stage_wipe_taunt`]). A seat with no actor falls back to the
    ///   leader.
    /// - An **escape** skips the framing (`0x8004E720`); the leader stands in.
    ///
    /// REF: FUN_8004E568 (`lbu s8,0x13(a0)` at `0x8004E5A4`), FUN_801E295C
    /// (the victory arm's `ctx[+0x13]` stores, `0x801E66EC..0x801E6760`)
    fn victory_pose_seat(&self, cause: BattleEndCause) -> usize {
        let seat = match (cause, self.battle.forced_victory_seat) {
            // The retail-compare seed's pose seat.
            (BattleEndCause::MonsterWipe, Some(forced)) => forced,
            _ => self.battle_ctx.active_actor,
        };
        match cause {
            BattleEndCause::MonsterWipe if seat < self.party.party_count => usize::from(seat),
            BattleEndCause::PartyWipe if usize::from(seat) < self.actors.len().min(8) => {
                usize::from(seat)
            }
            _ => 0,
        }
    }

    /// The pose's voice bank (`0x8004ECAC..0x8004ECE4`): the SCUS voice table
    /// row of the pose actor's character, column of the picked pose, less
    /// one: the `monster.snd` bank phase 0 hands `FUN_8003E104` for slot 7.
    /// Phase 2 installs it there (`FUN_8001E54C(7, ..)` at `0x8004ED68`),
    /// over the formation's own slot-7 bank, and the pose clip's cue `0xA1`
    /// keys it ([`World::battle_monster_sound_banks`]).
    ///
    /// PORT: FUN_8004E568 (`0x8004ECAC..0x8004ECE4`, the voice-clip pick)
    fn victory_voice_bank(&self, seat: usize, pose: Option<u8>) -> Option<u16> {
        let table = self.tables.victory_voice_table.as_ref()?;
        let char_id = (self.party_roster_slot(seat) as u8).saturating_add(1);
        legaia_game_tables::victory_pose::victory_voice_bank(table, char_id, pose?)
    }

    /// Phase 0's pose pick (`0x8004E78C..0x8004EAF0`) for `seat`.
    fn pick_victory_pose(&mut self, seat: usize) -> Option<u8> {
        let table = self.tables.victory_pose_table?;
        let actor = self.actors.get(seat)?;
        // The whole `+0x16E` word, tracker included.
        let (hp, hp_max, status) = (
            actor.battle.hp,
            actor.battle.max_hp,
            self.raw_status_word(seat as u8),
        );
        let round = self.battle.monster_ai_state.mode_flags;
        let tier = victory_pose_tier(hp, hp_max, round, status);
        // `DAT_8007BD10[seat]` is 1-based; the roster slot is 0-based.
        let char_id = (self.party_roster_slot(seat) as u8).saturating_add(1);
        let mut rng = || self.next_rand();
        let picked = victory_pose_id(&table, char_id, tier, &mut rng);
        // The held-button select and the Songi override run on the pick
        // (`0x8004EAF4..0x8004EBAC`), after its two draws.
        let held = self.input.retail_pad().held;
        let first_monster = self.formation_slot0_monster_id().unwrap_or(0);
        let pose = picked.map(|p| {
            victory_pose_formation_override(victory_pose_pad_select(p, held), first_monster)
        });
        log::info!(
            "battle end: seat {seat} (char {char_id}) hp {hp}/{hp_max} round {round} \
             status {status:#06x} -> pose tier {tier}, win pose {pose:#04x?}"
        );
        pose
    }

    /// One retail frame of the sequence. Runs instead of the action SM.
    pub(in crate::world) fn tick_battle_end_sequence(&mut self) {
        let Some(mut seq) = self.battle.victory else {
            return;
        };
        match seq.phase {
            VictoryPhase::Loading { frames_left } => {
                // `FUN_801D5854(seat, 8)` every load frame (`0x8004EE0C..`).
                self.victory_frame_pose(
                    seq.pose_actor,
                    legaia_engine_battle_vm::battle_action::Pose::Recover,
                );
                if frames_left > 1 {
                    seq.phase = VictoryPhase::Loading {
                        frames_left: frames_left - 1,
                    };
                } else {
                    self.open_battle_results_frame(&mut seq);
                }
            }
            VictoryPhase::Results { hold } => {
                // `FUN_801D5854(seat, 6)` every results frame (`0x8004FC90`).
                self.victory_frame_pose(
                    seq.pose_actor,
                    legaia_engine_battle_vm::battle_action::Pose::Idle,
                );
                let hold = hold.saturating_add(1);
                if hold >= VICTORY_RESULTS_HOLD_FRAMES {
                    // The exit fade (`0x8004F7B4..0x8004F7F0` / the wipe
                    // twin at `0x8004FB1C`): the same kind-2 template the
                    // escape teardown spawns - `B - F`, black -> white, a
                    // fade to black both hosts draw off `screen_fade`.
                    self.presentation.fade = Some(legaia_engine_system::fade::FadeState::load(
                        &legaia_engine_system::fade::escape_fade_template(),
                    ));
                    if seq.cause == BattleEndCause::PartyWipe {
                        // The annihilated arm floors EVERY member at 1 HP
                        // on this frame (`0x8004FB94..0x8004FBA4`, one
                        // `sh 1,0x14c` per party seat) - a scripted loss
                        // returns to the field with the party standing.
                        self.floor_wiped_party_hp();
                    }
                    seq.phase = VictoryPhase::Exit {
                        phase: VICTORY_FADE_PHASE_SEED,
                    };
                } else {
                    seq.phase = VictoryPhase::Results { hold };
                }
            }
            VictoryPhase::Exit { phase } => {
                let phase = phase.saturating_add(1);
                if phase >= VICTORY_EXIT_PHASE {
                    self.battle.victory = None;
                    self.battle.spoils_frames = 0;
                    self.finish_battle();
                    return;
                }
                seq.phase = VictoryPhase::Exit { phase };
            }
        }
        self.battle.victory = Some(seq);
    }

    /// The per-frame camera / pose request the sequencer makes for its pose
    /// actor - the same host path the action SM's `pose()` takes.
    fn victory_frame_pose(
        &mut self,
        seat: usize,
        pose: legaia_engine_battle_vm::battle_action::Pose,
    ) {
        if seat >= self.actors.len() {
            return;
        }
        self.pending_battle_events.push(BattleEvent::Pose {
            actor_id: seat as u8,
            pose,
        });
        self.apply_battle_pose(seat, pose as u8);
    }

    /// The results frame (`ctx[+0x6CE]` `0 -> 1`).
    ///
    /// The window each arm opens is gated on the special-battle word
    /// ([`VictorySequence::window_opened`]); the rewards the win arm credits
    /// are zeroed by the same word inside [`World::apply_battle_loot`].
    ///
    /// PORT: FUN_8004E568 (the two special-word window gates,
    /// `0x8004F614..0x8004F660` and `0x8004F8F0..0x8004F904`)
    fn open_battle_results_frame(&mut self, seq: &mut VictorySequence) {
        seq.phase = VictoryPhase::Results { hold: 0 };
        seq.window_opened = matches!(
            seq.cause,
            BattleEndCause::MonsterWipe | BattleEndCause::PartyWipe
        ) && self.special_battle_word() == 0;
        // Every window this frame opens spawns at its seat A and glides in
        // (`FUN_801D8DE8`'s default tail arms `FUN_801DB7B0`).
        self.battle.result_windows_glide = seq
            .window_opened
            .then(|| legaia_engine_battle_vm::battle_commit_log::LogLaunch::new(false));
        match seq.cause {
            BattleEndCause::MonsterWipe => {
                // `FUN_8003CE08(0x35)` + the round bump (`0x8004EEE4`).
                self.system_flag_set(VICTORY_STORY_FLAG);
                self.advance_battle_mode();
                // Stage the pose clip: `+0x1DA = pose`, `+0x1DC |= 2`
                // (`0x8004EEF8..0x8004EF14`). The commit runs in the
                // animation tick every host reaches.
                if let (Some(id), Some(a)) = (seq.pose_id, self.actors.get_mut(seq.pose_actor)) {
                    a.battle.queued_anim = id;
                    a.battle
                        .flag_bits
                        .set(legaia_engine_battle_vm::battle_action::ActorFlags::ADVANCE_DONE);
                }
                // A standing member whose queued item never ran gets the
                // copy back (`0x8004F354..0x8004F38C`); the floor below
                // reads the pre-floor HP the per-member loop tests first.
                self.refund_unacted_items();
                // A downed member leaves a won battle standing at 1 HP
                // (`0x8004F390..0x8004F398`).
                self.floor_downed_party_hp();
                // Rewards: XP / gold / drop / level-ups, then the windows.
                if let Some(formation) = self.battle.active_formation.clone() {
                    let catalog = std::mem::take(&mut self.tables.monster_catalog);
                    let rewards = self.apply_battle_loot(&formation, &catalog);
                    self.tables.monster_catalog = catalog;
                    let levelled = !rewards.level_ups.is_empty();
                    self.battle.last_rewards = Some(rewards);
                    self.battle.loot_applied = true;
                    if levelled {
                        // `FUN_8004FCC8(0x50)` at `0x8004F6E8`.
                        self.audio.battle_sfx_cues.push(BattleSfxCue {
                            kind: LEVEL_UP_CUE,
                            timing_frames: 0,
                            actor_slot: seq.pose_actor as u8,
                            target_slot: seq.pose_actor as u8,
                        });
                    }
                }
                self.battle.spoils_frames =
                    VICTORY_RESULTS_HOLD_FRAMES.saturating_add(VICTORY_EXIT_PHASE);
            }
            BattleEndCause::PartyWipe => {
                // The annihilated arm (`0x8004F8C0..`): the loss window
                // (`FUN_801D8DE8(0x42)`) and the same hold; the HP floor
                // waits for the fade frame.
                self.battle.spoils_frames =
                    VICTORY_RESULTS_HOLD_FRAMES.saturating_add(VICTORY_EXIT_PHASE);
                // The yaw counter the in-fight framing of the pose actor
                // reads (`li v0,0x7c0; sh v0,0x4(s4)`, `s4 = ctx + 0x6D6`, at
                // `0x8004F920`).
                if let Some(cam) = self.battle.camera.as_mut() {
                    cam.set_action_yaw_base(WIPE_YAW_BASE);
                }
                self.stage_wipe_taunt(seq.pose_actor);
                // The boss bark (`0x8004F9FC..0x8004FAE8`), once.
                let first_monster = self.formation_slot0_monster_id().unwrap_or(0);
                let mut rng = || self.next_rand() as i32;
                if let Some(cue) = party_wipe_bark(first_monster, &mut rng) {
                    self.dispatch_battle_cue(cue);
                }
            }
            BattleEndCause::Escaped => {}
        }
    }

    /// The annihilated arm's taunt (`0x8004F928..0x8004F9C4`): when the pose
    /// actor is a monster, its first action entry tagged `0x22` - the
    /// knockout taunt the action SM withholds from the wiping blow
    /// (`battle_action::attack::stage_ko_taunt`) - is staged there with the
    /// stage latch (`sb s4,0x1da(s3)`, `+0x1DC |= 2`) unless it is already
    /// the committed clip (`+0x1D9`). A monster with no such entry stays on
    /// its clip. The arm also stores `+0x1FA = 2`, the counter the commit
    /// counts down, which no end-sequence reader consults.
    ///
    /// Retail indexes the monster table with `ctx[+0x13] - 3` unguarded; a
    /// party seat there would read past the table's head, so the port
    /// stages nothing for one.
    ///
    /// PORT: FUN_8004E568 (`0x8004F928..0x8004F9C4`, the wipe taunt)
    fn stage_wipe_taunt(&mut self, seat: usize) {
        use legaia_engine_battle_vm::battle_action::{ActorFlags, KO_TAUNT_TAG};
        let Ok(slot) = u8::try_from(seat) else {
            return;
        };
        if self.battle_seat_map().is_party(slot) {
            return;
        }
        let Some(taunt) = self.battle_monster_action_tags(slot).and_then(|tags| {
            legaia_actor_formats::monster_archive::find_action_by_tag(&tags, KO_TAUNT_TAG)
        }) else {
            return;
        };
        let a = &mut self.actors[seat].battle;
        if a.current_anim != taunt {
            a.queued_anim = taunt;
            a.flag_bits.set(ActorFlags::ADVANCE_DONE);
        }
    }

    /// The win arm's unspent-item refund (`0x8004F354..0x8004F38C`), inside
    /// the per-member spoils loop, after the level-up applier: a seat that
    /// is standing (`+0x14C != 0`, else `beq a1,zero,0x8004F390` takes the
    /// HP floor instead) and not Stoned (`+0x16E & 4`, else the loop skips
    /// at `0x8004F248`), whose initiative key `+0x16C` is still unspent and
    /// whose committed category `+0x1DE` is Item, hands the reserved id
    /// `+0x1DF` back through `FUN_800421D4(id, 1)` (`0x8004F380`).
    ///
    /// The window's commit takes the copy out of the bag; the effect lands
    /// only when the member's turn dispatches. So a battle that ends before
    /// that turn - the last monster killed by an earlier actor, or fled on
    /// its own turn - returns the item rather than eating it. A seat that
    /// falls before its turn gets the same refund from the turn picker's
    /// dead-slot sweep (`World::next_combatant_by_initiative`).
    ///
    /// PORT: FUN_8004E568 (`0x8004F354..0x8004F38C`, the queued-item refund)
    pub(in crate::world) fn refund_unacted_items(&mut self) {
        use legaia_engine_battle_loop::round_flow::PendingPartyAction;
        const STONE_WORD_BIT: u16 = 0x0004;
        for slot in self.party_floor_seats() {
            let stoned = self.raw_status_word(slot as u8) & STONE_WORD_BIT != 0;
            let b = &self.actors[slot].battle;
            if b.hp == 0
                || stoned
                || b.init_key == 0
                || b.action_category
                    != legaia_engine_battle_vm::battle_action::ActionCategory::Item.as_byte()
            {
                continue;
            }
            if let Some(Some(PendingPartyAction::Item { item_id, .. })) = self
                .battle
                .round_flow
                .pending
                .get_mut(slot)
                .map(Option::take)
            {
                let _ = self.party.inventory.add(item_id, 1);
            }
        }
    }

    /// The win arm's HP floor (`0x8004F224..0x8004F3B8`): a party seat whose
    /// `actor[+0x14C]` reads `0` (`beq a1,zero,0x8004F390`) gets `1` there
    /// and in its roster record's current HP (`+0x6CE`); a standing seat
    /// keeps its HP. Mirrored onto the world liveness so the field return
    /// sees a standing member.
    fn floor_downed_party_hp(&mut self) {
        for slot in self.party_floor_seats() {
            let b = &mut self.actors[slot].battle;
            if b.max_hp > 0 && b.hp == 0 {
                b.liveness = 1;
                b.set_hp_synced(1);
            }
        }
    }

    /// The annihilated arm's HP floor (`0x8004FB94..0x8004FC30`): for
    /// **every** seat below the party count `ctx[+0]`, unconditionally,
    /// `sh 1,0x14c` (`0x8004FBA4`) and the roster record's current HP `+0x6CE
    /// = 1`, MP `+0x6D2 = actor[+0x150]` and `+0x6D6 = actor[+0x170]` - the
    /// last two are what [`Self::persist_battle_party_hp`] carries at the
    /// teardown. A stone member is floored too: retail tests nothing.
    ///
    /// Retail leaves the readout `+0x172` alone because no battle frame runs
    /// after this one; the port re-syncs it (`set_hp_synced`) so the pair a
    /// later reader sees is never the absorbing `hp != shown` with a zero
    /// accumulator that parks the `0x51` bar-drain gate.
    fn floor_wiped_party_hp(&mut self) {
        for slot in self.party_floor_seats() {
            let b = &mut self.actors[slot].battle;
            b.liveness = 1;
            b.set_hp_synced(1);
        }
    }

    fn party_floor_seats(&self) -> std::ops::Range<usize> {
        0..(self.party.party_count as usize)
            .min(3)
            .min(self.actors.len())
    }
}
