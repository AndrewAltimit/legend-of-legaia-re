//! From-scratch Rust **Noa dance (rhythm) minigame** rules engine, written
//! from the disassembly - no retail bytes are reproduced here.
//!
//! A faithful port of the dance overlay's per-frame rhythm logic - the beat
//! clock, the timing-window hit judge, the triangle "groovy move" wildcard, the
//! score / groove-gauge award, and the **three-dancer floor** (the human plus
//! the two competitors, who score through the very same award routine off a
//! chart auto-feed). Driven by the already-parsed step chart + scoring tables
//! ([`legaia_asset::dance_chart`]). This is the *rules* layer: it consumes pad
//! presses and produces judged results + running scores, exactly as the retail
//! overlay does. The visible dance-floor / arrow rendering is a separate host
//! concern and is not covered here.
//!
//! ## The retail shape, in one paragraph
//!
//! Three dancers stand on the floor; slot 0 is the human. Every frame the
//! per-dancer actor handler (`FUN_801d1358`) calls the award routine
//! (`FUN_801d1af4`) with a **pad word**: for the human that is the real pad, for
//! the competitors it is *synthesised from the chart* (`FUN_801d4040` ->
//! `FUN_801d1820`). So the rivals are not on a scripted score curve - they play
//! the same chart through the same judge, and differ only by their **kind** row
//! in two overlay tables (their sequence-bonus values and the schedule on which
//! they spend their triangles). Directional presses (Square `0x80` / Circle
//! `0x20`) are matched against the chart cell by `FUN_801d1960`; they score
//! **only when they close the lane's direction chain** (a "sequence"), for the
//! kind+lane value in `DAT_801d41a4`. The Triangle button (`0x10`) is the
//! **wildcard**: three per song, usable on any beat, worth `(lane+1) * 3` off
//! the beat but `(lane+1) * 0x19` when spent on the 4-beat combo slot - and it
//! throws the dancer into a multi-turn spin during which nothing is judged.
//!
//! Every constant and formula below is read from the overlay dumps
//! (`overlay_dance_801cf470/801d1358/801d1820/801d1960/801d1af4.txt`); see
//! [`docs/subsystems/minigame-dance.md`](../../../docs/subsystems/minigame-dance.md).
//! The two **data tables** (sequence bonus + triangle schedule) are disc
//! resident and parsed from the user's own image - no Sony bytes are baked in.
//!
//! Chain: retail `FUN_801cf470` (beat clock, state 10) -> `FUN_801d1358`
//! (per-dancer handler: latch decay, chart auto-feed) -> `FUN_801d1820` (AI
//! chart lookup) -> `FUN_801d1960` (hit judge) -> `FUN_801d1af4` (score/award).

use legaia_asset::dance_chart::{BEATS_PER_ROW, DanceChart, DanceScoreTables};

mod bodies;
mod camera_track;
mod finish;
mod game;
mod hud;
mod hud_kernels;
mod stage;
mod types;

pub use bodies::*;
pub use camera_track::*;
pub use finish::*;
pub use game::*;
pub use hud::*;
pub use hud_kernels::*;
pub use stage::*;
pub use types::*;

/// Beat period in phase units (`FUN_801d1960`'s `0x119` divisor): one beat slot
/// spans this many phase units. `phase % PERIOD` = intra-beat phase,
/// `phase / PERIOD` = beat index.
pub const BEAT_PERIOD: u32 = 0x119;

/// Acceptance-window width inside a beat slot (`0xd2`). An intra-beat phase past
/// this is the dead zone between beats - no note is active and a press misses.
pub const BEAT_WINDOW: u32 = 0xd2;

/// The beat phase counter wraps at this value (`FUN_801cf470` beat clock). It is
/// exactly [`BEATS_PER_ROW`] × [`BEAT_PERIOD`], so the beat index runs `0..=31`
/// and indexes a chart row directly.
pub const BEAT_PHASE_WRAP: u32 = 0x2320;

/// Per-frame phase advance = `frame_delta * PHASE_PER_DELTA` (`DAT_1f800393 * 10`
/// in the retail beat clock, framerate-compensated).
pub const PHASE_PER_DELTA: u32 = 10;

/// Peak accuracy weight (dead-on the beat). The weight ramps `0..=0x1000`,
/// maximal at phase 0 and decaying to 0 at the window edge.
pub const ACCURACY_MAX: u32 = 0x1000;

/// Song-length limit for the short mode (`FUN_801cf470` song-end test).
pub const SONG_LEN_SHORT: u32 = 0x41dc;
/// Song-length limit for the long mode.
pub const SONG_LEN_LONG: u32 = 0x64fc;

