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
    (903, &[0, 0, 1, 1, 4, 4, 1, 4, 4, 4, 4, 4, 4]),
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
            0, 1, 3, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 2, 2, 2, 2, 2, 2, 2, 2,
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

/// [`capture_arm_frame_step`] on the `pass`-th pass (from `0`) of the arm,
/// with a line's read ending inside it ([`CAPTURE_ARM_READ_TAILS`]).
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
    tail.map(|&(.., step)| step)
        .or_else(|| capture_arm_frame_step(spell, phase, timing))
}

#[cfg(test)]
mod tests {
    use super::*;

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
