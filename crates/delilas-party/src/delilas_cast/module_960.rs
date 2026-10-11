//! Cast module 960 (PROT 0960): the stage-remap, MP5 hold, damage, wipe, hazard
//! and teardown word edits and their installer.

use super::stage_caves::{module_960_unfold_edits, stage_cave_layout};
use super::{WordEdit, apply_word_edits, move_};
use anyhow::{Context, Result};
use legaia_code_hooks::mips::{self as m, A0, A1, A2, S3, V0, V1, ZERO};
use legaia_disc_patch::disc::DiscPatcher;

/// PROT 960 (Plasma Strike): consolidate the caster's staged-clip walk
/// into the two player-representable rows.
///
/// Unlike 958's increment chain, every 960 stage is literal-fed, in
/// module order: `li 0xE` (`0x801F7740`), `li 0xC` (`0x801F7A68`, dest
/// `$v1`), `li 0xD` (`0x801F7ADC`), `li 0xF` (`0x801F820C`) - the
/// probe-measured sequence `0E,0C,0D,0F` (three build stages, then the
/// burst). The `li 0xA` opener at `0x801F6B94` is already in range, and
/// the remaining `+0x1DA` stores are victim staging or wipes.
///
/// Mapping: the three build stages fold onto the windup row `0x0A`
/// (each restage restarts the clip - the channel loop), and the burst
/// lands on the payoff row `0x0B`. Same player-table rationale and
/// enemy-side trade-off as [`MODULE_958_STAGE_REMAP_EDITS`].
///
/// One stage carries a PAIRED CONFIRMATION GATE that must move in
/// lockstep: the mp5 arm stages id `0x0D` per tick, then holds the
/// phase until `lbu actor+0x1D9` (the PLAYING id) equals the same
/// literal (`0x801F7B60..68`) AND a progress halfword reaches `0x90`.
/// Remapping the stage without the compare stalls mp5 forever
/// (probe-measured on a slot-4 natural playout: restage/one-loop cycle
/// repeating past 2000 frames, finale never reached). The compare
/// literal folds to `0x0A` with the stage; the progress condition
/// keeps the exit timing. 959 never needed this - its gates compare
/// `+0x1D9` against `+0x1F2`, register-vs-register - and 958 has no
/// caster-literal gate at all (its tail gates use the settle id `8`,
/// which the remap never stages).
pub(super) const MODULE_960_STAGE_REMAP_EDITS: &[WordEdit] = &[
    WordEdit {
        // 0x801F7740: opening raise -> windup row.
        offset: 0x0D68,
        expect: 0x2402_000E, // li v0, 0xE
        replace: m::addiu(V0, ZERO, 0x000A),
    },
    WordEdit {
        // 0x801F7A68: mid channel -> windup restage (NB dest is $v1).
        offset: 0x1090,
        expect: 0x2403_000C, // li v1, 0xC
        replace: m::addiu(V1, ZERO, 0x000A),
    },
    WordEdit {
        // 0x801F7ADC: pre-burst -> windup restage.
        offset: 0x1104,
        expect: 0x2402_000D, // li v0, 0xD
        replace: m::addiu(V0, ZERO, 0x000A),
    },
    WordEdit {
        // 0x801F820C: the burst stages the payoff row.
        offset: 0x1834,
        expect: 0x2402_000F, // li v0, 0xF
        replace: m::addiu(V0, ZERO, 0x000B),
    },
    WordEdit {
        // 0x801F7B64: the mp5 played-id confirmation gate follows its stage.
        offset: 0x118C,
        expect: 0x2402_000D, // li v0, 0xD (compared against lbu +0x1D9)
        replace: m::addiu(V0, ZERO, 0x000A),
    },
];

