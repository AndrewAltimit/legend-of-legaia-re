//! Scene-transition **streaming actor** - the five-state machine that
//! streams the destination scene's bundle during a transition and then hands
//! the game to MAIN INIT.
//!
//! (The `PORT:` tag sits on [`SceneTransitionActor::tick`], the item that
//! implements the body.)
//!
//! `FUN_8001FD44` - the name-based scene-change packet - spawns this actor
//! from descriptor `0x80070734`, whose `+0x8` handler word reads
//! `0x80021934` (`docs/subsystems/asset-loader.md` has the descriptor
//! family; the word is on the disc in `extracted/SCUS_942.54`). The actor
//! then owns the whole transition: it holds a countdown while the fade runs,
//! streams `DATA\FIELD\<scene>.LZS` into the shared asset buffer
//! `_DAT_8007B85C`, and writes the game-mode global.
//!
//! ## The entry is `0x80021934`, not `0x80021940`
//!
//! Ghidra's body starts at the `addiu sp,sp,-0x120` prologue, but three
//! instructions sit in front of it, in never-analyzed space:
//!
//! ```text
//! 80021934  lui  v0, 0x1f80
//! 80021938  lbu  v0, 0x393(v0)     ; v0 = DAT_1F800393, the frame-skip factor
//! 8002193c  lw   v1, 0x710(gp)     ; v1 = the transition countdown
//! 80021940  addiu sp, sp, -0x120   ; <- what Ghidra calls the entry
//! ...
//! 80021960  subu v1, v1, v0        ; countdown -= dt
//! 80021968  sw   v1, 0x710(gp)
//! ```
//!
//! (read from `extracted/SCUS_942.54` at file offset `0x12134`; the load
//! base is the EXE header's `dst = 0x80010000` at file `0x800`). A call
//! landing on `0x80021940` would run that subtract on whatever `v0`/`v1`
//! happened to hold, which is why the port is of `0x80021934`. The countdown
//! decrement is therefore **unconditional** - it happens before the state
//! dispatch and even for out-of-range states.
//!
//! ## The five states (jump table `0x80010760`)
//!
//! | state | target | body |
//! |---|---|---|
//! | 0 | `0x80021990` | set the transition screen byte `DAT_8007B648 = 0x80`, seed the countdown to `0x46`, advance only while the start gate `_DAT_8007BC20` is zero |
//! | 1, 3 | `0x800219FC` | advance when the CD queue `FUN_8003DE7C(1)` reports idle |
//! | 2 | `0x800219BC` | index arm: stream chunk `DAT_8007B768 + 3`, cache the byte size at `gp+0x73C`, advance |
//! | 4 | `0x80021A20` | wait for the countdown to go negative, then stream by path and hand off |
//!
//! States 1 and 3 share a jump-table target, which is why the table has five
//! rows but four bodies.
//!
//! ## Index arm vs path arm
//!
//! `FUN_8001EEF0` resolves **by index** only when `_DAT_8007B868 == 0 &&
//! _DAT_8007B8C2 != 0` (`0x8001EEF0..0x8001EF3C`), and by path otherwise.
//! This actor splits on the same `_DAT_8007B8C2`: state 2 issues the stream
//! when it is non-zero, state 4 when it is zero. `_DAT_8007B8C2` lives past
//! the loaded image (`dst + size = 0x8007B800`), i.e. in BSS, and no dump in
//! the corpus writes it - so on a retail boot it reads `0` and **state 4 is
//! the arm that runs**. State 2's stream is the dev/index arm and is inert
//! on a shipped disc; state 2 still advances either way.
//!
//! ## The path
//!
//! State 4 builds `DATA\FIELD\` + the active scene name + `.LZS`. The prefix
//! is the 12-byte literal at `0x800106C4` and the suffix the one at
//! `0x8007B3CC`; both read out of `extracted/SCUS_942.54`. Ghidra names the
//! prefix symbol `s_DATA_FIELD_`, which is its identifier-safe mangling of
//! the backslashes - the byte string is `DATA\FIELD\`, matching the
//! `DATA\FIELD\<scene>.MAP` sidecars the field loader stages.
//!
//! Before building it, state 4 rotates the three scene-name buffers -
//! `0x80084558 <- 0x80084548 <- 0x800915C8` - so the previous scene name is
//! preserved one slot back while the pending one moves into the active slot.
//!
//! ## NOT WIRED
//!
//! The engine does not transition scenes through a staging buffer. Its
//! scene change is [`crate::scene::Scene`] resource loading driven by
//! `BootSession::enter_field_live`, so there is no `_DAT_8007B85C` buffer to
//! stream a raw `.LZS` bundle into and no actor pool node to tick between
//! the fade-out and MAIN INIT. What has to exist first is a staged-bundle
//! scene loader - a host that takes [`SceneTransitionEffect::StreamBundleByPath`]
//! and parks the bytes where the descriptor walker
//! ([`crate::scene_bundle`]) reads them - rather than a caller for this
//! state machine.