/// Per-player score clamp (`0x3e7`).
pub const SCORE_MAX: u32 = 999;

/// Groove-gauge step per **landed triangle** (`FUN_801d1af4`: `+= 1000` on the
/// combo slot). `gauge / GAUGE_STEP` selects the chart row (difficulty lane), so
/// crossing a step promotes the dancer to a denser, higher-scoring row.
pub const GAUGE_STEP: u32 = 1000;
/// Groove-gauge clamp ceiling (`[0, 2999]`).
pub const GAUGE_MAX: u32 = 2999;
/// Groove-gauge step per completed direction sequence (`DAT_801d6088 = 0xfa`).
pub const SEQUENCE_GAUGE_STEP: u32 = 0xfa;

/// Score multiplier for a triangle spent **off** the combo slot
/// (`(lane + 1) * 3`).
pub const MULT_ORDINARY: u32 = 3;
/// Score multiplier for a triangle spent **on** the 4-beat combo slot, inside
/// the window (`(lane + 1) * 0x19`) - the wildcard's payoff.
pub const MULT_COMBO: u32 = 0x19;
/// The award routine's *other* combo multiplier (`(lane + 1) * 0x22`). Retail
/// selects it by `DAT_801d5334 - 0xb < 2`, i.e. **only in the post-song Finish /
/// result-wipe states** (11 / 12), where the pad is still read
/// (`0x801D1CE0..0x801D1D30`); the rules engine pays it while
/// [`DanceGame::in_finale`].
pub const MULT_FINALE: u32 = 0x22;

/// Triangles ("groovy moves") each dancer gets per song (`FUN_801cf470` state 3
/// / `FUN_801d0750`: `DAT_801d534c[0..3] = 3`). Not replenished mid-run.
pub const TRIANGLE_STOCK: u32 = 3;

/// Feedback window armed when a triangle is spent (`DAT_801d5144 = 0x3c`,
/// counted down by the frame delta). The retail tutorial reads it to caption the
/// spend - praise when it landed on the combo slot, a timing scold when it did
/// not (`FUN_801d0750` case `0xd`, gated on `DAT_801d570c`).
pub const TRIANGLE_FEEDBACK_WINDOW: u32 = 0x3c;

/// Spin accumulator units per full turn of the groovy move (`FUN_801d1358`
/// wraps the dancer's yaw at `0x1000`).
pub const SPIN_TURN_UNITS: u32 = 0x1000;
/// Groovy-move spin rate at lane 0, in yaw units per frame-delta
/// (`FUN_801d1358`: `(lane * 0x20 + 0x80) * DAT_1f800393`).
pub const SPIN_RATE_BASE: u32 = 0x80;
/// Groovy-move spin-rate increment per difficulty lane.
pub const SPIN_RATE_PER_LANE: u32 = 0x20;

/// Hit-tier latch timer set on every judged press (`DAT_801d54cc = 0xf`),
/// decayed by `2 * frame_delta` each frame. While the latch is up the dancer's
/// presses are not re-judged.
pub const NOTE_LATCH_TIMER: i32 = 0xf;
/// Latch decay per frame delta (`FUN_801d1358`: `timer -= 2 * DAT_1f800393`).
pub const NOTE_LATCH_DECAY: i32 = 2;

/// The direction chain cursor (`DAT_801d550c`) is cleared every this many beats
/// (`FUN_801d1358`: `beat & 7 == 0`), so a sequence must be closed within one
/// 8-beat bar.
pub const CURSOR_RESET_BEATS: u32 = 8;

/// Dancers on the qualifier floor (`DAT_801d53cc[0..3]` - the human + two
/// competitors).
pub const DANCER_SLOTS: usize = 3;

/// Solo-style win threshold the results state compares the score against
/// (`0x12d`, retail mode 2). Modes 0/1 instead compare the human's score against
/// a rival's - see [`DanceGame::beating_rivals`].
pub const WIN_THRESHOLD_SOLO: u32 = 300;

/// The qualifier (yosenn) floor's dancer kinds: Noa in the centre flanked by the
/// dance hall's two competitor NPCs (`FUN_801d0190`'s mode-0 spawn table). Used
/// when no cast table is supplied; [`DanceGame::from_overlay`] reads the real one
/// off the disc.
pub const QUALIFIER_KINDS: [usize; DANCER_SLOTS] = [0, 2, 3];

#[cfg(test)]
mod tests;
