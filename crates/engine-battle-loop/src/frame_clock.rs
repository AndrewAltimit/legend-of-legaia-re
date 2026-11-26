//! The **battle frame**: how the engine's one-vsync ticks group into the
//! frames retail runs the battle on.
//!
//! Retail runs one battle pass a frame and spans each frame with the frame
//! step `DAT_1F800393` vsyncs. `FUN_80016B6C` measures that step every frame
//! from the frame's own cost: the newest `VSync(1)` hblank sample goes into
//! the sixteen-entry ring `0x80084098` (index `gp+0x440`, a sample of
//! `0x2D0` or more clamped to `0x2BC`), and the step is the ring's maximum
//! against the thresholds `0xF1` / `0x1FF` / `0x2D1` (`1` / `2` / `3`, else
//! `4`; `0x80017098..0x8001715C`), raised to the per-mode floor
//! `0x8007B9D8`. The battle's floor is `1` in every catalogued battle state,
//! so its step is the load alone: `2` on a lone member's frame (one field plus
//! the miss), `3` on a full party's or while a summon's creature and effects fill the frame
//! (`theeder_summon_mid_cast`'s ring peaks at `625`, `gimard_burning_attack`'s
//! sits at the `700` clamp). It is not a property of the game state, so the
//! engine runs the same law over a measured cost ([`FrameCostRing`],
//! [`measured_frame_cost`]), the cast arm tables inside a module band, and only
//! a replay that knows retail's step installs another.
//!
//! The engine ticks once a vsync and keeps every per-vsync rate; what the
//! step changes is where the frame boundaries fall - the root-motion carry's
//! truncation (`RootMotionCarry`) and the camera walker's steps
//! (`BattleCamera::set_frame_step`).
//!
//! REF: FUN_80016B6C

pub use legaia_engine_cast::cast_module_frame_step::CdTiming;

/// The engine's battle frame step: retail's step on an ordinary battle frame,
/// the one the camera and the root-motion carry assume by default.
pub const DEFAULT_BATTLE_FRAME_STEP: u8 = 2;

/// What the frame driver's pacing tail reads beside the frame's own cost
/// (`FUN_80016B6C`, `0x80017044..0x800171D8`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FramePace {
    /// The forced step `gp+0x5D8`: non-zero stores its low byte as the step
    /// and skips the history (`0x80017050..0x80017090`).
    pub forced: u32,
    /// The frame-rate mode `gp+0x4CE`: only `0x10` reads the step off the
    /// history, every other value stores step `1` (`0x80017108..0x80017164`).
    pub mode: i16,
    /// The step floor `0x8007B9D8` (`0x80017170..0x80017198`).
    pub floor: i32,
}

impl FramePace {
    /// The adaptive mode every catalogued battle state holds: mode `0x10`,
    /// no forced step, a floor the law never reads below.
    pub const ADAPTIVE: Self = Self {
        forced: 0,
        mode: 0x10,
        floor: 1,
    };
}

/// What one frame's pacing tail leaves: the step `0x1F800393` the next frame
/// runs on, and the count it hands `VSync` (`jal 0x8005FB84`), which is the
/// step the frame before stored - `0` (one vsync) while that is under `2`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameEnd {
    pub step: u8,
    pub vsync_wait: u32,
}

/// Port of the frame driver's step law (`FUN_80016B6C`): the sixteen-entry
/// history `0x80084098`, its write cursor `gp+0x440`, the step byte
/// `0x1F800393` and the step the frame before left (`0x1F800392`). What
/// feeds it differs. Retail stores the hblanks since the last vsync once the
/// CPU has finished the frame and `DrawSync(0)` has returned
/// (`FUN_800173BC`), so the entry is `max(cpu, gpu of the last table)`; the
/// engine has no such clock and pushes [`measured_frame_cost`].
///
/// REF: FUN_800173BC
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameCostRing {
    entries: [i16; 16],
    /// `gp+0x440` as the last pass left it: masked on the way in, so it
    /// reads `1..=16` after a store.
    cursor: u8,
    step: u8,
    prev_step: u8,
}

impl Default for FrameCostRing {
    fn default() -> Self {
        Self {
            entries: [0; 16],
            cursor: 0,
            step: 1,
            prev_step: 1,
        }
    }
}

impl FrameCostRing {
    /// A ring holding these sixteen entries with the write cursor at
    /// `cursor` (`gp+0x440`, the slot the next pass stores to), as a retail
    /// capture's `0x80084098` holds them.
    ///
    /// A main-RAM image does not hold the step byte itself (it is the
    /// scratchpad's), so the step is read back off the entries. That is
    /// exact but for a newest entry on the clamp `0x2BC`, which the history
    /// stores for a raw duration of `0x2BC` (step `3`), `0x2D0` (step `3`)
    /// and anything longer (step `4`) alike; it is taken as the long frame,
    /// the common case. A capture that holds the byte passes it through
    /// [`Self::with_step`].
    pub fn from_entries(entries: [u16; 16], cursor: u8) -> Self {
        let entries = entries.map(|e| e as i16);
        let cursor = cursor & 0xF;
        let newest = entries[usize::from(cursor.wrapping_sub(1) & 0xF)];
        let step = if newest == 0x2BC {
            4
        } else {
            step_of_cost(entries.iter().copied().max().unwrap_or(0).max(0) as u16)
        };
        Self {
            entries,
            cursor,
            step,
            prev_step: step,
        }
    }

    /// A ring from every byte the law reads: the entries, the cursor word
    /// `gp+0x440`, the step `0x1F800393` and the step before it
    /// (`0x1F800392`).
    pub fn from_state(entries: [i16; 16], cursor: u8, step: u8, prev_step: u8) -> Self {
        Self {
            entries,
            cursor,
            step,
            prev_step,
        }
    }

    /// The same ring with the step byte `0x1F800393` a capture read.
    pub fn with_step(mut self, step: u8) -> Self {
        self.step = step;
        self.prev_step = step;
        self
    }

    /// The sixteen entries as `0x80084098` holds them.
    pub fn entries(&self) -> [i16; 16] {
        self.entries
    }

    /// The cursor word `gp+0x440`.
    pub fn cursor(&self) -> u8 {
        self.cursor
    }

    /// The newest entry: what the last pass's frame cost.
    pub fn newest(&self) -> u16 {
        self.entries[usize::from(self.cursor.wrapping_sub(1) & 0xF)].max(0) as u16
    }

    /// The step byte `0x1F800393`: what the last pass's tail stored for the
    /// next frame.
    pub fn step(&self) -> u8 {
        self.step
    }

    /// The step the frame before the last stored (`0x1F800392`), which the
    /// last tail's `VSync` call waited on.
    pub fn prev_step(&self) -> u8 {
        self.prev_step
    }

    /// One frame's pacing tail: store the frame's raw cost and read the step.
    ///
    /// The cursor is masked to the ring first. A forced step skips
    /// everything else. Otherwise the cost is stored - a frame of `0x2D0`
    /// hblanks or more as `0x2BC` - and the step is read off the largest
    /// entry, or off the **raw** cost when it reached `0x2D0`
    /// (`slti v0,a2,0x2d0` / `move s0,a2` at `0x800170F8..0x80017104`): raw
    /// `0x2D0` reads step `3` like the clamp it stored, `0x2D1` and up step
    /// `4`. The floor raises the result, and the frame waits on the step the
    /// frame before it stored.
    // PORT: FUN_80016B6C (`0x80017044..0x800171D8`, the frame-step law)
    pub fn frame_end(&mut self, raw: i32, pace: FramePace) -> FrameEnd {
        self.cursor &= 0xF;
        if pace.forced != 0 {
            self.step = pace.forced as u8;
            let vsync_wait = if pace.forced == 1 { 0 } else { pace.forced };
            return FrameEnd {
                step: self.step,
                vsync_wait,
            };
        }
        let stored = if raw < 0x2D0 { raw } else { 0x2BC };
        self.entries[usize::from(self.cursor)] = stored as i16;
        self.cursor += 1;
        let mut longest = self
            .entries
            .iter()
            .copied()
            .fold(0i32, |m, e| m.max(i32::from(e)));
        if raw >= 0x2D0 {
            longest = raw;
        }
        let mut step: u8 = match longest {
            _ if pace.mode != 0x10 => 1,
            ..0xF1 => 1,
            0xF1..0x1FF => 2,
            0x1FF..0x2D1 => 3,
            _ => 4,
        };
        if i32::from(step) < pace.floor {
            step = pace.floor as u8;
        }
        let vsync_wait = if self.prev_step >= 2 {
            u32::from(self.prev_step)
        } else {
            0
        };
        self.step = step;
        self.prev_step = step;
        FrameEnd { step, vsync_wait }
    }

    /// Store one frame's cost on the adaptive mode ([`FramePace::ADAPTIVE`])
    /// and read the step the next frame runs on.
    pub fn push(&mut self, cost: u16) -> u8 {
        self.frame_end(i32::from(cost), FramePace::ADAPTIVE).step
    }
}

