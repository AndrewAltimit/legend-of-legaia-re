//! World-map entity state machine, ported clean-room from `FUN_801DA51C`
//! (overlay_world_map.bin base `0x801C0000`).
//!
//! PORT: FUN_801DA51C, FUN_801D9E1C
//!
//! One instance of [`WorldMapEntityCtx`] exists per on-map entity (NPCs,
//! town-portal tiles, monster spawn zones). The retail engine stores the
//! state in a per-entity record; the engine-side host trait bridges between
//! this SM and whatever data structure the engine uses.
//!
//! ## State machine
//!
//! ```text
//!   Idle (0) ──encounter──► Idle (0, with encounter handler invoked)
//!   Idle (0) ──interact──►  Idle (0, with interact handler invoked)
//!   Idle (0) ──[SM sets]──► Activating (1)
//!   Activating (1) ──countdown=0──► Transitioning (2)
//!   Transitioning (2/3) ──────────► Terminal (4)
//! ```
//!
//! States 2 and 3 share the same handler body (fall-through in the original C
//! switch). State 4 is a terminal stop state - the entity stops ticking.
//!
//! ## Source
//!
//! `ghidra/scripts/funcs/801da51c.txt` (decompiled from `overlay_world_map.bin`).
//! REF: FUN_800243F0

use crate::field_helpers::load_u16_le;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntityState {
    Idle = 0,
    Activating = 1,
    Transitioning = 2,
    Terminal = 4,
}

impl EntityState {
    pub fn from_u16(v: u16) -> Self {
        match v {
            0 => EntityState::Idle,
            1 => EntityState::Activating,
            2 | 3 => EntityState::Transitioning,
            _ => EntityState::Terminal,
        }
    }
}

/// Per-entity SM state. Corresponds to fields within the world-map entity
/// record at the following offsets (from `FUN_801DA51C`):
///
/// - `state`    ← `entity[+0x8A]` (`i16`)
/// - `pad_flags`← `entity[+0x10]` (`u32`): bit `0x80000` = walking-blocked; bit `0x100` = interact-cooldown
/// - `field_88` ← `entity[+0x88]` (`u16`)
#[derive(Debug, Clone, Default)]
pub struct WorldMapEntityCtx {
    /// Movement-state discriminant. Values 0..=4 are defined; see
    /// [`EntityState`].
    pub state: u16,
    /// Packed pad / flag bits for this entity. The SM mutates bits `0x80000`
    /// and `0x100`. Bit semantics per `FUN_801DA51C`:
    /// - `0x80000` = walking / movement-blocked flag (set in Activating state, cleared on scene transition)
    /// - `0x100` = interaction-cooldown flag (set once per interaction cycle)
    pub pad_flags: u32,
    /// Auxiliary field. Cleared by the SM on state advances (from `+0x88`).
    pub field_88: u16,
}

/// Engine-side callbacks consumed by [`step`].
///
/// Each method documents the retail global / call it replaces.
pub trait WorldMapEntityHost {
    /// `_DAT_8007b868 == 0`. When the door/portal is closed this gate is set
    /// non-zero and the whole SM body is skipped (only the post-SM interaction
    /// path below state-0 still runs when the gate is open elsewhere).
    fn activation_gate_open(&self) -> bool;

    /// `DAT_8007b604` - signed encounter-rate countdown shared across all
    /// entities. Decremented in the Idle state; the SM reads and writes it
    /// via the two methods below.
    fn encounter_countdown(&self) -> i8;
    fn set_encounter_countdown(&mut self, v: i8);

    /// `DAT_8007b5f8 != 0` - encounter-rate flag. When zero, encounters are
    /// disabled regardless of the countdown reaching zero.
    fn encounter_enabled(&self) -> bool;

    /// Called when the countdown hits zero and encounters are enabled.
    /// Wraps `FUN_801D9E1C(entity, resolver_result)`. The `resolver_result`
    /// is the return value of `FUN_800243F0` (BGM/asset resolver) at the
    /// start of the tick.
    fn on_encounter(&mut self, entity_idx: usize, resolver_result: u32);

