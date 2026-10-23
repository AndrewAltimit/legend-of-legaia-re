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
//! The pose actor is `ctx[+0x13]`. No writer in the battle overlay ever
//! stores a seat there - every store is a zero at a round boundary or the
//! magic menu's MP-cost scratch - and the one three-member capture of the
//! results frame (`noa_levelup_banner`: Noa levelled, seat 0 carries the
//! staged pose `0x14`, `ctx[+0x13] == 0`) agrees: **the party leader
//! poses**, not the member who landed the last hit.
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
//! voice clip (`monster.snd` tail entries) is **not** staged: no engine bank
//! carries `monster.snd`, so the pose plays silent.
//!
//! An **escape** (`0x66` -> `0x67`) takes the sequencer's `0x67` arm instead
//! (`lbu v1,0x7(a0); li v0,0x67` at `0x8004E63C`): no results, the phase
//! halfword counts up from its battle-start zero by the vsync delta
//! (`0x8004E70C..0x8004E724`) behind the fade the SM's `0x66` arm
//! spawned, and the same `0x43` gate exits. A **party wipe** takes the
//! annihilated arm: the loss window, the same `0x100` hold and fade,
//! every member's HP floored at 1, then the MAIN INIT game-over gate that
//! [`World::finish_battle`] folds.

use super::*;
use legaia_engine_battle::battle_events::BattleSfxCue;
use legaia_engine_battle::victory::*;

impl World {
    /// Re-exported for hosts / tests that pace the sequence.
    pub const VICTORY_LOAD_FRAMES: u16 = VICTORY_LOAD_FRAMES;
    pub const VICTORY_RESULTS_HOLD_FRAMES: u16 = VICTORY_RESULTS_HOLD_FRAMES;
    pub const VICTORY_EXIT_PHASE: u16 = VICTORY_EXIT_PHASE;
    pub const VICTORY_FADE_PHASE_SEED: u16 = VICTORY_FADE_PHASE_SEED;

    /// The battle-end presentation is up: the scene is still in
    /// [`SceneMode::Battle`](legaia_engine_mode::mode::SceneMode::Battle) but the action SM no longer runs.
    pub fn battle_end_sequence_active(&self) -> bool {
        self.battle.victory.is_some()
    }

    /// The result windows are on screen. Hosts hide the in-fight party
    /// readout behind this (retail's results frame draws neither the card
    /// nor the pill - `noa_levelup_banner`).
    pub fn battle_result_screen_active(&self) -> bool {
        self.battle.victory.is_some_and(|v| v.results_shown())
    }

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
            pose_actor: 0,
            pose_id: None,
            window_opened: false,
        };
        match cause {
            BattleEndCause::MonsterWipe => {
                // Phase 0: the pose pick. Retail reads the round count
                // BEFORE the results frame bumps it.
                seq.pose_id = self.pick_victory_pose(seq.pose_actor);
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

    /// Phase 0's pose pick (`0x8004E78C..0x8004EAF0`) for `seat`.
    fn pick_victory_pose(&mut self, seat: usize) -> Option<u8> {
        let table = self.tables.victory_pose_table?;
        let actor = self.actors.get(seat)?;
        let (hp, hp_max, status) = (
            actor.battle.hp,
            actor.battle.max_hp,
            actor.battle.field_flags,
        );
        let round = self.battle.monster_ai_state.mode_flags;
        let tier = victory_pose_tier(hp, hp_max, round, status);
        // `DAT_8007BD10[seat]` is 1-based; the roster slot is 0-based.
        let char_id = (self.party_roster_slot(seat) as u8).saturating_add(1);
        let mut rng = || self.next_rand();
        let pose = victory_pose_id(&table, char_id, tier, &mut rng);
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
            }
            BattleEndCause::Escaped => {}
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