/// The step a frame cost reads (`0x80017124..0x8001715C`).
pub const fn step_of_cost(cost: u16) -> u8 {
    if cost < 0xF1 {
        1
    } else if cost < 0x1FF {
        2
    } else if cost < 0x2D1 {
        3
    } else {
        4
    }
}

/// A frame's cost as the ring would have stored it, for a party of this size in
/// this action state: the median entry of the library's battle states and of
/// three whole fights recorded on PCSX-Redux (lone member, two, full party).
/// A member adds about `0x7C` hblanks, and on this machine the entry is the CPU's
/// alone: `gp+0x3A8` (CPU done) and `gp+0x3AC` (after `DrawSync`) read the same.
/// It is a measurement of one emulated machine, like the cast arm tables, and no
/// model of a frame's work: Mednafen reads a full party's action about `100` lower.
/// That gap is the CPU's (PCSX-Redux charges two cycles an instruction), not the GPU's,
/// whose wait only ever adds; the GPU's share of a real console's frame is modelled in
/// `docs/subsystems/battle-stage-camera.md` ("What a real console's frame costs") and
/// rarely passes this table's CPU figure. No real hardware has been measured.
///
/// The strike states `0x1E..=0x20` of a lone member and a full party are the
/// monster strike recordings' frames with no effect part live (the probe's
/// `cost` column, the ring's newest entry): what a strike's own spark burst
/// and a special clip's ghosts add is [`frame_load_cost`]'s. The library's
/// full-party medians (`700` in `0x1F`, `667` in `0x20`) held that load in
/// the base, where a monster strike on a full party with no part live reads
/// `596`.
///
/// The summon close-up `0x33` and the flash `0x34` are read off the step
/// rather than a ring entry: on every PCSX-Redux summon film of a lone caster
/// the step drops from `2` to `1` sixteen frames after the close-up's first few
/// frames (so those frames cost under `0xF1`), and on a full party's from `3`
/// to `2` sixteen frames after the close-up's first (under `0x1FF`). The
/// two-member figure is the member step between them; no film records it.
/// The close-up's opening frames cost more than the rest
/// ([`measured_frame_cost_at`]).
pub const fn measured_frame_cost(party_count: u8, action_state: u8) -> u16 {
    let row: [u16; 3] = match action_state {
        0x33 | 0x34 => [200, 330, 460],
        0x35 | 0x36 => [SUMMON_BAND_FRAME_COST; 3],
        0x00 => [282, 411, 545],
        0x0A => [340, 457, 527],
        0x0C => [354, 463, 608],
        0x14 => [329, 453, 593],
        0x19 => [379, 483, 594],
        0x1E => [393, 479, 622],
        0x1F => [389, 453, 596],
        0x20 => [386, 540, 596],
        0x29 => [284, 411, 500],
        0x2B => [354, 387, 534],
        0x2E => [354, 494, 560],
        0x50 => [349, 468, 599],
        0x51 => [341, 457, 595],
        0x5A => [310, 452, 428],
        _ => [345, 460, 595],
    };
    row[match party_count {
        0 | 1 => 0,
        2 => 1,
        _ => 2,
    }]
}

/// What a summon band's frame (`0x35` / `0x36`) costs on an arm no arm table
/// prices: the band's opening arms, the page-in pass and the drive's seek.
/// The module draws its own scene with the party and the stage off the frame,
/// so the cost does not read the party: over the lone-caster films of
/// `party_basic_attack_vs_gobu_gobu` the ring's entries on those arms run
/// `122`..`236` hblanks (three casts whose opening is a black frame read under
/// `100`), and the full-party films (`kor`, `tower`) `165`..`221`. All sit
/// under `0xF1`, so the band opens on step `1` once the close-up's entries
/// have left the ring. Priced off the ordinary row instead, each opening pass
/// stored a step-`2` entry (`3` on a full party) and the band's first arm ran
/// sixteen passes on it: Gimard's arm 2 drew `49` frames where retail's draws
/// `64`, and the `rand()` stream stood fifteen frame-driver draws short from
/// there on. One emulated machine's measurement, like the table.
pub const SUMMON_BAND_FRAME_COST: u16 = 200;

/// The longest hold of a summon module's seek arm that leaves its frame's
/// cost alone, in vsyncs: PCSX-Redux's ordinary seek takes `1..4`, inside the
/// frame's own wait for its step. A longer one loads the frame it ends on
/// (the world's cost push). Two full-party recordings place it: a hold of `3`
/// stores no dear entry, one of `9` stores a step-`3` one.
pub const SEEK_HOLD_FREE_VSYNCS: u8 = 4;

/// The passes the summon close-up `0x33` opens on at a higher cost
/// ([`measured_frame_cost_at`]).
pub const CLOSE_UP_OPENING_PASSES: u8 = 6;

/// What each of the close-up's opening passes costs over the state's own
/// entry ([`measured_frame_cost_at`]).
pub const CLOSE_UP_OPENING_EXTRA: u16 = 45;

/// [`measured_frame_cost`] on the `passes`-th pass (from `0`) the action SM
/// has run in `action_state`. The summon close-up `0x33` opens on
/// [`CLOSE_UP_OPENING_PASSES`] frames that cost [`CLOSE_UP_OPENING_EXTRA`]
/// over the rest, which is what places the step's drop on the PCSX-Redux
/// films: thirty-nine lone-caster films of `party_basic_attack_vs_gobu_gobu`
/// drop from `2` to `1` forty-two vsyncs into `0x33` (six opening frames
/// over `0xF1`, then fifteen passes until the ring has none), where a close-up
/// costed flat from its first frame dropped at thirty; the six full-party films (`kor`, `tower`)
/// drop from `3` to `2` forty-nine vsyncs in, which the same extra keeps
/// under `0x1FF` (`460 + 45`). Like the table it is one emulated machine's
/// measurement: the `map01` lone caster drops at fifty-four.
pub const fn measured_frame_cost_at(party_count: u8, action_state: u8, passes: u8) -> u16 {
    let cost = measured_frame_cost(party_count, action_state);
    if action_state == 0x33 && passes < CLOSE_UP_OPENING_PASSES {
        cost + CLOSE_UP_OPENING_EXTRA
    } else {
        cost
    }
}

/// What a frame costs while the CD drive streams a battle CD-XA clip: the
/// clip's sector interrupts run through the CD response dispatcher
/// `FUN_8005DAB0` (`0x8005DAEC`, sampled across the frame on a gate-replay of
/// the town01 fight) on the same CPU as the frame, and the ring's entry reads
/// its clamp (`0x2BC`) for the whole read span (`1063` raw hblanks on an item
/// use's clip, an arts swing's about `1200`, a Spirit's `1411`..`1660`). One
/// emulated machine's measurement, like the table above, and the floor of a
/// loaded frame's cost: what a dearer frame costs is
/// [`XA_POLL_COST_RATIO_MILLI`] times its unloaded cost.
pub const XA_STREAM_FRAME_COST: u16 = 1063;

/// What a frame the clip's polls load costs on PCSX-Redux, in thousandths of the
/// cost the same frame has outside them (the table's and the draw load's, what
/// [`xa_stream_pass_cost`] is handed). The stop watch's polls run back to back
/// from the CD interrupt (`FUN_8003D764` re-issuing `GetStat` / `GetlocL` from
/// its completion callback), and the emulator answers each `0x800` cycles
/// after it (`cdrom.cc`, `AddIrqQueue(m_cmd, 0x800)`). A `--cd-trace`
/// recording of Gala's Flying Knee on a full party
/// (`battle_ropeway_party_gala_flying_knee_mid`) spends `1.63` hblanks of each
/// poll from the exception vector to the dispatcher's exit and leaves `.92`,
/// so its loaded frames cost `2.75`..`2.87` times what runs outside those
/// spans (`681`..`931` hblanks, `1952`..`2584` of cost). That remainder is
/// more than the frame costs unloaded (`620`..`660` on the frames before the
/// cue, `707`..`821` after it): part of each poll runs outside the traced
/// span. Over the unloaded cost the spans read `3.1`..`3.6` times on that film,
/// `3.1`..`3.9` on the full party's heals (`ropeway_orb_pre_heal`,
/// `deene_garnet_pre_heal`, `taiku_topaz_pre_hyper_heal`: frames of `8` and
/// `9` vsyncs over the table's `595`), and `3.0`..`4.5` on Amethyst's on a lone
/// Vahn (`5` and `6` over `345`). The player arts' loaded frames sit at `8`
/// vsyncs at the median over some `620` (Super Ironhead's at `6` to `8`), so
/// `3.1`, the low end the three share, takes the arts' typical frame.
pub const XA_POLL_COST_RATIO_MILLI: u32 = 3100;

/// What a frame of `work` hblanks costs while the clip's polls load it on
/// PCSX-Redux ([`XA_POLL_COST_RATIO_MILLI`]), at least
/// [`XA_STREAM_FRAME_COST`].
pub const fn xa_loaded_frame_cost(work: u16) -> u16 {
    let c = (work as u32 * XA_POLL_COST_RATIO_MILLI + 500) / 1000;
    if c > u16::MAX as u32 {
        u16::MAX
    } else if c < XA_STREAM_FRAME_COST as u32 {
        XA_STREAM_FRAME_COST
    } else {
        c as u16
    }
}