    /// Called during the Activating → Transitioning advance when the
    /// countdown drains to zero. The engine should copy any pending scene
    /// data and set up the transition. Wraps the block starting with
    /// `func_0x8004313c()` in case 1.
    fn on_activating(&mut self, entity_idx: usize);

    /// Called in states 2 / 3 (and the fall-through from state 1) to
    /// perform the actual scene transition. Wraps `func_0x8003ce34(0x35)`,
    /// `_DAT_8007b5f4 = 1`, fade-globals, `_DAT_8007b83c = 8`.
    fn on_scene_transition(&mut self, entity_idx: usize);

    /// `_DAT_1f800394 & 0x8000` - dialog / menu is active. When set, the
    /// post-SM interaction check is suppressed.
    fn dialog_active(&self) -> bool;

    /// `_DAT_8007c364[+0x10] & 0x80000` - player's movement-blocked flag.
    /// When set alongside the entity's interact-cooldown being clear, the
    /// interaction check is skipped.
    fn player_walking(&self) -> bool;

    /// Called when the SM determines the entity should be interacted with.
    /// Wraps `func_0x80039b7c(entity)`.
    fn on_interact(&mut self, entity_idx: usize);

    /// `_DAT_8007b6b0 == -1000`. Sentinel check run after the interact call.
    fn encounter_counter_is_sentinel(&self) -> bool;

    /// `_DAT_8007b6b0 = 0`. Clears the encounter counter when the sentinel
    /// was detected.
    fn clear_encounter_counter(&mut self);
}

/// Step one frame of the world-map entity state machine.
///
/// `entity_idx` is the engine's slot index for this entity (passed through to
/// the host callbacks for context).
pub fn step<H: WorldMapEntityHost>(entity_idx: usize, ctx: &mut WorldMapEntityCtx, host: &mut H) {
    if host.activation_gate_open() {
        let countdown = host.encounter_countdown();
        match ctx.state {
            0 => {
                // Idle: decrement encounter countdown; fire encounter when it
                // hits 0 and the encounter rate is enabled.
                if countdown == 0 && host.encounter_enabled() {
                    host.on_encounter(entity_idx, 0);
                } else {
                    host.set_encounter_countdown(countdown.saturating_sub(1));
                }
            }
            1 => {
                // Activating: set the entity-blocked flag on the player, then
                // drain the countdown. When it hits 0, advance to state 2 and
                // fall through to the Transitioning handler.
                ctx.pad_flags |= 0x80000;
                if countdown > 0 {
                    host.set_encounter_countdown(countdown - 1);
                    return;
                }
                host.on_activating(entity_idx);
                ctx.field_88 = 0;
                ctx.scene_data_consumed();
                ctx.state += 1; // 1 → 2, fall through below
                host.set_encounter_countdown(host.encounter_countdown()); // re-read
                // FALLTHROUGH to states 2/3 below:
                host.on_scene_transition(entity_idx);
                ctx.state = 4;
                ctx.pad_flags &= !0x80000;
                ctx.field_88 = 0;
                return;
            }
            2 | 3 => {
                // Transitioning: initiate scene change and move to Terminal.
                host.on_scene_transition(entity_idx);
                ctx.state = 4;
                ctx.pad_flags &= !0x80000;
                ctx.field_88 = 0;
                return;
            }
            _ => {
                // State 4 (Terminal) and any out-of-range: nothing to do.
            }
        }
    }

    // Post-SM: interaction check only while Idle.
    // Runs regardless of activation_gate_open (the gate only gates the SM
    // body above, not this path - per the original C structure).
    if ctx.state == 0 && !host.dialog_active() {
        let blocked = (ctx.pad_flags & 0x80000) != 0;
        if !blocked {
            let cooldown_set = (ctx.pad_flags & 0x100) != 0;
            let player_not_walking = !host.player_walking();
            if cooldown_set || player_not_walking {
                ctx.pad_flags |= 0x100;
                host.on_interact(entity_idx);
            }
            if host.encounter_counter_is_sentinel() {
                host.clear_encounter_counter();
            }
        }
    }
}

