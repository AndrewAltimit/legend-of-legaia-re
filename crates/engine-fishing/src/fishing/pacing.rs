//! The venue's frame pacing: how many vsyncs each pond game frame spans,
//! and the frame step `DAT_1F800393` it runs on.
//!
//! The fishing mode runs the master frame driver like the field and the
//! battle, so its step is the driver's measured one: `FUN_80016B6C` stores
//! each frame's hblanks into the sixteen-entry ring `0x80084098` (cursor
//! `gp+0x440`), and the next frame's step is the ring's longest entry against
//! `0xF1` / `0x1FF` / `0x2D1` (`1` / `2` / `3`, else `4`), raised to the
//! floor `_DAT_8007B9D8` the fishing init stores (`2`). A frame then waits
//! `VSync(step)` on the step the frame before stored, so it spans its step,
//! or longer when its own cost runs past it.
//!
//! The venue's frames sit either side of the `0x1FF` rung, so the step moves
//! with the phase. PCSX-Redux films of `minigame_fishing_retail_shore` with
//! the ring beside the pond's words read each frame's entry: the shore
//! `578..635`, the hub menu `589..650`, the wind-up `590` (step `3`); the
//! power meter `488..504` and the flight `495..510`, which fall to step `2`
//! sixteen frames after the wind-up leaves the ring; the lure in the water
//! `488..495` with the line idle (step `2`, the spread the lure's distance
//! makes); every lure ring the reel or a hooked fish seats `8..13` more while
//! its part lives (`~60` vsyncs), so a reel pull's three rings take a waiting
//! line past `511` and step `3`, the landing's splash about as much, and a
//! fight, which seats one a frame, sits at `680..714`; the catch close-up's frames
//! run past `0x2D0` (step `4`, spans of `5..6` vsyncs: the frame runner prices
//! `minigame_fishing_retail_catch_lift`'s driver at `1241..1319` hblanks);
//! the plate `501..516` (step `2`).
//!
//! [`PondPacer`] runs that law over [`frame_work`]'s price of each frame.
//!
//! REF: FUN_80016B6C, FUN_8005FB84, FUN_801d4948

use super::PondPhase;

/// Hblanks a vsync spans: the NTSC field's `263` lines, the unit of the ring.
pub const HBLANKS_PER_VSYNC: u32 = 263;

/// Hblanks a frame spends past its ring entry before its `VSync(n)` can see
/// the next blank (the battle frame clock's and the cabinet's figure).
pub const FRAME_END_HBLANKS: u32 = 20;

/// The venue's step floor: the fishing init stores `2` to `_DAT_8007B9D8`
/// (`li v0,2` / `sw v0,-0x4628(v1)` at `0x801CF33C..0x801CF344`).
pub const POND_STEP_FLOOR: u8 = 2;

/// A frame on the shore (states `0xA..0xC`, the no-lure screen) and through
/// the wind-up: the venue, the party and the HUD (`578..636` on the films).
pub const SHORE_WORK: u32 = 588;
/// A frame with the hub menu up (states `0x64..0x7A`, `589..650`).
pub const HUB_WORK: u32 = 610;
/// The cast wind-up `0xD` (`587..592`).
pub const WIND_UP_WORK: u32 = 590;
/// The power meter `0x14` (`488..504`).
pub const POWER_WORK: u32 = 495;
/// The lure in flight `0x15` (`495..510`).
pub const FLIGHT_WORK: u32 = 504;
/// The lure in the water `0x19` with no lure ring live (`487..489` on a long
/// cast, `494..496` on a short one).
pub const WATER_WORK: u32 = 490;
/// What a hooked fish adds to a water frame past its rings: the fish's
/// actor and the catch HUD (fitted with [`LURE_RING_WORK`] and
/// [`LURE_RING_LIFE`] over a reeled wait and a fight).
pub const HOOKED_WORK: u32 = 11;
/// What a live lure ring adds to a frame (record `0x801D892C`, seated at the
/// landing and on every frame of a reel pull or a hooked fish): `8..9` on a
/// long cast, `12..13` on a short one, whose rings stand nearer the eye.
pub const LURE_RING_WORK: u32 = 10;
/// The vsyncs a lure ring's part lives: a reel pull's rings leave the
/// waiting line's ring entries `60..66` vsyncs after they are seated.
pub const LURE_RING_LIFE: u16 = 60;
/// The landing's splash parts beside its ring, priced as rings: a short
/// cast's landing frames store `526`, then `513` for six frames, over a
/// water frame of `495`. The fit of [`WATER_WORK`], [`LURE_RING_WORK`],
/// [`LURE_RING_LIFE`] and this over three films of reeled waits, fights and
/// idle lines calls the next frame's step right on `95%` of the water frames.
pub const LANDING_SPLASH_RINGS: usize = 2;
/// The catch close-up (state `0x1E`, line-actor sub-state `4`): the frame
/// runner prices its driver at `1241..1319` hblanks, the ring keeps `0x2BC`
/// and the films span its frames `5..6` vsyncs.
pub const CLOSE_UP_WORK: u32 = 1250;
/// The plate's first frames after the lift seats it (sub-state `2`, then the
/// showcase states `0x20` / `0x21` as the camera cuts and tilts), from the
/// films' ring.
pub const PLATE_OPEN_WORK: [u32; 16] = [
    637, 645, 740, 740, 740, 740, 740, 740, 740, 740, 740, 717, 662, 620, 582, 544,
];
/// The plate held (state `0x22`, `501..516`).
pub const PLATE_WORK: u32 = 503;

