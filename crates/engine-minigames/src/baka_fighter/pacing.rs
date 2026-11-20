//! The cabinet's frame pacing: how many vsyncs each Baka Fighter game frame
//! spans, and the frame step `DAT_1F800393` it runs on.
//!
//! Mode `0x19` runs the same master loop as the field and the battle, so its
//! frame step is the frame driver's measured one: `FUN_80016B6C` stores each
//! frame's hblanks (from the vertical blank to the CPU finishing) into the
//! sixteen-entry ring `0x80084098`, and the next frame's step is the ring's
//! longest entry against `0xF1` / `0x1FF` / `0x2D1` (`1` / `2` / `3`, else
//! `4`), raised to the floor `_DAT_8007B9D8` the cabinet stores (`1`, or `4`
//! through the secret tally). The frame then waits `VSync(step)` on the step
//! the frame before it stored, so it spans its step, or longer when its own
//! cost runs past it.
//!
//! A duel frame's own work is well under a vsync: the runner prices the
//! rung-one duel at `162` hblanks with the vblank interrupt, the ring stores
//! `165`. What lengthens the cabinet's frames is the announcer. Every line
//! goes through the clip player `FUN_8003D53C`, whose CdSync callback
//! `FUN_8003D764` polls the drive from interrupt context until the head
//! passes the clip's end sector, and on PCSX-Redux those polls leave a frame
//! about a third of the CPU. A PCSX-Redux film of the cabinet with the ring and
//! the drive word `0x8007BC20` beside the cabinet state reads it frame by
//! frame: a duel frame stores `165` with the drive idle and `526..546` while a
//! line plays, the spin `146` and `435..500`, the NEXT GAME sheet `138` and
//! `437..456`. So the round's intro and the duel's first second run at step
//! `3`, and the finishing special's line at `4`.
//!
//! [`BakaPacer`] runs that law over [`frame_work`]'s price of each frame and
//! the clip windows the cabinet's own lines open ([`BakaPacer::cue_xa`]).
//!
//! REF: FUN_80016B6C, FUN_8003D53C, FUN_8003D764, FUN_8001B964, FUN_801CF388, FUN_8005FB84

use crate::baka_cabinet as cab;

/// Hblanks a vsync spans: the NTSC field's `263` lines, the unit of the ring.
pub const HBLANKS_PER_VSYNC: u32 = 263;

/// Hblanks a frame spends past its ring entry before its `VSync(n)` can see
/// the next blank: the vblank handler and the `VSync` return that open the
/// frame and the ring update between the cost store and the call
/// (`0x80017044..0x800171C0`). The battle frame clock's figure, read off
/// PCSX-Redux's strike recordings.
pub const FRAME_END_HBLANKS: u32 = 20;

/// Which machine's CD response the announcer's polls run against.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CdPollTiming {
    /// A console's: `GetStat` answers in `0xC4E1` cycles on average (psx-spx),
    /// and the polls take about a sixteenth of the CPU
    /// ([`CONSOLE_POLL_COST_MILLI`]), so the cabinet's frames hold step `1`
    /// but for its two dear front-end frames.
    Console,
    /// PCSX-Redux's: every command answers `0x800` cycles after it
    /// (`cdrom.cc`, `AddIrqQueue(m_cmd, 0x800)`), and a frame under the polls
    /// costs [`PCSX_POLL_COST_MILLI`] of its work. The films and stills are
    /// recorded on it; the default, as the battle's (`CdTiming::PcsxRedux`).
    #[default]
    PcsxRedux,
}

/// What a frame the announcer's polls load costs on PCSX-Redux, in thousandths
/// of its own work: the ring entries a PCSX-Redux film of the cabinet stores
/// with the drive word `0x8007BC20` at `2` over those it stores with it idle,
/// on the same cabinet state (the duel `165` -> `526..546`, the round banner's
/// last state `166` -> `526..540`, the NEXT GAME sheet `138` -> `437..456`,
/// the spin `146` -> `435..500`). The battle's figure for the same stop watch
/// is `3100`, read on dearer frames.
pub const PCSX_POLL_COST_MILLI: u32 = 3220;

/// A console's: the stop watch's `65` per mille of the CPU
/// (`cast_module_frame_step::CONSOLE_POLL_SHARE_PER_MILLE`), so a frame costs
/// its work over the `935` the polls leave.
pub const CONSOLE_POLL_COST_MILLI: u32 = 1070;