impl WorldMapEntityCtx {
    fn scene_data_consumed(&mut self) {
        // Marks entity[+0x94] as consumed (pointer cleared). In the retail
        // engine this zeroes a pointer; here it is a no-op since the engine
        // side owns the scene-data reference through the host.
    }
}

/// World-map atmospheric fog-RGB script interpreter, ported clean-room from
/// `FUN_801E3E00` (overlay_world_map.bin base `0x801C0000`; dump
/// `ghidra/scripts/funcs/overlay_world_map_801e3e00.txt`).
///
/// PORT: FUN_801E3E00 - atmospheric-actor tick: keyframe script driving the
/// fog color word at actor `+0x74` (the GTE far-color / haze source) plus a
/// secondary color word at `+0x88` and three 16-bit aux params.
///
/// The retail actor stores the script pointer at `+0x94`, the byte cursor
/// (PC) at `+0x9e` and the segment phase clock at `+0x9c`, stepped each tick
/// by the scratchpad frame-delta byte `DAT_1F800393`. The interpreter loops
/// until an opcode "does work" (retail `s3` counter), so cursor-only opcodes
/// are consumed within the same tick:
///
/// | opcode | behaviour |
/// |---|---|
/// | `0x00` | sets actor flag `+0x10 \|= 8` (script finished); ends tick |
/// | `0x01` | resets the cursor to 0 (restart/loop); ends tick |
/// | `0x02` | 15-byte keyframe segment (see below); ends tick |
/// | `0x40` (`'@'`) | skips 2 bytes (opcode + operand); continues same tick |
/// | other  | wraps the cursor to 0; continues same tick |
///
/// A `0x02` segment record is `[op][duration:u16le][target_a:u16le]`
/// `[target_b:u16le][rgb_a:3][rgb_b:3][target_c:u16le]` (0xF bytes; all
/// 16-bit operands read via the unaligned-LE16 helper `FUN_8003CE9C` =
/// [`load_u16_le`]). Per tick, with `phase` at `+0x9c`:
///
/// * `phase == 0`: **latch only** - copies the current output values into the
///   segment-start latches (`+0x40/+0x58/+0x6a` and the six channel bytes at
///   `+0x80..+0x85`), writes nothing else, then steps the phase clock.
/// * `0 < phase < duration`: writes each output as
///   `start + (target - start) * phase / duration` (truncating signed i32
///   division, exactly the MIPS `div`), packing the three color channels of
///   each word as `c0<<16 | c1<<8 | c2` (targets from script `+7/+8/+9` for
///   the `+0x74` word, `+0xa/+0xb/+0xc` for `+0x88`). The aux-C channel
///   (`+0x16`) interpolates toward the **negated** target
///   (`start + (-target - start) * phase / duration`). Steps the phase clock.
/// * `phase >= duration`: snaps every output to its target (`aux_c` to
///   `-target`), zeroes the phase clock (no step) and advances the cursor by
///   0xF to the next record.
#[derive(Debug, Clone, Default)]
pub struct AtmosphericFogTick {
    /// Keyframe script bytes (retail: pointer at actor `+0x94`).
    pub script: Vec<u8>,
    /// Script byte cursor (retail: actor `+0x9e`, `i16`).
    pub pc: u16,
    /// Phase clock within the current segment (retail: actor `+0x9c`, `i16`).
    pub phase: u16,
    /// Current primary packed color word (retail: actor `+0x74`; the value
    /// `crates/web-viewer/src/sentinel_placements.rs` snapshots as the
    /// world-overview fog anchor). Byte layout `c0<<16 | c1<<8 | c2` from
    /// script bytes `+7/+8/+9`; under the PSX `0x00BBGGRR` color-word
    /// convention the low byte (`+9`) is red.
    pub fog_rgb: u32,
    /// Secondary packed color word (retail: actor `+0x88`), channels from
    /// script bytes `+0xa/+0xb/+0xc`.
    pub aux_rgb: u32,
    /// Aux 16-bit param A (retail: actor `+0x3c`), target at script `+3`.
    pub aux_a: u16,
    /// Aux 16-bit param B (retail: actor `+0x3e`), target at script `+5`.
    pub aux_b: u16,
    /// Aux 16-bit param C (retail: actor `+0x16`), driven toward the
    /// **negation** of the script `+0xd` target.
    pub aux_c: u16,
    /// Set once opcode `0x00` runs (retail: actor flags `+0x10 |= 8`).
    pub finished: bool,
    // Segment-start latches, captured on the phase-0 tick of each segment.
    start_a: u16,       // +0x40
    start_b: u16,       // +0x58
    start_c: u16,       // +0x6a
    fog_start: [u8; 3], // +0x80 / +0x81 / +0x82
    aux_start: [u8; 3], // +0x83 / +0x84 / +0x85
}