/// PROT 960: re-time the mp5 hold to the retail schedule.
///
/// Retail's mp5 arm holds phase 5 until the PREVIOUS stage's clip ends
/// (`lbu +0x1D9 == 0x0D` - a clip-boundary wait worth ~320 ticks on the
/// enemy walk) and the playhead cursor (`lh [*(actor+0x22C)]+0x68`)
/// reaches `0x90`. The stage-row fold breaks that clock: every stage
/// stages row `0x0A`, so the playing-id half of the gate is true the
/// tick mp5 opens, and the cursor half is nearly met by the looping
/// windup - mp5 exits ~50 ticks in, pulling the burst/damage stage from
/// retail's `mph0+909` up to `+829` (probe-measured both sides). The
/// XA bed is authored against the retail schedule: its blast bump sits
/// at stream `+14.8 s`, which with the real-CD stream-start latency of
/// actual hardware / DuckStation lands exactly on retail's `+909`
/// damage. On the folded walk the whiteout ran ~2-3.5 s AHEAD of the
/// blast (worse the more accurate the CD timing), which is the
/// user-reported "audio starts late" - the bed was never late; the
/// walk was early.
///
/// Fix: replace the cursor half of the gate with a deterministic tick
/// counter. The counter cell is a dead word INSIDE the module image
/// (`0x801F8588`, first word of the party-wipe body that
/// [`MODULE_960_WIPE_EDIT`] unreaches; zeroed on disc by the edit
/// below) - the module re-streams from disc at every cast, so the
/// counter self-resets to zero with no reset code. NB the wipe body's
/// FIRST words (`0x801F8588..0x85AC`) are already claimed by the
/// teardown cave ([`MODULE_960_TEARDOWN_EDITS`]) - the counter takes
/// `0x801F85B0`, the first word past its rejoin. The caster's
/// `+0x176` hold cell is NOT usable: it is the clip player's live
/// hold budget, and writing it freezes the staged clip (probe-measured
/// deadlock, cursor pinned at 0). The cursor itself is not a safe gate
/// for longer holds either - the fold's per-stage re-commit wraps it.
/// The count+1 store rides the wait branch's delay slot; the wait
/// path's return value stays `1` because `slti` is `1` exactly when
/// the branch is taken (retail set it via `move v0, fp` with `fp = 1`).
/// `MP5_HOLD_TICKS` is probe-calibrated against the retail enemy walk
/// (damage stage at `mph0+909`, walk end `+1264`). The arm is entered
/// once per ~4-5 vsyncs (the restage cadence) and the later stages'
/// spans shift with the windup cursor's wrap phase at mp5 exit, so the
/// response is jagged; the sweep's floor is `0x1E` -> damage stage at
/// `mph0+938`, walk end `+1324` (retail `+909`/`+1264`), vs `+829` /
/// `+1190` unpatched. That puts the bed's blast bump (stream `+14.8 s`
/// plus real-CD stream-start latency) back at the damage whiteout the
/// way the retail schedule has it.
const MP5_HOLD_TICKS: i16 = 0x1E;
/// VA `0x801F85B0` = `lui v1, 0x8020` base minus `0x7A50`.
const MP5_COUNTER_LO: u16 = 0x85B0;
const MODULE_960_MP5_HOLD_EDITS: &[WordEdit] = &[
    WordEdit {
        // 0x801F7B70: cursor deref -> counter-cell base.
        offset: 0x1198,
        expect: 0x8E42_022C, // lw v0, 0x22C(s2)
        replace: m::lui(V1, 0x8020),
    },
    WordEdit {
        // 0x801F7B74: the old load-delay nop -> counter load.
        offset: 0x119C,
        expect: 0x0000_0000, // nop
        replace: m::lw(A0, V1, MP5_COUNTER_LO),
    },
    WordEdit {
        // 0x801F7B78: cursor load -> the counter load's delay slot.
        offset: 0x11A0,
        expect: 0x8442_0068, // lh v0, 0x68(v0)
        replace: m::nop(),
    },
    WordEdit {
        // 0x801F7B7C: free slot -> counter bump.
        offset: 0x11A4,
        expect: 0x0000_0000, // nop
        replace: m::addiu(A0, A0, 1),
    },
    WordEdit {
        // 0x801F7B80: threshold; the `bnez -> wait` that follows stays.
        offset: 0x11A8,
        expect: 0x2842_0090, // slti v0, v0, 0x90
        replace: m::slti(V0, A0, MP5_HOLD_TICKS),
    },
    WordEdit {
        // 0x801F7B88: the branch delay (`move v0, fp`) -> counter store.
        offset: 0x11B0,
        expect: 0x03C0_1021, // move v0, fp
        replace: m::sw(A0, V1, MP5_COUNTER_LO),
    },
    WordEdit {
        // 0x801F85B0: the dead wipe-body word becomes the counter cell
        // (first word past the teardown cave's rejoin).
        offset: 0x1BD8,
        expect: 0xA0A2_BD60, // sb v0, -0x42A0(a1) (unreached)
        replace: 0x0000_0000,
    },
];