/// Hblanks from the frame that cues a line to the drive's first streamed
/// sector, the seek the polls do not load: a film's frames go loaded about
/// `3.3` vsyncs after the cue (`0x8007BC20` reads `1` through the seek, then
/// `2`), the frame the polls open on costing between the two
/// (`383` for the FIGHT flash's, `334` for the KO line's).
pub const XA_SEEK_HBLANKS: u32 = 870;

/// Vsyncs the polls run past the clip's `dur` on PCSX-Redux: the clip player
/// polls until the head passes `dur * 150 / 60` sectors on, and the emulator
/// defers a sector while a command's interrupt is pending. The film's drive
/// word goes idle `17..20` vsyncs after `dur` from the cue (the round banner's
/// `0x48` at `89`, the FIGHT flash's `0x36` at `74`, the KO line's `0x45` at
/// `87`, NEXT GAME's `0x4E` at `95`). A console's drive streams on time.
pub const XA_READ_TAIL_VSYNCS: u32 = 18;

/// Hblanks of one afterimage pass. The special's ghost draws its owner's mesh
/// once more through the animated mesh renderer `FUN_8001B964`, which the
/// runner prices at `44` hblanks a fighter on the duel's opaque leaves
/// (`88.2` for the two, `retail-exec frame-cost` on
/// `minigame_baka_fighter_retail_fight`). The ghost's colour word
/// `0x81000000` takes the depth-cued leaves with the off-screen test instead,
/// whose linked gouraud primitives cost `331` / `281` cycles against the
/// opaque bank's `184` / `151` (`docs/tooling/retail-frame-cost.md`):
/// `44 * 612 / 335`, about `80` a pass. A film's frames after the KO cost `324..347`
/// with the special's ghosts up, `234` as they run out and `155` once they have.
pub const GHOST_PASS_WORK: u32 = 80;

/// What one game frame's own work costs, in PCSX-Redux hblanks with the vblank
/// interrupt: what the ring stores for the frame with the drive idle.
///
/// The duel's frames are priced by the frame runner (`retail-exec
/// frame-cost --interrupts` on `minigame_baka_fighter_retail_fight` and
/// `..._round2`, two cycles an instruction and one a data access): the cabinet
/// arm with the arena (`FUN_801CF388`, `55` hblanks with the camera settled,
/// `28..41` while it turns), the fighters' two mesh draws (`88`), the combat
/// tick (`13`), the HUD (`2.5`) and the loop around them, `162` settled and
/// `135..147` through the spin; the CD and DMA interrupts add about `3` to what
/// the ring stores. The front end's and the result screens' frames, which no
/// library state stands in, are the ring entries of a PCSX-Redux film of the
/// cabinet with the drive idle: the attract card `7` (the runner's too), the
/// lineup's live select `280`, the select's wipe and pose `64`, the round setup
/// `301`, the tally `119..163`, the NEXT GAME sheet `138`.
pub fn frame_work(state: u32, ghost_passes: u32) -> u32 {
    let base = match state {
        cab::ST_SELECT_SETUP => 10,
        cab::ST_SELECT | cab::ST_SELECT_DONE => 280,
        cab::ST_SELECT_WIPE | cab::ST_SELECT_POSE | cab::ST_OPPONENT_INSTALL => 64,
        s if cab::front_end(s) => 7,
        cab::ST_ROUND_SETUP => 301,
        cab::ST_ROUND_BANNER_A | cab::ST_ROUND_BANNER_B => 150,
        cab::ST_ROUND_BANNER_C => 146,
        cab::ST_PERFECT => 163,
        cab::ST_TALLY => 119,
        cab::ST_TALLY_OUT => 136,
        cab::ST_CHOICE | cab::ST_CHOICE_SECRET | cab::ST_NEXT_RUNG => 138,
        _ => 165,
    };
    base + ghost_passes * GHOST_PASS_WORK
}

/// The vsyncs a frame spans when it costs `raw` hblanks and waits on `step`:
/// `VSync(n)` waits out the step, then for the first blank after the frame's
/// work (`FUN_8005FB84`), the battle frame clock's `frame_span`.
pub const fn frame_span(raw: u32, step: u8) -> u32 {
    let need = (raw + FRAME_END_HBLANKS) / HBLANKS_PER_VSYNC + 1;
    let step = if step < 1 { 1 } else { step as u32 };
    if need > step { need } else { step }
}

/// The step a ring's longest entry reads (`0x80017124..0x8001715C`).
pub const fn step_of_cost(cost: u32) -> u8 {
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

/// The frame driver's step law over the cabinet's frames, with the clock the
/// announcer's polls are laid on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BakaPacer {
    ring: [u16; 16],
    cursor: u8,
    step: u8,
    /// Hblanks since the pacer started, at the start of the frame in flight.
    clock: u64,
    /// The announcer's polls: the hblank window of the clock they load.
    xa_load: Option<(u64, u64)>,
    timing: CdPollTiming,
}

