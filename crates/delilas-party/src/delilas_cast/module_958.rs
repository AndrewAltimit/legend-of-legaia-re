//! Cast module 958 (PROT 0958): the stage-remap, damage, wipe and wait word
//! edits and their installer.

use super::stage_caves::{GATE_958_OFFSET, module_958_unfold_edits, stage_cave_layout};
use super::{WordEdit, apply_word_edits, move_};
use anyhow::{Context, Result};
use legaia_code_hooks::mips::{self as m, A0, A1, A3, S1, T0, T4, V0, V1, ZERO};
use legaia_disc_patch::disc::DiscPatcher;

/// PROT 958 (Blazing Slash): consolidate the caster's staged-clip walk
/// into the two player-representable rows.
///
/// The module drives the caster's staged index (`actor+0x1DA`; restage
/// counter `+0x1DC`) through six stages, in module order: `li 0xA`
/// (`0x801F6F84`), `+1` (`0x801F72B8`), `+1` (`0x801F7620`), `-2`
/// (`0x801F839C`), `+1` (`0x801F8544`), `li 0xD` (`0x801F89AC`) - the
/// probe-measured sequence `0A,0B,0C,0A,0B,0D` (two swings, then the
/// finale). Every other `+0x1DA` store in the image is victim staging
/// (`lbu +0x1F1`-fed), an `sb zero` wipe, or already in range.
///
/// A PARTY caster resolves staged ids through the player record[0]
/// action-offset table, which has exactly 12 rows (`0x00..0x0B`) - ids
/// `>= 0x0C` index past it. The staged-row machinery
/// (`legaia_party_swap::cast_stage`) authors real content on rows `0x0A`/`0x0B`
/// only, so the walk is folded to stay inside that pair:
/// `0A,0B,B(restage),A,B,B`. The restage still bumps the `+0x1DC`
/// counter, so each stage boundary restarts a clip rather than freezing.
/// The ENEMY-side cast shares the module and plays the folded walk too -
/// two of six stages replay the windup/smash clips; the effect barrage,
/// camera track and damage build-up are untouched.
pub(super) const MODULE_958_STAGE_REMAP_EDITS: &[WordEdit] = &[
    WordEdit {
        // 0x801F7620: the second step (0x0B -> 0x0C) restages 0x0B.
        offset: 0x0C48,
        expect: 0x2442_0001, // addiu v0, v0, 1
        replace: m::nop(),
    },
    WordEdit {
        // 0x801F839C: the swing reset (0x0C -> 0x0A) now steps 0x0B -> 0x0A.
        offset: 0x19C4,
        expect: 0x2442_FFFE, // addiu v0, v0, -2
        replace: m::addiu(V0, V0, 0xFFFF),
    },
    WordEdit {
        // 0x801F89AC: the finale stages the payoff row instead of id 0x0D.
        offset: 0x1FD4,
        expect: 0x2402_000D, // li v0, 0xD
        replace: m::addiu(V0, ZERO, 0x000B),
    },
];

/// The victim scratch cell for PROT 958's finale damage arms: the LAST
/// word of the dead party-wipe body ([`MODULE_958_WIPE_EDIT`] makes the
/// body unreachable). VA `0x801F8BB4`, addressed as
/// `lui base, 0x8020; lw/sw rX, -0x744C(base)`.
///
/// The module keeps the derived victim in `$s1` only per-arm - the
/// finale arm burns `$s1`/`$s2`/`$s3`/`$s4` as GPU-packet constants
/// before its two damage pairs (`li $s1, 0x40` at file `+0x1CF4`,
/// `lui $s2, 0x801d` at `+0x1CD0`) - so the FIRST damage arm (whose
/// `$s1` is verified intact from arm entry, and whose own body stages
/// the victim's `+0x1F1` through `$s1` two instructions earlier) stores
/// the victim pointer here, and the finale pairs load it back. The walk
/// always runs the first hit before the finale, and the module is
/// re-read from disc each cast, so the cell is written before every
/// finale read.
const CELL_958_HI: u16 = 0x8020;
const CELL_958_LO: u16 = 0x8BB4; // 0x80200000 - 0x744C = 0x801F8BB4

