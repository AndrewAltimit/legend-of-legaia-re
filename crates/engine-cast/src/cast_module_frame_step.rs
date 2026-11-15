//! The frame step a retail recording ran each arm of a summon module at.
//!
//! This table is a measurement, not game data, and it does not model the cause.
//! The frame driver `FUN_80016B6C` sets `DAT_1F800393` from the slowest of the
//! last sixteen frames, capped at `4`
//! (`docs/subsystems/battle-stage-camera.md`, "The frame step"), and nothing on
//! the disc says what a frame costs. Each entry is the step one recording
//! (`scripts/ci/cast-sequence.py`, `meta.csv`'s `step`, the Gobu Gobu fight, the
//! value the arm spends the most vsyncs on) on the emulator's cost model. Some
//! arms one battle frame long read that frame's length in vsyncs instead, since
//! the `step` column lags the frame that set it: Gimard's arm 7 reads
//! `1, 1, 1, 4` and its one pass drains `32`. An arm too short to read takes the
//! arm before it; a `0` is an arm no recording has reached.
//!
//! The engine pushes each arm's entry into the frame-cost ring as the cost of the
//! arm's frames, and the ring's step opens the next pass
//! (`World::apply_cast_arm_frame_step`), so an entry is read as a cost. On an arm
//! of several passes the modal step is that cost. An arm one pass long runs on
//! the step the frames before it left, so its `step` column names the arm before
//! it; those entries are the recording's `cost` column instead (the ring's newest
//! entry on the next pass, read through the law): Iota's arm 4 costs the clamp
//! and its arm 5 opens on `4`, where the step column read `1`.
//!
//! What the reading is good for and where it stops is on that page. In short, a
//! whole-screen arm reads the same in every fight, an arm that shows the stage
//! does not, and the step moves inside an arm as the ring empties.
//!
//! The capture band has its own table, [`CAPTURE_ARM_STEPS`].
//!
//! The step matters beyond the camera's rate: a record an arm seats every pass
//! seats half as many at `4` as at `2`.

/// `(PROT entry, step by module phase)`, read on PCSX-Redux
/// ([`CdTiming::PcsxRedux`]).
const ARM_STEPS: &[(u32, &[u8])] = &[
    // gimard
    (903, &[0, 0, 1, 1, 4, 4, 4, 4, 4, 4, 4, 4, 4]),
    // theeder
    (904, &[0, 0, 1, 1, 1, 4, 3, 3, 4, 4, 4, 3, 4, 4, 4]),
    // vera
    (905, &[0, 0, 2, 2, 4, 4, 4, 4, 4, 4, 4]),
    // gizam
    (906, &[0, 0, 0, 1, 1, 4, 3, 3, 4, 4, 4, 4, 4, 4, 2]),
    // nighto
    (907, &[0, 0, 0, 1, 1, 4, 3, 4, 3, 3, 3, 4, 4, 4, 4, 4]),
    // zenoir
    (908, &[0, 0, 0, 1, 1, 4, 4, 4, 4, 4, 4, 4, 4]),
    // viguro
    (909, &[0, 0, 1, 1, 2, 2, 2, 2, 2, 2, 4, 4, 4, 4, 4]),
    // swordie
    (910, &[0, 0, 1, 4, 4, 4, 4, 4, 4, 4, 4]),
    // orb
    (911, &[0, 0, 1, 4, 4, 3, 3, 3, 3, 3, 4]),
    // freed
    (
        912,
        &[0, 0, 1, 4, 4, 4, 4, 4, 4, 4, 3, 4, 4, 4, 4, 3, 4, 4, 2, 3],
    ),
    // nova
    (
        913,
        &[
            0, 0, 1, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 2, 2, 2, 3,
        ],
    ),
    // gola
    (914, &[0, 0, 0, 1, 4, 4, 4, 4, 4, 3, 3, 3, 4, 4, 4]),
    // mushura
    (915, &[0, 0, 0, 1, 4, 4, 4, 4, 4, 4, 4, 4, 4, 2]),
    // aluru
    (
        916,
        &[0, 0, 1, 1, 3, 4, 4, 4, 4, 3, 4, 4, 4, 4, 3, 3, 3, 3, 1, 4],
    ),
    // barra
    (917, &[0, 1, 1, 3, 2, 3, 3, 3, 3, 3, 3, 3, 4, 4, 4, 3]),
    // kemaro
    (
        918,
        &[0, 1, 1, 1, 4, 4, 3, 2, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4],
    ),
    // spoon
    (919, &[0, 0, 1, 4, 3, 4, 4, 4, 4]),
    // slippery
    (920, &[0, 0, 1, 3, 3, 3, 4, 4, 4, 4, 4]),
    // iota
    (921, &[0, 0, 1, 2, 4, 4, 4, 4, 4, 4, 4, 4]),
    // puera
    (
        922,
        &[
            0, 0, 1, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 2,
        ],
    ),
    // gilium
    (
        923,
        &[
            0, 0, 1, 4, 4, 4, 4, 2, 3, 4, 3, 4, 3, 2, 2, 2, 3, 3, 3, 3, 4, 4, 2,
        ],
    ),
    // juggernaut
    (
        927,
        &[
            0, 0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 3, 3, 2,
        ],
    ),
    // palma
    (
        928,
        &[0, 2, 3, 3, 2, 4, 4, 3, 4, 3, 3, 4, 4, 2, 4, 4, 4, 4, 2, 2],
    ),
    // mule
    (
        929,
        &[0, 1, 3, 3, 3, 4, 3, 3, 3, 3, 3, 3, 4, 4, 4, 4, 4, 4, 3, 3],
    ),
    // horn
    (930, &[0, 0, 2, 2, 4, 4, 4, 4, 4, 4, 4, 2]),
    // jedo
    (
        931,
        &[
            0, 0, 1, 3, 3, 4, 4, 4, 4, 4, 3, 3, 2, 2, 3, 3, 3, 3, 3, 3, 4, 4, 4, 4, 4, 4, 4, 4, 4,
            4, 2, 1,
        ],
    ),
    // meta
    (
        932,
        &[
            0, 1, 1, 1, 3, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 3, 4, 3, 3, 3, 3, 3,
        ],
    ),
    // terra
    (
        933,
        &[
            0, 1, 3, 2, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 2, 2, 2, 2, 2, 2, 2, 2,
        ],
    ),
    // ozma
    (
        934,
        &[
            0, 0, 1, 2, 2, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 3, 4, 4, 3,
        ],
    ),
];

/// Which machine's CD response timing a battle frame's cost is read on.
///
/// While a CD-XA voice clip plays, the clip player's stop watch `FUN_8003D764`
/// (installed as the CdSync callback) polls the drive from interrupt context:
/// each completion issues the next `GetlocL` / `GetStat` (`0x8003D984`,
/// `0x8003D9FC`, `0x8003DA14`) until the head passes the clip's end, so the
/// polls take whatever share of the CPU the drive's response time leaves them,
/// and the frame stretches by it. The two machines this table and the films
/// were read on differ there by an order of magnitude
/// (`docs/subsystems/battle-stage-camera.md`, "The CD stop watch").
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CdTiming {
    /// A console's: `GetStat` answers in `0xC4E1` cycles on average (psx-spx,
    /// CDROM response timings), and the polls take [`CONSOLE_POLL_SHARE_PER_MILLE`]
    /// of the CPU. Opt-in (`LEGAIA_BATTLE_CD_TIMING=console`): under it five
    /// enemy casts' cues no longer play in the films, which is open.
    Console,
    /// PCSX-Redux's: every command answers `0x800` cycles after it
    /// (`cdrom.cc`, `AddIrqQueue(m_cmd, 0x800)`), and the polls take about two
    /// thirds of the CPU. The cast films and the PCSX-Redux battle captures
    /// were recorded on it. The engine's default.
    #[default]
    PcsxRedux,
}