/// PROT 960 (Plasma Strike): the two hardcoded `table[0]` damage/HP
/// loads (`$s6` holds `0x801D0000` tick-wide). The tick derives the
/// victim into `$s3` once (file `+0x0B6C`) and never rewrites it before
/// the burst pair - the pair's own tail writes the victim's `+0x4`
/// through `$s3` three instructions later - so plain `move`s are safe.
const MODULE_960_DAMAGE_EDITS: &[WordEdit] = &[
    WordEdit {
        offset: 0x17AC,
        expect: 0x8EC5_9370, // lw a1, -0x6c90(s6)
        replace: move_(A1, S3),
    },
    WordEdit {
        offset: 0x17DC,
        expect: 0x8EC6_9370, // lw a2, -0x6c90(s6)
        replace: move_(A2, S3),
    },
];

/// PROT 960 finale (phase `0x10` of the tick's `beq`-chain dispatcher):
/// the dead-victim arm, same fix as [`MODULE_958_WIPE_EDIT`]. The HP
/// check itself reads the derived victim (`lhu +0x14C($s3)`); only the
/// wipe consequence assumes a hero. Nop'd, a dead victim falls through
/// the bit-`0x80` fork's delay slot into the alive path (caster settle
/// staging + phase `0xFF`). Single inbound branch verified.
const MODULE_960_WIPE_EDIT: WordEdit = WordEdit {
    offset: 0x1B88,
    expect: 0x1040_0009, // beqz v0, 0x801F8588 (the wipe body)
    replace: m::nop(),
};

/// PROT 960: the two seat-3 monster-record accesses. Both walk
/// `(0x801C9348)[0]` - the FIRST monster seat's record - to a pointer at
/// record `+0x80` and toggle the halfword at its `+0xC` (`1` in the
/// phase-`0xFF` settle arm at `+0x1C20`, back to `0` mid-walk at
/// `+0x1230`). In the retail duel the sole monster IS the caster, so
/// this is a caster-record-keyed transient; on a PLAYER cast seat 3
/// holds an arbitrary monster whose record `+0x80` word is not a
/// vetted pointer - the store lands wherever it points (the kernel-RAM
/// hazard). The loads stay (always-mapped reads); the stores are nop'd
/// for both caster kinds - the toggle's resting state is the value the
/// second store restores.
const MODULE_960_HAZARD_EDITS: &[WordEdit] = &[
    WordEdit {
        offset: 0x1230,
        expect: 0xA440_000C, // sh zero, 0xC(v0)
        replace: m::nop(),
    },
    WordEdit {
        offset: 0x1C20,
        expect: 0xA445_000C, // sh a1, 0xC(v0)
        replace: m::nop(),
    },
];