/// Vsyncs a battle clip's polls load the frame past its span on PCSX-Redux. The
/// clip player sets its end sector `span * 150 / 60` sectors on
/// (`0x8003D698..0x8003D6D8`, the drive's double speed) and polls until the
/// head passes it, but the emulator defers a sector while a command's
/// interrupt is pending (`cdrom.cc` `readInterrupt`, `scheduleCDReadIRQ(irqReschedule)`),
/// and the polls keep one pending nearly all the time. Flying Knee's polls
/// (`--cd-trace`) run `18` vsyncs past its `90` from the cue, `3` of them the
/// seek; the full party's heals' loaded frames end `10` to `17` past their
/// `261`; and the stop that clears the drive word once the read is done
/// (`FUN_8003EE00`, `sw zero` at `0x8003EE2C`) runs `20` past Medusa's Glare's
/// `539` (`--watch 0x8007BC20`).
pub const XA_READ_TAIL_VSYNCS: u16 = 20;

/// Hblanks a vsync spans: the NTSC field's `263` lines, the unit of libetc's
/// `VSync(1)` count (`FUN_8005FB84`, root counter 1 less its base at
/// `0x800797D0`).
pub const HBLANKS_PER_VSYNC: u32 = 263;

/// Hblanks a frame spends past its cost word before its `VSync(n)` can see the
/// next vertical blank: the vblank handler and the `VSync` return that open
/// the frame (the counter's base `0x800797D0` is stored after both,
/// `0x8005FCA4..0x8005FCB0`), and the ring update between `FUN_800173BC`'s
/// return and the call (`0x80017044..0x800171C0`). Fit to two kinds of frame:
/// PCSX-Redux's strike recordings' spans read it best at `20` (`.968` of
/// `4145` frames), and the Flying Knee `--cd-trace` recording bounds it to
/// `-4..38` from its ten loaded frames of `1952`..`2584` hblanks, `8` to `10`
/// vsyncs long.
pub const FRAME_END_HBLANKS: u32 = 20;

/// The vsyncs a frame spans when it costs `raw` hblanks and waits on `step`.
/// `VSync(n)` (`FUN_8005FB84`) waits for the vsync count to reach the last
/// return's plus `n - 1` (`0x8005FBE8..0x8005FC18`), then for the next vertical
/// blank after that (`0x8005FC2C..0x8005FC38`, the count plus one): a frame
/// that has not finished by its step's last blank runs on to the first blank
/// after its work. A step under `2` waits `VSync(0)`, the next blank. So
/// the span is the larger of the step and the blanks the frame's cost and
/// [`FRAME_END_HBLANKS`] run past.
pub const fn frame_span(raw: u16, step: u8) -> u8 {
    let need = (raw as u32 + FRAME_END_HBLANKS) / HBLANKS_PER_VSYNC + 1;
    let step = if step < 1 { 1 } else { step as u32 };
    let span = if need > step { need } else { step };
    if span > u8::MAX as u32 {
        u8::MAX
    } else {
        span as u8
    }
}

/// The overrun of a frame the XA stream loads at the item's measured cost
/// ([`XA_STREAM_FRAME_COST`]): the frame's raw cost past its step-`4` span, in
/// vsyncs ([`frame_span`]). `1` for the item's and art's `1063`..`1200`
/// (`5`-vsync passes on the capture), which an item use's passes read
/// exactly.
pub const XA_STREAM_SPAN_EXTRA: u8 = frame_span(XA_STREAM_FRAME_COST, 4) - 4;

/// What a battle CD-XA clip adds to the frames after its cue. The cue's own
/// pass carries none: the cue is issued inside it and the drive has yet to
/// stream. The pass after it carries [`XA_STREAM_ONSET_COST`] over its base,
/// and every pass from the second on runs its work under the polls
/// ([`xa_loaded_frame_cost`]). Two
/// PCSX-Redux recordings of a monster's Heal read it: Orb's on a full party
/// (`ropeway_orb_pre_heal`) dispatches the clip `0x20C` on the last `0x3D` pass
/// (from `0x801F3C20`), which costs `621`, and the next pass reads the clamp;
/// Amethyst's on a lone Vahn (`battle_nilboa_amethyst_heal_mid_cast`) costs
/// `392` on the cue's pass, `507` on the next, then the clamp.
///
/// Those are PCSX-Redux's costs ([`CdTiming::PcsxRedux`]): the clip player's
/// stop watch polls the drive from interrupt context for the whole clip, and
/// a frame's work runs in the third of the CPU the polls leave there. On a
/// console ([`CdTiming::Console`]) a pass from the
/// cue's next on costs its base over the console's share
/// ([`legaia_engine_cast::cast_module_frame_step::console_poll_cost`]).
pub const fn xa_stream_pass_cost(
    passes_since_cue: u64,
    base: u16,
    timing: CdTiming,
    poll_share: bool,
) -> u16 {
    if let CdTiming::Console | CdTiming::Mednafen = timing {
        return if passes_since_cue == 0 {
            base
        } else {
            legaia_engine_cast::cast_module_frame_step::console_poll_cost(base)
        };
    }
    match passes_since_cue {
        0 => base,
        1 => base.saturating_add(XA_STREAM_ONSET_COST),
        _ if poll_share => xa_loaded_frame_cost(base),
        _ => {
            if base > XA_STREAM_FRAME_COST {
                base
            } else {
                XA_STREAM_FRAME_COST
            }
        }
    }
}

/// Hblanks of a battle frame's work that do not scale with what it draws, the
/// intercept of [`polygon_frame_work`].
pub const POLY_FRAME_BASE: u16 = 43;

/// Hblanks a battle frame's work spends per polygon it links, in thousandths.
/// Every polygon of a battle object runs through the per-primitive dispatcher
/// `FUN_80043390` (its handler transforms the corners, culls on `NCLIP`, depth
/// sorts and writes the packet into the ordering table), so the CPU half of a
/// frame grows with what it links rather than with the action state. Read off
/// the `--cd-trace` recordings of the enemy casts (the frame's cost from its
/// `VSync` return to the CPU-done mark, less the CD interrupt spans,
/// `scripts/ci/cd-poll-steps.py`) against the polygons the engine's host links
/// for the same state and arm: `43 + .184` a polygon over `171` state-and-arm
/// medians of `32` films, rms `89` hblanks where the action-state table reads
/// `206`; fit on half the films and scored on the other, rms `50` to `114`.
/// One emulated machine (PCSX-Redux, two cycles an instruction).
pub const POLY_WORK_MILLI: u32 = 184;

/// The work a battle frame that links `polys` polygons costs
/// ([`POLY_FRAME_BASE`], [`POLY_WORK_MILLI`]), in hblanks.
pub const fn polygon_frame_work(polys: u32) -> u16 {
    let c = POLY_FRAME_BASE as u32 + (polys as u64 * POLY_WORK_MILLI as u64 / 1000) as u32;
    if c > u16::MAX as u32 {
        u16::MAX
    } else {
        c as u16
    }
}

/// Hblanks of a battle frame's work that do not scale with what it links,
/// the intercept of [`linked_frame_work`]: the frame driver, the battle VMs,
/// the 2D layers the census does not count.
pub const LINKED_FRAME_BASE: u16 = 75;

/// Hblanks a battle frame spends on the backdrop pair and the ground grid
/// while they draw, over the primitives of theirs it links: every backdrop
/// primitive is transformed before the quad leaves' off-screen reject
/// (`FUN_80044798`) or the winding test drops it, and the grid walks its
/// cells to cull them (`battle_ground_grid::visible_cell_count`), so the
/// pair costs a near-constant share of the frame the census's linked count
/// does not see. Read off the cost-carrying PCSX-Redux recordings of the
/// strike, arts and enemy film sets: the frames that draw neither (a black
/// backdrop's close-ups: Final Crisis's arm 6, Fatal Decision's arms 6 and
/// 7, Blazing Slash's last arms) cost `40` to `60` hblanks under the line
/// [`LINKED_FRAME_BASE`] `+ 42` reads the rest on, and with the term the
/// step those frames read agrees with retail's on `.77` of the enemy films'
/// state-and-arm medians against `.73`, the strikes' and arts' unchanged.
pub const LINKED_BACKDROP_WORK: u16 = 42;

/// Hblanks a battle frame's work spends per primitive it links, in
/// thousandths, over the census of world state
/// (`legaia_engine_scene_host::battle_link_census`), which counts what
/// retail's ordering table holds rather than what a host's draw lists carry.
/// Fitted on retail's unloaded frames (cost under the clamp, frame inside its
/// step plus one, the summon band's arms left out) of half the four film sets
/// against the census on the paired engine tick: scored on the other half, rms
/// `82` hblanks and the step read right on `.80` of frames, where the
/// action-state table reads rms `137` and `.75`
/// (`docs/subsystems/battle-stage-camera.md`, "The census").
pub const LINKED_WORK_MILLI: u32 = 200;