impl AtmosphericFogTick {
    pub fn new(script: Vec<u8>) -> Self {
        Self {
            script,
            ..Default::default()
        }
    }

    /// One frame of the atmospheric actor tick. `step` is the frame delta
    /// added to the phase clock (retail reads the scratchpad byte
    /// `DAT_1F800393` via `lbu`, so retail steps are `0..=255`).
    pub fn tick(&mut self, step: u16) {
        if self.finished {
            // Retail keeps re-running opcode 0 (re-setting the flag) every
            // tick; equivalent and cheaper to stop here.
            return;
        }
        // Retail loops until an opcode increments the did-work counter; an
        // unknown opcode at PC 0 would spin forever there. Bail after every
        // script byte could have been visited once.
        let mut guard = self.script.len() + 2;
        loop {
            match self.byte_at(self.pc) {
                0 => {
                    // Opcode 0: mark the actor finished (flags |= 8).
                    self.finished = true;
                    return;
                }
                1 => {
                    // Opcode 1: restart the script from the top.
                    self.pc = 0;
                    return;
                }
                2 => {
                    self.tick_segment(step);
                    return;
                }
                0x40 => {
                    // '@' marker: skip opcode + one operand byte, keep going.
                    self.pc = self.pc.wrapping_add(2);
                }
                _ => {
                    // Unknown opcode: wrap the cursor to 0, keep going.
                    self.pc = 0;
                }
            }
            guard -= 1;
            if guard == 0 {
                return;
            }
        }
    }

    /// Opcode-2 keyframe segment body.
    fn tick_segment(&mut self, step: u16) {
        let pc = self.pc as usize;
        let duration = i32::from(self.u16_at(pc + 1));
        let phase = i32::from(self.phase as i16);
        if phase < duration {
            if phase == 0 {
                // First tick of the segment: latch the segment-start state
                // from the current outputs; write nothing else.
                self.start_a = self.aux_a;
                self.start_b = self.aux_b;
                self.start_c = self.aux_c;
                self.fog_start = unpack_channels(self.fog_rgb);
                self.aux_start = unpack_channels(self.aux_rgb);
            } else {
                self.aux_a = interp_u16(self.start_a, self.u16_at(pc + 3), phase, duration);
                self.aux_b = interp_u16(self.start_b, self.u16_at(pc + 5), phase, duration);
                // Aux C interpolates toward the NEGATED target:
                // start + (-target - start) * phase / duration.
                let target_c = i32::from(self.u16_at(pc + 0xd));
                let delta_c = -target_c - i32::from(self.start_c as i16);
                self.aux_c = (i32::from(self.start_c) + delta_c * phase / duration) as u16;
                self.fog_rgb = pack_channels([
                    interp_channel(
                        self.fog_start[0],
                        self.byte_at_usize(pc + 7),
                        phase,
                        duration,
                    ),
                    interp_channel(
                        self.fog_start[1],
                        self.byte_at_usize(pc + 8),
                        phase,
                        duration,
                    ),
                    interp_channel(
                        self.fog_start[2],
                        self.byte_at_usize(pc + 9),
                        phase,
                        duration,
                    ),
                ]);
                self.aux_rgb = pack_channels([
                    interp_channel(
                        self.aux_start[0],
                        self.byte_at_usize(pc + 0xa),
                        phase,
                        duration,
                    ),
                    interp_channel(
                        self.aux_start[1],
                        self.byte_at_usize(pc + 0xb),
                        phase,
                        duration,
                    ),
                    interp_channel(
                        self.aux_start[2],
                        self.byte_at_usize(pc + 0xc),
                        phase,
                        duration,
                    ),
                ]);
            }
            // Both sub-branches step the phase clock (retail LAB_801e4404).
            self.phase = self.phase.wrapping_add(step);
        } else {
            // Segment complete: snap to targets, reset the phase clock
            // (NOT stepped on this path) and advance to the next record.
            self.aux_a = self.u16_at(pc + 3);
            self.aux_b = self.u16_at(pc + 5);
            self.aux_c = self.u16_at(pc + 0xd).wrapping_neg();
            self.fog_rgb = pack_channels([
                self.byte_at_usize(pc + 7),
                self.byte_at_usize(pc + 8),
                self.byte_at_usize(pc + 9),
            ]);
            self.aux_rgb = pack_channels([
                self.byte_at_usize(pc + 0xa),
                self.byte_at_usize(pc + 0xb),
                self.byte_at_usize(pc + 0xc),
            ]);
            self.phase = 0;
            self.pc = self.pc.wrapping_add(0xf);
        }
    }