/// PROT 958 (Blazing Slash): the twelve hardcoded `table[0]` damage/HP
/// loads - six clamp/write pairs, one per damage arm. Arms 1-4 keep the
/// derived victim in `$s1` (zero `$s1` writes from each arm's dispatch
/// entry to its pair - verified over the phase-table entries), so their
/// loads become plain `move`s; arm 1 additionally banks the victim in
/// [`CELL_958_LO`] for the two finale arms, whose registers are burned.
/// Every load sits in the retail `lw; nop; use` shape - no replacement
/// introduces a load-use hazard (`move`/`sw` are ALU/store ops, and the
/// cell `lw`s keep the retail delay padding).
const MODULE_958_DAMAGE_EDITS: &[WordEdit] = &[
    // Arm 1 (power 0x30, file +0x0B14): bank the victim, then move.
    WordEdit {
        offset: 0x0B30,
        expect: 0x3C05_801D,              // lui a1, 0x801d
        replace: m::lui(A1, CELL_958_HI), // a1 = cell page
    },
    WordEdit {
        offset: 0x0B34,
        expect: 0x8CA3_9370,                 // lw v1, -0x6c90(a1)
        replace: m::sw(S1, A1, CELL_958_LO), // bank the victim
    },
    WordEdit {
        offset: 0x0B38,
        expect: 0x0000_0000, // nop (retail load-delay padding)
        replace: move_(V1, S1),
    },
    WordEdit {
        offset: 0x0B64,
        expect: 0x8CA4_9370, // lw a0, -0x6c90(a1)
        replace: move_(A0, S1),
    },
    // Arm 2 (power 0x38, +0x0E7C).
    WordEdit {
        offset: 0x0E9C,
        expect: 0x8D07_9370, // lw a3, -0x6c90(t0)
        replace: move_(A3, S1),
    },
    WordEdit {
        offset: 0x0ED4,
        expect: 0x8D08_9370, // lw t0, -0x6c90(t0)
        replace: move_(T0, S1),
    },
    // Arm 3 (power 0x38, +0x12CC).
    WordEdit {
        offset: 0x12EC,
        expect: 0x8D07_9370,
        replace: move_(A3, S1),
    },
    WordEdit {
        offset: 0x1324,
        expect: 0x8D08_9370,
        replace: move_(T0, S1),
    },
    // Arm 4 (power 0x38, +0x170C).
    WordEdit {
        offset: 0x172C,
        expect: 0x8D07_9370,
        replace: move_(A3, S1),
    },
    WordEdit {
        offset: 0x1764,
        expect: 0x8D08_9370,
        replace: move_(T0, S1),
    },
    // Finale arm A (power 0x40, +0x1D7C): registers burned - load the cell.
    WordEdit {
        offset: 0x1D98,
        expect: 0x3C05_801D,
        replace: m::lui(A1, CELL_958_HI),
    },
    WordEdit {
        offset: 0x1D9C,
        expect: 0x8CA3_9370,
        replace: m::lw(V1, A1, CELL_958_LO),
    },
    WordEdit {
        offset: 0x1DCC,
        expect: 0x8CA5_9370, // lw a1, -0x6c90(a1)
        replace: m::lw(A1, A1, CELL_958_LO),
    },
    // Finale arm B (power 0x30 barrage, +0x1F00).
    WordEdit {
        offset: 0x1F1C,
        expect: 0x3C05_801D,
        replace: m::lui(A1, CELL_958_HI),
    },
    WordEdit {
        offset: 0x1F20,
        expect: 0x8CA3_9370,
        replace: m::lw(V1, A1, CELL_958_LO),
    },
    WordEdit {
        offset: 0x1F50,
        expect: 0x8CA4_9370,
        replace: m::lw(A0, A1, CELL_958_LO),
    },
];

/// PROT 958 finale: the dead-victim arm. Same shape as
/// [`MODULE_959_WIPE_EDIT`]: a dead victim whose char record lacks bit
/// `0x80` at `+0x6C0` falls into the party-wipe body
/// (`0x801F8B8C..0x801F8BB4`: `BD71 = 0xFE`, `BD2C = 5`, jingle call) -
/// correct only while the victim is a hero. Nop the `beqz` and a dead
/// victim whose reaction ENDED takes the alive path to the convergence
/// at `0x801F8BB8` (clear `+0x21D` on both actors, phase++); real deaths
/// on either side are caught by the end-of-action liveness sweep (state
/// `0x5A` - in the 1v1 duels the sole hero dying still ends the battle
/// through it). The single inbound branch is this `beqz` (whole-image
/// scan), so the body becomes dead words - the victim cell lives in its
/// last one. NB this edit alone does NOT unblock a monster killed by the
/// finale: the arm's earlier reaction-row wait never resolves for a
/// corpse - [`MODULE_958_WAIT_EDITS`] carries that half.
pub(super) const MODULE_958_WIPE_EDIT: WordEdit = WordEdit {
    offset: 0x21A4,
    expect: 0x1040_0003, // beqz v0, 0x801F8B8C (the wipe body)
    replace: m::nop(),
};

