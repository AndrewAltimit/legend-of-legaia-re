//! The song-end **`3 2 1 FINISH!`** - `FUN_801cf470` states `0xB` / `0xC`.
//!
//! When the song timer reaches its limit, state `0xB` (`0x801CFD5C..0x801CFDB8`)
//! spawns four sprite parts at `(0xA0, 0x78)` through `FUN_801d3fd0` in one run
//! and advances: sprite ids `0x17` (`FINISH!`), `0x18` (`1`), `0x19` (`2`) and
//! `0x1A` (`3`), each on its own move program in the overlay's rodata. State
//! `0xC` (`0x801CFDD4..0x801CFE54`) then ramps `DAT_801D515C` by `dt * 3` a run
//! and hands over to the results at `0x489`, with the beat clock and the judge
//! still running underneath.
//!
//! The four programs share one shape and differ only in a `WAIT` and a cue:
//!
//! ```text
//! [ffff 0000]                 spawn header: no model (FUN_80021B04 starts PC at word 2)
//! 000c 0 0 0 0 0              +0x74 / +0x78 = 0
//! 000d 0400                   +0x90 = 0x2000 (the +0x78 rate)
//! 000f 0000                   +0x92 = 0
//! 0009 <delay>                WAIT 0 / 50 / 100 / 150
//! 0000 0 0 0                  anim bank 0
//! 0018 3  0020 2 0  0009 0  0019    four draws, one a tick
//! 001d <cue>                  DAT_8007B6DE = 0x209 / 0x208 / 0x207 / 0x206
//! 000d 0000                   rate 0 (hold)
//! 0018 3  0020 2 0  0009 0  0019
//! 000f 0000  000d fc00        rate -0x2000 (fade)
//! 0018 3  0020 2 0  0009 0  0019
//! 0008                        retire
//! ```
//!
//! Op `0x20 2` is the overlay's sprite hook `FUN_801D387C` (installed at
//! `gp+0x714` by the hall init, `0x801CF07C`), whose case 2 emits the part's
//! widget at its brightness - so a part is on screen only on the ticks its
//! program calls it. The brightness is the hook prologue's
//! [`super::sprite_part_fade_weight`] of `+0x78`, which the actor tick
//! `FUN_80021DF4` steps before the move VM runs (`0x80022B4C..0x80022B7C`:
//! `+0x78 += (+0x90 * dt * DAT_1F80037D) >> 6`) and clamps after it
//! (`0x80022BC0..0x80022BEC`: past `0x3E80` -> `0`, past `0x1000` -> `0x1000`).
//! This module runs the real programs through the port's move VM with exactly
//! that tick around them.

use legaia_engine_vm::move_vm::{
    ActorState, ActorTickOutcome, MoveHost, actor_tick, decrement_wait_timer,
};

/// Spawn record of each countdown part: `(sprite id, program VA)` in the order
/// state `0xB` spawns them.
pub const FINISH_PARTS: [(u16, u32); 4] = [
    (0x17, 0x801D_4CF4),
    (0x18, 0x801D_4BBC),
    (0x19, 0x801D_4C24),
    (0x1A, 0x801D_4C8C),
];

/// Halfwords each program spans (it ends on the `0x0008` retire).
pub const FINISH_PROGRAM_WORDS: usize = 52;

/// Where the parts stand: `(0xA0, 0x78)`, stored `<< 3` as the spawner does.
pub const FINISH_SEAT: (i16, i16) = (0xA0 << 3, 0x78 << 3);

/// The hall's frame-skip factor `dt` (`DAT_1F800393`).
const DT: i32 = 3;
/// The game-speed rate byte `DAT_1F80037D`.
const RATE_BYTE: i32 = 8;
/// State `0xC`'s accumulator step per run (`dt * 3`, `0x801CFDE4`).
const WIPE_STEP: i32 = DT * 3;
/// State `0xC` hands over to the results once the accumulator reaches this
/// (`slti v0,a0,0x489` at `0x801CFE38`).
pub const FINISH_WIPE_END: i32 = 0x489;
/// Vsyncs per run.
const RUN_VSYNCS: u8 = 3;

/// The four countdown programs, read out of the as-loaded dance overlay.
/// Empty when the image is too short.
pub fn finish_programs(overlay: &[u8]) -> Vec<(u16, Vec<u16>)> {
    let base = legaia_asset::dance_chart::DANCE_OVERLAY_BASE_VA;
    FINISH_PARTS
        .iter()
        .filter_map(|&(sprite, va)| {
            let off = va.checked_sub(base)? as usize;
            let bytes = overlay.get(off..off + FINISH_PROGRAM_WORDS * 2)?;
            let words = bytes
                .as_chunks::<2>()
                .0
                .iter()
                .map(|c| u16::from_le_bytes([c[0], c[1]]))
                .collect();
            Some((sprite, words))
        })
        .collect()
}