    fn byte_at(&self, pc: u16) -> u8 {
        self.byte_at_usize(pc as usize)
    }

    fn byte_at_usize(&self, off: usize) -> u8 {
        self.script.get(off).copied().unwrap_or(0)
    }

    /// Unaligned-LE16 operand read (retail helper `FUN_8003CE9C`; see
    /// [`load_u16_le`]). Out-of-range bytes read as zero.
    fn u16_at(&self, off: usize) -> u16 {
        self.script.get(off..).map(load_u16_le).unwrap_or(0)
    }
}

/// Truncating per-channel interpolation: `start + (target - start) * phase /
/// duration`, with the retail `lh`/`lhu` split - the delta uses the
/// sign-extended start, the final add the zero-extended start, and the store
/// keeps the low 16 bits.
fn interp_u16(start: u16, target: u16, phase: i32, duration: i32) -> u16 {
    let delta = i32::from(target) - i32::from(start as i16);
    (i32::from(start) + delta * phase / duration) as u16
}

/// Truncating byte-channel interpolation; the retail masks the sum with
/// `0xff` before packing (`as u8` here).
fn interp_channel(start: u8, target: u8, phase: i32, duration: i32) -> u8 {
    let delta = i32::from(target) - i32::from(start);
    (delta * phase / duration + i32::from(start)) as u8
}

/// Pack three channel bytes as `c0<<16 | c1<<8 | c2` (retail order for the
/// `+0x74` / `+0x88` color words).
fn pack_channels(c: [u8; 3]) -> u32 {
    (u32::from(c[0]) << 16) | (u32::from(c[1]) << 8) | u32::from(c[2])
}