/// PROT 960 finale teardown: neutralise the cached finale entity, same
/// hazard class (and same fix) as [`MODULE_959_TEARDOWN_EDITS`].
///
/// 960 carries 959's exact halt-quad pattern (file `+0x19A8..+0x19C4`:
/// `lw a2, 0x102C(ctx); lw v1, 0x10(a2); ori v1, 8; sw`) - a spawned
/// finale entity cached at `ctx+0x102C` and halt-marked in the arm that
/// also arms the white-out fade. On a PARTY caster 959's equivalent
/// entity never gets stream words bound, and the script's kill mark
/// (`+0x10 |= 0x02000000`) turns the carrier's colour read into an
/// unmapped dereference on the exact frame the choreography ends
/// (see 959's table for the full attribution).
///
/// Host: phase `0x10`'s settle exit `j 0x801F85C0` (`+0x1BA8`) runs once
/// per cast, after the halt quad and before the phase-`0xFF` write it
/// jumps to; its delay slot (`li v0, 0xFF`) still executes and the stub
/// preserves `$v0` for the rejoined `sb v0, 0x279(s4)`. The stub lives
/// in the dead party-wipe body (`0x801F8588..0x801F85B0`,
/// [`MODULE_960_WIPE_EDIT`] unreached it): null-guarded, it writes the
/// entity's `+0x10`/`+0x14` back to the walk-safe zero state and clears
/// the cache word (`$s4` = ctx tick-wide).
const MODULE_960_TEARDOWN_EDITS: &[WordEdit] = &[
    WordEdit {
        offset: 0x1BA8,             // settle exit
        expect: 0x0807_E170,        // j 0x801F85C0 (phase := 0xFF write)
        replace: m::j(0x801F_8588), // j into the dead wipe body
    },
    WordEdit {
        offset: 0x1BB0, // 0x801F8588 (dead: lui v1)
        expect: 0x3C03_8008,
        replace: m::lw(A2, m::S4, 0x102C), // a2 = cached finale entity
    },
    WordEdit {
        offset: 0x1BB4, // 0x801F858C (dead: li 0xFE)
        expect: 0x2402_00FE,
        replace: m::nop(), // load delay
    },
    WordEdit {
        offset: 0x1BB8, // 0x801F8590 (dead: lui a2)
        expect: 0x3C06_8008,
        replace: m::beq(A2, ZERO, 5), // null guard -> 0x801F85A8
    },
    WordEdit {
        offset: 0x1BBC, // 0x801F8594 (dead: lui a1): branch delay
        expect: 0x3C05_8008,
        replace: m::sw(ZERO, m::S4, 0x102C), // clear the cache (both paths)
    },
    WordEdit {
        offset: 0x1BC0, // 0x801F8598 (dead: sb wipe flag)
        expect: 0xA062_BD71,
        replace: m::sw(ZERO, A2, 0x10), // colour stream -> mapped null
    },
    WordEdit {
        offset: 0x1BC4, // 0x801F859C (dead: lbu)
        expect: 0x90A2_BD60,
        replace: m::sw(ZERO, A2, 0x14), // prim stream -> clean walk exit
    },
    WordEdit {
        offset: 0x1BC8, // 0x801F85A0 (dead: li 5)
        expect: 0x2403_0005,
        replace: m::j(0x801F_85C0), // rejoin the phase-0xFF write
    },
    WordEdit {
        offset: 0x1BCC, // 0x801F85A4 (dead: sw wipe-state): j delay slot
        expect: 0xACC3_BD2C,
        replace: m::nop(),
    },
    WordEdit {
        offset: 0x1BD0, // 0x801F85A8 (dead: andi): null-guard target
        expect: 0x3042_007F,
        replace: m::j(0x801F_85C0),
    },
    WordEdit {
        offset: 0x1BD4, // 0x801F85AC (dead: jal jingle): j delay slot
        expect: 0x0C00_C66A,
        replace: m::nop(),
    },
];

/// Apply the full PROT 960 cast-route patch set: the staged-id remap +
/// confirmation gate ([`MODULE_960_STAGE_REMAP_EDITS`]), the un-folded
/// stage caves when `unfold` carries Lu's authored entry offsets, the
/// two-site damage retarget, the dead-victim wipe skip and the seat-3
/// record-toggle neutralisation. Gaza's Neo Star Slash tick (`+0x8A8`)
/// shares this image and is untouched by every set.
pub fn patch_module_960(p: &mut DiscPatcher, unfold: Option<&[usize]>) -> Result<bool> {
    let entry = p.read_entry(960).context("read PROT 960")?;
    let mut edits: Vec<WordEdit> = MODULE_960_STAGE_REMAP_EDITS.to_vec();
    edits.extend_from_slice(MODULE_960_MP5_HOLD_EDITS);
    if let Some(offs) = unfold {
        edits.extend(module_960_unfold_edits(&stage_cave_layout()?, offs)?);
    }
    edits.extend_from_slice(MODULE_960_DAMAGE_EDITS);
    edits.push(MODULE_960_WIPE_EDIT);
    edits.extend_from_slice(MODULE_960_HAZARD_EDITS);
    edits.extend_from_slice(MODULE_960_TEARDOWN_EDITS);
    apply_word_edits(p, 960, &entry, &edits)
}