/// States the machine dispatches. `actor+0x1A` values at or above this fall
/// through to the epilogue (`sltiu v0, a0, 0x5` at `0x80021964`).
pub const STATE_COUNT: u16 = 5;

/// `gp+0x710` seed in state 0 - the transition countdown, in display frames.
pub const TRANSITION_COUNTDOWN: i32 = 0x46;

/// `DAT_8007B648` value state 0 writes. The in-field save/load driver
/// ([`crate::field_save_screen_actor`]) clears the same byte when it retires.
pub const TRANSITION_SCREEN_BYTE: u8 = 0x80;

/// The destination bundle is the scene block's raw TOC entry `base + 3`.
pub const DATA_FIELD_CHUNK_BIAS: i16 = 3;

/// `_DAT_8007B83C` value the hand-off writes: mode 2, MAIN INIT.
pub const HANDOFF_GAME_MODE: u16 = 2;

/// The 12-byte literal at `0x800106C4`, backslashes and all.
pub const PATH_PREFIX: &str = r"DATA\FIELD\";

/// The suffix at `0x8007B3CC`.
pub const PATH_SUFFIX: &str = ".LZS";

/// `FUN_8001EEF0` returns a **sector** count; `gp+0x73C` caches it as bytes.
pub fn staged_byte_size(sectors: i32) -> i32 {
    sectors << 11
}

/// Build the path state 4 streams: `DATA\FIELD\<scene>.LZS`.
pub fn bundle_path(scene_name: &str) -> String {
    format!("{PATH_PREFIX}{scene_name}{PATH_SUFFIX}")
}

/// The globals one tick reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SceneTransitionInput<'a> {
    /// `DAT_1F800393` - the adaptive frame-skip factor, subtracted from the
    /// countdown every tick.
    pub dt: u8,
    /// `_DAT_8007BC20` - state 0 holds while this is non-zero.
    pub start_gate: i32,
    /// `_DAT_8007B8C2 != 0` - the index-resolution arm. Zero on retail.
    pub index_mode: bool,
    /// `FUN_8003DE7C(1) != 0` - the CD queue is still working.
    pub stream_busy: bool,
    /// `DAT_8007B768` - the staged scene block index the index arm uses.
    pub staged_index: i16,
    /// `_DAT_8007B9A0` - the pending destination index state 4 promotes into
    /// `DAT_8007B768`.
    pub pending_index: u16,
    /// `0x80084548` - the active scene name.
    pub scene_name: &'a str,
}

/// What a tick asks the host to do, in emission order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SceneTransitionEffect {
    /// `uRam8007BA24 = 0`, run unconditionally at the head of every tick
    /// before the state dispatch.
    ClearTransitionScratch,
    /// `DAT_8007B648 = value`.
    SetTransitionScreenByte(u8),
    /// `gp+0x710 = value` - an absolute seed, which overwrites the decrement
    /// this same tick performed.
    SeedCountdown(i32),
    /// State 2's index arm: `FUN_8001EEF0(<scratch>, chunk, _DAT_8007B85C, 0)`.
    /// The host caches [`staged_byte_size`] of the returned sector count at
    /// `gp+0x73C`.
    StreamChunkByIndex {
        /// `DAT_8007B768 + 3`.
        chunk: i16,
    },
    /// `0x80084558 <- 0x80084548 <- 0x800915C8`.
    RotateSceneNameBuffers,
    /// `DAT_8007B768 = _DAT_8007B9A0`.
    SetStagedIndex(u16),
    /// State 4's path arm: `FUN_8001EEF0(path, chunk, _DAT_8007B85C, 0)`.
    StreamBundleByPath {
        /// `DATA\FIELD\<scene>.LZS`.
        path: String,
        /// `DAT_8007B768 + 3`, after the promotion above.
        chunk: i16,
    },
    /// `_DAT_8007B9EC = 1` - the bundle is staged.
    MarkBundleStaged,
    /// `_DAT_8007B83C = mode`.
    EnterGameMode(u16),
}

/// The actor's own state: `actor+0x1A` plus the countdown it owns at
/// `gp+0x710`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SceneTransitionActor {
    /// `actor+0x1A`.
    pub state: u16,
    /// `gp+0x710`. A global in retail, but this actor is the only writer of
    /// it during a transition.
    pub countdown: i32,
}