impl Default for BakaPacer {
    fn default() -> Self {
        Self {
            ring: [0; 16],
            cursor: 0,
            step: 1,
            clock: 0,
            xa_load: None,
            timing: CdPollTiming::default(),
        }
    }
}

impl BakaPacer {
    /// The step the next frame runs on (`DAT_1F800393`).
    pub fn step(&self) -> u8 {
        self.step
    }

    /// The newest ring entry: what the last frame stored.
    pub fn newest(&self) -> u16 {
        self.ring[usize::from(self.cursor.wrapping_sub(1) & 0xF)]
    }

    pub fn timing(&self) -> CdPollTiming {
        self.timing
    }

    pub fn set_timing(&mut self, timing: CdPollTiming) {
        self.timing = timing;
    }

    /// Whether the announcer's polls load the clock at hblank `at`.
    pub fn xa_loading(&self, at: u64) -> bool {
        self.xa_load.is_some_and(|(a, b)| (a..b).contains(&at))
    }

    /// A line cued on the frame that starts the pacer's clock now, of `dur`
    /// vsyncs (`FUN_8003D53C`'s third argument). A new line replaces the one
    /// the drive was streaming, as the clip player re-seeks.
    pub fn cue_xa(&mut self, dur: u16) {
        let tail = match self.timing {
            CdPollTiming::PcsxRedux => XA_READ_TAIL_VSYNCS,
            CdPollTiming::Console => 0,
        };
        let start = self.clock + u64::from(XA_SEEK_HBLANKS);
        let end = self.clock + u64::from((u32::from(dur) + tail) * HBLANKS_PER_VSYNC);
        self.xa_load = (end > start).then_some((start, end));
    }

    /// What a frame of `work` hblanks starting on the pacer's clock costs:
    /// its work, run at the share of the CPU the polls leave wherever they
    /// load the clock.
    pub fn frame_cost(&self, work: u32) -> u32 {
        let milli = match self.timing {
            CdPollTiming::PcsxRedux => PCSX_POLL_COST_MILLI,
            CdPollTiming::Console => CONSOLE_POLL_COST_MILLI,
        };
        let Some((a, b)) = self.xa_load else {
            return work;
        };
        // Work in thousandths of an hblank, time in hblanks.
        let start = self.clock;
        let mut t = start;
        let mut w = u64::from(work) * 1000;
        if t < a {
            let run = (w / 1000).min(a - t);
            t += run;
            w -= run * 1000;
        }
        if w > 0 && t < b {
            let room = b - t;
            let need = (w * u64::from(milli)).div_ceil(1_000_000);
            if need <= room {
                t += need;
                w = 0;
            } else {
                t = b;
                w = w.saturating_sub(room * 1_000_000 / u64::from(milli));
            }
        }
        t += w.div_ceil(1000);
        (t - start) as u32
    }

    /// One frame's pacing tail: store what the frame cost and read the step
    /// the next frame runs on, raised to the cabinet's `floor`. Returns the
    /// vsyncs the frame spans, which moves the clock on to the next frame's
    /// start.
    ///
    /// The ring keeps a frame of `0x2D0` hblanks or more as `0x2BC`, and such
    /// a frame reads its step off its raw cost (`0x800170F8..0x80017104`).
    // PORT: FUN_80016B6C (`0x80017044..0x800171D8`, the frame-step law)
    pub fn end_frame(&mut self, raw: u32, floor: i32) -> u32 {
        let ran_on = self.step;
        let stored = if raw < 0x2D0 { raw } else { 0x2BC };
        self.cursor &= 0xF;
        self.ring[usize::from(self.cursor)] = stored as u16;
        self.cursor = (self.cursor + 1) & 0xF;
        let mut longest = self.ring.iter().copied().map(u32::from).max().unwrap_or(0);
        if raw >= 0x2D0 {
            longest = raw;
        }
        let mut step = step_of_cost(longest);
        if i32::from(step) < floor {
            step = floor.clamp(1, 255) as u8;
        }
        self.step = step;
        let span = frame_span(raw, ran_on);
        self.clock += u64::from(span * HBLANKS_PER_VSYNC);
        if self.xa_load.is_some_and(|(_, b)| b <= self.clock) {
            self.xa_load = None;
        }
        span
    }

    /// The clock moved on by `vsyncs` the cabinet spent outside its frames
    /// (the opponent install's synchronous load, whose waits reset the
    /// vsync count the ring measures from).
    pub fn skip(&mut self, vsyncs: u32) {
        self.clock += u64::from(vsyncs * HBLANKS_PER_VSYNC);
        if self.xa_load.is_some_and(|(_, b)| b <= self.clock) {
            self.xa_load = None;
        }
    }
}