/// The stop watch's share of a console's CPU, per mille: about `3300` cycles
/// of interrupt work a poll (the exception vector to `FUN_8005DAB0`'s exit,
/// measured on PCSX-Redux) over a `0xC4E1`-cycle response.
pub const CONSOLE_POLL_SHARE_PER_MILLE: u32 = 65;

/// What a frame whose own work costs `work` hblanks costs on a console while
/// the stop watch polls: the work over the share the polls leave.
pub const fn console_poll_cost(work: u16) -> u16 {
    let c = work as u32 * 1000 / (1000 - CONSOLE_POLL_SHARE_PER_MILLE);
    if c > u16::MAX as u32 {
        u16::MAX
    } else {
        c as u16
    }
}

/// [`ARM_STEPS`] with each arm the voice clip's stop watch polls through read
/// at a console's share ([`CdTiming::Console`]): the recordings' frames less
/// their CD interrupt time, over [`CONSOLE_POLL_SHARE_PER_MILLE`]
/// (`scripts/ci/cd-poll-steps.py` over `--cd-trace` recordings of the same
/// casts). An arm the clip does not span keeps the PCSX-Redux reading.
const CONSOLE_ARM_STEPS: &[(u32, &[u8])] = &[
    // gimard
    (903, &[0, 0, 1, 1, 4, 3, 1, 2, 2, 2, 2, 2, 4]),
    // theeder
    (904, &[0, 0, 1, 1, 1, 2, 2, 2, 2, 2, 2, 2, 2, 2, 3]),
    // vera
    (905, &[0, 0, 2, 2, 2, 4, 4, 4, 2, 2, 4]),
    // gizam
    (906, &[0, 0, 0, 1, 1, 2, 2, 2, 2, 2, 2, 2, 2, 3, 2]),
    // nighto
    (907, &[0, 0, 0, 1, 1, 2, 2, 2, 2, 2, 2, 2, 2, 2, 4, 2]),
    // zenoir
    (908, &[0, 0, 0, 1, 1, 3, 2, 2, 2, 2, 2, 2, 4]),
    // viguro
    (909, &[0, 0, 1, 1, 1, 1, 1, 1, 1, 1, 2, 2, 3, 2, 3]),
    // swordie
    (910, &[0, 0, 1, 2, 2, 2, 2, 3, 2, 3, 4]),
    // orb
    (911, &[0, 0, 1, 2, 2, 2, 3, 3, 3, 2, 3]),
    // freed
    (
        912,
        &[0, 0, 1, 2, 2, 2, 2, 3, 4, 4, 2, 2, 2, 3, 3, 2, 2, 3, 2, 3],
    ),
    // nova
    (
        913,
        &[
            0, 0, 1, 3, 3, 3, 3, 2, 2, 2, 2, 2, 2, 2, 3, 3, 4, 2, 2, 2, 3,
        ],
    ),
    // gola
    (914, &[0, 0, 0, 1, 3, 3, 4, 4, 3, 2, 2, 2, 3, 3, 4]),
    // mushura
    (915, &[0, 0, 0, 2, 2, 2, 3, 3, 3, 3, 3, 3, 3, 2]),
    // aluru
    (
        916,
        &[0, 0, 1, 1, 2, 4, 4, 2, 2, 2, 2, 3, 3, 3, 2, 3, 3, 3, 1, 4],
    ),
    // barra
    (917, &[0, 1, 1, 1, 1, 1, 2, 2, 2, 2, 2, 2, 3, 2, 3, 3]),
    // kemaro
    (
        918,
        &[0, 1, 1, 2, 2, 2, 2, 1, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 4],
    ),
    // spoon
    (919, &[0, 0, 1, 4, 3, 2, 2, 2, 4]),
    // slippery
    (920, &[0, 0, 1, 2, 2, 2, 2, 3, 3, 3, 4]),
    // iota
    (921, &[0, 0, 1, 1, 2, 2, 2, 2, 2, 3, 2, 4]),
    // puera
    (
        922,
        &[
            0, 0, 1, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 3, 3, 3, 3, 4, 3, 4, 3, 4, 2,
        ],
    ),
    // gilium
    (
        923,
        &[
            0, 0, 1, 2, 2, 4, 3, 1, 1, 2, 2, 2, 2, 1, 1, 1, 1, 1, 1, 2, 2, 2, 2,
        ],
    ),
    // juggernaut
    (
        927,
        &[
            0, 0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 2, 3, 2, 3, 2, 2, 2, 2, 3, 3, 2, 4, 2, 3, 3, 3, 3, 2,
        ],
    ),
    // palma
    (
        928,
        &[0, 2, 3, 2, 1, 3, 3, 2, 2, 2, 2, 2, 2, 2, 4, 4, 3, 2, 2, 2],
    ),
    // mule
    (
        929,
        &[0, 2, 3, 1, 2, 2, 2, 1, 1, 2, 2, 2, 2, 2, 2, 2, 3, 3, 3, 3],
    ),
    // horn
    (930, &[0, 0, 2, 3, 3, 2, 2, 4, 3, 3, 4, 2]),
    // jedo
    (
        931,
        &[
            0, 0, 1, 4, 3, 4, 4, 2, 2, 2, 2, 1, 1, 1, 2, 1, 2, 1, 1, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2,
            2, 2, 1,
        ],
    ),
    // meta
    (
        932,
        &[
            0, 1, 1, 1, 1, 3, 4, 4, 4, 4, 2, 3, 3, 3, 3, 4, 3, 4, 3, 3, 3, 3, 3,
        ],
    ),
    // terra
    (
        933,
        &[
            0, 1, 3, 3, 3, 2, 4, 4, 3, 3, 3, 3, 4, 4, 2, 2, 2, 2, 2, 2, 2, 2,
        ],
    ),
    // ozma
    (
        934,
        &[
            0, 0, 2, 2, 2, 2, 4, 4, 4, 4, 3, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 4, 3, 4, 4, 3,
        ],
    ),
];

/// The step retail runs PROT `entry`'s arm `phase` at on `timing`'s CD,
/// `None` when no recording says (a module the table does not hold, or an
/// arm it passed through inside a frame).
pub fn cast_arm_frame_step(entry: u32, phase: u8, timing: CdTiming) -> Option<u8> {
    let table = match timing {
        CdTiming::Console => CONSOLE_ARM_STEPS,
        CdTiming::PcsxRedux => ARM_STEPS,
    };
    let (_, steps) = table.iter().find(|(e, _)| *e == entry)?;
    steps.get(usize::from(phase)).copied().filter(|&s| s != 0)
}