impl SceneTransitionActor {
    /// One frame.
    ///
    /// PORT: FUN_80021934
    ///
    /// NOT WIRED: the engine loads scenes as [`crate::scene::Scene`]
    /// resources, not by streaming a raw `.LZS` bundle into a shared staging
    /// buffer, so there is no `_DAT_8007B85C` equivalent for
    /// [`SceneTransitionEffect::StreamBundleByPath`] to fill and no
    /// transition-time actor pool to tick this from. A staged-bundle scene
    /// loader is the prerequisite; see the module docs.
    pub fn tick(&mut self, input: SceneTransitionInput<'_>) -> Vec<SceneTransitionEffect> {
        use SceneTransitionEffect as E;
        let mut out = vec![E::ClearTransitionScratch];
        self.countdown -= i32::from(input.dt);

        if self.state >= STATE_COUNT {
            return out;
        }

        match self.state {
            0 => {
                out.push(E::SetTransitionScreenByte(TRANSITION_SCREEN_BYTE));
                out.push(E::SeedCountdown(TRANSITION_COUNTDOWN));
                self.countdown = TRANSITION_COUNTDOWN;
                if input.start_gate == 0 {
                    self.state += 1;
                }
            }
            1 | 3 => {
                if !input.stream_busy {
                    self.state += 1;
                }
            }
            2 => {
                if input.index_mode {
                    out.push(E::StreamChunkByIndex {
                        chunk: input.staged_index.wrapping_add(DATA_FIELD_CHUNK_BIAS),
                    });
                }
                self.state += 1;
            }
            _ => {
                // State 4. The countdown gate is the only thing holding the
                // stream back while the fade plays.
                if self.countdown >= 0 {
                    return out;
                }
                if !input.index_mode {
                    out.push(E::RotateSceneNameBuffers);
                    out.push(E::SetStagedIndex(input.pending_index));
                    out.push(E::StreamBundleByPath {
                        path: bundle_path(input.scene_name),
                        chunk: (input.pending_index as i16).wrapping_add(DATA_FIELD_CHUNK_BIAS),
                    });
                }
                out.push(E::MarkBundleStaged);
                out.push(E::EnterGameMode(HANDOFF_GAME_MODE));
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::SceneTransitionEffect as E;
    use super::*;

    fn input(dt: u8) -> SceneTransitionInput<'static> {
        SceneTransitionInput {
            dt,
            start_gate: 0,
            index_mode: false,
            stream_busy: false,
            staged_index: 0,
            pending_index: 0x54,
            scene_name: "town01",
        }
    }

    /// The whole point of the actor: a retail transition must reach mode 2
    /// with the destination bundle streamed, and it must not get there
    /// before the countdown lapses. Driving it frame by frame is what proves
    /// every state is reachable in order.
    #[test]
    fn a_retail_transition_streams_the_bundle_then_hands_off_to_main_init() {
        let mut a = SceneTransitionActor::default();
        let mut seen_states = vec![a.state];
        let mut effects = Vec::new();
        for _ in 0..200 {
            let out = a.tick(input(3));
            let handed_off = out.contains(&E::EnterGameMode(HANDOFF_GAME_MODE));
            effects.extend(out);
            if a.state != *seen_states.last().unwrap() {
                seen_states.push(a.state);
            }
            if handed_off {
                break;
            }
        }
        assert_eq!(seen_states, vec![0, 1, 2, 3, 4], "every state is reached");
        assert!(effects.contains(&E::SetTransitionScreenByte(TRANSITION_SCREEN_BYTE)));
        assert!(effects.contains(&E::RotateSceneNameBuffers));
        assert!(effects.contains(&E::SetStagedIndex(0x54)));
        assert!(effects.contains(&E::StreamBundleByPath {
            path: r"DATA\FIELD\town01.LZS".to_string(),
            chunk: 0x54 + 3,
        }));
        assert!(effects.contains(&E::MarkBundleStaged));
        // The terminal value is the documented one, and nothing before it.
        let handoffs = effects
            .iter()
            .filter(|e| matches!(e, E::EnterGameMode(_)))
            .count();
        assert_eq!(handoffs, 1);
    }

    /// State 0 is a gate, not a pass-through: while `_DAT_8007BC20` is set
    /// the actor re-seeds the countdown every frame and never advances, so a
    /// transition cannot start early.
    #[test]
    fn state_zero_holds_and_re_seeds_while_the_start_gate_is_set() {
        let mut a = SceneTransitionActor::default();
        for _ in 0..10 {
            let out = a.tick(SceneTransitionInput {
                start_gate: 1,
                ..input(3)
            });
            assert_eq!(a.state, 0);
            assert_eq!(a.countdown, TRANSITION_COUNTDOWN);
            assert!(out.contains(&E::SeedCountdown(TRANSITION_COUNTDOWN)));
        }
        a.tick(input(3));
        assert_eq!(a.state, 1);
    }

    /// The hand-off waits for the countdown to go **negative**, not to reach
    /// zero, and the wait is denominated in display frames - so the cadence
    /// changes the tick count but not the elapsed frames.
    #[test]
    fn the_handoff_waits_for_the_countdown_in_display_frames() {
        for dt in [1u8, 2, 3, 5] {
            let mut a = SceneTransitionActor {
                state: 4,
                countdown: TRANSITION_COUNTDOWN,
            };
            let mut ticks = 0;
            loop {
                let out = a.tick(input(dt));
                ticks += 1;
                if out.contains(&E::EnterGameMode(HANDOFF_GAME_MODE)) {
                    break;
                }
                assert!(ticks < 500, "never handed off at dt {dt}");
            }
            let elapsed = ticks * u32::from(dt);
            assert!(
                elapsed > TRANSITION_COUNTDOWN as u32,
                "dt {dt}: handed off after only {elapsed} display frames"
            );
            assert!(
                elapsed < TRANSITION_COUNTDOWN as u32 + u32::from(dt) + 1,
                "dt {dt}: overshot to {elapsed} display frames"
            );
        }
    }

    /// Retail (`_DAT_8007B8C2 == 0`) streams by path in state 4; a dev build
    /// with the flag set streams by index in state 2 and state 4 only hands
    /// off. Exactly one stream is issued either way.
    #[test]
    fn the_two_resolution_arms_are_exclusive() {
        for index_mode in [false, true] {
            let mut a = SceneTransitionActor::default();
            let mut streams = 0;
            for _ in 0..200 {
                let out = a.tick(SceneTransitionInput {
                    index_mode,
                    staged_index: 0x54,
                    ..input(3)
                });
                streams += out
                    .iter()
                    .filter(|e| {
                        matches!(
                            e,
                            E::StreamChunkByIndex { .. } | E::StreamBundleByPath { .. }
                        )
                    })
                    .count();
                if out.contains(&E::EnterGameMode(HANDOFF_GAME_MODE)) {
                    break;
                }
            }
            assert_eq!(streams, 1, "index_mode {index_mode}");
        }
    }

    /// Both arms name the same chunk - the scene block's raw TOC entry
    /// `base + 3` - which is what makes dev and retail load identical bytes.
    #[test]
    fn both_arms_resolve_the_same_chunk() {
        let mut dev = SceneTransitionActor {
            state: 2,
            countdown: 0,
        };
        let by_index = dev.tick(SceneTransitionInput {
            index_mode: true,
            staged_index: 0x54,
            ..input(0)
        });
        let mut retail = SceneTransitionActor {
            state: 4,
            countdown: -1,
        };
        let by_path = retail.tick(SceneTransitionInput {
            pending_index: 0x54,
            ..input(0)
        });
        assert!(by_index.contains(&E::StreamChunkByIndex { chunk: 0x57 }));
        assert!(by_path.contains(&E::StreamBundleByPath {
            path: r"DATA\FIELD\town01.LZS".to_string(),
            chunk: 0x57,
        }));
    }

    /// States 1 and 3 share a jump-table row: both stall until the CD queue
    /// reports idle.
    #[test]
    fn the_two_queue_waits_stall_until_the_cd_queue_is_idle() {
        for state in [1u16, 3] {
            let mut a = SceneTransitionActor {
                state,
                countdown: 0,
            };
            for _ in 0..5 {
                a.tick(SceneTransitionInput {
                    stream_busy: true,
                    ..input(1)
                });
                assert_eq!(a.state, state);
            }
            a.tick(input(1));
            assert_eq!(a.state, state + 1);
        }
    }

    /// The countdown decrement precedes the dispatch, so it runs even for a
    /// state the table does not cover.
    #[test]
    fn the_countdown_runs_for_out_of_range_states() {
        let mut a = SceneTransitionActor {
            state: 9,
            countdown: 100,
        };
        let out = a.tick(input(4));
        assert_eq!(a.state, 9);
        assert_eq!(a.countdown, 96);
        assert_eq!(out, vec![E::ClearTransitionScratch]);
    }

    /// `FUN_8001EEF0` returns sectors; the cache is bytes.
    #[test]
    fn staged_byte_size_converts_sectors_to_bytes() {
        assert_eq!(staged_byte_size(1), 2048);
        assert_eq!(staged_byte_size(0x54), 0x54 * 2048);
    }
}