/// The retail pad pipeline over the cabinet's frames. The frame driver
/// builds the held word `_DAT_8007B850` and its edge `_DAT_8007B874` from the
/// libpad buffer (refreshed at every vblank) at the end of each frame body
/// (`jal 0x8001822C` at `0x80017034`), ahead of the cost sample and the
/// `VSync` wait, and the frame after reads them. So a frame opened at vsync
/// `B` samples the pad as of the vblank its body ends after, `B + cost / 263`,
/// and a tap that comes and goes after a frame's sample and before the next
/// one's is never seen. The field's pipeline (`World::retail_frame_pad`) on
/// the cabinet's frames.
///
/// REF: FUN_8001822C
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FramePad {
    /// Held words by vsync, `hist[v % 16]`.
    hist: [u16; 16],
    /// Vsyncs the cabinet has been handed.
    vsync: u32,
    /// The vsync whose held word the next frame takes.
    sample: Option<u32>,
    /// The word the frame before took: the edge is the rise over it.
    prev: u16,
}

impl FramePad {
    /// Store the held word of the vsync the cabinet is handed now.
    pub fn push(&mut self, held: u16) {
        self.vsync = self.vsync.wrapping_add(1);
        self.hist[self.vsync as usize % 16] = held;
    }

    /// The held and edge words of the frame that opens on this vsync: the
    /// sample the frame before took, and its rise over the one before that.
    pub fn open(&mut self) -> (u16, u16) {
        let held = match self.sample {
            Some(s) if self.vsync.wrapping_sub(s) < 16 => self.hist[s as usize % 16],
            _ => self.hist[self.vsync as usize % 16],
        };
        let edge = held & !self.prev;
        self.prev = held;
        (held, edge)
    }

    /// The frame just run costs `cost` hblanks: its sample is the pad of the
    /// vblank its body ends after, the vsync before it opened plus the
    /// body's whole vsyncs, within its span.
    pub fn close(&mut self, cost: u32, span: u32) {
        let k = (cost / HBLANKS_PER_VSYNC).min(span.saturating_sub(1));
        self.sample = Some(self.vsync.wrapping_sub(1).wrapping_add(k));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_idle_duel_runs_at_step_one() {
        let mut p = BakaPacer::default();
        for _ in 0..20 {
            let c = p.frame_cost(frame_work(cab::ST_DUEL, 0));
            assert_eq!(p.end_frame(c, 1), 1);
        }
        assert_eq!(p.step(), 1);
        assert_eq!(p.newest(), 165);
    }

    /// The FIGHT flash's line on PCSX-Redux: the frames under the polls store
    /// what the film stores (`526..546`), the step goes to `3` and holds for
    /// sixteen frames past the last loaded one.
    #[test]
    fn the_fight_flash_line_paces_the_duel_at_step_three() {
        let mut p = BakaPacer::default();
        let w = frame_work(cab::ST_DUEL, 0);
        p.cue_xa(0x36);
        let mut steps = Vec::new();
        let mut vsyncs = 0;
        while vsyncs < 200 {
            let c = p.frame_cost(w);
            vsyncs += p.end_frame(c, 1);
            steps.push((c, p.step()));
        }
        assert!(steps.iter().any(|&(c, _)| (526..=546).contains(&c)));
        assert_eq!(steps.iter().map(|s| s.1).max(), Some(3));
        assert_eq!(steps.last().unwrap().1, 1);
    }

    #[test]
    fn a_console_drive_leaves_the_duel_at_step_one() {
        let mut p = BakaPacer::default();
        p.set_timing(CdPollTiming::Console);
        p.cue_xa(0x36);
        for _ in 0..100 {
            let c = p.frame_cost(frame_work(cab::ST_DUEL, 0));
            p.end_frame(c, 1);
            assert_eq!(p.step(), 1);
        }
    }

    #[test]
    fn the_floor_raises_the_step() {
        let mut p = BakaPacer::default();
        p.end_frame(frame_work(cab::ST_CHOICE_SECRET, 0), 4);
        assert_eq!(p.step(), 4);
        assert_eq!(p.end_frame(138, 4), 4);
    }

    #[test]
    fn a_frame_past_the_clamp_steps_at_four() {
        let mut p = BakaPacer::default();
        p.end_frame(0x2D1, 1);
        assert_eq!(p.newest(), 0x2BC);
        assert_eq!(p.step(), 4);
    }
}