/// What one pond frame's own work costs, in PCSX-Redux hblanks with the
/// vblank interrupt: what the ring stores for it.
///
/// `phase` is the phase the frame ran in, `hub` whether the hub menu owned
/// it, `lifting` whether the catch's lift (the close-up) ran, `plate_frame`
/// the frame's ordinal on the plate once the lift has seated it, and
/// `live_rings` the lure rings alive at the frame's end.
pub fn frame_work(
    phase: PondPhase,
    hub: bool,
    lifting: bool,
    plate_frame: u32,
    live_rings: u32,
) -> u32 {
    if hub {
        return HUB_WORK;
    }
    let rings = live_rings * LURE_RING_WORK;
    match phase {
        PondPhase::Idle | PondPhase::NoLure => SHORE_WORK,
        PondPhase::WindUp => WIND_UP_WORK,
        PondPhase::Power => POWER_WORK,
        PondPhase::Flight => FLIGHT_WORK + rings,
        PondPhase::Waiting | PondPhase::Snapped => WATER_WORK + rings,
        PondPhase::Hooked => WATER_WORK + HOOKED_WORK + rings,
        PondPhase::Landed if lifting => CLOSE_UP_WORK,
        PondPhase::Landed => PLATE_OPEN_WORK
            .get(plate_frame as usize)
            .copied()
            .unwrap_or(PLATE_WORK),
    }
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

/// The vsyncs a frame spans when it costs `raw` hblanks and waits on `step`:
/// `VSync(n)` waits out the step, then for the first blank after the frame's
/// work (`FUN_8005FB84`).
pub const fn frame_span(raw: u32, step: u8) -> u32 {
    let need = (raw + FRAME_END_HBLANKS) / HBLANKS_PER_VSYNC + 1;
    let step = if step < 1 { 1 } else { step as u32 };
    if need > step { need } else { step }
}

/// The frame driver's step law over the pond's frames, and the lure rings
/// whose parts load them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PondPacer {
    ring: [u16; 16],
    cursor: u8,
    step: u8,
    /// The vsyncs each live lure ring has left.
    rings: Vec<u16>,
}

impl Default for PondPacer {
    fn default() -> Self {
        Self {
            ring: [0; 16],
            cursor: 0,
            step: POND_STEP_FLOOR,
            rings: Vec::new(),
        }
    }
}

impl PondPacer {
    /// The step the next frame runs on (`DAT_1F800393`).
    pub fn step(&self) -> u8 {
        self.step
    }

    /// The newest ring entry: what the last frame stored.
    pub fn newest(&self) -> u16 {
        self.ring[usize::from(self.cursor.wrapping_sub(1) & 0xF)]
    }

    /// The lure rings alive now.
    pub fn live_rings(&self) -> u32 {
        self.rings.len() as u32
    }

    /// `n` lure rings seated this frame.
    pub fn seat_rings(&mut self, n: usize) {
        self.rings.extend(std::iter::repeat_n(LURE_RING_LIFE, n));
    }

    /// One vsync of the rings' parts.
    pub fn vsync(&mut self) {
        for r in &mut self.rings {
            *r = r.saturating_sub(1);
        }
        self.rings.retain(|&r| r > 0);
    }