/// Hblanks of CPU work PROT 0898's colour wash adds to a frame in its scanline
/// modes (`ctx[+0x27D]` `1` / `2`), which the census does not count (it is a
/// screen layer, linked at the farthest bucket). The emitter
/// (`FUN_801F2410`, `0x801F2958..0x801F2D20`) builds every screen line
/// `0..0xE0` itself: a loop body of `236` instructions with two `div`, two
/// `mult` and a `POLY_FT4` linked per strip through `FUN_8003D2C4` (two or
/// three strips a line), some `580` cycles a line on PCSX-Redux's two cycles an
/// instruction, `224` lines, `60` hblanks of `2152` cycles. The band mode
/// (`0`) links a handful of `POLY_G4` and costs nothing worth pricing. Under
/// it, Genocidal Cannon's caster close-ups (arms 7, 9 and `0x0B`, about `500`
/// primitives) priced `217` on the census, `603` under the line's polls, step
/// `3`, where every retail frame there runs step `4` five vsyncs long.
pub const SCANLINE_WASH_WORK: u16 = 60;

/// Packets the battle template's tick `FUN_8004DA00` links on a frame with
/// no command surface up: the HUD's name plates, bars and readouts. The
/// census line's base ([`LINKED_FRAME_BASE`]) was fitted on such frames, so
/// it carries them. `retail-exec battle-cost` on `battle_rugi_barra_strike`
/// (a full party's strike, no surface).
pub const HUD_PACKETS_NO_SURFACE: u16 = 76;

/// Packets `FUN_8004DA00` links while the command flow `ctx[+0x06]` stands
/// on `flow`: the HUD and the surface's own windows, rows and cursors. Read
/// with `retail-exec battle-cost --mode parts` (interrupts out) on the
/// library's full-party menu states of the Gaza and `rugi` fights:
///
/// | Flow | Surface | Packets |
/// |---|---|---:|
/// | `0x1E` | the round prompt | `152..156` |
/// | `0x28` | the command ring | `281` |
/// | `0x32` | the run confirm | `184` |
/// | `0x3C` / `0x46` | the item / magic window | `518` / `501` |
/// | `0x50` | the arts entry | `278` |
/// | `0x5A` | the target cursor | `137..145` |
/// | `0x5C` | the magic window's enemy target | `412` |
/// | `0x64` / `0x66` | the item window's ally target / all-allies confirm | `501` / `456` |
/// | `0x6E` | the commit confirm | `209` |
/// | `0x78` | the Auto / Command prompt | `162` |
///
/// The other target sub-cursors keep their window on screen and take its
/// nearest measured row (`0x5B` the item target's, `0x5D` the all-allies
/// confirm's, `0x5E` / `0x65` / `0x67` the magic target's). Anything else is
/// the HUD alone.
pub const fn surface_hud_packets(flow: u8) -> u16 {
    match flow {
        0x1E => 155,
        0x28 => 281,
        0x32 => 184,
        0x3C => 518,
        0x46 => 501,
        0x50 => 278,
        0x5A => 141,
        0x5B | 0x64 => 501,
        0x5D | 0x66 => 456,
        0x5C | 0x5E | 0x65 | 0x67 => 412,
        0x6E => 209,
        0x78 => 162,
        _ => HUD_PACKETS_NO_SURFACE,
    }
}

/// Hblanks a command surface adds to a battle frame over the census line,
/// whose base holds the HUD of a frame with no surface: its packets over
/// [`HUD_PACKETS_NO_SURFACE`] at the template tick's cost a packet
/// ([`TEMPLATE_TICK_PACKET_CYCLES`](crate::derived_cost::TEMPLATE_TICK_PACKET_CYCLES)).
/// The item window adds `54`, the round prompt `10`. The command flow
/// `FUN_801D0748` itself runs about `2000` cycles a frame (one hblank), and
/// is in the base.
pub const fn surface_frame_work(flow: u8) -> u16 {
    let packets = surface_hud_packets(flow);
    if packets <= HUD_PACKETS_NO_SURFACE {
        return 0;
    }
    let cycles = (packets - HUD_PACKETS_NO_SURFACE) as u32
        * crate::derived_cost::TEMPLATE_TICK_PACKET_CYCLES;
    ((cycles + CYCLES_PER_HBLANK / 2) / CYCLES_PER_HBLANK) as u16
}

/// Hblanks a floor-clipped body's full-TMD walk costs a primitive of its
/// drawn objects, over what the census prices its linked primitives at.
/// A sinking body (`+0x42` non-zero, stored beside the sink's height at
/// `0x800504AC`) leaves the dispatcher's leaves: `FUN_80048A08` sends each of
/// its objects through `FUN_8001C204` and the TMD renderer `FUN_8002735C`,
/// which runs its GTE ops on every primitive. PCSX-Redux recordings sampled
/// a vsync at a time (`--gpu-cost`) put the pass inside `0x8002735C..` and
/// `0x8001C2xx` through the dead target's sink in state `0x20`, and nowhere
/// else; the arts films' unloaded `0x20` frames under a sink read over the
/// census by `.13` to `.19` hblanks a mesh primitive (Gimard-class `318`,
/// Orb `518`) against the same films' `0x51` frames.
pub const FLOOR_CLIP_WALK_MILLI: u32 = 160;

/// The work [`FLOOR_CLIP_WALK_MILLI`] adds for `prims` walked primitives.
pub const fn floor_clip_walk_work(prims: u32) -> u16 {
    let c = prims as u64 * FLOOR_CLIP_WALK_MILLI as u64 / 1000;
    if c > u16::MAX as u64 {
        u16::MAX
    } else {
        c as u16
    }
}

/// Instructions the `"ME"` archive reader `FUN_8002B28C` (with its
/// channel-delta codec `FUN_8002A9CC`) executes a decoded part-frame: the
/// anim commit `FUN_8004AD80` decodes a bank record's keyframe stream out of
/// the side-band slot on every party commit of a staged id `>= 0x10` (`jal
/// 0x8002B28C` at `0x8004BCC8`). Run in the retail interpreter over
/// every entry of the `readef.DAT` archives, the reader's instruction count
/// is `512` per part-frame to within `3%` (Gala's `13`-frame entries
/// `98k`, Noa's `60`-frame `481k`).
pub const ME_DECODE_STEPS_PER_PART_FRAME: u32 = 512;

/// The hblanks a bank commit's `"ME"` decode of `steps` instructions
/// (`legaia_actor_formats::me_archive::reader_steps`) adds to its pass on
/// PCSX-Redux (two cycles an instruction, `2152` cycles an hblank). A `--cd-trace`
/// recording of Dragon Fangs (`jou_noa_dragon_fangs_super_open`) puts the
/// first art's commit pass inside the reader for its first `100` hblanks,
/// ahead of the shout's `CdlSetloc`, and the arts films' art-commit passes
/// read `60..120` hblanks over their census.
pub const fn me_decode_steps_work(steps: u32) -> u16 {
    let c = (steps as u64 * 2 + 1076) / 2152;
    if c > u16::MAX as u64 {
        u16::MAX
    } else {
        c as u16
    }
}

/// [`me_decode_steps_work`] on the per-part-frame law
/// ([`ME_DECODE_STEPS_PER_PART_FRAME`]), for a clip whose entry's own count
/// is not known.
pub const fn me_decode_work(parts: u32, frames: u32) -> u16 {
    let c =
        (parts as u64 * frames as u64 * ME_DECODE_STEPS_PER_PART_FRAME as u64 * 2 + 1076) / 2152;
    if c > u16::MAX as u64 {
        u16::MAX
    } else {
        c as u16
    }
}

/// The work a battle frame whose census counts `linked` primitives costs
/// ([`LINKED_FRAME_BASE`], [`LINKED_WORK_MILLI`]), in hblanks, with
/// [`LINKED_BACKDROP_WORK`] while the backdrop pair or the grid draws.
pub const fn linked_frame_work(linked: u32, backdrop: bool) -> u16 {
    let base = LINKED_FRAME_BASE as u32
        + if backdrop {
            LINKED_BACKDROP_WORK as u32
        } else {
            0
        };
    let c = base + (linked as u64 * LINKED_WORK_MILLI as u64 / 1000) as u32;
    if c > u16::MAX as u32 {
        u16::MAX
    } else {
        c as u16
    }
}

/// What a frame the clip's polls load costs over its work, in thousandths,
/// when the work is the frame's own ([`polygon_frame_work`]): the median of
/// the `--cd-trace` recordings' polled state-and-arm medians, cost over cost
/// less the CD interrupt spans (`2.78`, tenth to ninetieth percentile of the
/// fully polled `2.70..2.85`). [`XA_POLL_COST_RATIO_MILLI`] is the same
/// measurement over the action-state table, which reads such frames low.
pub const XA_POLL_WORK_RATIO_MILLI: u32 = 2780;

/// What a frame of `work` hblanks of its own costs under a clip's polls on
/// PCSX-Redux ([`XA_POLL_WORK_RATIO_MILLI`]).
pub const fn xa_polled_work_cost(work: u16) -> u16 {
    let c = (work as u32 * XA_POLL_WORK_RATIO_MILLI + 500) / 1000;
    if c > u16::MAX as u32 {
        u16::MAX
    } else {
        c as u16
    }
}