/// Summon-band arms whose frames change cost inside the arm:
/// `(PROT entry, module phase, pass, step)`, the arm's frames costing `step`'s
/// rung from its `pass`-th pass (from `0`) on, and its [`ARM_STEPS`] entry
/// before the first row. Read off the same Gobu Gobu recordings as the table,
/// frame by frame, by `scripts/ci/cast-arm-cost-rows.py --rust`: the `cost`
/// column on a frame's first row is the entry that frame's own pass stored
/// (the probe reads it once the pass and its frame end have run), and a step
/// of `4` on that row is a raw cost past the clamp. Run-length coded per arm.
/// PCSX-Redux only, as [`CAPTURE_ARM_READ_TAILS`] is, and a measurement of
/// one emulated machine in one fight, as the table is.
///
/// One modal step an arm cannot say this, and the ring cannot recover it: the
/// ring holds a dear arm's entries for sixteen frames and no longer, so an arm
/// whose own frames turn cheap partway drains its countdown on smaller steps
/// from there and runs more passes. Ozma's arm 21 runs `24` frames at the
/// clamp and `54` under it (`674`..`712` hblanks) where a flat `4` runs `64`;
/// Gimard's arm 6 opens on `16` clamped frames while its cast line still
/// loads and then costs under `0xF1`, so the ring walks `4`, `3`, `1` over
/// `99` frames; Nova's arm 16 and Meta's arm 15 fall as their whirl and their
/// rays leave the frame; Terra's arm 3 climbs as its vortex fills it. An arm
/// can also open on one frame of another cost (Gimard's arm 4, the flame's
/// first frame under the clamp), and a frame that sits on a rung's edge
/// crosses it now and then (Barra's arms 6 and 7). Every frame draws the
/// frame driver's `rand()`, so a pass count is a draw count: one pass off
/// parts the stream for the rest of the cast.
pub const CAST_ARM_COST_TAILS: &[(u32, u8, u16, u8)] = &[
    // gimard
    (903, 4, 0, 3),
    (903, 4, 1, 4),
    (903, 6, 16, 1),
    (903, 12, 20, 3),
    (903, 12, 35, 2),
    (903, 12, 43, 3),
    (903, 12, 46, 2),
    (903, 12, 65, 1),
    // theeder
    (904, 4, 3, 2),
    (904, 4, 4, 3),
    (904, 6, 0, 4),
    (904, 6, 1, 3),
    (904, 11, 0, 4),
    (904, 11, 3, 3),
    (904, 14, 11, 3),
    // vera
    (905, 4, 0, 2),
    (905, 4, 2, 4),
    (905, 10, 12, 3),
    (905, 10, 13, 2),
    // gizam
    (906, 5, 0, 2),
    (906, 5, 2, 4),
    (906, 10, 5, 3),
    (906, 14, 0, 4),
    (906, 14, 5, 1),
    (906, 14, 10, 2),
    (906, 14, 21, 1),
    (906, 14, 25, 2),
    (906, 14, 56, 1),
    // nighto
    (907, 5, 0, 2),
    (907, 5, 2, 4),
    (907, 5, 13, 3),
    (907, 5, 17, 4),
    (907, 5, 20, 3),
    (907, 5, 23, 4),
    (907, 5, 24, 3),
    (907, 5, 31, 4),
    (907, 5, 32, 3),
    (907, 7, 3, 3),
    (907, 7, 5, 4),
    (907, 8, 0, 4),
    (907, 8, 5, 3),
    // zenoir
    (908, 5, 0, 3),
    (908, 5, 1, 4),
    (908, 6, 32, 3),
    (908, 7, 0, 3),
    (908, 7, 19, 4),
    (908, 7, 52, 3),
    (908, 12, 29, 3),
    (908, 12, 32, 1),
    // viguro
    (909, 7, 60, 1),
    (909, 9, 28, 1),
    (909, 9, 29, 2),
    (909, 14, 30, 2),
    (909, 14, 36, 1),
    // swordie
    (910, 3, 1, 3),
    (910, 3, 2, 4),
    (910, 4, 7, 3),
    (910, 4, 8, 4),
    (910, 4, 9, 3),
    (910, 4, 10, 4),
    (910, 4, 12, 3),
    (910, 4, 13, 4),
    (910, 4, 14, 3),
    (910, 4, 16, 4),
    (910, 4, 17, 3),
    (910, 5, 3, 3),
    (910, 5, 4, 4),
    (910, 5, 5, 3),
    (910, 5, 7, 4),
    (910, 5, 11, 3),
    (910, 5, 12, 4),
    (910, 5, 13, 3),
    (910, 5, 14, 4),
    (910, 5, 15, 3),
    (910, 5, 18, 4),
    (910, 5, 19, 3),
    (910, 5, 20, 4),
    (910, 5, 22, 3),
    (910, 5, 23, 4),
    (910, 5, 25, 3),
    (910, 5, 26, 4),
    (910, 5, 28, 3),
    (910, 5, 30, 4),
    (910, 5, 31, 3),
    (910, 5, 33, 4),
    (910, 5, 39, 3),
    (910, 5, 40, 4),
    (910, 5, 49, 3),
    (910, 10, 27, 2),
    (910, 10, 56, 1),
    // orb
    (911, 4, 1, 3),
    (911, 4, 3, 4),
    (911, 4, 25, 3),
    (911, 10, 57, 2),
    // freed
    (912, 10, 0, 4),
    (912, 10, 2, 3),
    (912, 10, 5, 4),
    (912, 10, 6, 3),
    (912, 10, 9, 4),
    (912, 10, 10, 3),
    (912, 10, 17, 4),
    (912, 10, 18, 3),
    (912, 10, 25, 4),
    (912, 10, 26, 3),
    (912, 10, 37, 4),
    (912, 10, 38, 3),
    (912, 12, 1, 3),
    (912, 12, 2, 4),
    (912, 14, 15, 3),
    (912, 18, 0, 4),
    (912, 18, 3, 2),
    (912, 19, 0, 2),
    (912, 19, 9, 3),
    (912, 19, 14, 2),
    (912, 19, 30, 1),
    // nova
    (913, 16, 29, 3),
    (913, 16, 46, 2),
    (913, 17, 0, 1),
    (913, 18, 0, 1),
    (913, 19, 0, 1),
    (913, 19, 4, 2),
    (913, 19, 6, 1),
    (913, 19, 7, 2),
    (913, 19, 22, 1),
    (913, 19, 23, 2),
    (913, 19, 30, 1),
    (913, 19, 31, 2),
    (913, 19, 37, 1),
    (913, 19, 39, 2),
    (913, 20, 1, 2),
    (913, 20, 6, 3),
    (913, 20, 8, 2),
    (913, 20, 12, 3),
    (913, 20, 20, 2),
    (913, 20, 25, 3),
    (913, 20, 27, 2),
    // gola
    (914, 9, 27, 4),
    (914, 10, 0, 4),
    (914, 10, 13, 3),
    (914, 11, 62, 4),
    (914, 11, 63, 3),
    (914, 11, 64, 4),
    (914, 11, 66, 3),
    (914, 11, 68, 4),
    (914, 11, 71, 3),
    (914, 11, 73, 4),
    (914, 11, 77, 3),
    (914, 13, 22, 3),
    (914, 13, 29, 2),
    (914, 14, 0, 3),
    (914, 14, 1, 4),
    // mushura
    (915, 12, 47, 2),
    (915, 13, 63, 1),
    // aluru
    (916, 8, 0, 3),
    (916, 9, 0, 4),
    (916, 9, 1, 3),
    (916, 9, 12, 4),
    (916, 9, 13, 3),
    (916, 9, 33, 4),
    (916, 9, 34, 3),
    (916, 9, 41, 4),
    (916, 13, 24, 2),
    (916, 14, 0, 2),
    (916, 16, 0, 1),
    (916, 17, 0, 1),
    (916, 19, 0, 1),
    (916, 19, 1, 4),
    // barra
    (917, 3, 0, 1),
    (917, 3, 1, 2),
    (917, 3, 3, 3),
    (917, 3, 4, 2),
    (917, 4, 3, 1),
    (917, 4, 4, 2),
    (917, 4, 7, 1),
    (917, 4, 9, 2),
    (917, 4, 11, 1),
    (917, 4, 12, 2),
    (917, 4, 14, 1),
    (917, 5, 0, 2),
    (917, 5, 11, 3),
    (917, 6, 0, 4),
    (917, 6, 3, 3),
    (917, 6, 5, 4),
    (917, 6, 7, 3),
    (917, 6, 52, 2),
    (917, 7, 7, 4),
    (917, 7, 12, 3),
    (917, 7, 13, 4),
    (917, 7, 16, 3),
    (917, 7, 21, 4),
    (917, 7, 22, 3),
    (917, 7, 26, 4),
    (917, 7, 27, 3),
    (917, 7, 33, 4),
    (917, 7, 34, 3),
    (917, 7, 44, 4),
    (917, 7, 46, 3),
    (917, 7, 59, 4),
    (917, 7, 63, 3),
    (917, 7, 65, 4),
    (917, 7, 66, 3),
    (917, 7, 70, 4),
    (917, 7, 71, 3),
    (917, 7, 72, 4),
    (917, 7, 73, 3),
    (917, 8, 18, 4),
    (917, 8, 19, 3),
    (917, 9, 0, 2),
    (917, 9, 1, 3),
    (917, 15, 0, 4),
    (917, 15, 3, 3),
    (917, 15, 16, 4),
    (917, 15, 18, 3),
    (917, 15, 32, 2),
    (917, 15, 36, 1),
    // kemaro
    (918, 5, 15, 3),
    (918, 5, 20, 4),
    (918, 5, 23, 2),
    (918, 5, 26, 3),
    (918, 5, 27, 2),
    (918, 6, 0, 2),
    (918, 6, 1, 3),
    (918, 6, 3, 2),
    (918, 6, 6, 3),
    (918, 7, 0, 3),
    (918, 7, 2, 4),
    (918, 7, 10, 2),
    (918, 19, 12, 2),
    // spoon
    (919, 4, 0, 4),
    (919, 4, 3, 3),
    (919, 4, 33, 4),
    (919, 4, 34, 3),
    (919, 8, 48, 2),
    // slippery
    (920, 9, 24, 3),
    (920, 10, 0, 3),
    (920, 10, 12, 4),
    (920, 10, 14, 3),
    (920, 10, 15, 4),
    (920, 10, 17, 3),
    (920, 10, 19, 4),
    (920, 10, 21, 3),
    (920, 10, 23, 4),
    (920, 10, 36, 3),
    (920, 10, 37, 4),
    (920, 10, 39, 2),
    // iota
    (921, 3, 0, 1),
    (921, 3, 1, 2),
    (921, 3, 8, 1),
    (921, 11, 15, 3),
    (921, 11, 16, 2),
    (921, 11, 20, 3),
    (921, 11, 35, 2),
    (921, 11, 41, 1),
    // puera
    (922, 12, 0, 4),
    (922, 12, 1, 3),
    (922, 12, 9, 2),
    (922, 12, 13, 3),
    (922, 12, 14, 2),
    (922, 23, 10, 2),
    (922, 23, 51, 4),
    (922, 24, 29, 1),
    (922, 24, 30, 2),
    (922, 24, 61, 1),
    // gilium
    (923, 6, 17, 3),
    (923, 8, 0, 2),
    (923, 8, 1, 3),
    (923, 8, 5, 2),
    (923, 8, 8, 3),
    (923, 8, 13, 2),
    (923, 9, 0, 3),
    (923, 9, 11, 4),
    (923, 9, 12, 3),
    (923, 9, 21, 4),
    (923, 12, 0, 2),
    (923, 16, 0, 2),
    (923, 16, 3, 3),
    (923, 22, 0, 4),
    (923, 22, 7, 2),
    (923, 22, 18, 1),
    (923, 22, 23, 2),
    (923, 22, 54, 1),
    // juggernaut
    (927, 25, 31, 3),
    (927, 25, 32, 2),
    (927, 27, 0, 2),
    (927, 27, 27, 3),
    (927, 27, 28, 2),
    (927, 27, 31, 3),
    (927, 27, 32, 2),
    (927, 27, 35, 3),
    (927, 27, 36, 2),
    (927, 27, 39, 3),
    (927, 27, 40, 2),
    (927, 27, 43, 3),
    (927, 27, 44, 2),
    (927, 27, 53, 3),
    (927, 27, 74, 2),
    // palma
    (928, 1, 0, 1),
    (928, 1, 11, 2),
    (928, 1, 12, 1),
    (928, 1, 13, 2),
    (928, 1, 18, 1),
    (928, 1, 19, 2),
    (928, 2, 1, 1),
    (928, 3, 0, 4),
    (928, 3, 21, 3),
    (928, 3, 36, 2),
    (928, 3, 43, 1),
    (928, 3, 44, 2),
    (928, 7, 0, 4),
    (928, 7, 16, 3),
    (928, 8, 0, 3),
    (928, 8, 3, 4),
    (928, 9, 0, 4),
    (928, 9, 3, 3),
    (928, 9, 4, 4),
    (928, 9, 5, 3),
    (928, 9, 6, 4),
    (928, 9, 7, 3),
    (928, 11, 0, 3),
    (928, 11, 1, 4),
    (928, 12, 15, 1),
    (928, 13, 65, 3),
    (928, 13, 71, 4),
    (928, 13, 82, 3),
    (928, 13, 84, 4),
    (928, 13, 87, 3),
    (928, 13, 89, 4),
    (928, 13, 92, 3),
    (928, 13, 98, 2),
    (928, 13, 100, 4),
    (928, 17, 24, 3),
    (928, 17, 27, 4),
    (928, 17, 29, 3),
    (928, 19, 7, 1),
    (928, 19, 26, 2),
    (928, 19, 27, 1),
    (928, 19, 32, 2),
    // mule
    (929, 1, 25, 2),
    (929, 2, 0, 4),
    (929, 2, 1, 1),
    (929, 3, 0, 2),
    (929, 4, 5, 2),
    (929, 6, 0, 4),
    (929, 6, 11, 3),
    (929, 12, 38, 3),
    (929, 12, 39, 4),
    (929, 12, 40, 3),
    (929, 12, 41, 4),
    (929, 18, 0, 4),
    (929, 18, 12, 2),
    (929, 18, 22, 3),
    (929, 19, 13, 2),
    (929, 19, 26, 3),
    (929, 19, 29, 2),
    (929, 19, 48, 1),
    // horn
    (930, 2, 0, 1),
    (930, 2, 11, 2),
    (930, 2, 13, 1),
    (930, 2, 17, 2),
    (930, 3, 0, 3),
    (930, 3, 1, 2),
    (930, 9, 40, 2),
    (930, 9, 42, 3),
    (930, 9, 43, 2),
    (930, 9, 46, 3),
    (930, 9, 47, 2),
    (930, 9, 50, 3),
    (930, 10, 0, 3),
    (930, 10, 12, 4),
    (930, 10, 13, 3),
    (930, 10, 15, 4),
    // jedo
    (931, 3, 1, 1),
    (931, 4, 4, 2),
    (931, 4, 6, 3),
    (931, 4, 7, 2),
    (931, 4, 9, 3),
    (931, 4, 10, 2),
    (931, 4, 12, 3),
    (931, 4, 28, 2),
    (931, 4, 42, 3),
    (931, 10, 0, 4),
    (931, 10, 31, 3),
    (931, 11, 0, 2),
    (931, 14, 35, 2),
    (931, 15, 0, 2),
    (931, 15, 10, 3),
    (931, 16, 10, 4),
    (931, 16, 11, 3),
    (931, 16, 45, 2),
    (931, 16, 46, 3),
    (931, 16, 50, 2),
    (931, 16, 52, 3),
    (931, 16, 53, 2),
    (931, 16, 55, 3),
    (931, 16, 59, 2),
    (931, 16, 60, 3),
    (931, 16, 61, 2),
    (931, 18, 15, 2),
    (931, 18, 16, 3),
    (931, 20, 0, 3),
    (931, 20, 4, 4),
    (931, 23, 0, 3),
    (931, 23, 2, 4),
    (931, 29, 15, 2),
    (931, 30, 1, 3),
    (931, 30, 7, 2),
    // meta
    (932, 4, 4, 2),
    (932, 4, 22, 1),
    (932, 15, 50, 2),
    (932, 16, 0, 2),
    (932, 16, 12, 3),
    (932, 16, 24, 4),
    (932, 16, 26, 3),
    (932, 16, 36, 4),
    (932, 16, 37, 3),
    (932, 16, 48, 4),
    (932, 16, 49, 3),
    (932, 16, 52, 4),
    (932, 16, 53, 3),
    (932, 16, 60, 4),
    (932, 16, 61, 3),
    (932, 16, 76, 4),
    (932, 16, 77, 3),
    (932, 16, 80, 4),
    (932, 16, 81, 3),
    (932, 17, 0, 3),
    (932, 17, 1, 4),
    (932, 17, 2, 3),
    (932, 17, 17, 4),
    (932, 18, 0, 2),
    (932, 19, 0, 2),
    (932, 19, 1, 3),
    (932, 22, 49, 4),
    (932, 22, 52, 3),
    (932, 22, 55, 4),
    (932, 22, 56, 3),
    // terra
    (933, 2, 1, 1),
    (933, 3, 17, 3),
    (933, 3, 33, 4),
    (933, 12, 12, 3),
    (933, 12, 13, 2),
    (933, 13, 0, 2),
    (933, 14, 121, 1),
    (933, 20, 24, 3),
    (933, 20, 25, 2),
    (933, 20, 119, 1),
    (933, 21, 48, 1),
    // ozma
    (934, 2, 20, 2),
    (934, 2, 21, 1),
    (934, 2, 23, 2),
    (934, 3, 0, 3),
    (934, 3, 1, 1),
    (934, 3, 5, 2),
    (934, 3, 6, 1),
    (934, 3, 7, 2),
    (934, 3, 34, 1),
    (934, 4, 0, 1),
    (934, 21, 24, 3),
    (934, 22, 0, 4),
    (934, 23, 4, 3),
    (934, 23, 9, 4),
    (934, 25, 0, 4),
    (934, 25, 2, 3),
    (934, 25, 25, 4),
    (934, 25, 41, 3),
];