/// Inverse of [`pack_channels`], used by the phase-0 start latch (retail
/// reads the bytes back out of the packed word at `+0x74` / `+0x88`).
fn unpack_channels(word: u32) -> [u8; 3] {
    [(word >> 16) as u8, (word >> 8) as u8, word as u8]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct RecHost {
        gate_open: bool,
        countdown: i8,
        encounter_en: bool,
        dialog: bool,
        player_walk: bool,
        encounter_sentinel: bool,
        pub events: Vec<String>,
    }

    impl WorldMapEntityHost for RecHost {
        fn activation_gate_open(&self) -> bool {
            self.gate_open
        }
        fn encounter_countdown(&self) -> i8 {
            self.countdown
        }
        fn set_encounter_countdown(&mut self, v: i8) {
            self.countdown = v;
            self.events.push(format!("countdown={v}"));
        }
        fn encounter_enabled(&self) -> bool {
            self.encounter_en
        }
        fn on_encounter(&mut self, idx: usize, _r: u32) {
            self.events.push(format!("encounter:{idx}"));
        }
        fn on_activating(&mut self, idx: usize) {
            self.events.push(format!("activating:{idx}"));
        }
        fn on_scene_transition(&mut self, idx: usize) {
            self.events.push(format!("transition:{idx}"));
        }
        fn dialog_active(&self) -> bool {
            self.dialog
        }
        fn player_walking(&self) -> bool {
            self.player_walk
        }
        fn on_interact(&mut self, idx: usize) {
            self.events.push(format!("interact:{idx}"));
        }
        fn encounter_counter_is_sentinel(&self) -> bool {
            self.encounter_sentinel
        }
        fn clear_encounter_counter(&mut self) {
            self.events.push("clear_counter".into());
        }
    }

    #[test]
    fn idle_gate_closed_skips_sm_body() {
        let mut ctx = WorldMapEntityCtx {
            state: 0,
            pad_flags: 0,
            field_88: 0,
        };
        let mut host = RecHost {
            gate_open: false,
            ..Default::default()
        };
        step(0, &mut ctx, &mut host);
        // Gate closed → SM body skipped, no countdown change.
        assert!(!host.events.iter().any(|e| e.starts_with("countdown")));
        assert_eq!(ctx.state, 0);
    }

    #[test]
    fn idle_gate_open_decrements_countdown() {
        let mut ctx = WorldMapEntityCtx {
            state: 0,
            pad_flags: 0,
            field_88: 0,
        };
        let mut host = RecHost {
            gate_open: true,
            countdown: 5,
            encounter_en: false,
            ..Default::default()
        };
        step(0, &mut ctx, &mut host);
        assert_eq!(host.countdown, 4);
        assert_eq!(ctx.state, 0);
    }

    #[test]
    fn idle_encounter_fires_at_zero() {
        let mut ctx = WorldMapEntityCtx {
            state: 0,
            pad_flags: 0,
            field_88: 0,
        };
        let mut host = RecHost {
            gate_open: true,
            countdown: 0,
            encounter_en: true,
            ..Default::default()
        };
        step(0, &mut ctx, &mut host);
        assert!(host.events.contains(&"encounter:0".to_string()));
    }

    #[test]
    fn idle_no_encounter_when_disabled() {
        let mut ctx = WorldMapEntityCtx {
            state: 0,
            pad_flags: 0,
            field_88: 0,
        };
        let mut host = RecHost {
            gate_open: true,
            countdown: 0,
            encounter_en: false,
            ..Default::default()
        };
        step(0, &mut ctx, &mut host);
        assert!(!host.events.iter().any(|e| e.starts_with("encounter")));
    }

    #[test]
    fn interact_fires_when_cooldown_set_and_not_blocked() {
        let mut ctx = WorldMapEntityCtx {
            state: 0,
            pad_flags: 0x100,
            field_88: 0,
        };
        let mut host = RecHost {
            gate_open: true,
            dialog: false,
            player_walk: true, // player walking, but cooldown already set
            ..Default::default()
        };
        step(0, &mut ctx, &mut host);
        assert!(host.events.contains(&"interact:0".to_string()));
    }

    #[test]
    fn interact_fires_when_player_not_walking() {
        let mut ctx = WorldMapEntityCtx {
            state: 0,
            pad_flags: 0,
            field_88: 0,
        };
        let mut host = RecHost {
            gate_open: true,
            dialog: false,
            player_walk: false, // player stopped
            ..Default::default()
        };
        step(0, &mut ctx, &mut host);
        assert!(host.events.contains(&"interact:0".to_string()));
        assert_ne!(ctx.pad_flags & 0x100, 0, "cooldown flag should be set");
    }

    #[test]
    fn interact_suppressed_by_dialog() {
        let mut ctx = WorldMapEntityCtx {
            state: 0,
            pad_flags: 0,
            field_88: 0,
        };
        let mut host = RecHost {
            gate_open: true,
            dialog: true,
            player_walk: false,
            ..Default::default()
        };
        step(0, &mut ctx, &mut host);
        assert!(!host.events.iter().any(|e| e.starts_with("interact")));
    }

    #[test]
    fn transitioning_state_advances_to_terminal() {
        let mut ctx = WorldMapEntityCtx {
            state: 2,
            pad_flags: 0x80000,
            field_88: 1,
        };
        let mut host = RecHost {
            gate_open: true,
            ..Default::default()
        };
        step(0, &mut ctx, &mut host);
        assert_eq!(ctx.state, 4, "should advance to Terminal");
        assert_eq!(ctx.pad_flags & 0x80000, 0, "walking flag cleared");
        assert_eq!(ctx.field_88, 0);
        assert!(host.events.contains(&"transition:0".to_string()));
    }

    #[test]
    fn terminal_state_does_nothing() {
        let mut ctx = WorldMapEntityCtx {
            state: 4,
            pad_flags: 0,
            field_88: 0,
        };
        let mut host = RecHost {
            gate_open: true,
            countdown: 5,
            ..Default::default()
        };
        step(0, &mut ctx, &mut host);
        assert_eq!(ctx.state, 4);
        // No SM action, no countdown change from terminal.
        assert!(!host.events.iter().any(|e| e.starts_with("countdown")));
    }

    #[test]
    fn encounter_sentinel_cleared_after_interact() {
        let mut ctx = WorldMapEntityCtx {
            state: 0,
            pad_flags: 0x100,
            field_88: 0,
        };
        let mut host = RecHost {
            gate_open: true,
            dialog: false,
            player_walk: true,
            encounter_sentinel: true,
            ..Default::default()
        };
        step(0, &mut ctx, &mut host);
        assert!(host.events.contains(&"clear_counter".to_string()));
    }
}