/// PROT 958 finale: the dead-victim reaction-row DEADLOCK (the
/// user-reported Blazing Slash softlock, savestate-pinned: `mph 0x18`,
/// victim `HP 0` parked in reaction row `0x03 == +0x1F1` with its clip
/// long finished, caster parked on row `0x0B` with a full `0xFF` hold
/// budget and `+0x21D = 0`).
///
/// Unique to 958: its phase-`0x18` arm opens with
/// `beq playing(+0x1D9), reaction(+0x1F1) -> wait` BEFORE the HP fork -
/// 959's finale forks on HP immediately and 960's waits on a countdown,
/// which is why only Gi's kill hangs. A HERO victim leaves the reaction
/// row (the battle SM re-stages a KO row), so retail never blocks; a
/// MONSTER corpse is re-staged by nothing until the action ends - which
/// this very wait gates. The module waits on the victim, the victim
/// waits on the module.
///
/// Fix: gate the wait on the victim being alive. The arm's free `nop` at
/// `+0x213C` loads the victim's HP into the dead `$t4` (last read at
/// `+0x212C`; the `beq` + its delay slot cover the load delay), the wait
/// branch retargets into a 4-word cave in the wipe body
/// ([`GATE_958_OFFSET`] - dead under [`MODULE_958_WIPE_EDIT`], after the
/// three 2-word stubs), and the cave re-branches: alive -> the retail
/// wait (`0x801F8CFC`, the no-phase-bump epilogue), dead -> the
/// convergence at `0x801F8BB8` (clear `+0x21D` on both actors, phase++),
/// after which the liveness sweep (state `0x5A`) runs the real death.
/// A live victim's wait is behaviourally retail-identical.
pub(super) const MODULE_958_WAIT_EDITS: &[WordEdit] = &[
    WordEdit {
        offset: 0x213C,
        expect: 0x0000_0000,            // free delay-filler nop
        replace: m::lhu(T4, S1, 0x14C), // victim HP
    },
    WordEdit {
        offset: 0x2140,
        expect: 0x1062_0078,           // beq v1, v0, 0x801F8CFC (the wait)
        replace: m::beq(V1, V0, 0x22), // -> the HP gate at 0x801F8BA4
    },
    // The gate cave (wipe-body words +0x21CC..+0x21D8).
    WordEdit {
        offset: GATE_958_OFFSET,
        expect: 0x2403_0005,             // dead: li 5
        replace: m::bne(T4, ZERO, 0x55), // alive -> the wait at 0x801F8CFC
    },
    WordEdit {
        offset: GATE_958_OFFSET + 4,
        expect: 0xACC3_BD2C, // dead: sw wipe-state
        replace: m::nop(),   // branch delay
    },
    WordEdit {
        offset: GATE_958_OFFSET + 8,
        expect: 0x3042_007F,        // dead: andi
        replace: m::j(0x801F_8BB8), // dead victim -> convergence (phase++)
    },
    WordEdit {
        offset: GATE_958_OFFSET + 12,
        expect: 0x0C00_C66A, // dead: jal jingle
        replace: m::nop(),   // j delay slot
    },
];

/// `$s2` - the caster-actor register at every hooked staging store
/// except 960's opener (`$s4`).
pub(super) const S2_REG: u32 = 18;

/// Apply the full PROT 958 cast-route patch set: the staged-id handling
/// (un-folded stage caves when `unfold` carries Gi's authored entry
/// offsets, the folded [`MODULE_958_STAGE_REMAP_EDITS`] otherwise), the
/// twelve-site damage retarget (+ victim cell) and the dead-victim wipe
/// skip. Wired by `apply_delilas_party` for a Gi-mapped slot together
/// with Gi's staged caster rows, so a shipped patch never changes the
/// walk without the player-side rows existing.
pub fn patch_module_958(p: &mut DiscPatcher, unfold: Option<&[usize]>) -> Result<bool> {
    let entry = p.read_entry(958).context("read PROT 958")?;
    let mut edits: Vec<WordEdit> = match unfold {
        Some(offs) => module_958_unfold_edits(&stage_cave_layout()?, offs)?,
        None => MODULE_958_STAGE_REMAP_EDITS.to_vec(),
    };
    edits.extend_from_slice(MODULE_958_DAMAGE_EDITS);
    edits.push(MODULE_958_WIPE_EDIT);
    edits.extend_from_slice(MODULE_958_WAIT_EDITS);
    apply_word_edits(p, 958, &entry, &edits)
}
