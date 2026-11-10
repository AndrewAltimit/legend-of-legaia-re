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

/// The engine's battle frame step: retail's step on an ordinary battle frame,
/// the one the camera and the root-motion carry assume by default.
pub const DEFAULT_BATTLE_FRAME_STEP: u8 = 2;

/// Port of the frame driver's step law (`FUN_80016B6C`): the sixteen-entry
/// history `0x80084098` and its maximum against `0xF1` / `0x1FF` / `0x2D1`
/// (`0x80017098..0x8001715C`). What feeds it differs. Retail stores the hblanks
/// since the last vsync once the CPU has finished the frame and `DrawSync(0)`
/// has returned (`FUN_800173BC`), so the entry is `max(cpu, gpu of the last
/// table)`; the engine has no such clock and pushes [`measured_frame_cost`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FrameCostRing {
    entries: [u16; 16],
    cursor: u8,
}

impl FrameCostRing {
    /// A ring holding these sixteen entries with the write cursor at
    /// `cursor` (`gp+0x440`, the slot the next pass stores to), as a retail
    /// capture's `0x80084098` holds them.
    pub fn from_entries(entries: [u16; 16], cursor: u8) -> Self {
        Self {
            entries,
            cursor: cursor & 0xF,
        }
    }

    /// The newest entry: what the last pass's frame cost.
    pub fn newest(&self) -> u16 {
        self.entries[usize::from(self.cursor.wrapping_sub(1) & 0xF)]
    }

    /// The step the ring's entries read now, before another push: what the
    /// pass that last stored here left for the next one. A newest entry on
    /// the clamp `0x2BC` stands for a raw duration of `0x2D0` or more, which
    /// read step `4` (`0x800170F8..0x80017104`).
    pub fn step(&self) -> u8 {
        let newest = self.entries[usize::from(self.cursor.wrapping_sub(1) & 0xF)];
        if newest == 0x2BC {
            return 4;
        }
        step_of_cost(self.entries.iter().copied().max().unwrap_or(0))
    }

    /// Store one frame's cost (`li s0,0x2bc` clamps a frame of `0x2D0` or
    /// more) and read the step the next frame runs on.
    pub fn push(&mut self, cost: u16) -> u8 {
        let raw = cost;
        self.entries[usize::from(self.cursor & 0xF)] = if raw >= 0x2D0 { 0x2BC } else { raw };
        self.cursor = self.cursor.wrapping_add(1) & 0xF;
        let longest = self.entries.iter().copied().max().unwrap_or(0);
        // The newest raw duration stands in for the clamped entry
        // (`0x800170F8..0x80017104`), which is how a clamped frame reads step 4.
        let longest = if raw >= 0x2D0 { raw } else { longest };
        step_of_cost(longest)
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
/// model of a frame's work: Mednafen reads a full party's action about `100` lower,
/// with the GPU wait in it, and no real hardware has been measured.
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
/// emulated machine's measurement, like the table above.
pub const XA_STREAM_FRAME_COST: u16 = 1063;

/// The overrun of a frame the XA stream loads: the frame's raw cost past its
/// step-`4` span, in vsyncs. `1` for the item's and art's `1063`..`1200`
/// (`5`-vsync passes on the capture), which an item use's passes read
/// exactly; a Spirit's run longer (`6` and `7`), and this is a floor there.
pub const XA_STREAM_SPAN_EXTRA: u8 = 1;

/// Hblanks one after-image ghost adds to a frame: the draw tick's walk
/// `FUN_80049348` re-runs the body's whole object draw `FUN_80048A08` per
/// ghost. Read off `battle_vell_nighto_49_strike`'s recording, whose two
/// ghosts raise a lone member's `0x20` frame by `37` and `35` on the frames
/// they appear, before any effect part is live.
pub const GHOST_DRAW_COST: u16 = 36;

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
pub const EFFECT_POOL_CHILD_COST_MILLI: u32 = 1600;

/// What one after-image ghost of a body of `triangles` costs a frame
/// ([`GHOST_TRIANGLE_COST_MILLI`]); [`GHOST_DRAW_COST`] for a body whose
/// polygon count the frame does not know (the party's assembled forms).
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
    /// Passes the engine has run under each key.
    passes: std::collections::BTreeMap<(u8, u8), usize>,
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

    /// The step of the pass just run under this key: retail's frame of the
    /// same ordinal under the key, its last one past that, `None` for a key
    /// the film never saw.
    pub fn next_step(&mut self, state: u8, phase: u8) -> Option<u8> {
        let frames = self.frames.get(&(state, phase))?;
        let n = self.passes.entry((state, phase)).or_default();
        let step = frames.get(*n).or(frames.last()).copied();
        *n += 1;
        step
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

    /// A ghost costs its body's polygon load: Caruban's two ghosts take a
    /// lone Vahn's breath frame (`0x2B`) to the step-`3` edge, and the
    /// breath's first pool children carry it over; an unknown body keeps the
    /// flat walk cost.
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
        assert_eq!(ring.push(400), 3, "the clamp `0x2BC` reads step 3 after it");
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

    #[test]
    fn a_full_partys_fight_settles_on_three_and_a_lone_members_on_two() {
        assert_eq!(steady_battle_frame_step(3), 3);
        assert_eq!(steady_battle_frame_step(2), 2);
        assert_eq!(steady_battle_frame_step(1), 2);
    }
}