#[cfg(test)]
mod fog_tests {
    use super::*;

    /// Build one 0xF-byte opcode-2 keyframe segment record.
    fn seg(dur: u16, a: u16, b: u16, rgb_a: [u8; 3], rgb_b: [u8; 3], c: u16) -> Vec<u8> {
        let mut v = vec![2u8];
        v.extend_from_slice(&dur.to_le_bytes());
        v.extend_from_slice(&a.to_le_bytes());
        v.extend_from_slice(&b.to_le_bytes());
        v.extend_from_slice(&rgb_a);
        v.extend_from_slice(&rgb_b);
        v.extend_from_slice(&c.to_le_bytes());
        assert_eq!(v.len(), 0xf);
        v
    }

    #[test]
    fn segment_ramps_each_channel_then_snaps() {
        // Segment: 4-frame ramp from the initial all-zero state.
        let mut fog = AtmosphericFogTick::new(seg(4, 1000, 2000, [100, 40, 8], [200, 80, 16], 100));

        // Tick 1 (phase 0): latch only - no output change, phase steps.
        fog.tick(1);
        assert_eq!(fog.fog_rgb, 0);
        assert_eq!(fog.aux_a, 0);
        assert_eq!((fog.phase, fog.pc), (1, 0));

        // Tick 2 (phase 1 of 4): quarter of the way, truncating division.
        fog.tick(1);
        assert_eq!(fog.fog_rgb, (25 << 16) | (10 << 8) | 2);
        assert_eq!(fog.aux_rgb, (50 << 16) | (20 << 8) | 4);
        assert_eq!(fog.aux_a, 250);
        assert_eq!(fog.aux_b, 500);
        // Aux C runs toward the NEGATED target: 0 + (-100 - 0) * 1 / 4 = -25.
        assert_eq!(fog.aux_c as i16, -25);

        // Tick 3 (phase 2): halfway.
        fog.tick(1);
        assert_eq!(fog.fog_rgb, (50 << 16) | (20 << 8) | 4);
        assert_eq!(fog.aux_a, 500);

        // Tick 4 (phase 3): three quarters.
        fog.tick(1);
        assert_eq!(fog.fog_rgb, (75 << 16) | (30 << 8) | 6);
        assert_eq!(fog.aux_c as i16, -75);
        assert_eq!(fog.phase, 4);

        // Tick 5 (phase 4 >= duration): snap to targets, reset phase,
        // cursor advances 0xF; the phase clock is NOT stepped on this path.
        fog.tick(1);
        assert_eq!(fog.fog_rgb, (100 << 16) | (40 << 8) | 8);
        assert_eq!(fog.aux_rgb, (200 << 16) | (80 << 8) | 16);
        assert_eq!(fog.aux_a, 1000);
        assert_eq!(fog.aux_b, 2000);
        assert_eq!(fog.aux_c as i16, -100);
        assert_eq!((fog.phase, fog.pc), (0, 0xf));
    }