/// What a frame an arts shout's polls load costs over the census's work of the
/// same pass ([`linked_frame_work`]), in thousandths. The census prices a pass's
/// own work, which under the polls reads below retail's: the work left between
/// the CD interrupt spans carries the exception exits too
/// ([`ARTS_SHOUT_POLL_WORK_RATIO_MILLI`]). Fitted over `--cd-trace` recordings of
/// four arts (Flying Knee, Tri-Somersault, Gala's Chain, Sonic Javelin), each
/// polled frame's cost over the engine's census work on the paired tick: the
/// films' medians read `3.06` to `3.38`, and over their `73` polled frames
/// `3.2` puts the frame's span on retail's vsync count for `35` where
/// [`XA_POLL_WORK_RATIO_MILLI`] puts `21`, short on `51` of them.
pub const ARTS_SHOUT_CENSUS_RATIO_MILLI: u32 = 3200;

/// What a frame of census `work` hblanks costs under an arts shout's polls
/// ([`ARTS_SHOUT_CENSUS_RATIO_MILLI`]).
pub const fn arts_shout_census_cost(work: u16) -> u16 {
    let c = (work as u32 * ARTS_SHOUT_CENSUS_RATIO_MILLI + 500) / 1000;
    if c > u16::MAX as u32 {
        u16::MAX
    } else {
        c as u16
    }
}

/// What the pass after a clip's cue costs over its base: Amethyst's Heal's
/// `507` against the lone member's `0x3E` frames near `385`.
pub const XA_STREAM_ONSET_COST: u16 = 122;

/// Hblanks from the opening of an arts shout's pass to the clip's first poll
/// on PCSX-Redux when the drive already stands on the clip's file. The battle
/// seek-ahead selector (`FUN_8004DA00`) parks the head on the acting member's
/// long bank `(char - 1) * 2` (`0x8004DAFC`) for an Attack whenever the drive
/// is idle, so a shout from that bank (`FUN_8004C140` -> `FUN_8003D53C`) seeks
/// nowhere: `--cd-trace` recordings of Frost Breath, Dragon Fangs' third line,
/// Tri-Somersault's third and Rogue Flame issue the `CdlSeekL` `35k..50k`
/// cycles into the frame and `CdlReadS` `21k` after it, so the polls load the
/// shout's own pass (Frost Breath's runs `8` vsyncs).
pub const ARTS_SHOUT_PARKED_POLL_HBLANKS: u32 = 31;

/// Hblanks from the opening of an arts shout's pass to the clip's first poll
/// on PCSX-Redux when the clip lies in another file than the parked bank:
/// PCSX-Redux answers the seek in `1827k` cycles whatever its distance (every
/// such seek of the arts and enemy `--cd-trace` recordings), issued `255k..320k`
/// cycles into the frame. The shout's own pass runs out unloaded and the
/// polls open part way into the next (Dragon Fangs' first line: `CdlReadS`
/// `0.66` vsyncs into the pass after the cue, which runs `7` vsyncs).
pub const ARTS_SHOUT_SEEK_POLL_HBLANKS: u32 = 983;

/// Vsyncs an arts shout's polls run past its span, counted from its
/// `CdlReadS`: the stop watch `FUN_8003D764` polls `GetlocL` / `GetStat` until
/// the head passes the end sector the player set `span * 150 / 60` sectors on
/// (`0x8003D698..0x8003D6D8`), then issues the `Pause`. `--cd-trace`
/// recordings of Gala's Chain (`jou_gala_chain_open`, span `48`) and Sonic
/// Javelin (`ropeway_noa_sonic_javelin_open`, span `44`) read `62.3` and
/// `58.2` vsyncs from `CdlReadS` to `Pause`, both `14` past the span; the
/// seek's `3.24` vsyncs come before the read. [`XA_READ_TAIL_VSYNCS`] is the
/// drive word's later clear, counted from the cue.
pub const ARTS_SHOUT_READ_TAIL_VSYNCS: u16 = 14;

/// What a frame an arts shout's polls load costs over the work of the same
/// pass unloaded, in thousandths. The same two recordings' fully polled
/// frames run `1851..1930` hblanks (eight vsyncs) where the passes beside the
/// line cost `624..671`: `2.85` to `3.04`, median `2.97`.
/// [`XA_POLL_WORK_RATIO_MILLI`] reads lower because it divides by the cost
/// less the CD interrupt spans, which here is `511..577`, under the
/// unloaded passes' own work: the exception exits are the poll's too.
pub const ARTS_SHOUT_POLL_WORK_RATIO_MILLI: u32 = 2970;

/// What a frame of `work` hblanks costs under an arts shout's polls
/// ([`ARTS_SHOUT_POLL_WORK_RATIO_MILLI`]).
pub const fn arts_shout_polled_work_cost(work: u16) -> u16 {
    let c = (work as u32 * ARTS_SHOUT_POLL_WORK_RATIO_MILLI + 500) / 1000;
    if c > u16::MAX as u32 {
        u16::MAX
    } else {
        c as u16
    }
}

/// What a frame of `work` hblanks costs when the clip's polls open `wait`
/// hblanks into it, `polled` pricing the work they load: the work before the
/// first poll runs at full speed, the rest under the polls' share.
pub fn xa_partly_polled_cost(wait: u32, work: u16, polled: impl Fn(u16) -> u16) -> u16 {
    match u16::try_from(wait) {
        Ok(w) if w < work => w.saturating_add(polled(work - w)),
        _ => work,
    }
}

/// What a frame the polls load from its opening costs when the read ends
/// `left` vsyncs into it: the stop watch's sequencer `FUN_8003D764` issues the
/// `Pause` once the head passes the end sector, and the frame's work past that
/// point runs at full speed. `loaded` is the frame's cost polled throughout,
/// `work` its own work; the share of `work` done inside the first `left`
/// vsyncs is `left / loaded` of it.
pub fn xa_poll_tail_cost(left: u32, work: u16, loaded: u16) -> u16 {
    let left = left * HBLANKS_PER_VSYNC;
    if loaded <= work || u32::from(loaded) <= left {
        return loaded;
    }
    let done = left * u32::from(work) / u32::from(loaded);
    (left + (u32::from(work) - done)).min(u32::from(u16::MAX)) as u16
}

/// Hblanks one after-image ghost adds to a frame: the draw tick's walk
/// `FUN_80049348` re-runs the body's whole object draw `FUN_80048A08` per
/// ghost. Read off `battle_vell_nighto_49_strike`'s recording, whose two
/// ghosts raise a lone member's `0x20` frame by `37` and `35` on the frames
/// they appear, before any effect part is live.
pub const GHOST_DRAW_COST: u16 = 36;

/// Hblanks one after-image ghost of a party member adds to a frame. PCSX-Redux
/// recordings of the three Super Arts on a full party
/// (`battle_jou_party_*_super_open`, `--ghost-trace`) hold five frames that
/// draw one ghost beside frames that draw none, outside any clip's polls:
/// Vahn's cost `687` and `685` over `596`..`608`, Gala's `535` and `537` over
/// `442`, Noa's `716` over `620`: `85` to `96` a ghost whoever draws it, and
/// Gala's body, the lightest of the three by triangles, costs the most. So a
/// flat price, not the mesh's: two ghosts carry a full party's `0x1E` frame
/// over `0x2D0` (step `4`) and one leaves it under (step `3`), as the frame
/// that draws the slow motion's first ghost alone reads (`692`, then a
/// step-`3` pass). One emulated machine's measurement.
pub const PARTY_GHOST_DRAW_COST: u16 = 90;

/// Hblanks one live move-VM effect part adds to a frame (its tick
/// `FUN_80021DF4` and its sprite or mesh draw). Read off the lone-member strike
/// recordings: each hit's spark burst of about sixteen parts raises
/// `battle_dolk_drake_ghost_strike`'s frames from `390` to `424`, and the
/// cost falls back as the burst's parts retire.
pub const EFFECT_PART_COST: u16 = 2;

/// What a frame's draw load adds over [`measured_frame_cost`]: `parts` live
/// effect parts and `ghosts` after-image ghosts. The base rows of the strike
/// states `0x1E..=0x20` are the recordings' frames with no part live, so a
/// strike's burst and a special clip's ghosts are what lift its frames.
pub const fn frame_load_cost(parts: u16, ghosts: u16) -> u16 {
    parts
        .saturating_mul(EFFECT_PART_COST)
        .saturating_add(ghosts.saturating_mul(GHOST_DRAW_COST))
}

/// Hblanks one triangle of a body costs each time the draw tick re-runs it
/// for an after-image ghost, in thousandths. The ghost walk `FUN_80049348`
/// calls the body's whole object draw `FUN_80048A08` once per ghost
/// (`jal 0x80048A08` at `0x8004950C`, inside the loop `0x80049458..0x80049544`),
/// so what a ghost costs is the body's polygon load: Caruban's two ghosts
/// (`842` triangles) lift a lone Vahn's breath frames from `348` to `427` and
/// `504` on the two frames they appear, `78` each (`.092` a triangle), and the
/// strike recordings read `.087` on Caruban and `.081` on Frog (`608`). One
/// emulated machine's measurement, like the base table.
pub const GHOST_TRIANGLE_COST_MILLI: u32 = 92;