/// [`cast_arm_frame_step`] on the `pass`-th pass (from `0`) of the arm, the
/// passes counted through the band's two states, with the arm's frames
/// changing cost inside it ([`CAST_ARM_COST_TAILS`]).
pub fn cast_arm_frame_step_on_pass(
    entry: u32,
    phase: u8,
    pass: u16,
    timing: CdTiming,
) -> Option<u8> {
    let base = cast_arm_frame_step(entry, phase, timing)?;
    if timing != CdTiming::PcsxRedux {
        return Some(base);
    }
    Some(
        CAST_ARM_COST_TAILS
            .iter()
            .rfind(|&&(e, p, n, _)| e == entry && p == phase && pass >= n)
            .map_or(base, |&(.., step)| step),
    )
}

/// `(action id, (module phase, step))`, the capture band's reading of the same
/// measurement: a harvest state's cast (`*_pre_*`, `scripts/ci/cast-sequence.py`)
/// with the step each arm spends the most vsyncs on in state `0x70`, merged over
/// the fights a spell was recorded in, an arm under eight vsyncs left out as too short to
/// read. Keyed on the action, since one module's
/// bodies differ (PROT 0941, 0955). A phase no recording reached takes the default.
/// Read on PCSX-Redux ([`CdTiming::PcsxRedux`]).
const CAPTURE_ARM_STEPS: &[(u8, &[(u8, u8)])] = &[
    // earthquake
    (0x4A, &[(1, 4), (2, 4), (3, 4), (4, 4), (5, 4), (6, 3)]),
    // spore_gas
    (0x4F, &[(1, 4), (2, 4), (3, 4), (4, 4), (5, 2)]),
    // power_up
    (0x52, &[(1, 4), (2, 4), (3, 3)]),
    // big_wave: arm 7 (the wave's crest) runs `4` on both of its films,
    // Zeto's (`garmel_zeto_pre_big_wave`) and Slippery's
    // (`battle_jou_slippery_v3_big_wave_mid_cast`); arm 8 splits by fight
    // (`3` and `4`) and keeps `4`
    (0x56, &[(5, 4), (6, 4), (7, 4), (8, 4)]),
    // v_windhash
    (0x57, &[(1, 4), (2, 4), (3, 4), (4, 4), (5, 3)]),
    // cross_beam
    (0x58, &[(0, 3), (1, 4), (2, 4), (3, 4), (4, 4), (5, 3)]),
    // rolling_flare
    (0x5A, &[(3, 4), (4, 4)]),
    // fatal_decision: the wheel's arms draw next to nothing, and a frame costs
    // under a field
    (
        0x5F,
        &[
            (1, 3),
            (2, 2),
            (4, 3),
            (5, 3),
            (6, 1),
            (7, 1),
            (8, 3),
            (9, 3),
            (10, 3),
            (12, 2),
        ],
    ),
    // scythe_wind
    (0x5B, &[(0, 3), (1, 3), (2, 3), (3, 4), (4, 4), (5, 3)]),
    // terror_scream
    (0x70, &[(1, 4), (2, 4), (3, 4), (4, 4)]),
    // void_accessories
    (0x73, &[(1, 4), (2, 4), (3, 4), (4, 4)]),
    // element_change
    (0xAF, &[(1, 4), (2, 3)]),
    // rogue_wind
    (
        0xB0,
        &[
            (1, 4),
            (50, 4),
            (51, 3),
            (52, 4),
            (53, 4),
            (54, 4),
            (55, 4),
            (56, 1),
        ],
    ),
    // rogue_thunder
    (
        0xB1,
        &[
            (1, 4),
            (100, 3),
            (101, 3),
            (102, 3),
            (103, 3),
            (104, 4),
            (105, 4),
            (106, 1),
        ],
    ),
    // rogue_flame
    (
        0xB2,
        &[
            (1, 4),
            (2, 3),
            (3, 3),
            (4, 4),
            (5, 4),
            (6, 4),
            (7, 4),
            (8, 1),
        ],
    ),
    // stone_circle
    (0xB9, &[(1, 4), (2, 4), (3, 4), (4, 3)]),
    // The enemy cast films (`battle_*_mid_cast`, `--physical`, the step each
    // arm of `0x70` spends the most vsyncs on, merged over the films).
    // glare
    (0x3C, &[(1, 4), (2, 4), (3, 4)]),
    // hyper_crush
    (
        0x4B,
        &[
            (1, 4),
            (2, 4),
            (3, 4),
            (4, 4),
            (5, 4),
            (6, 4),
            (7, 4),
            (8, 3),
        ],
    ),
    // hyper_lightning
    (
        0x4C,
        &[(1, 4), (2, 4), (3, 4), (4, 4), (5, 4), (6, 4), (7, 3)],
    ),
    // kiss_of_death: arm 0 and the pull-in run at `4` under the lips, arm 5
    // (the victim alone) at `2`
    (0x6E, &[(1, 4), (2, 4), (3, 4), (4, 4), (5, 2)]),
    // divide
    (0x50, &[(1, 4), (2, 4), (3, 3)]),
    // steal: the thief's dash and the item flash draw next to nothing
    (0x51, &[(1, 3), (2, 3), (3, 3)]),
    // water_column
    (0x54, &[(1, 4), (2, 4), (4, 4), (5, 4), (6, 4), (7, 3)]),
    // water_crystals
    (0x59, &[(1, 4), (2, 4), (3, 4), (4, 4), (5, 2)]),
    // chaos_flare
    (
        0x36,
        &[
            (1, 3),
            (3, 4),
            (4, 4),
            (5, 4),
            (6, 4),
            (7, 4),
            (8, 4),
            (9, 4),
            (10, 4),
            (11, 2),
        ],
    ),
    // terio_punch
    (0x5D, &[(5, 4), (6, 4), (7, 4), (8, 4)]),
    // bull_charge
    (0x5E, &[(2, 4)]),
    // melt_spray
    (0x6F, &[(1, 3), (3, 4), (4, 4)]),
    // water_hazard
    (0x71, &[(1, 4), (2, 4), (3, 4)]),
    // thunder_storm: every arm `4` on all three fights
    (0x77, &[(1, 4), (2, 4), (3, 4), (4, 4), (5, 4), (6, 4)]),
    // bloody_horns (Xain's cast, `battle_tunnelc_xain_strike --monster-pick`)
    (0x5C, &[(3, 4), (4, 4), (5, 4)]),
    // megaton_press: the duel's pull-ins and the closing dive read `4`
    (
        0x7A,
        &[
            (2, 4),
            (3, 4),
            (4, 4),
            (5, 3),
            (7, 3),
            (9, 4),
            (10, 4),
            (11, 4),
            (12, 3),
            (15, 4),
        ],
    ),
    // plasma_strike
    (
        0x7B,
        &[
            (2, 4),
            (3, 4),
            (4, 4),
            (5, 4),
            (6, 4),
            (7, 4),
            (9, 4),
            (11, 4),
            (13, 4),
            (14, 4),
        ],
    ),
    // white_shield
    (0x60, &[(1, 4), (2, 4), (3, 3)]),
    // power_charge
    (0x72, &[(1, 4), (2, 4), (3, 4)]),
    // paralyzing_wave
    (0x75, &[(1, 4), (2, 4), (3, 4)]),
    // death_game: retail runs the dice arms at `1` and `3` (arm 1 `4`, arms
    // 2..3 `3`, 4..8 `1`, 9 and 11 `3`), but the port's arms 3 and 5 score
    // `.94` to `.79` and `.99` to `.78` on those steps against the ordinary
    // one, so the module's bodies still count something per pass that retail
    // counts per vsync. Held at the ordinary step until they do.
    (0x76, &[]),
    // dark_typhoon
    (0xAA, &[(1, 4), (2, 4), (3, 4), (4, 3), (5, 4), (6, 4)]),
    // The boss casts, off their harvest states (`chitei2_cort_pre_*`,
    // `garmel_zeto_pre_*`, `nilboa_koru_pre_*`, `--caster 3`), read as above.
    // guilty_cross: arm 5 (the cut back) splits `2` / `3` near evenly and
    // keeps the ordinary step
    (0x37, &[(1, 4), (2, 4), (3, 4), (4, 4)]),
    // call_wave: arm 1 is too short to read
    (0x55, &[(2, 4), (3, 3)]),
    // blade_breath, thunder_needle, gigaton_press: a lone Vahn's arm 1 runs
    // the ordinary `2`, the breath and the hit `4`; thunder needle's hit
    // mostly runs `3`, and on the ring after arm 3's `4`
    (0xA2, &[(3, 4), (4, 4)]),
    (0xA3, &[(3, 4)]),
    (0xA4, &[(3, 4), (4, 4)]),
    // ultra_charge
    (0xA5, &[(1, 4)]),
    // mystic_shield: the shield's last arms draw the party alone
    (
        0xAC,
        &[(1, 4), (2, 4), (3, 4), (4, 4), (5, 4), (6, 2), (7, 2)],
    ),
    // mystic_circle
    (0xB7, &[(1, 4), (2, 4), (3, 3), (4, 4)]),
    // evil_seru_magic (`chitei2_cort_pre_mystic_circle`, the full party): every
    // arm to the sweeps' end clamps but the gather arm 24, whose cost climbs
    // with its parts ([`CAPTURE_ARM_COST_TAILS`]); arm 28 draws the party alone
    (
        0xAD,
        &[
            (1, 4),
            (2, 4),
            (3, 4),
            (4, 4),
            (10, 4),
            (11, 4),
            (12, 4),
            (13, 4),
            (14, 4),
            (15, 4),
            (16, 4),
            (17, 4),
            (18, 4),
            (19, 4),
            (20, 4),
            (21, 4),
            (22, 4),
            (23, 4),
            (24, 3),
            (25, 4),
            (26, 4),
            (27, 4),
            (28, 2),
        ],
    ),
];