    #[test]
    fn second_segment_latches_from_first_segment_end() {
        let mut script = seg(4, 1000, 2000, [100, 40, 8], [200, 80, 16], 100);
        script.extend(seg(2, 3000, 2500, [50, 240, 8], [0, 0, 0], 0));
        let mut fog = AtmosphericFogTick::new(script);
        for _ in 0..5 {
            fog.tick(1); // run segment 1 to its snap
        }
        assert_eq!(fog.pc, 0xf);

        // Segment 2, tick 1: latch (starts = segment-1 end values).
        fog.tick(1);
        assert_eq!(fog.fog_rgb, (100 << 16) | (40 << 8) | 8);

        // Segment 2, tick 2 (phase 1 of 2): interpolate from the latched
        // starts - including a downward channel (100 -> 50) and an aux-C
        // start that is already negative (-100 -> -0).
        fog.tick(1);
        assert_eq!(fog.fog_rgb, (75 << 16) | (140 << 8) | 8);
        assert_eq!(fog.aux_a, 2000); // 1000 + (3000-1000)*1/2
        assert_eq!(fog.aux_c as i16, -50); // -100 + (0 - -100)*1/2

        // Segment 2, tick 3: snap.
        fog.tick(1);
        assert_eq!(fog.fog_rgb, (50 << 16) | (240 << 8) | 8);
        assert_eq!(fog.pc, 0x1e);
    }

    #[test]
    fn opcode_1_restarts_script() {
        let mut script = seg(1, 10, 20, [1, 2, 3], [4, 5, 6], 7);
        script.push(1); // restart
        let mut fog = AtmosphericFogTick::new(script);
        fog.tick(1); // latch (phase 0 -> 1)
        fog.tick(1); // phase 1 >= dur 1: snap, pc = 0xf
        assert_eq!(fog.pc, 0xf);
        fog.tick(1); // opcode 1: cursor back to 0, tick ends
        assert_eq!((fog.pc, fog.phase), (0, 0));
        assert!(!fog.finished);
        fog.tick(1); // segment re-runs: phase-0 latch again
        assert_eq!(fog.phase, 1);
    }

    #[test]
    fn opcode_0_sets_finished_flag() {
        let mut fog = AtmosphericFogTick::new(vec![0]);
        fog.tick(1);
        assert!(fog.finished, "opcode 0 = actor flags |= 8");
        // Empty script reads opcode 0 as well (OOB bytes read as zero).
        let mut empty = AtmosphericFogTick::new(vec![]);
        empty.tick(1);
        assert!(empty.finished);
    }

    #[test]
    fn opcode_0x40_skips_two_bytes_same_tick() {
        // '@' marker + operand, then a segment: one tick both skips the
        // marker and runs the segment's phase-0 latch.
        let mut script = vec![0x40, 0xaa];
        script.extend(seg(2, 0, 0, [9, 9, 9], [0, 0, 0], 0));
        let mut fog = AtmosphericFogTick::new(script);
        fog.tick(1);
        assert_eq!((fog.pc, fog.phase), (2, 1));
    }

    #[test]
    fn unknown_opcode_wraps_cursor_to_zero_same_tick() {
        // Segment then a junk opcode: the tick after the snap wraps the
        // cursor to 0 and re-enters the segment in the same tick.
        let mut script = seg(1, 0, 0, [9, 9, 9], [0, 0, 0], 0);
        script.push(7);
        let mut fog = AtmosphericFogTick::new(script);
        fog.tick(1); // latch
        fog.tick(1); // snap, pc = 0xf (the junk byte)
        assert_eq!(fog.pc, 0xf);
        fog.tick(1); // wrap to 0, then phase-0 latch of the segment
        assert_eq!((fog.pc, fog.phase), (0, 1));
    }

    #[test]
    fn all_junk_script_terminates() {
        // Unknown opcode at PC 0 would spin forever in retail; the port's
        // guard bails instead of hanging.
        let mut fog = AtmosphericFogTick::new(vec![7, 7, 7]);
        fog.tick(1);
        assert_eq!(fog.pc, 0);
        assert!(!fog.finished);
    }
}