/// One countdown part in flight.
#[derive(Debug, Clone)]
struct FinishPart {
    sprite: u16,
    words: Vec<u16>,
    /// The part's actor; `+0x78` (`field_78`) is its brightness weight.
    state: ActorState,
    /// The brightness the hook emitted this run, when it drew.
    drawn: Option<u8>,
}

/// The move-VM host one part runs under: the sprite hook and the cue word.
struct PartHost {
    drawn: Option<u8>,
    cue: Option<u16>,
}

impl MoveHost for PartHost {
    fn ext_20(&mut self, state: &mut ActorState, arg0: i16, _arg1: i16) {
        if arg0 == 2 {
            self.drawn = Some(super::sprite_part_fade_weight(state.field_78));
        }
    }
    fn global_write_1d(&mut self, value: u16) {
        self.cue = Some(value);
    }
}

/// One countdown sprite on screen this vsync.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FinishDraw {
    /// Widget / sprite id (`0x17` `FINISH!`, `0x18..0x1A` `1 2 3`).
    pub sprite: u16,
    /// Brightness, the hook prologue's `0..=0xFF`.
    pub fade: u8,
}

/// What one [`FinishCountdown::step`] produced.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FinishStep {
    /// Cues the parts wrote this vsync (`0x206..=0x209`).
    pub cues: Vec<u16>,
    /// State `0xC` reached the results this vsync.
    pub done: bool,
}

/// States `0xB` / `0xC`: the four countdown parts and the wipe that outlasts
/// them.
#[derive(Debug, Clone)]
pub struct FinishCountdown {
    parts: Vec<FinishPart>,
    phase: u8,
    wipe: i32,
}

impl FinishCountdown {
    /// State `0xB`: spawn the parts from their programs.
    pub fn new(programs: &[(u16, Vec<u16>)]) -> Self {
        let parts = programs
            .iter()
            .map(|(sprite, words)| {
                let mut state = ActorState::new();
                state.pc = 2;
                state.wait_timer = 0;
                FinishPart {
                    sprite: *sprite,
                    words: words.clone(),
                    state,
                    drawn: None,
                }
            })
            .collect();
        Self {
            parts,
            phase: 0,
            wipe: 0,
        }
    }

    /// The parts drawing this vsync (a run's picture holds for its three
    /// vsyncs).
    pub fn draws(&self) -> Vec<FinishDraw> {
        self.parts
            .iter()
            .filter_map(|p| {
                p.drawn.map(|fade| FinishDraw {
                    sprite: p.sprite,
                    fade,
                })
            })
            .collect()
    }

    /// Whether every part has retired.
    pub fn parts_done(&self) -> bool {
        self.parts.is_empty()
    }

    /// Advance one vsync.
    pub fn step(&mut self) -> FinishStep {
        let mut out = FinishStep::default();
        if self.phase == 0 {
            self.wipe += WIPE_STEP;
            for p in &mut self.parts {
                // 0x80022B4C..0x80022B7C: the rate is stepped before the VM.
                let step = (i32::from(p.state.tween_src_x) * DT * RATE_BYTE) >> 6;
                p.state.field_78 = (i32::from(p.state.field_78) + step) as u16;
                decrement_wait_timer(&mut p.state, (DT * RATE_BYTE) as u16);
                let mut host = PartHost {
                    drawn: None,
                    cue: None,
                };
                let r = actor_tick(&mut host, &mut p.state, &p.words, 64);
                p.drawn = host.drawn;
                if let Some(c) = host.cue {
                    out.cues.push(c);
                }
                // 0x80022BC0..0x80022BEC: clamp after the VM.
                if p.state.field_78 > 0x3E80 {
                    p.state.field_78 = 0;
                } else if p.state.field_78 > 0x1000 {
                    p.state.field_78 = 0x1000;
                }
                if r == ActorTickOutcome::Halted {
                    p.drawn = None;
                    p.words.clear();
                }
            }
            self.parts.retain(|p| !p.words.is_empty());
            out.done = self.wipe >= FINISH_WIPE_END;
        }
        self.phase += 1;
        if self.phase >= RUN_VSYNCS {
            self.phase = 0;
        }
        out
    }
}