/// Hblanks one live child of the effect pool `_DAT_8007BD30` adds to a frame
/// (the 2D sprites the effect VM walks and draws: a breath's flames, a
/// burning body's puffs), in thousandths. Caruban's Fire Breath on a lone
/// Vahn runs its pool from `0` to `50` children with the body and its ghosts
/// unchanged, and its frames' cost follows at `1.6` a child (`R^2` `.71`).
/// The action-state table's price, which counts no link of the child's; the
/// census line prices the walker's branches instead ([`effect_walk_cost`]).
pub const EFFECT_POOL_CHILD_COST_MILLI: u32 = 1600;

/// CPU cycles in one hblank on PCSX-Redux's clock: `3413` video cycles a
/// scanline, `3413 * 7 / 11` CPU cycles (psx-spx).
pub const CYCLES_PER_HBLANK: u32 = 2172;

/// What the effect walker `FUN_801E0080` spends on each branch it takes, in
/// PCSX-Redux CPU cycles (two an instruction, one more a load or store, the
/// GTE free). Counted, not fitted to the films: the retail walker run in
/// `retail-exec` over `3600` random walks at steps `1..=4`, the projector
/// `FUN_800195A8` and the ordering-table link `FUN_8003D2C4` included, against
/// the port's pool counting the branches each walk took
/// (`legaia_engine_vm::effect_vm::WalkWork`), fits to `1.2` hblanks rms
/// (`the_walker_costs_its_branches`). The branches:
///
/// - a **sweep** of the first pass, which visits all `32` master and `128`
///   child slots whatever they hold (`0x801E0104..0x801E06CC`), once per vsync
///   of the frame step while anything is live;
/// - an active **master** visit, a **spawn** record consumed, a **child** on
///   its countdown branch (with its motion step), a **frame advance** (with
///   its motion step);
/// - an **overrun** advance: retiring a child zeroes its wait and count but
///   the advance loop tests the wait alone, so retail steps on through the
///   `efect.dat` bytes after the batch until a delay byte reads non-zero
///   (`EffectCatalog::anim_overrun`); the port does not step them, but they
///   cost the frame;
/// - a **drawn** child of the second pass: one `POLY_FT4` built, projected
///   and linked a child, no cull (`0x801E06D8..0x801E09C0`).
pub const EFFECT_WALK_SWEEP_CYCLES: u32 = 4243;
/// See [`EFFECT_WALK_SWEEP_CYCLES`].
pub const EFFECT_WALK_MASTER_CYCLES: u32 = 108;
/// See [`EFFECT_WALK_SWEEP_CYCLES`].
pub const EFFECT_WALK_SPAWN_CYCLES: u32 = 179;
/// See [`EFFECT_WALK_SWEEP_CYCLES`].
pub const EFFECT_WALK_CHILD_WAIT_CYCLES: u32 = 157;
/// See [`EFFECT_WALK_SWEEP_CYCLES`].
pub const EFFECT_WALK_ADVANCE_CYCLES: u32 = 166;
/// See [`EFFECT_WALK_SWEEP_CYCLES`].
pub const EFFECT_WALK_OVERRUN_CYCLES: u32 = 149;
/// See [`EFFECT_WALK_SWEEP_CYCLES`].
pub const EFFECT_WALK_DRAWN_CYCLES: u32 = 652;

/// The branches one battle frame's walks took, as [`effect_walk_cost`]
/// prices them.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct EffectWalkCounts {
    pub live_sweeps: u32,
    pub masters: u32,
    pub spawns: u32,
    pub child_waits: u32,
    pub advances: u32,
    pub overruns: u32,
    pub drawn: u32,
}

/// What one battle frame's effect walk costs over the census line, in
/// hblanks. An idle pool's walk (one call, one early-out sweep) is in the
/// line's base already, so the first live sweep is too; and the census
/// prices each drawn child's linked `POLY_FT4` at [`LINKED_WORK_MILLI`], so a
/// drawn child adds only its second-pass work over that.
pub const fn effect_walk_cost(c: EffectWalkCounts) -> u16 {
    let sweeps = c.live_sweeps.saturating_sub(1);
    let link = LINKED_WORK_MILLI * CYCLES_PER_HBLANK / 1000;
    let drawn = EFFECT_WALK_DRAWN_CYCLES.saturating_sub(link);
    let cycles = sweeps as u64 * EFFECT_WALK_SWEEP_CYCLES as u64
        + c.masters as u64 * EFFECT_WALK_MASTER_CYCLES as u64
        + c.spawns as u64 * EFFECT_WALK_SPAWN_CYCLES as u64
        + c.child_waits as u64 * EFFECT_WALK_CHILD_WAIT_CYCLES as u64
        + c.advances as u64 * EFFECT_WALK_ADVANCE_CYCLES as u64
        + c.overruns as u64 * EFFECT_WALK_OVERRUN_CYCLES as u64
        + c.drawn as u64 * drawn as u64;
    let h = (cycles + CYCLES_PER_HBLANK as u64 / 2) / CYCLES_PER_HBLANK as u64;
    if h > u16::MAX as u64 {
        u16::MAX
    } else {
        h as u16
    }
}

/// What one after-image ghost of a body of `triangles` costs a frame
/// ([`GHOST_TRIANGLE_COST_MILLI`]); [`GHOST_DRAW_COST`] for a body whose
/// polygon count the frame does not know. A monster's price: a party
/// member's ghost is [`PARTY_GHOST_DRAW_COST`].
pub const fn ghost_draw_cost(triangles: Option<u16>) -> u16 {
    match triangles {
        Some(t) => {
            let c = (t as u32 * GHOST_TRIANGLE_COST_MILLI + 500) / 1000;
            if c > u16::MAX as u32 {
                u16::MAX
            } else {
                c as u16
            }
        }
        None => GHOST_DRAW_COST,
    }
}

/// What `children` live effect-pool children cost a frame
/// ([`EFFECT_POOL_CHILD_COST_MILLI`]).
pub const fn effect_pool_cost(children: u16) -> u16 {
    let c = (children as u32 * EFFECT_POOL_CHILD_COST_MILLI + 500) / 1000;
    if c > u16::MAX as u32 {
        u16::MAX
    } else {
        c as u16
    }
}

/// The cost a frame an arm table names its step for leaves in the ring, so the
/// frames after a whole-screen arm read what its cost left there: the mid-rung
/// of that step.
pub const fn cost_of_step(step: u8) -> u16 {
    match step {
        0 | 1 => 200,
        2 => 400,
        3 => 600,
        _ => 0x2D1,
    }
}

/// The step a full fight of this party settles on, for a caller with no action
/// state to read.
pub const fn steady_battle_frame_step(party_count: u8) -> u8 {
    step_of_cost(measured_frame_cost(party_count, 0xFF))
}

/// Groups display ticks into battle frames of [`Self::step`] vsyncs.
///
/// The default clock is `display_frames / 2`; a step change starts a fresh
/// frame on the tick it is made and counts from there.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BattleFrameClock {
    /// `DAT_1F800393`, the step a frame's counters and clips advance by.
    pub step: u8,
    /// Vsyncs a frame runs past [`Self::step`]: an overrun. Retail's frame
    /// reads its step from the ring and spans what its work costs, so a
    /// frame the ring clamps at step `4` runs on to the next vsync after
    /// its cost.
    pub span_extra: u8,
    /// The display tick the current step took effect on.
    origin_tick: u64,
    /// The battle frame id at [`Self::origin_tick`].
    origin_frame: u64,
}

impl Default for BattleFrameClock {
    fn default() -> Self {
        Self {
            step: DEFAULT_BATTLE_FRAME_STEP,
            span_extra: 0,
            origin_tick: 0,
            origin_frame: 0,
        }
    }
}

impl BattleFrameClock {
    /// The battle frame display tick `tick` falls in.
    pub fn frame_of(&self, tick: u64) -> u64 {
        self.origin_frame + tick.saturating_sub(self.origin_tick) / self.span()
    }

    /// The vsyncs one frame spans: the step, plus any overrun.
    pub fn span(&self) -> u64 {
        u64::from(self.step.max(1)) + u64::from(self.span_extra)
    }

    /// Run the frame `tick` opens with an overrun of `extra` vsyncs,
    /// keeping its id and step.
    pub fn retime_span(&mut self, tick: u64, extra: u8) {
        if extra == self.span_extra {
            return;
        }
        let id = self.frame_of(tick);
        *self = Self {
            step: self.step,
            span_extra: extra,
            origin_tick: tick,
            origin_frame: id,
        };
    }

    /// Re-time the frame `tick` opens to `step` vsyncs, keeping its id: the
    /// pass that ran on `tick` belongs to a frame whose length retail
    /// measures only at its end (`FUN_80016B6C` samples the frame's cost at
    /// the frame-end pass and waits `VSync(step)` before the next pass), so
    /// a step taken on the pass's own tick spans that pass's frame. A new
    /// id here would open a second frame on the same tick, and the next
    /// tick would claim it for a second pass.
    pub fn retime_frame(&mut self, tick: u64, step: u8) {
        let step = step.max(1);
        if step == self.step {
            return;
        }
        let id = self.frame_of(tick);
        *self = Self {
            step,
            span_extra: self.span_extra,
            origin_tick: tick,
            origin_frame: id,
        };
    }