/// [`CAPTURE_ARM_STEPS`] with each arm a voice clip's stop watch polls
/// through read at a console's share ([`CdTiming::Console`]), off `--cd-trace`
/// recordings of the enemy cast films (`scripts/ci/cd-poll-steps.py`). An
/// arm no clip spans keeps the PCSX-Redux reading.
const CONSOLE_CAPTURE_ARM_STEPS: &[(u8, &[(u8, u8)])] = &[
    // earthquake
    (0x4A, &[(1, 3), (2, 3), (3, 3), (4, 4), (5, 4), (6, 3)]),
    // spore_gas
    (0x4F, &[(1, 3), (2, 3), (3, 3), (4, 3), (5, 2)]),
    // power_up
    (0x52, &[(1, 4), (2, 4), (3, 3)]),
    // big_wave
    (0x56, &[(5, 3), (6, 4), (7, 4), (8, 4)]),
    // v_windhash
    (0x57, &[(1, 2), (2, 2), (3, 2), (4, 3), (5, 3)]),
    // cross_beam
    (0x58, &[(0, 3), (1, 3), (2, 3), (3, 3), (4, 3), (5, 2)]),
    // rolling_flare
    (0x5A, &[(3, 3), (4, 3)]),
    // fatal_decision
    (
        0x5F,
        &[
            (1, 3),
            (2, 1),
            (4, 1),
            (5, 2),
            (6, 1),
            (7, 1),
            (8, 3),
            (9, 3),
            (10, 3),
            (12, 2),
        ],
    ),
    // scythe_wind
    (0x5B, &[(0, 3), (1, 3), (2, 3), (3, 4), (4, 4), (5, 3)]),
    // terror_scream
    (0x70, &[(1, 3), (2, 2), (3, 2), (4, 2)]),
    // void_accessories
    (0x73, &[(1, 3), (2, 2), (3, 3), (4, 4)]),
    // element_change
    (0xAF, &[(1, 2), (2, 2)]),
    // rogue_wind
    (
        0xB0,
        &[
            (1, 2),
            (50, 2),
            (51, 2),
            (52, 2),
            (53, 2),
            (54, 4),
            (55, 4),
            (56, 1),
        ],
    ),
    // rogue_thunder
    (
        0xB1,
        &[
            (1, 2),
            (100, 2),
            (101, 2),
            (102, 2),
            (103, 2),
            (104, 4),
            (105, 4),
            (106, 1),
        ],
    ),
    // rogue_flame
    (
        0xB2,
        &[
            (1, 2),
            (2, 2),
            (3, 2),
            (4, 2),
            (5, 2),
            (6, 4),
            (7, 4),
            (8, 1),
        ],
    ),
    // stone_circle
    (0xB9, &[(1, 2), (2, 4), (3, 4), (4, 3)]),
    // glare
    (0x3C, &[(1, 2), (2, 2), (3, 2)]),
    // hyper_crush
    (
        0x4B,
        &[
            (1, 2),
            (2, 3),
            (3, 3),
            (4, 4),
            (5, 4),
            (6, 4),
            (7, 4),
            (8, 3),
        ],
    ),
    // hyper_lightning
    (
        0x4C,
        &[(1, 3), (2, 3), (3, 3), (4, 3), (5, 4), (6, 4), (7, 3)],
    ),
    // kiss_of_death
    (0x6E, &[(1, 2), (2, 3), (3, 3), (4, 3), (5, 2)]),
    // divide
    (0x50, &[(1, 2), (2, 3), (3, 3)]),
    // steal
    (0x51, &[(1, 3), (2, 3), (3, 3)]),
    // water_column
    (0x54, &[(1, 3), (2, 3), (4, 3), (5, 3), (6, 4), (7, 3)]),
    // water_crystals
    (0x59, &[(1, 2), (2, 4), (3, 4), (4, 4), (5, 2)]),
    // chaos_flare
    (
        0x36,
        &[
            (1, 3),
            (3, 4),
            (4, 4),
            (5, 4),
            (6, 4),
            (7, 4),
            (8, 4),
            (9, 4),
            (10, 4),
            (11, 2),
        ],
    ),
    // terio_punch
    (0x5D, &[(5, 3), (6, 3), (7, 4), (8, 4)]),
    // bull_charge
    (0x5E, &[(2, 2)]),
    // melt_spray
    (0x6F, &[(1, 3), (3, 4), (4, 4)]),
    // water_hazard
    (0x71, &[(1, 2), (2, 4), (3, 4)]),
    // thunder_storm
    (0x77, &[(1, 2), (2, 4), (3, 3), (4, 3), (5, 4), (6, 4)]),
    // bloody_horns
    (0x5C, &[(3, 3), (4, 3), (5, 3)]),
    // megaton_press
    (
        0x7A,
        &[
            (2, 3),
            (3, 3),
            (4, 3),
            (5, 3),
            (7, 1),
            (9, 3),
            (10, 4),
            (11, 3),
            (12, 3),
            (15, 4),
        ],
    ),
    // plasma_strike
    (
        0x7B,
        &[
            (2, 4),
            (3, 4),
            (4, 4),
            (5, 3),
            (6, 2),
            (7, 2),
            (9, 2),
            (11, 2),
            (13, 2),
            (14, 2),
        ],
    ),
    // white_shield
    (0x60, &[(1, 4), (2, 3), (3, 3)]),
    // power_charge
    (0x72, &[(1, 2), (2, 2), (3, 3)]),
    // paralyzing_wave
    (0x75, &[(1, 3), (2, 3), (3, 4)]),
    // death_game
    (0x76, &[]),
    // dark_typhoon
    (0xAA, &[(1, 4), (2, 3), (3, 4), (4, 3), (5, 4), (6, 4)]),
    // guilty_cross
    (0x37, &[(1, 3), (2, 3), (3, 3), (4, 3)]),
    // call_wave
    (0x55, &[(2, 4), (3, 3)]),
    // blade_breath
    (0xA2, &[(3, 3), (4, 3)]),
    // thunder_needle
    (0xA3, &[(3, 4)]),
    // gigaton_press
    (0xA4, &[(3, 3), (4, 4)]),
    // ultra_charge
    (0xA5, &[(1, 4)]),
    // mystic_shield
    (
        0xAC,
        &[(1, 3), (2, 3), (3, 3), (4, 4), (5, 4), (6, 2), (7, 2)],
    ),
    // mystic_circle
    (0xB7, &[(1, 4), (2, 4), (3, 3), (4, 4)]),
];