    /// One frame's pacing tail: store what the frame cost and read the step
    /// the next frame runs on, raised to the venue's floor. Returns the
    /// vsyncs the frame spans.
    ///
    /// The ring keeps a frame of `0x2D0` hblanks or more as `0x2BC`, and such
    /// a frame reads its step off its raw cost (`0x800170F8..0x80017104`).
    // PORT: FUN_80016B6C (`0x80017044..0x800171D8`, the frame-step law)
    pub fn end_frame(&mut self, raw: u32) -> u32 {
        let ran_on = self.step;
        let stored = if raw < 0x2D0 { raw } else { 0x2BC };
        self.cursor &= 0xF;
        self.ring[usize::from(self.cursor)] = stored as u16;
        self.cursor = (self.cursor + 1) & 0xF;
        let mut longest = self.ring.iter().copied().map(u32::from).max().unwrap_or(0);
        if raw >= 0x2D0 {
            longest = raw;
        }
        self.step = step_of_cost(longest).max(POND_STEP_FLOOR);
        frame_span(raw, ran_on)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(p: &mut PondPacer, work: u32, frames: usize) -> Vec<(u32, u8)> {
        (0..frames)
            .map(|_| {
                let span = p.end_frame(work);
                for _ in 0..span {
                    p.vsync();
                }
                (span, p.step())
            })
            .collect()
    }

    /// The shore's frames read step `3`, as every film and the frame runner
    /// read them (`minigame_fishing_retail_shore`'s ring `581..592`).
    #[test]
    fn the_shore_runs_at_step_three() {
        let mut p = PondPacer::default();
        let w = frame_work(PondPhase::Idle, false, false, 0, 0);
        assert!(run(&mut p, w, 20).iter().all(|&(s, st)| s >= 2 && st == 3));
    }

    /// The power meter falls to step `2` once the wind-up's entries leave the
    /// ring: sixteen frames, as the films read it.
    #[test]
    fn the_meter_falls_to_the_floor_sixteen_frames_on() {
        let mut p = PondPacer::default();
        run(&mut p, frame_work(PondPhase::WindUp, false, false, 0, 0), 5);
        let steps = run(&mut p, frame_work(PondPhase::Power, false, false, 0, 0), 20);
        assert!(steps[..15].iter().all(|s| s.1 == 3));
        assert!(steps[15..].iter().all(|s| s.1 == 2));
    }

    /// An idle line holds the floor; a reel pull's three rings lift it to `3`
    /// until their parts end; a fight, a ring a frame, holds `3`.
    #[test]
    fn the_rings_set_the_waiting_lines_step() {
        let mut p = PondPacer::default();
        let water = |p: &PondPacer| frame_work(PondPhase::Waiting, false, false, 0, p.live_rings());
        for _ in 0..20 {
            let w = water(&p);
            let span = p.end_frame(w);
            (0..span).for_each(|_| p.vsync());
        }
        assert_eq!(p.step(), 2);
        assert_eq!(p.newest(), 490);
        p.seat_rings(3);
        let span = p.end_frame(water(&p));
        (0..span).for_each(|_| p.vsync());
        assert_eq!(p.newest(), 520);
        assert_eq!(p.step(), 3);
        for _ in 0..60 {
            let span = p.end_frame(water(&p));
            (0..span).for_each(|_| p.vsync());
        }
        assert_eq!(p.live_rings(), 0);
        assert_eq!(p.step(), 2);
        let mut entries = Vec::new();
        for _ in 0..60 {
            p.seat_rings(1);
            let w = frame_work(PondPhase::Hooked, false, false, 0, p.live_rings());
            let span = p.end_frame(w);
            (0..span).for_each(|_| p.vsync());
            entries.push(w);
        }
        assert_eq!(p.step(), 3);
        let settled = *entries.last().unwrap();
        assert!((660..=714).contains(&settled), "{settled}");
    }

    /// The close-up's frames run past the clamp: step `4`, five-vsync spans.
    #[test]
    fn the_close_up_steps_at_four() {
        let mut p = PondPacer::default();
        let steps = run(&mut p, frame_work(PondPhase::Landed, false, true, 0, 0), 4);
        assert!(steps.iter().all(|&(s, st)| s == 5 && st == 4));
        assert_eq!(p.newest(), 0x2BC);
    }
}