    /// Run frames of `step` vsyncs from display tick `tick` on, the first one
    /// opening on `tick`.
    pub fn set_step(&mut self, tick: u64, step: u8) {
        let step = step.max(1);
        if step == self.step {
            return;
        }
        let next = self.frame_of(tick) + 1;
        *self = Self {
            step,
            span_extra: self.span_extra,
            origin_tick: tick,
            origin_frame: next,
        };
    }
}

impl BattleFrameClock {
    /// How many display ticks into its battle frame `tick` falls.
    pub fn ticks_into_frame(&self, tick: u64) -> u64 {
        tick.saturating_sub(self.origin_tick) % self.span()
    }
}

/// A retail recording's frame steps, per action-SM key, for a film that wants
/// the frame's cost taken out of what it scores (`--retail-steps` in
/// `scripts/ci/cast-sequence.py`). Retail's step is the emulated machine's
/// measure of each frame ([`crate::frame_clock`]), which the engine does not
/// model, so a recording's camera and timers walk on steps the engine's table
/// only samples. A harness seat: no play path installs one.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RetailStepTrack {
    /// `(action state, module phase)` to the step of each retail frame under it.
    frames: std::collections::BTreeMap<(u8, u8), Vec<u8>>,
    /// Passes the engine has run under each key since the film began.
    passes: std::collections::BTreeMap<(u8, u8), usize>,
    /// Whether the film has begun ([`Self::arm`]). The recording's frames
    /// start at its own first vsync, while the engine runs the whole fight up
    /// to it: the rounds a drive replays and, for a state held mid-cast, the
    /// cast's opening passes. Before the film begins the engine keeps its own
    /// step, so those passes neither spend the recording's frames nor run on
    /// them.
    armed: bool,
}

impl RetailStepTrack {
    /// Parse `state phase step` lines, one retail frame each, in film order.
    pub fn parse(text: &str) -> Self {
        let mut track = Self::default();
        for line in text.lines() {
            let f: Vec<u8> = line
                .split_whitespace()
                .filter_map(|w| w.parse().ok())
                .collect();
            if let [state, phase, step] = f[..] {
                track
                    .frames
                    .entry((state, phase))
                    .or_default()
                    .push(step.max(1));
            }
        }
        track
    }

    /// The step of the pass that opens after one just run under this key:
    /// retail's frame of the same ordinal among those opening on the key (a
    /// track line is the state a frame's pass opens on, which is the state
    /// the pass before it left), its last one past that, `None` for a key
    /// the film never saw. The step a pass leaves is the next pass's, which
    /// both drains by it and spans it, as retail's frame does.
    pub fn next_step(&mut self, state: u8, phase: u8) -> Option<u8> {
        if !self.armed {
            return None;
        }
        let frames = self.frames.get(&(state, phase))?;
        let n = self.passes.entry((state, phase)).or_default();
        let step = frames.get(*n).or(frames.last()).copied();
        *n += 1;
        step
    }

    /// The film began on a pass the engine has just run, which left the
    /// action SM under `(state, phase)`: the recording's first frame opens
    /// on that state, so its step is the next pass's, returned here for the
    /// caller to hand on. Every key counts from there.
    pub fn arm(&mut self, state: u8, phase: u8) -> Option<u8> {
        self.passes.clear();
        self.armed = true;
        self.next_step(state, phase)
    }

    /// Whether the film has begun.
    pub fn armed(&self) -> bool {
        self.armed
    }
}

/// A retail recording's `rand()` seed word going into each battle frame, per
/// action-SM key, for a film whose arms draw on every pass (`--rand-track` in
/// `scripts/ci/cast-sequence.py`). PROT 0935's Earthquake bursts a rock pair
/// a frame off `rand()`, and retail's other passes make tens of draws the
/// engine does not, so one held seed (`LEGAIA_CAST_RAND_SEED`) deals the first
/// pair and leaves the rest on the engine's own stream. The track hands the
/// frame driver's pass, ahead of its discarded draw (`0x80046D2C`), the word
/// retail's generator held at the end of the frame before. A harness seat: no
/// play path installs one.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RetailRandTrack {
    /// `(action state, module phase)` to the word each retail frame under it
    /// ended on, in film order.
    words: std::collections::BTreeMap<(u8, u8), Vec<u32>>,
    /// Passes the engine has opened under each key since the band began.
    passes: std::collections::BTreeMap<(u8, u8), usize>,
    /// Whether the last pass stood in a module band.
    in_band: bool,
}

impl RetailRandTrack {
    /// Parse `state phase word` lines, one retail frame each, in film order.
    pub fn parse(text: &str) -> Self {
        let mut track = Self::default();
        for line in text.lines() {
            let f: Vec<u32> = line
                .split_whitespace()
                .filter_map(|w| w.parse().ok())
                .collect();
            if let [state, phase, word] = f[..]
                && let (Ok(state), Ok(phase)) = (u8::try_from(state), u8::try_from(phase))
            {
                track.words.entry((state, phase)).or_default().push(word);
            }
        }
        track
    }

    /// The word to seat for the pass that opens under this key: retail's
    /// frame of the same ordinal under it, `None` past the recording's last
    /// or for a key it never saw (the engine's own stream runs on).
    pub fn next_word(&mut self, state: u8, phase: u8) -> Option<u32> {
        let words = self.words.get(&(state, phase))?;
        let n = self.passes.entry((state, phase)).or_default();
        let word = words.get(*n).copied();
        *n += 1;
        word
    }

    /// Start every key's count again: a new cast band opened, and the
    /// recording's frames are the last band's.
    pub fn restart(&mut self) {
        self.passes.clear();
    }

    /// The word for a pass opening under `(state, phase)`, the phase counted
    /// only inside a module band. The track covers the bands alone: a drive
    /// that replays rounds passes through other states the recording never
    /// held, and a band entered afresh restarts the counts.
    pub fn pass_word(&mut self, state: u8, phase: u8, band: bool) -> Option<u32> {
        if band && !self.in_band {
            self.restart();
        }
        self.in_band = band;
        band.then(|| self.next_word(state, phase)).flatten()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Frost Breath on a full party: the shout from Noa's parked bank loads
    /// its own pass (retail `8` vsyncs), where a seek leaves it to its step
    /// and opens the polls part way into the next (Dragon Fangs' first line:
    /// its cue pass runs `3`, the next `7`).
    #[test]
    fn an_arts_shout_loads_from_where_its_seek_ends() {
        let polled = xa_polled_work_cost;
        let parked = xa_partly_polled_cost(ARTS_SHOUT_PARKED_POLL_HBLANKS, 542, polled);
        assert!(frame_span(parked, 3) >= 6, "parked cue pass {parked}");
        let seek = xa_partly_polled_cost(ARTS_SHOUT_SEEK_POLL_HBLANKS, 600, polled);
        assert_eq!(seek, 600, "the seek outlasts the cue pass's work");
        assert_eq!(frame_span(seek, 3), 3);
        let next = ARTS_SHOUT_SEEK_POLL_HBLANKS - 3 * HBLANKS_PER_VSYNC;
        let onset = xa_partly_polled_cost(next, 620, polled);
        assert!(
            frame_span(onset, 3) >= 6,
            "pass after the seek's cue {onset}"
        );
        assert_eq!(xa_partly_polled_cost(0, 620, polled), polled(620));
    }

    #[test]
    fn the_step_track_counts_from_the_films_first_pass() {
        let mut t = RetailStepTrack::parse("112 1 3\n112 1 4\n112 1 4\n81 0 2\n");
        // The rounds and the cast's opening passes before the film run on
        // the engine's own step and spend nothing.
        assert_eq!(t.next_step(81, 0), None);
        assert_eq!(t.next_step(112, 1), None);
        // The film opens on a pass that left 112:1: the recording's first
        // frame opens there, and its step is the next pass's.
        assert_eq!(t.arm(112, 1), Some(3));
        assert_eq!(t.next_step(112, 1), Some(4));
        assert_eq!(t.next_step(112, 1), Some(4));
        assert_eq!(t.next_step(112, 1), Some(4));
        assert_eq!(t.next_step(81, 0), Some(2));
    }

    #[test]
    fn the_rand_track_hands_out_each_keys_words_in_order() {
        let mut t = RetailRandTrack::parse("112 4 10\n112 4 11\n112 5 20\nbad line\n");
        assert_eq!(t.next_word(112, 4), Some(10));
        assert_eq!(t.next_word(112, 5), Some(20));
        assert_eq!(t.next_word(112, 4), Some(11));
        assert_eq!(t.next_word(112, 4), None);
        assert_eq!(t.next_word(112, 3), None);
        t.restart();
        assert_eq!(t.next_word(112, 4), Some(10));
        // Outside a band nothing is seated; a band entered afresh counts again.
        assert_eq!(t.pass_word(0x51, 0, false), None);
        assert_eq!(t.pass_word(112, 4, true), Some(10));
        assert_eq!(t.pass_word(112, 4, true), Some(11));
    }

    #[test]
    fn ticks_into_frame_counts_from_the_step_origin() {
        let mut c = BattleFrameClock::default();
        assert_eq!(
            (10..14).map(|t| c.ticks_into_frame(t)).collect::<Vec<_>>(),
            [0, 1, 0, 1]
        );
        c.set_step(7, 3);
        assert_eq!(
            (7..11).map(|t| c.ticks_into_frame(t)).collect::<Vec<_>>(),
            [0, 1, 2, 0]
        );
    }

    /// A step retimed on the tick a pass ran keeps that pass's frame: the
    /// next tick is still inside it, so no second pass opens there.
    #[test]
    fn a_retimed_frame_keeps_its_id() {
        let mut c = BattleFrameClock::default();
        let id = c.frame_of(10);
        c.retime_frame(10, 3);
        assert_eq!(c.frame_of(10), id);
        assert_eq!(c.frame_of(11), id);
        assert_eq!(c.frame_of(12), id);
        assert_eq!(c.frame_of(13), id + 1);
        assert_eq!(c.ticks_into_frame(13), 0);
    }

    #[test]
    fn the_default_clock_is_two_vsync_frames() {
        let c = BattleFrameClock::default();
        for t in 0..20u64 {
            assert_eq!(c.frame_of(t), t / 2);
        }
    }

    #[test]
    fn a_step_change_opens_a_fresh_frame_and_counts_on() {
        let mut c = BattleFrameClock::default();
        // Tick 7 is in frame 3; frame 4 opens on tick 7 at step 3.
        c.set_step(7, 3);
        assert_eq!(c.frame_of(7), 4);
        assert_eq!(c.frame_of(9), 4);
        assert_eq!(c.frame_of(10), 5);
        // Ids never run backwards across a change.
        c.set_step(11, 2);
        assert_eq!(c.frame_of(11), 6);
        assert_eq!(c.frame_of(13), 7);
    }
}

#[cfg(test)]
mod frame_cost_tests {
    use super::*;