/// The step retail ran module phase `phase` of the capture cast `spell` at:
/// the table's reading for a recorded cast, else `None` (the ordinary step).
/// A cast no film records keeps the ordinary step too. Over the enemy cast
/// films `.62` of such arms' vsyncs run at `4`, but a still whose gate
/// counts passes moves onto another pass at `4` (the boss card casts from
/// `0xA6`, `gaza2_card_neo_star_slash_mid_cast` `.999` to `.739`), so the
/// step waits for a recording of each.
pub fn capture_arm_frame_step(spell: u8, phase: u8, timing: CdTiming) -> Option<u8> {
    let table = match timing {
        CdTiming::Console => CONSOLE_CAPTURE_ARM_STEPS,
        CdTiming::PcsxRedux => CAPTURE_ARM_STEPS,
    };
    let (_, steps) = table.iter().find(|(s, _)| *s == spell)?;
    steps.iter().find(|(p, _)| *p == phase).map(|&(_, s)| s)
}

/// Capture-band arms whose step falls inside the arm when the module's own
/// CD-XA line stops loading the frame: `(spell, phase, passes, step)`, the arm
/// running its [`CAPTURE_ARM_STEPS`] step for its first `passes` passes and
/// `step`, its own frames' cost, from there on. PCSX-Redux only: a console's
/// table already reads the arm at its own cost.
///
/// Kiss of Death's arm 4 (PROT 0955) is the case. The line's read holds the
/// drive word until `444` vsyncs after its start (a `--watch` recording), and
/// both PCSX-Redux recordings (`battle_map01_lippian_kiss_of_death_mid_cast`,
/// `battle_retona_lip_king_kiss_of_death_mid_cast`) drop from `4` to `3` at
/// vsync `445` / `448`, with `28` / `27` of the arm's passes behind them at
/// `4`. The arm drains its countdown `scalar * step` a pass
/// (`0x801F8C6C..0x801F8CA4`) and its one `rand()` past the gate is the
/// hit-or-miss coin (`0x801F8CBC`), so the pass count is the roll: run every
/// pass at `4` the arm is two passes short and both films roll the miss where
/// retail's hit (`26` to `28` passes at `4` roll retail's hit on both).
pub const CAPTURE_ARM_READ_TAILS: &[(u8, u8, u16, u8)] = &[(0x6E, 4, 27, 3)];