    /// A command surface's packets price over the HUD the census base holds:
    /// the item window's `518` against the bare HUD's `76` is the `54`
    /// hblanks `retail-exec battle-cost` reads between the template tick's
    /// cycles on `battle_item_window_gaza2_retail` and on
    /// `battle_rugi_barra_strike` (`137893` against `18816`).
    #[test]
    fn a_surfaces_packets_price_over_the_bare_hud() {
        assert_eq!(surface_frame_work(0x00), 0);
        assert_eq!(surface_frame_work(0xFF), 0);
        assert_eq!(surface_frame_work(0x3C), 54);
        assert_eq!(surface_frame_work(0x1E), 10);
        assert_eq!(surface_frame_work(0x50), 25);
        assert_eq!(
            surface_frame_work(0x3C),
            ((137_893u32 - 18_816) / 2172) as u16
        );
    }

    /// A ghost costs its body's polygon load: Caruban's two ghosts take a
    /// lone Vahn's breath frame (`0x2B`) to the step-`3` edge, and the
    /// breath's first pool children carry it over; an unknown body keeps the
    /// flat walk cost.
    #[test]
    fn a_frames_work_follows_its_polygons_and_the_polls_multiply_it() {
        assert_eq!(polygon_frame_work(0), POLY_FRAME_BASE);
        assert_eq!(polygon_frame_work(1000), 43 + 184);
        // Tail Flare's flight (`3884` linked) reads retail's step `4`.
        assert_eq!(step_of_cost(polygon_frame_work(3884)), 4);
        // Kiss of Death's arm 4 under its line: an eight-vsync frame.
        assert_eq!(frame_span(xa_polled_work_cost(665), 4), 8);
        // An arts shout's polled frames over the census's work: Sonic
        // Javelin's `600` spans retail's eight vsyncs, Tri-Somersault's first
        // line's `666` its nine.
        assert_eq!(frame_span(arts_shout_census_cost(600), 4), 8);
        assert_eq!(frame_span(arts_shout_census_cost(666), 4), 9);
        assert_eq!(polygon_frame_work(u32::MAX), u16::MAX);
    }

    #[test]
    fn a_ghost_costs_its_bodys_triangles() {
        assert_eq!(ghost_draw_cost(Some(842)), 77);
        assert_eq!(ghost_draw_cost(Some(608)), 56);
        assert_eq!(ghost_draw_cost(None), GHOST_DRAW_COST);
        let breath = measured_frame_cost(1, 0x2B) + 2 * ghost_draw_cost(Some(842));
        assert_eq!(step_of_cost(breath), 2);
        assert_eq!(step_of_cost(breath + effect_pool_cost(2)), 3);
        assert_eq!(effect_pool_cost(50), 80);
    }

    #[test]
    fn the_ring_holds_a_costly_frame_for_sixteen_passes() {
        let mut ring = FrameCostRing::default();
        assert_eq!(ring.push(400), 2);
        assert_eq!(ring.push(600), 3);
        for _ in 0..14 {
            assert_eq!(ring.push(400), 3);
        }
        assert_eq!(
            ring.push(400),
            3,
            "the costly entry is one write from its slot"
        );
        assert_eq!(ring.push(400), 2);
    }

    #[test]
    fn a_frame_past_the_clamp_reads_step_four_and_stores_the_clamp() {
        let mut ring = FrameCostRing::default();
        assert_eq!(ring.push(0x2D1), 4);
        assert_eq!(ring.step(), 4);
        assert_eq!(ring.push(400), 3, "the clamp `0x2BC` reads step 3 after it");
    }

    #[test]
    fn a_raw_cost_on_the_clamps_edge_reads_step_three() {
        let mut ring = FrameCostRing::default();
        assert_eq!(ring.push(0x2D0), 3, "the raw cost is compared, under 0x2D1");
        assert_eq!(ring.newest(), 0x2BC);
        assert_eq!(
            ring.step(),
            3,
            "the stored step, not a reading of the clamp"
        );
        assert_eq!(ring.push(0x2BC), 3, "a raw 0x2BC is stored as it is");
        assert_eq!(ring.step(), 3);
    }

    #[test]
    fn the_frame_waits_on_the_step_before_it() {
        let mut ring = FrameCostRing::default();
        assert_eq!(ring.frame_end(600, FramePace::ADAPTIVE).vsync_wait, 0);
        assert_eq!(ring.frame_end(100, FramePace::ADAPTIVE).vsync_wait, 3);
        let forced = FramePace {
            forced: 2,
            ..FramePace::ADAPTIVE
        };
        let cursor = ring.cursor();
        assert_eq!(
            ring.frame_end(100, forced),
            FrameEnd {
                step: 2,
                vsync_wait: 2
            }
        );
        assert_eq!(ring.cursor(), cursor, "a forced step stores no cost");
    }

    #[test]
    fn an_overrun_stretches_the_frame_and_leaves_the_step() {
        let mut clock = BattleFrameClock {
            step: 4,
            ..BattleFrameClock::default()
        };
        clock.retime_span(100, XA_STREAM_SPAN_EXTRA);
        assert_eq!(clock.step, 4, "the counters advance by the step");
        let id = clock.frame_of(100);
        assert_eq!(clock.frame_of(104), id, "frame spans the step + 1");
        assert_eq!(clock.frame_of(105), id + 1);
    }

    /// The spans the Flying Knee `--cd-trace` recording and the strike
    /// recordings read: `VSync(n)` runs a frame past its step to the first
    /// blank after its cost.
    #[test]
    fn a_frame_spans_the_larger_of_its_step_and_its_cost() {
        assert_eq!(frame_span(630, 3), 3);
        assert_eq!(frame_span(750, 3), 3);
        assert_eq!(frame_span(796, 3), 4);
        assert_eq!(frame_span(707, 4), 4);
        assert_eq!(frame_span(1120, 4), 5);
        assert_eq!(frame_span(1952, 4), 8);
        assert_eq!(frame_span(2113, 4), 9);
        assert_eq!(frame_span(2584, 4), 10);
        assert_eq!(frame_span(200, 0), 1);
        assert_eq!(frame_span(250, 1), 2);
        assert_eq!(XA_STREAM_SPAN_EXTRA, 1);
    }

    #[test]
    fn a_loaded_frame_costs_its_work_over_the_polls_share() {
        assert_eq!(xa_loaded_frame_cost(282), XA_STREAM_FRAME_COST);
        // A full party's heal: eight- and nine-vsync frames over the table's `595`.
        assert_eq!(frame_span(xa_loaded_frame_cost(595), 4), 8);
        // A lone member's: five over `345`.
        assert_eq!(frame_span(xa_loaded_frame_cost(345), 4), 5);
        // The player arts' typical eight over some `630`.
        assert_eq!(frame_span(xa_loaded_frame_cost(630), 4), 8);
        assert_eq!(
            xa_stream_pass_cost(5, 630, CdTiming::PcsxRedux, true),
            xa_loaded_frame_cost(630)
        );
    }

    #[test]
    fn a_full_partys_fight_settles_on_three_and_a_lone_members_on_two() {
        assert_eq!(steady_battle_frame_step(3), 3);
        assert_eq!(steady_battle_frame_step(2), 2);
        assert_eq!(steady_battle_frame_step(1), 2);
    }
}