/// Capture-band arms whose own draw load changes cost class inside the arm:
/// `(spell, phase, pass, step)`, the arm's frames reading `step` from its
/// `pass`-th pass (from `0`) on, the capture band's
/// [`CAST_ARM_COST_TAILS`]. PCSX-Redux only.
///
/// Evil Seru Magic's gather arm 24 (PROT 0966) is the case. Its recording
/// (`chitei2_cort_pre_mystic_circle`) costs `512`..`548` hblanks for the arm's
/// first `20` frames, `480`..`493` for the next `25`, `533` and up for `29`
/// more and the clamp for the last `43`; behind arm 23's clamped entries the
/// ring reads `3`, `2` for ten frames, `3` and `4`. The arm drains its
/// countdown by the step, so the classes are its pass count: `117` frames on
/// retail, `97` on one step of `4`.
pub const CAPTURE_ARM_COST_TAILS: &[(u8, u8, u16, u8)] =
    &[(0xAD, 24, 20, 2), (0xAD, 24, 45, 3), (0xAD, 24, 74, 4)];

/// [`capture_arm_frame_step`] on the `pass`-th pass (from `0`) of the arm,
/// with a line's read ending inside it ([`CAPTURE_ARM_READ_TAILS`]) or the
/// arm's own cost changing class ([`CAPTURE_ARM_COST_TAILS`]).
pub fn capture_arm_frame_step_on_pass(
    spell: u8,
    phase: u8,
    pass: u16,
    timing: CdTiming,
) -> Option<u8> {
    let tail = (timing == CdTiming::PcsxRedux)
        .then(|| {
            CAPTURE_ARM_READ_TAILS
                .iter()
                .find(|&&(s, p, n, _)| s == spell && p == phase && pass >= n)
        })
        .flatten();
    let tail = tail.or_else(|| {
        (timing == CdTiming::PcsxRedux)
            .then(|| {
                CAPTURE_ARM_COST_TAILS
                    .iter()
                    .rfind(|&&(s, p, n, _)| s == spell && p == phase && pass >= n)
            })
            .flatten()
    });
    tail.map(|&(.., step)| step)
        .or_else(|| capture_arm_frame_step(spell, phase, timing))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_summon_arm_changes_cost_on_its_measured_pass() {
        let pcsx = CdTiming::PcsxRedux;
        assert_eq!(cast_arm_frame_step_on_pass(934, 21, 0, pcsx), Some(4));
        assert_eq!(cast_arm_frame_step_on_pass(934, 21, 23, pcsx), Some(4));
        assert_eq!(cast_arm_frame_step_on_pass(934, 21, 24, pcsx), Some(3));
        assert_eq!(cast_arm_frame_step_on_pass(903, 12, 19, pcsx), Some(4));
        assert_eq!(cast_arm_frame_step_on_pass(903, 12, 20, pcsx), Some(3));
        // An arm can open on one frame of another cost.
        assert_eq!(cast_arm_frame_step_on_pass(903, 4, 0, pcsx), Some(3));
        assert_eq!(cast_arm_frame_step_on_pass(903, 4, 1, pcsx), Some(4));
        assert_eq!(cast_arm_frame_step_on_pass(903, 12, 200, pcsx), Some(1));
        assert_eq!(cast_arm_frame_step_on_pass(933, 3, 0, pcsx), Some(2));
        assert_eq!(cast_arm_frame_step_on_pass(933, 3, 32, pcsx), Some(3));
        assert_eq!(cast_arm_frame_step_on_pass(933, 3, 33, pcsx), Some(4));
        // An arm with no row keeps its table step, an unread arm stays unread.
        assert_eq!(cast_arm_frame_step_on_pass(934, 20, 500, pcsx), Some(4));
        assert_eq!(cast_arm_frame_step_on_pass(903, 0, 0, pcsx), None);
        // A console's table reads each arm at its own cost throughout.
        assert_eq!(
            cast_arm_frame_step_on_pass(934, 21, 40, CdTiming::Console),
            cast_arm_frame_step(934, 21, CdTiming::Console)
        );
        // Every row names an arm the table holds, and an arm's rows climb
        // by pass (the lookup takes the last one at or under the pass).
        for &(e, p, _, s) in CAST_ARM_COST_TAILS {
            assert!(cast_arm_frame_step(e, p, pcsx).is_some(), "{e} arm {p}");
            assert!((1..=4).contains(&s));
        }
        for w in CAST_ARM_COST_TAILS.windows(2) {
            let (a, b) = (w[0], w[1]);
            assert!((a.0, a.1, a.2) < (b.0, b.1, b.2), "{a:?} before {b:?}");
        }
    }

    #[test]
    fn kiss_of_death_arm_4_falls_to_its_own_step_when_the_line_stops_loading() {
        let pcsx = CdTiming::PcsxRedux;
        assert_eq!(capture_arm_frame_step_on_pass(0x6E, 4, 0, pcsx), Some(4));
        assert_eq!(capture_arm_frame_step_on_pass(0x6E, 4, 26, pcsx), Some(4));
        assert_eq!(capture_arm_frame_step_on_pass(0x6E, 4, 27, pcsx), Some(3));
        assert_eq!(capture_arm_frame_step_on_pass(0x6E, 3, 40, pcsx), Some(4));
        assert_eq!(capture_arm_frame_step_on_pass(0x6E, 5, 0, pcsx), Some(2));
        // A console's table reads the arm at its own cost throughout.
        assert_eq!(
            capture_arm_frame_step_on_pass(0x6E, 4, 0, CdTiming::Console),
            Some(3)
        );
    }

    #[test]
    fn a_whole_screen_arm_reads_four_and_the_band_opening_one() {
        let pcsx = CdTiming::PcsxRedux;
        assert_eq!(cast_arm_frame_step(921, 2, pcsx), Some(1));
        // Iota's one-pass arm 4 reads its frame's clamped cost.
        assert_eq!(cast_arm_frame_step(921, 4, pcsx), Some(4));
        assert_eq!(cast_arm_frame_step(921, 5, pcsx), Some(4));
        assert_eq!(cast_arm_frame_step(921, 200, pcsx), None);
        assert_eq!(cast_arm_frame_step(960, 1, pcsx), None);
    }

    #[test]
    fn a_console_reads_the_voice_arms_on_their_own_work() {
        // Freed's arms 3 to 6 poll the drive for the whole voice clip: two
        // thirds of PCSX-Redux's CPU, a sixteenth of a console's.
        for arm in 3..=6 {
            assert_eq!(cast_arm_frame_step(912, arm, CdTiming::PcsxRedux), Some(4));
            assert_eq!(cast_arm_frame_step(912, arm, CdTiming::Console), Some(2));
        }
        // The band's opening arms play no clip and read the same on both.
        assert_eq!(cast_arm_frame_step(912, 2, CdTiming::Console), Some(1));
        assert_eq!(console_poll_cost(400), 427);
    }

    #[test]
    fn the_capture_band_reads_by_action_and_arm() {
        // V-Windhash's blade arms: a trail seated per pass seats half as many
        // at `4` as at the default `2`.
        let pcsx = CdTiming::PcsxRedux;
        assert_eq!(capture_arm_frame_step(0x57, 3, pcsx), Some(4));
        assert_eq!(capture_arm_frame_step(0x57, 0, pcsx), None);
        assert_eq!(capture_arm_frame_step(0x57, 200, pcsx), None);
        assert_eq!(capture_arm_frame_step(0x76, 5, pcsx), None);
        // Thunder Storm runs every arm at `4` in all three fights recorded.
        assert_eq!(capture_arm_frame_step(0x77, 4, pcsx), Some(4));
        // Steal draws next to nothing; a cast no film records keeps the default.
        assert_eq!(capture_arm_frame_step(0x51, 2, pcsx), Some(3));
        assert_eq!(capture_arm_frame_step(0x01, 1, pcsx), None);
        assert_eq!(capture_arm_frame_step(0x01, 0, pcsx), None);
        // Evil Seru Magic's gather arm changes class three times.
        for (pass, step) in [(0, 3), (19, 3), (20, 2), (44, 2), (45, 3), (74, 4)] {
            assert_eq!(
                capture_arm_frame_step_on_pass(0xAD, 24, pass, pcsx),
                Some(step)
            );
        }
        assert_eq!(
            capture_arm_frame_step_on_pass(0xAD, 24, 80, CdTiming::Console),
            None
        );
        // Guilty Cross's voiced arms read `3` on a console, as mednafen's
        // `cort_guilty_cross_mid_cast` ring does.
        for arm in 1..=4 {
            assert_eq!(capture_arm_frame_step(0x37, arm, pcsx), Some(4));
            assert_eq!(
                capture_arm_frame_step(0x37, arm, CdTiming::Console),
                Some(3)
            );
        }
    }
}
