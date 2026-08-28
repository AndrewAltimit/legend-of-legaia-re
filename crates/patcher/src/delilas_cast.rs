//! Delilas party swap: route the signature art into the real enemy cast
//! module, so a swapped hero's special plays the retail boss choreography
//! (camera track, lift, multi-hit damage build-up, face cut-in) instead of
//! the art-side approximation.
//!
//! ## How retail runs a boss special
//!
//! A Delilas signature attack is a capture-class spell (`'c'` at spell-table
//! `+0`): Blazing Slash `0x79` / Megaton Press `0x7A` / Plasma Strike `0x7B`,
//! whose `+1` sub-id pages the per-spell cast module PROT `958`/`959`/`960`
//! into the slot-B overlay window (`0x801F69D8`) at cast time. Battle states
//! `0x28 -> 0x6E..0x71` drive it; state `0x70` re-enters the module's tick
//! every frame ([spell-table.md](../../../docs/formats/spell-table.md),
//! [battle-action.md](../../../docs/subsystems/battle-action.md)).
//!
//! The route is fully data-keyed off the spell id, so a PARTY actor whose
//! action is `category 2 (Magic), +0x1DF = 0x7A` runs the whole spectacle -
//! verified live (PCSX-Redux, `autorun_player_special_cast.lua`): the module
//! pages in, its phase machine advances, the caster plays the staged clips
//! and the retail pillar/impact effects and camera track render around the
//! player character.
//!
//! ## What this module patches
//!
//! 1. **The queue hook** (SCUS arena + one `jal` redirect in PROT 0898):
//!    after the retail Super-Art applier `FUN_801EF9E4` runs over a finished
//!    arts queue, a small stub scans the queue for the slot's signature-art
//!    constant and, when present (and no Super fired - `DAT_801F696C == 0`),
//!    rewrites the action to `+0x1DE = 2` (Magic) with `+0x1DF` = the mapped
//!    sibling's spell id. The attack's chosen target seat (`+0x1DD`) is kept.
//!    The applier has exactly one caller - the queue builder `FUN_801EED1C`'s
//!    `jal` at `0x801EF9AC` - and at that site `a0` = actor slot, `a1` (after
//!    the delay slot) = 0-based character index.
//!
//! 2. **The cast modules** (PROT 958/959/960, link base `0x801F69D8`): four
//!    defect classes separate an enemy cast from a player cast. Three have
//!    edit tables below - the hardcoded `table[0]` damage/HP sites
//!    (retarget to the derived victim), the dead-victim party-wipe arm
//!    (branch to the settle tail), and the finale teardown corpse-model
//!    (neutralise its stream words at the settle tail). The fourth - the
//!    caster staged-clip rows - is fixed on the DATA side by default:
//!    `party_swap::cast_stage` authors real wind-up/payoff records on the
//!    player rows the module stages (Block re-homed to a spare row, see
//!    [`relocate_block_reaction`]), so the module's retail two-stage step
//!    survives on both caster kinds; the stage-pin edit remains only as
//!    the fallback when that rewrite cannot land. Only a module whose
//!    classes are patched AND probe-verified end-to-end is routed to;
//!    PROT 959 (Megaton Press) is.
//!
//! Verified live for PROT 959: the full choreography - pillar, blackout
//! lift, rock-shatter with the damage counter, smash - runs on a player
//! caster against the chosen target, and the battle continues past the
//! finale.
//!
//! No Sony bytes ship here: every patched word is derived from (and verified
//! against) the user's own disc before writing.

use anyhow::{Context, Result, bail};

use crate::disc::DiscPatcher;
use crate::mips::{
    self as m, A0, A1, A2, A3, RA, S1, S3, S5, SP, T0, T1, T2, T3, T4, T5, T6, T7, T8, T9, V0, V1,
    ZERO,
};

/// Battle-action overlay (hosts the queue builder + applier).
const BATTLE_OVERLAY_PROT: usize = 898;
const BATTLE_OVERLAY_BASE: u32 = 0x801C_E818;

/// The applier call site inside `FUN_801EED1C` (its only caller).
const APPLIER_CALL_VA: u32 = 0x801E_F9AC;
/// The retail Super-Art applier the stub still runs first.
const APPLIER_VA: u32 = 0x801E_F9E4;
/// `sw 1 -> 0x801F696C` - the applier's own "a Super fired" flag.
const SUPER_FIRED_VA: u32 = 0x801F_696C;
/// 8-slot battle actor pointer table.
const ACTOR_TABLE_VA: u32 = 0x801C_9370;

/// Per-slot signature route: scan the finished queue for `art_constant`;
/// on a hit the action becomes a Magic cast of `spell_id`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CastRoute {
    /// 0-based character index (0 Vahn / 1 Noa / 2 Gala).
    pub char_index: u8,
    /// The host art's queue constant (bank row + 0x10).
    pub art_constant: u8,
    /// The mapped sibling's capture-class spell id.
    pub spell_id: u8,
}

/// One-word expect/replace edit inside a PROT entry.
#[derive(Debug, Clone, Copy)]
struct WordEdit {
    offset: u64,
    expect: u32,
    replace: u32,
}

/// `move rd, rs` == `addu rd, rs, zero`.
const fn move_(rd: u32, rs: u32) -> u32 {
    m::addu(rd, rs, ZERO)
}

/// PROT 959 (Megaton Press): the five hardcoded `table[0]` damage/HP loads.
/// The tick keeps the true target in `$s4` from prologue to epilogue
/// (written exactly twice in the whole image: the derivation and the
/// register restore), so a plain `move` is safe at every site - each is the
/// `lw; nop; use` shape with no delay-slot dependence on the old value.
const S4: u32 = 20;
const MODULE_959_DAMAGE_EDITS: &[WordEdit] = &[
    WordEdit {
        offset: 0x082C,
        expect: 0x8FC3_9370,
        replace: move_(V1, S4),
    },
    WordEdit {
        offset: 0x085C,
        expect: 0x8FC4_9370,
        replace: move_(A0, S4),
    },
    WordEdit {
        offset: 0x10D4,
        expect: 0x8FC2_9370,
        replace: move_(V0, S4),
    },
    WordEdit {
        offset: 0x1500,
        expect: 0x8FC3_9370,
        replace: move_(V1, S4),
    },
    WordEdit {
        offset: 0x1530,
        expect: 0x8FC4_9370,
        replace: move_(A0, S4),
    },
];

/// PROT 959 finale (phase 0x11, `0x801F8158`): the dead-victim arm. Retail
/// branches live victims to the settle path (`bnez hp, 0x801F81A8` - kept)
/// and falls through to a party-wipe shortcut (`BD2C = 5`) for a dead one,
/// because its victim is always a hero. A dead MONSTER victim must do
/// neither: the wipe ends the battle as a loss, and the settle path stages
/// an idle on the corpse (probe-measured freeze). The wipe body's first
/// word becomes a branch straight into the settle path's TAIL
/// (`0x801F81B8`: recover speeds + phase `0xFF`), past the victim staging;
/// the delay slot executes the following harmless `lui`. Real deaths on
/// either side are still caught by the end-of-action liveness sweep
/// (state `0x5A`).
const MODULE_959_WIPE_EDIT: WordEdit = WordEdit {
    offset: 0x1788,
    expect: 0x3C04_8008,  // lui a0, 0x8008 (wipe body head)
    replace: 0x1000_0015, // b 0x801F81B8
};

/// PROT 959 lift arm (`0x801F767C`): the staged-index step to entry `0x0B`.
///
/// The module stages the caster's clips through `actor+0x1DA` (index) +
/// `actor+0x1DC` (stage counter - the restage trigger): the cast opens on
/// staged entry `0x0A` (`li 0xA` at `0x801F6F3C`) and the lift arm
/// increments both fields to stage entry `0x0B` for the smash. On a
/// MONSTER caster rows `0x0A`/`0x0B` of the block are the wind-up and
/// smash swing; on a PARTY caster they resolve through the record[0]
/// action table, where retail row `0x0A` is an empty placeholder and row
/// `0x0B` is the Block record the module's stage boundary chokes on
/// (probe-measured hard freeze; re-pointing row `0x0B` at row `0x0A`'s
/// record at runtime carried the whole choreography).
///
/// Two production forms exist, chosen by `patch_module_959`'s
/// `pin_stage`:
///
/// - **Pinned** (the fallback): nop the INDEX increment only - the stage
///   counter still bumps, so the restage fires and delivers row `0x0A`'s
///   record again. Caster holds a pose; also costs the ENEMY-side cast
///   its smash stage (the module writes the same index for both caster
///   kinds).
/// - **Retail** (the default once the staged rows are authored): leave
///   the increment alone - `party_swap::cast_stage` gives the player
///   rows `0x0A`/`0x0B` real wind-up/payoff records (Block re-homed to
///   row `0x06`), and the monster side keeps its retail two-stage
///   choreography.
const MODULE_959_STAGE_EDIT: WordEdit = WordEdit {
    offset: 0x0CAC,      // 0x801F7684 in the lift arm
    expect: 0x2442_0001, // addiu v0, v0, 1 (the staged-index step)
    replace: m::nop(),   // index stays 0x0A; counter bump still restages
};

/// PROT 959 finale teardown: neutralise the corpse-model the finale leaves
/// behind, before the end-of-action draw dereferences it.
///
/// The phase-0xE arm spawns the finale effect barrage; one spawned entity
/// (cached at `ctx + 0x102C` - the module's own halt quad targets it) is
/// installed as **slot 0 of a carrier entity's model table** and drawn as
/// a model every frame from then on. On a PARTY caster it never gets real
/// stream words bound: its `+0x10` (colour stream) stays `0`, which the
/// TMD walk `FUN_80043390` tolerates (address 0 is mapped RAM). When the
/// entity's script kill-marks it (`+0x10 |= 0x02000000`, on top of the
/// halt `|8`), the next carrier draw reads colours from `0x02000008` -
/// unmapped - and the game hard-freezes on the exact frame the
/// choreography ends (probe-pinned: carrier `0x800835E4`, model table
/// slot `[0x11, entity]`, fault `lw` at `0x80043580`).
///
/// Freeing the entity is WRONG here - the carrier keeps its reference, so
/// a freed slab would be redrawn as whatever reuses it. The fix writes
/// the two stream words the walk consumes back to the proven-safe state:
/// `+0x10 = 0` (colour reads at mapped address 0, exactly the state the
/// carrier drew safely for the whole cast) and `+0x14 = 0` (the walk's
/// first gate exits cleanly on a zero prim stream).
///
/// Host: the settle tail (`0x801F81B8`) runs exactly once per cast - it is
/// the site that writes phase `0xFF`, after the kill mark and before the
/// same frame's draw pass - with the caster in `$s2`, the victim in `$s4`
/// and the battle ctx in `$s5`. Its exit jump is rerouted through the
/// dead-victim wipe body (`0x801F81D0..`), which [`MODULE_959_WIPE_EDIT`]
/// already made unreachable; the stub also clears the `ctx[+0x102C]`
/// cache and the two probe-observed stale actor chain cells
/// (`caster+0x44` held a masked `0x08000000` all cast; healthy actors
/// carry `0`). The phase write stays in the rerouted jump's delay slot.
const MODULE_959_TEARDOWN_EDITS: &[WordEdit] = &[
    WordEdit {
        offset: 0x17F0,             // 0x801F81C8: settle-tail exit
        expect: 0x0807_E087,        // j 0x801F821C (epilogue)
        replace: m::j(0x801F_81D0), // j into the dead wipe body
    },
    WordEdit {
        offset: 0x17F8, // 0x801F81D0 (dead: lui v1)
        expect: 0x3C03_8008,
        replace: m::lw(A0, S5, 0x102C), // a0 = cached finale entity
    },
    WordEdit {
        offset: 0x17FC, // 0x801F81D4 (dead: addiu)
        expect: 0x2402_00FE,
        replace: m::sw(ZERO, m::S2, 0x44), // caster chain cell (load delay)
    },
    WordEdit {
        offset: 0x1800, // 0x801F81D8 (dead: lui a2)
        expect: 0x3C06_8008,
        replace: m::beq(A0, ZERO, 6), // null guard -> 0x801F81F4
    },
    WordEdit {
        offset: 0x1804, // 0x801F81DC (dead: lui a1)
        expect: 0x3C05_8008,
        replace: m::sw(ZERO, m::S4, 0x44), // victim chain cell (branch delay)
    },
    WordEdit {
        offset: 0x1808, // 0x801F81E0 (dead: sb wipe flag)
        expect: 0xA062_BD71,
        replace: m::sw(ZERO, A0, 0x10), // colour stream -> mapped null
    },
    WordEdit {
        offset: 0x180C, // 0x801F81E4 (dead: lbu)
        expect: 0x90A2_BD60,
        replace: m::sw(ZERO, A0, 0x14), // prim stream -> clean walk exit
    },
    WordEdit {
        offset: 0x1810, // 0x801F81E8 (dead: li 5)
        expect: 0x2403_0005,
        replace: m::sw(ZERO, S5, 0x102C), // clear the cache
    },
    WordEdit {
        offset: 0x1814, // 0x801F81EC (dead: sw wipe-state)
        expect: 0xACC3_BD2C,
        replace: m::j(0x801F_821C), // rejoin the epilogue
    },
    WordEdit {
        offset: 0x1818, // 0x801F81F0 (dead: andi): j delay slot
        expect: 0x3042_007F,
        replace: m::nop(),
    },
    WordEdit {
        offset: 0x181C, // 0x801F81F4 (dead: jal jingle): null-guard target
        expect: 0x0C00_C66A,
        replace: m::j(0x801F_821C), // rejoin the epilogue
    },
    WordEdit {
        offset: 0x1820, // 0x801F81F8 (dead: sb, jal delay): j delay slot
        expect: 0xA0A2_BD60,
        replace: m::nop(),
    },
];

/// Apply the PROT 959 module patches.
///
/// `pin_stage` selects the staged-index handling (see
/// [`MODULE_959_STAGE_EDIT`]): `false` keeps the retail two-stage step
/// (the caster rows must have been authored by
/// `party_swap::cast_stage`); `true` pins both stages to row `0x0A`
/// (the fallback when the staged-row rewrite could not land). A stage
/// pin left by an earlier build is restored to retail when
/// `pin_stage` is `false`.
pub fn patch_module_959(p: &mut DiscPatcher, pin_stage: bool) -> Result<bool> {
    let entry = p.read_entry(959).context("read PROT 959")?;
    let mut edits: Vec<WordEdit> = MODULE_959_DAMAGE_EDITS.to_vec();
    edits.push(MODULE_959_WIPE_EDIT);
    if pin_stage {
        edits.push(MODULE_959_STAGE_EDIT);
    }
    edits.extend_from_slice(MODULE_959_TEARDOWN_EDITS);

    let word = |off: u64| -> Result<u32> {
        let off = off as usize;
        Ok(u32::from_le_bytes(
            entry
                .get(off..off + 4)
                .with_context(|| format!("PROT 959 short at +{off:#x}"))?
                .try_into()?,
        ))
    };

    // Upgrade path: an earlier build's stage pin is restored to retail
    // when this run keeps the two-stage step.
    if !pin_stage && word(MODULE_959_STAGE_EDIT.offset)? == MODULE_959_STAGE_EDIT.replace {
        p.patch_prot_entry(
            959,
            MODULE_959_STAGE_EDIT.offset,
            &MODULE_959_STAGE_EDIT.expect.to_le_bytes(),
        )?;
    }

    apply_word_edits(p, 959, &entry, &edits)
}

/// Verify-then-apply a word-edit set against one PROT entry, with the
/// jewel-fix idempotence contract: all-already-patched is a clean skip
/// (`Ok(false)`), a partial mix or any non-`expect` word refuses before
/// a single byte is written.
fn apply_word_edits(
    p: &mut DiscPatcher,
    prot: usize,
    entry: &[u8],
    edits: &[WordEdit],
) -> Result<bool> {
    let word = |off: u64| -> Result<u32> {
        let off = off as usize;
        Ok(u32::from_le_bytes(
            entry
                .get(off..off + 4)
                .with_context(|| format!("PROT {prot} short at +{off:#x}"))?
                .try_into()?,
        ))
    };
    let already = edits
        .iter()
        .filter(|e| word(e.offset).map(|w| w == e.replace).unwrap_or(false))
        .count();
    if already == edits.len() {
        return Ok(false);
    }
    if already != 0 {
        bail!(
            "PROT {prot} is partially patched ({already}/{} sites) - refusing",
            edits.len()
        );
    }
    for e in edits {
        let w = word(e.offset)?;
        if w != e.expect {
            bail!(
                "PROT {prot} +{:#x}: expected {:#010x}, found {:#010x} - not a known retail image",
                e.offset,
                e.expect,
                w
            );
        }
    }
    for e in edits {
        p.patch_prot_entry(prot, e.offset, &e.replace.to_le_bytes())?;
    }
    Ok(true)
}

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
/// (`party_swap::cast_stage`) authors real content on rows `0x0A`/`0x0B`
/// only, so the walk is folded to stay inside that pair:
/// `0A,0B,B(restage),A,B,B`. The restage still bumps the `+0x1DC`
/// counter, so each stage boundary restarts a clip rather than freezing.
/// The ENEMY-side cast shares the module and plays the folded walk too -
/// two of six stages replay the windup/smash clips; the effect barrage,
/// camera track and damage build-up are untouched.
const MODULE_958_STAGE_REMAP_EDITS: &[WordEdit] = &[
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
const MODULE_960_STAGE_REMAP_EDITS: &[WordEdit] = &[
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
const MODULE_958_WIPE_EDIT: WordEdit = WordEdit {
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
const MODULE_958_WAIT_EDITS: &[WordEdit] = &[
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

// ---------------------------------------------------------------------------
// Stage caves: the un-folded retail walks.
//
// With only rows `0x0A`/`0x0B` player-representable, the folded walks
// replay the wind-up/payoff clips where retail staged distinct entries
// (the "loops some portions, skips others" playout). The un-fold keeps
// the folded staged IDS - so every gate literal stays valid and the
// enemy-side caster still resolves its own archive entries 10/11 - and
// instead REPOINTS the head-table word the id resolves through
// (`FUN_8004AD80` party arm: `*(DAT_801C9360[slot] + idx*4)`) at a
// different authored entry per stage. The player file hosts the FULL
// retail chain below `clut_a` (`cast_stage::build_staged_cast_rows`),
// and each stage's `sb id, 0x1DA(actor)` word becomes a `jal` into a
// small SCUS-resident cave that repoints the word, redoes the staging
// store and returns.
//
// Why the hook rides the `sb` word: `$ra` is dead between calls at every
// site (the module ticks save/reload it on their own frames), the word
// after each `sb` is an independent store/load that is safe as the
// `jal`'s delay slot, and no hooked word is a branch target (whole-image
// scan). `$t5..$t9` are dead across every site (per-arm liveness scan).
//
// Where the caves live - three pools, all free exactly when the cast
// route runs, all SCUS-resident (reachable from module code whenever a
// cast is in flight):
//
// * the SCUS injection-gap tail right after the queue hook
//   ([`assemble_hook`] is fixed-size, so the tail base is stable);
// * shiny-seru's read-watch-verified dead pockets `ARENA2`
//   (`0x8007AFF8..0x8007B040`) and `SLOT6` (`0x80078A88..0x80078ACC`) -
//   shiny-seru / show-super-arts / arts-ap are option-exclusive with
//   the cast route (`cmd_randomize` + [`install_cast_hook`]'s free
//   check), so the pockets cannot be double-claimed;
// * PROT 958's own dead party-wipe body (`+0x21B4..+0x21D8`, made
//   unreachable by [`MODULE_958_WIPE_EDIT`]) hosts three of 958's
//   stubs (2-word form) + the dead-victim HP gate
//   ([`MODULE_958_WAIT_EDITS`]); its last word stays the finale
//   victim cell.
//
// An ENEMY cast through a patched module executes the same caves: the
// staging store is identical (the caster reg holds the enemy actor),
// and the repoint touches the HERO slot's head-table word - dirty but
// harmless, because rows `0x0A`/`0x0B` are exclusively module-staged
// (Block is re-homed to row 6) and every cast's opener cave restores
// the word before the first commit reads it.

/// Runtime per-slot record[0] base-pointer table (`FUN_80052FA0` writes
/// `0x801C9360 + char*4`; party slots are fixed character indices).
const RECORD0_BASE_TABLE_VA: u32 = 0x801C_9360;
/// Head-table byte offset of row `0x0A` (the variable stage row).
const ROW_A_WORD: u16 = 0x28;
/// Head-table byte offset of row `0x0B` (the payoff/finale row).
const ROW_B_WORD: u16 = 0x2C;

/// Shared repoint core (9 words): the stub preloads `$t7` with the
/// entry's decoded-image offset; the core repoints `table_word` of slot
/// `slot`'s record[0] head at `base + $t7`, writes the entry's `+0x88`
/// stream pointer (`entry + 0xAC` - the loader only writes it for
/// TABLE-BOUND entries, so a mid-chain entry commits a NULL stream
/// without this; idempotent for bound entries), stores staged id `id`
/// through the caster reg and returns to the hooked site.
fn assemble_stage_core(slot: u32, table_word: u16, id: u16, actor_reg: u32) -> Vec<u32> {
    let ptr = RECORD0_BASE_TABLE_VA + slot * 4;
    vec![
        m::lui(T8, m::hi(ptr)),
        m::lw(T8, T8, m::lo(ptr)),
        m::addiu(m::T6, ZERO, id), // load-delay filler
        m::addu(T9, T8, T7),
        m::addiu(T5, T9, 0xAC),
        m::sw(T5, T9, 0x88), // entry stream pointer
        m::sw(T9, T8, table_word),
        m::jr(RA),
        m::sb(m::T6, actor_reg, 0x1DA), // delay slot
    ]
}

/// Standalone cave (8 words) with the entry offset inline - the 960
/// opener, whose caster lives in `$s4` (every other site uses `$s2`).
/// No `+0x88` write: the opener always repoints at the TABLE-BOUND
/// chain head, whose stream pointer the loader wrote at battle load.
fn assemble_stage_cave(slot: u32, table_word: u16, id: u16, actor_reg: u32, off: u32) -> Vec<u32> {
    let ptr = RECORD0_BASE_TABLE_VA + slot * 4;
    vec![
        m::lui(T8, m::hi(ptr)),
        m::lw(T8, T8, m::lo(ptr)),
        m::ori(T7, ZERO, off as u16), // load-delay filler
        m::addu(T9, T8, T7),
        m::addiu(m::T6, ZERO, id),
        m::sw(T9, T8, table_word),
        m::jr(RA),
        m::sb(m::T6, actor_reg, 0x1DA),
    ]
}

/// Per-site stub (2 words): tail into the shared core with the entry
/// offset preloaded in the `j` delay slot (`ori` - offsets can exceed
/// the `addiu` sign bound). The compact form is what makes room for the
/// two word-budgeted caves: 958's dead-victim HP gate in its wipe body
/// and 960's bed-preempt cave in the SCUS pools.
fn assemble_stage_stub2(off: u32, core_va: u32) -> Vec<u32> {
    vec![m::j(core_va), m::ori(T7, ZERO, off as u16)]
}

/// The fixed cave layout over the three pools. Byte-fit is asserted by
/// [`stage_cave_layout`]; the unit test pins every VA.
struct StageCaveLayout {
    // 960 (slot 0 / Vahn file), SCUS gap tail + SLOT6:
    shared_960: u32,
    stub_960_s2: u32,
    stub_960_s3: u32,
    stub_960_s4: u32,
    opener_960: u32,
    preempt_960_a: u32,
    preempt_960_b: u32,
    // 958 (slot 1 / Noa file), ARENA2 + SLOT6 + the module wipe body:
    shared_958_a: u32,
    shared_958_b: u32,
    stub_958_opener: u32,
    stub_958_s2: u32,
    stub_958_s3: u32,
    stub_958_s4: u32,
    stub_958_s6: u32,
}

/// PROT 958 file offsets of the three wipe-body stubs. Each stub is the
/// 2-word form (`j core` with the `ori $t7` in the delay slot), packing
/// all three into `+0x21B4..+0x21C8` so the body's next four words host
/// the dead-victim HP gate ([`MODULE_958_WAIT_EDITS`]); the `+0x21DC`
/// victim cell stays.
const STUB_958_BODY_OFFSETS: [u64; 3] = [0x21B4, 0x21BC, 0x21C4];
/// File offset of the 4-word HP gate cave inside the wipe body.
const GATE_958_OFFSET: u64 = 0x21CC;

fn stage_cave_layout() -> Result<StageCaveLayout> {
    use crate::shiny_seru::{ARENA2_END_VA, ARENA2_VA, SLOT6_END_VA, SLOT6_VA};
    // The queue hook is fixed-size (its shape does not depend on the
    // routes), so the tail base is stable across builds.
    let hook_len = assemble_hook(SCUS_GAP_VA_LOCAL, &[]).len() as u32;
    let tail = SCUS_GAP_VA_LOCAL + hook_len.div_ceil(4) * 4;
    let l = StageCaveLayout {
        // Gap tail (16 words): the 9-word 960 core + two 2-word stubs +
        // the 3-word half A of the bed-preempt cave.
        shared_960: tail,
        stub_960_s3: tail + 9 * 4,
        stub_960_s4: tail + 11 * 4,
        preempt_960_a: tail + 13 * 4,
        // SLOT6 (17 words): the 8-word 960 opener + three 2-word stubs +
        // the 2-word half B of the bed-preempt cave.
        opener_960: SLOT6_VA,
        stub_958_opener: SLOT6_VA + 8 * 4,
        stub_958_s2: SLOT6_VA + 10 * 4,
        stub_960_s2: SLOT6_VA + 12 * 4,
        preempt_960_b: SLOT6_VA + 14 * 4,
        // ARENA2 (18 words): exactly the two 9-word 958 cores.
        shared_958_a: ARENA2_VA,
        shared_958_b: ARENA2_VA + 9 * 4,
        stub_958_s3: MODULE_LINK_BASE + STUB_958_BODY_OFFSETS[0] as u32,
        stub_958_s4: MODULE_LINK_BASE + STUB_958_BODY_OFFSETS[1] as u32,
        stub_958_s6: MODULE_LINK_BASE + STUB_958_BODY_OFFSETS[2] as u32,
    };
    if l.preempt_960_a + 3 * 4 > SCUS_GAP_END_VA_LOCAL {
        bail!("960 stage caves overrun the SCUS gap tail");
    }
    if l.shared_958_b + 9 * 4 > ARENA2_END_VA {
        bail!("958 stage cores overrun ARENA2");
    }
    if l.preempt_960_b + 2 * 4 > SLOT6_END_VA {
        bail!("stage caves overrun SLOT6");
    }
    Ok(l)
}

/// The retail XA cue player `FUN_8003D53C(slot, chan, dur_ticks)` - the
/// guard-free layer under the jingle wrapper `FUN_8004FCC8`. Its own
/// head stops whatever stream is active before starting the new one, so
/// a direct call PREEMPTS instead of dropping.
const XA_PLAY_VA: u32 = 0x8003_D53C;
/// Module 960's cast bed as `FUN_8003D53C` arguments: id `0x19A` ->
/// slot `0x13` (`XA20.XA`), channel `2`, duration `(1689*60+99)/100 =
/// 0x3F6` ticks (16.9 s) from the fanfare table row.
const BED_960_SLOT: u16 = 0x13;
const BED_960_CHAN: u16 = 2;
const BED_960_DUR: u16 = 0x3F6;

/// Link base of the cast modules (slot-B); 958's wipe-body stubs are
/// addressed as `MODULE_LINK_BASE + file offset`.
const MODULE_LINK_BASE: u32 = 0x801F_69D8;
const SCUS_GAP_VA_LOCAL: u32 = crate::shiny_seru::SCUS_GAP_VA;
const SCUS_GAP_END_VA_LOCAL: u32 = crate::shiny_seru::SCUS_GAP_END_VA;

fn words_to_bytes(words: &[u32]) -> Vec<u8> {
    words.iter().flat_map(|w| w.to_le_bytes()).collect()
}

/// Validate an un-fold offset list: the authored entries' decoded-image
/// offsets, chain order, each within the `ori` zero-extension bound.
fn check_unfold_offsets(offs: &[usize], want: usize, module: usize) -> Result<()> {
    if offs.len() != want {
        bail!(
            "module {module} un-fold expects {want} entry offsets, got {}",
            offs.len()
        );
    }
    for &o in offs {
        if o == 0 || o > 0xFFFF {
            bail!("module {module} entry offset {o:#x} outside the 16-bit stub bound");
        }
    }
    Ok(())
}

/// PROT 958's UN-FOLDED stage edit set: the id literals still fold to
/// rows `0x0A`/`0x0B` (see [`MODULE_958_STAGE_REMAP_EDITS`] for the walk
/// and the enemy-side reading), but each staging store is hooked into a
/// stage cave, so the PLAYER walk plays the full retail chain
/// `10,11,12,10,11,13`: opener resets row `0x0A` -> crouch, s3 repoints
/// it at the slash, s4 back at the crouch, s6 repoints row `0x0B` at the
/// finale, and the s2 hook resets `0x0B` -> leap at the next cast.
fn module_958_unfold_edits(l: &StageCaveLayout, offs: &[usize]) -> Result<Vec<WordEdit>> {
    check_unfold_offsets(offs, 4, 958)?;
    let (e10, e11, e12, e13) = (
        offs[0] as u32,
        offs[1] as u32,
        offs[2] as u32,
        offs[3] as u32,
    );
    let mut edits = vec![
        // Stage-id literals: s3 (retail +1 -> 0x0C) and the swing reset
        // s4 (retail -2) both stage row 0x0A outright; the finale keeps
        // the folded li 0x0B.
        WordEdit {
            offset: 0x0C48,
            expect: 0x2442_0001,
            replace: m::addiu(V0, ZERO, 0x000A),
        },
        WordEdit {
            offset: 0x19C4,
            expect: 0x2442_FFFE,
            replace: m::addiu(V0, ZERO, 0x000A),
        },
        WordEdit {
            offset: 0x1FD4,
            expect: 0x2402_000D,
            replace: m::addiu(V0, ZERO, 0x000B),
        },
        // The five staging stores become cave calls.
        WordEdit {
            offset: 0x05B0,
            expect: 0xA242_01DA,
            replace: m::jal(l.stub_958_opener),
        },
        WordEdit {
            offset: 0x08E8,
            expect: 0xA242_01DA,
            replace: m::jal(l.stub_958_s2),
        },
        WordEdit {
            offset: 0x0C50,
            expect: 0xA242_01DA,
            replace: m::jal(l.stub_958_s3),
        },
        WordEdit {
            offset: 0x19C8,
            expect: 0xA242_01DA,
            replace: m::jal(l.stub_958_s4),
        },
        WordEdit {
            offset: 0x1FD8,
            expect: 0xA242_01DA,
            replace: m::jal(l.stub_958_s6),
        },
    ];
    // The three wipe-body stubs (s3 -> slash, s4 -> crouch, s6 -> finale),
    // 2-word form so the body's tail stays free for the HP gate
    // ([`MODULE_958_WAIT_EDITS`]).
    let body: [(u64, Vec<u32>); 3] = [
        (
            STUB_958_BODY_OFFSETS[0],
            assemble_stage_stub2(e12, l.shared_958_a),
        ),
        (
            STUB_958_BODY_OFFSETS[1],
            assemble_stage_stub2(e10, l.shared_958_a),
        ),
        (
            STUB_958_BODY_OFFSETS[2],
            assemble_stage_stub2(e13, l.shared_958_b),
        ),
    ];
    // Retail bytes of the (unreachable) wipe body the stubs overwrite,
    // indexed from `+0x21B4`.
    const BODY_RETAIL: [u32; 6] = [
        0x3C03_8008,
        0x2402_00FE, // +0x21B4..
        0x3C06_8008,
        0x3C05_8008, // +0x21BC..
        0xA062_BD71,
        0x90A2_BD60, // +0x21C4..
    ];
    for (base, words) in body.iter() {
        for (k, &w) in words.iter().enumerate() {
            let idx = ((base - STUB_958_BODY_OFFSETS[0]) / 4) as usize + k;
            edits.push(WordEdit {
                offset: base + (k as u64) * 4,
                expect: BODY_RETAIL[idx],
                replace: w,
            });
        }
    }
    let _ = e11; // e11 is a SCUS-side stub (s2 reset), not a module word.
    Ok(edits)
}

/// PROT 960's UN-FOLDED stage edits: the four staging stores become cave
/// calls (the id literals of [`MODULE_960_STAGE_REMAP_EDITS`] stay), so
/// the PLAYER walk plays the full retail chain `10,14,12,13,15`: opener
/// resets row `0x0A` -> raise, then charge / channel / strike repoints,
/// with the burst staging the statically-bound flourish row `0x0B`.
fn module_960_unfold_edits(l: &StageCaveLayout, offs: &[usize]) -> Result<Vec<WordEdit>> {
    check_unfold_offsets(offs, 5, 960)?;
    Ok(vec![
        // The phase-0 opener's cast-bed fire (`jal FUN_8004FCC8` with
        // `li a0,0x19A` in the delay slot) reroutes through the
        // bed-preempt cave - see `install_stage_caves` for why the
        // guarded jingle wrapper loses the bed on the combo-input path.
        // The retail delay slot stays; the cave overwrites `$a0`.
        WordEdit {
            offset: 0x0C80,
            expect: 0x0C01_3F32, // jal 0x8004FCC8
            replace: m::jal(l.preempt_960_a),
        },
        WordEdit {
            offset: 0x01C0,
            expect: 0xA282_01DA,
            replace: m::jal(l.opener_960),
        },
        WordEdit {
            offset: 0x0D6C,
            expect: 0xA242_01DA,
            replace: m::jal(l.stub_960_s2),
        },
        WordEdit {
            offset: 0x1094,
            expect: 0xA243_01DA,
            replace: m::jal(l.stub_960_s3),
        },
        WordEdit {
            offset: 0x1108,
            expect: 0xA242_01DA,
            replace: m::jal(l.stub_960_s4),
        },
    ])
}

/// Write the SCUS-resident cave code for whichever modules run
/// un-folded. Every pool byte is required zero (or already exactly ours)
/// before writing - the same "not free means another feature owns it"
/// contract as [`install_cast_hook`].
pub fn install_stage_caves(
    p: &mut DiscPatcher,
    offs_958: Option<&[usize]>,
    offs_960: Option<&[usize]>,
) -> Result<bool> {
    const SCUS_NAME: &str = "SCUS_942.54";
    if offs_958.is_none() && offs_960.is_none() {
        return Ok(false);
    }
    let l = stage_cave_layout()?;
    // (va, words) pieces, assembled per active module.
    let mut pieces: Vec<(u32, Vec<u32>)> = Vec::new();
    if let Some(offs) = offs_960 {
        check_unfold_offsets(offs, 5, 960)?;
        let (e10, e14, e12, e13) = (
            offs[0] as u32,
            offs[1] as u32,
            offs[2] as u32,
            offs[3] as u32,
        );
        pieces.push((
            l.shared_960,
            assemble_stage_core(0, ROW_A_WORD, 0x0A, S2_REG),
        ));
        pieces.push((l.stub_960_s2, assemble_stage_stub2(e14, l.shared_960)));
        pieces.push((l.stub_960_s3, assemble_stage_stub2(e12, l.shared_960)));
        pieces.push((l.stub_960_s4, assemble_stage_stub2(e13, l.shared_960)));
        pieces.push((
            l.opener_960,
            assemble_stage_cave(0, ROW_A_WORD, 0x0A, S4, e10),
        ));
        // The bed-preempt cave (split 3 + 2 across the two pools): the
        // module opener's jingle fire is rerouted here, calling the
        // guard-free XA player directly with the bed's resolved
        // (slot, chan, dur). `FUN_8004FCC8` DROPS a cue whenever the XA
        // system is busy - on the real combo-input path a shout /
        // fanfare cue is still live at module open, so the 16.9 s bed
        // silently vanished and whatever fired later played ~8 s late,
        // with the settle then holding the choreography until it
        // finished (user-video-measured). The direct call preempts:
        // `FUN_8003D53C`'s own head stops the active stream, so the bed
        // starts at cast open exactly as the enemy-side cast does.
        pieces.push((
            l.preempt_960_a,
            vec![
                m::addiu(A0, ZERO, BED_960_SLOT),
                m::j(l.preempt_960_b),
                m::addiu(A1, ZERO, BED_960_CHAN), // delay slot
            ],
        ));
        pieces.push((
            l.preempt_960_b,
            vec![
                m::j(XA_PLAY_VA),
                m::addiu(A2, ZERO, BED_960_DUR), // delay slot
            ],
        ));
    }
    if let Some(offs) = offs_958 {
        check_unfold_offsets(offs, 4, 958)?;
        let (e10, e11) = (offs[0] as u32, offs[1] as u32);
        pieces.push((
            l.shared_958_a,
            assemble_stage_core(1, ROW_A_WORD, 0x0A, S2_REG),
        ));
        pieces.push((
            l.shared_958_b,
            assemble_stage_core(1, ROW_B_WORD, 0x0B, S2_REG),
        ));
        pieces.push((l.stub_958_opener, assemble_stage_stub2(e10, l.shared_958_a)));
        pieces.push((l.stub_958_s2, assemble_stage_stub2(e11, l.shared_958_b)));
    }
    let scus = p.read_named_file(SCUS_NAME).context("read SCUS_942.54")?;
    let mut writes: Vec<(u64, Vec<u8>)> = Vec::new();
    let mut changed = false;
    for (va, words) in &pieces {
        let bytes = words_to_bytes(words);
        let off = legaia_asset::item_names::file_offset_for_va(&scus, *va)
            .with_context(|| format!("resolve SCUS offset of stage cave {va:#010x}"))?;
        let cur = &scus[off..off + bytes.len()];
        if cur == bytes.as_slice() {
            continue;
        }
        if cur.iter().any(|&b| b != 0) {
            bail!(
                "stage-cave pool at {va:#010x} is not free - another injected feature \
                 owns the bytes (shiny-seru / show-super-arts / arts-ap must be off)"
            );
        }
        writes.push((off as u64, bytes));
        changed = true;
    }
    for (off, bytes) in writes {
        p.patch_named_file(SCUS_NAME, off, &bytes)?;
    }
    Ok(changed)
}

/// `$s2` - the caster-actor register at every hooked staging store
/// except 960's opener (`$s4`).
const S2_REG: u32 = 18;

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

/// Assemble the queue-hook stub for `stub_va`, with its 4-row route table
/// appended immediately after the code (row = char index, `[const, spell]`;
/// a zero spell row is inert).
pub fn assemble_hook(stub_va: u32, routes: &[CastRoute]) -> Vec<u8> {
    let mut tbl = [0u8; 8];
    for r in routes {
        let i = usize::from(r.char_index).min(3);
        tbl[i * 2] = r.art_constant;
        tbl[i * 2 + 1] = r.spell_id;
    }

    // Code first; the table lands right after the last word.
    let mut w: Vec<u32> = Vec::with_capacity(40);

    // Prologue: keep slot/char across the real applier call.
    w.push(m::addiu(SP, SP, 0xFFE8)); // addiu sp, sp, -0x18
    w.push(m::sw(RA, SP, 0x14));
    w.push(m::sw(A0, SP, 0x10));
    w.push(m::sw(A1, SP, 0x0C));
    w.push(m::jal(APPLIER_VA));
    w.push(m::nop());
    w.push(m::lw(A0, SP, 0x10));
    w.push(m::lw(A1, SP, 0x0C));
    // v0 = "a Super fired" flag.
    w.push(m::lui(V0, m::hi(SUPER_FIRED_VA)));
    w.push(m::lw(V0, V0, m::lo(SUPER_FIRED_VA)));
    // Bounds: char index must be 0..3.
    w.push(m::sltiu(V1, A1, 4));
    let b_bounds = w.len();
    w.push(0); // beq v1, zero, DONE
    w.push(m::nop());
    let b_super = w.len();
    w.push(0); // bne v0, zero, DONE
    w.push(m::sll(T0, A1, 1)); // delay: table row offset
    // t1 = &TBL[char]
    let tbl_hi_at = w.len();
    w.push(m::lui(T1, 0)); // lui t1, hi(TBL)
    let tbl_lo_at = w.len();
    w.push(m::addiu(T1, T1, 0)); // addiu t1, t1, lo(TBL)
    w.push(m::addu(T1, T1, T0));
    w.push(m::lbu(T2, T1, 0)); // art constant
    w.push(m::lbu(T3, T1, 1)); // spell id
    w.push(m::nop());
    let b_nospell = w.len();
    w.push(0); // beq t3, zero, DONE
    w.push(m::sll(T4, A0, 2)); // delay: actor table offset
    // v1 = actor pointer.
    w.push(m::lui(V1, m::hi(ACTOR_TABLE_VA)));
    w.push(m::addiu(V1, V1, m::lo(ACTOR_TABLE_VA)));
    w.push(m::addu(V1, V1, T4));
    w.push(m::lw(V1, V1, 0));
    w.push(m::addiu(T5, ZERO, 0x14)); // scan bound (load-delay gap for v1)
    w.push(m::addiu(T6, V1, 0x1DF)); // queue cursor
    // SCAN:
    let scan = w.len();
    w.push(m::lbu(T7, T6, 0));
    w.push(m::nop());
    let b_hit = w.len();
    w.push(0); // beq t7, t2, HIT
    w.push(m::addiu(T6, T6, 1)); // delay: advance cursor
    w.push(m::addiu(T5, T5, 0xFFFF)); // t5 -= 1
    let b_scan = w.len();
    w.push(0); // bne t5, zero, SCAN
    w.push(m::nop());
    let b_done = w.len();
    w.push(0); // beq zero, zero, DONE
    w.push(m::nop());
    // HIT: (t6 sits one past the matched byte - the taken branch's delay
    // slot advanced it). The conversion policy lives in the ARENA1
    // queue-edit routine ([`assemble_delilas_arena`]): pure input
    // converts to the cast as before, a chained input keeps its leading
    // arts and defers the conversion to the strike-loop morph. The
    // routine jumps back to DONE. Fixed 4-word footprint keeps the stub
    // size (and the stage-cave tail base) stable.
    let hit = w.len();
    w.push(m::j(ARENA1_QEDIT_VA));
    w.push(m::nop()); // delay
    w.push(m::nop()); // (unreached - size filler)
    w.push(m::nop()); // (unreached - size filler)
    // DONE:
    let done = w.len();
    w.push(m::lw(RA, SP, 0x14));
    // The sp restore doubles as the load-delay filler: `jr` must not read
    // `$ra` in the slot right after its `lw` (R3000 load delay - the sim
    // does not model it, so only this ordering is live-safe).
    w.push(m::addiu(SP, SP, 0x18));
    w.push(m::jr(RA));
    w.push(m::nop());

    // Fix up branches (offset counts from the delay slot).
    let rel = |from: usize, to: usize| -> i16 { (to as i32 - from as i32 - 1) as i16 };
    w[b_bounds] = m::beq(V1, ZERO, rel(b_bounds, done));
    w[b_super] = m::bne(V0, ZERO, rel(b_super, done));
    w[b_nospell] = m::beq(T3, ZERO, rel(b_nospell, done));
    w[b_hit] = m::beq(T7, T2, rel(b_hit, hit));
    w[b_scan] = m::bne(T5, ZERO, rel(b_scan, scan));
    w[b_done] = m::beq(ZERO, ZERO, rel(b_done, done));

    // Table VA = stub + code bytes.
    let tva = stub_va + (w.len() as u32) * 4;
    w[tbl_hi_at] = m::lui(T1, m::hi(tva));
    w[tbl_lo_at] = m::addiu(T1, T1, m::lo(tva));

    let mut out: Vec<u8> = Vec::with_capacity(w.len() * 4 + 8);
    for word in &w {
        out.extend_from_slice(&word.to_le_bytes());
    }
    out.extend_from_slice(&tbl);
    out
}

/// ARENA1 (`shiny_seru` layout, `0x8007AE00..0x8007AF00`) hosts the two
/// leading-arts routines. It is certifiably free under the cast route:
/// every other ARENA1 claimant (`--shiny-seru`, `--show-super-arts`,
/// `--arts-ap-grant`) is option-exclusive with it, and
/// [`install_delilas_arena`] still verifies free-or-identical bytes.
const ARENA1_QEDIT_VA: u32 = crate::shiny_seru::ARENA1_VA;
/// The strike-loop fetch site in overlay 0898 (`FUN_801E295C` state
/// `0x1E`, `lbu v1,0x1df(v0)` at `0x801E374C` staging into `+0x1DA`,
/// cursor `ctx[+0x15]`; see `docs/subsystems/battle-action.md`).
const STRIKE_FETCH_VA: u32 = 0x801E_374C;
/// Original words at the fetch site (byte-verified against PROT 0898 at
/// base `0x801CE818`): the fetch and the busy-latch load it displaces.
const STRIKE_FETCH_WORD: u32 = 0x9043_01DF; // lbu v1, 0x1DF(v0)
const STRIKE_LATCH_WORD: u32 = 0x9262_01DC; // lbu v0, 0x1DC(s3)

/// Assemble the ARENA1 image: the queue-edit routine at `+0` (entered
/// from the stub's HIT block) and the strike-loop morph after it
/// (entered from the `0x801E374C` detour). `stub_done_va` = the stub's
/// DONE label; `tbl_va` = the stub's 4-row route table (the morph
/// re-reads it rather than carrying a copy).
///
/// Queue-edit policy at HIT (`t6` = one past the matched marker,
/// `v1` = actor, `t3` = spell): a marker not preceded by a `0x19`/`0x1A`
/// starter is left alone; a **pure** queue (only direction bytes before
/// the starter) converts to the cast exactly as the one-piece hook did
/// (consume marker, category 2, spell at `+0x1DF`) - the immediate-cast
/// feel of a bare signature input is unchanged; a **chained** queue
/// (any leading art) keeps its leading arts: only the signature's own
/// windup directions (the run immediately before the starter) are
/// deleted, and the conversion happens at playout when the strike loop
/// fetches the `[starter][marker]` pair.
///
/// Morph (entered with `v1` = fetched byte, `v0` = actor+cursor,
/// `s3` = actor, `s5` = ctx+0x11; `a0` is LIVE in the caller and never
/// touched; `t2..t9` are dead across the site - the only t-register in
/// the state-`0x1E` window, `t2`, is `mflo`-written before its next
/// read): when the fetched byte is a starter whose next byte is the
/// active slot's route marker, the action morphs into the cast -
/// category 2, spell over the (already-consumed) queue head,
/// `ctx[7] = 0x28` (Magic cast begin; the capture-class spell routes
/// `0x28 -> 0x6E` to the module, so the mid-queue cursor is never
/// re-read) - and a neutral stage id 0 returns. Both exits reload the
/// displaced `+0x1DC` busy-latch byte into `v0` in the return delay
/// slot for the caller's `ori v0,v0,2`.
pub fn assemble_delilas_arena(stub_done_va: u32, tbl_va: u32) -> Vec<u8> {
    let mut w: Vec<u32> = Vec::with_capacity(62);
    let rel = |from: usize, to: usize| -> i16 { (to as i32 - from as i32 - 1) as i16 };

    // --- Queue-edit routine (from the stub HIT) ---
    w.push(m::lbu(T7, T6, 0xFFFE)); // starter candidate at marker-1
    w.push(m::addiu(T4, V1, 0x1DF)); // queue base (load-delay filler)
    w.push(m::addiu(T8, ZERO, 0x19));
    let b_s19 = w.len();
    w.push(0); // beq t7, t8, QSTART
    w.push(m::addiu(T8, ZERO, 0x1A)); // delay
    let b_s1a = w.len();
    w.push(0); // bne t7, t8, QDONE
    w.push(m::nop());
    // QSTART: back-scan the direction run before the starter.
    let qstart = w.len();
    w.push(m::addiu(T0, T6, 0xFFFE)); // t0 = &starter
    let qback = w.len();
    let b_base = w.len();
    w.push(0); // beq t0, t4, QCLASS (reached queue base)
    w.push(m::lbu(T7, T0, 0xFFFF)); // delay: byte before the run start
    w.push(m::nop());
    w.push(m::sltiu(T9, T7, 0x10));
    let b_ge10 = w.len();
    w.push(0); // beq t9, zero, QCLASS (>= 0x10: not a direction)
    w.push(m::sltiu(T9, T7, 0x0C)); // delay
    let b_lt0c = w.len();
    w.push(0); // bne t9, zero, QCLASS (< 0x0C: not a direction)
    w.push(m::nop());
    let b_back = w.len();
    w.push(0); // beq zero, zero, QBACK
    w.push(m::addiu(T0, T0, 0xFFFF)); // delay: run start -= 1
    // QCLASS: pure iff the run reaches the queue base.
    let qclass = w.len();
    let b_chain = w.len();
    w.push(0); // bne t0, t4, QCHAIN
    w.push(m::nop());
    // PURE: the one-piece hook's conversion, verbatim.
    w.push(m::addiu(T7, ZERO, 2));
    w.push(m::sb(ZERO, T6, 0xFFFF)); // consume the matched marker
    w.push(m::sb(T7, V1, 0x1DE)); // category = Magic
    let b_pure_done = w.len();
    w.push(0); // beq zero, zero, QDONE
    w.push(m::sb(T3, V1, 0x1DF)); // delay: spell id
    // QCHAIN: delete the windup run - copy from the starter down to the
    // run start until the write cursor hits base+0x14 (reads past the
    // window are actor fields, harmless; the round driver's 16-zero
    // preseed guarantees a terminator inside the copied span).
    let qchain = w.len();
    w.push(m::addiu(T8, T6, 0xFFFE)); // src = &starter
    w.push(m::addiu(T9, T4, 0x14)); // write bound
    let qcopy = w.len();
    w.push(m::lbu(T7, T8, 0));
    w.push(m::addiu(T8, T8, 1));
    w.push(m::sb(T7, T0, 0));
    w.push(m::addiu(T0, T0, 1));
    let b_copy = w.len();
    w.push(0); // bne t0, t9, QCOPY
    w.push(m::nop());
    // QDONE:
    let qdone = w.len();
    w.push(m::j(stub_done_va));
    w.push(m::nop());

    w[b_s19] = m::beq(T7, T8, rel(b_s19, qstart));
    w[b_s1a] = m::bne(T7, T8, rel(b_s1a, qdone));
    w[b_base] = m::beq(T0, T4, rel(b_base, qclass));
    w[b_ge10] = m::beq(T9, ZERO, rel(b_ge10, qclass));
    w[b_lt0c] = m::bne(T9, ZERO, rel(b_lt0c, qclass));
    w[b_back] = m::beq(ZERO, ZERO, rel(b_back, qback));
    w[b_chain] = m::bne(T0, T4, rel(b_chain, qchain));
    w[b_pure_done] = m::beq(ZERO, ZERO, rel(b_pure_done, qdone));
    w[b_copy] = m::bne(T0, T9, rel(b_copy, qcopy));

    // --- Strike-loop morph (from the 0x801E374C detour) ---
    let morph = w.len();
    w.push(m::addiu(T7, ZERO, 0x19));
    let b_m19 = w.len();
    w.push(0); // beq v1, t7, MCHK
    w.push(m::addiu(T7, ZERO, 0x1A)); // delay
    let b_m1a = w.len();
    w.push(0); // bne v1, t7, MNORM
    w.push(m::nop());
    // MCHK:
    let mchk = w.len();
    w.push(m::lbu(T6, V0, 0x1E0)); // byte after the starter
    w.push(m::lbu(T5, S5, 0x2)); // active slot = ctx[+0x13]
    w.push(m::lui(T4, m::hi(tbl_va))); // (load-delay filler for t5)
    w.push(m::sltiu(T7, T5, 3));
    let b_mslot = w.len();
    w.push(0); // beq t7, zero, MNORM
    w.push(m::sll(T5, T5, 1)); // delay: table row offset
    w.push(m::addiu(T4, T4, m::lo(tbl_va)));
    w.push(m::addu(T4, T4, T5));
    w.push(m::lbu(T3, T4, 0)); // route marker
    w.push(m::lbu(T2, T4, 1)); // route spell
    let b_mconst = w.len();
    w.push(0); // bne t6, t3, MNORM
    w.push(m::nop());
    let b_mspell = w.len();
    w.push(0); // beq t2, zero, MNORM
    w.push(m::nop());
    // MORPH: category 2, spell at the queue head, state 0x28.
    w.push(m::addiu(T7, ZERO, 2));
    w.push(m::sb(T7, S3, 0x1DE));
    w.push(m::sb(T2, S3, 0x1DF));
    w.push(m::addiu(T7, ZERO, 0x28));
    w.push(m::sb(T7, S5, 0xFFF6)); // ctx[7] = s5 - 0x0A
    w.push(m::addiu(V1, ZERO, 0)); // neutral stage id
    // MNORM:
    let mnorm = w.len();
    w.push(m::jr(RA));
    w.push(m::lbu(V0, S3, 0x1DC)); // delay: the displaced latch load

    w[b_m19] = m::beq(V1, T7, rel(b_m19, mchk));
    w[b_m1a] = m::bne(V1, T7, rel(b_m1a, mnorm));
    w[b_mslot] = m::beq(T7, ZERO, rel(b_mslot, mnorm));
    w[b_mconst] = m::bne(T6, T3, rel(b_mconst, mnorm));
    w[b_mspell] = m::beq(T2, ZERO, rel(b_mspell, mnorm));

    debug_assert_eq!(morph, ARENA1_MORPH_WORD_INDEX, "morph offset moved");
    let mut out: Vec<u8> = Vec::with_capacity(w.len() * 4);
    for word in &w {
        out.extend_from_slice(&word.to_le_bytes());
    }
    out
}

/// Word index of the morph routine inside the arena image - pinned so
/// the 0898 detour's `jal` target is a compile-time constant.
const ARENA1_MORPH_WORD_INDEX: usize = 35;
/// VA of the strike-loop morph inside ARENA1.
const ARENA1_MORPH_VA: u32 = ARENA1_QEDIT_VA + (ARENA1_MORPH_WORD_INDEX as u32) * 4;

/// The stub's route-table VA and DONE-label VA. Both are functions of
/// the fixed-shape hook alone (the table is appended after the last
/// code word; DONE is the epilogue's first word, 4 words from the end).
fn hook_layout(stub_va: u32) -> (u32, u32) {
    let bytes = assemble_hook(stub_va, &[]);
    let code_len = (bytes.len() - 8) as u32; // 8-byte route table appended
    let tbl_va = stub_va + code_len;
    let done_va = stub_va + code_len - 4 * 4; // lw ra / addiu sp / jr / nop
    (tbl_va, done_va)
}

/// Write the ARENA1 image (queue-edit + strike morph). Free-or-identical
/// contract like [`install_cast_hook`]: bails when another injected
/// feature owns the arena.
pub fn install_delilas_arena(p: &mut DiscPatcher) -> Result<bool> {
    use crate::shiny_seru::{ARENA1_END_VA, ARENA1_VA};
    const SCUS_NAME: &str = "SCUS_942.54";
    let (tbl_va, done_va) = hook_layout(crate::shiny_seru::SCUS_GAP_VA);
    let bytes = assemble_delilas_arena(done_va, tbl_va);
    if ARENA1_VA + bytes.len() as u32 > ARENA1_END_VA {
        bail!("delilas arena does not fit ARENA1 ({} bytes)", bytes.len());
    }
    let scus = p.read_named_file(SCUS_NAME).context("read SCUS_942.54")?;
    let off = legaia_asset::item_names::file_offset_for_va(&scus, ARENA1_VA)
        .context("resolve ARENA1 file offset")?;
    let cur = &scus[off..off + bytes.len()];
    if cur == bytes.as_slice() {
        return Ok(false);
    }
    if cur.iter().any(|&b| b != 0) {
        bail!(
            "ARENA1 at {ARENA1_VA:#010x} is not free - another injected feature              (shiny-seru / show-super-arts / arts-ap-grant) owns the arena"
        );
    }
    p.patch_named_file(SCUS_NAME, off as u64, &bytes)?;
    Ok(true)
}

/// Detour the strike-loop fetch through the ARENA1 morph: two words in
/// overlay 0898 (`jal` + the fetch moved into the delay slot). No branch
/// targets either word (whole-dump scan), and the displaced busy-latch
/// load is re-issued by the morph's return delay slot.
pub fn install_strike_morph(p: &mut DiscPatcher) -> Result<bool> {
    let entry = p.read_entry(BATTLE_OVERLAY_PROT).context("read PROT 898")?;
    let edits = [
        WordEdit {
            offset: (STRIKE_FETCH_VA - BATTLE_OVERLAY_BASE) as u64,
            expect: STRIKE_FETCH_WORD,
            replace: m::jal(ARENA1_MORPH_VA),
        },
        WordEdit {
            offset: (STRIKE_FETCH_VA + 4 - BATTLE_OVERLAY_BASE) as u64,
            expect: STRIKE_LATCH_WORD,
            replace: STRIKE_FETCH_WORD, // the fetch rides the delay slot
        },
    ];
    apply_word_edits(p, BATTLE_OVERLAY_PROT, &entry, &edits)
}

/// The state-`0x28` spell-name label gate in overlay 0898
/// (`FUN_801E295C`, Magic cast begin). Retail heads the label block with
/// `lbu v0,0x2(s5); sltiu v0,v0,3; bne v0,zero,+skip` - the TAKEN side
/// skips the block, so the `0x4C` spell-name banner is raised for
/// **monster** casters only (an address sweep of the descriptor slots
/// `0x77332`/`0x77344` finds no party-side writer anywhere; the
/// `battle-action.md` "for party" reading had the branch sense
/// inverted). A converted signature cast therefore kept whatever the
/// arts chain last wrote - the leading art's name.
///
/// The fix is two in-place words: retest on the actor's action
/// **category** instead of the caster's slot. `lbu v0,0x1DE(s3);
/// sltiu v0,v0,2` makes the untouched `bne` skip the label only for
/// the Item band (category 1, the summon items `0x98`/`0x99` - whose
/// `+0x1DF` of `0x96`/`0x97` indexes Rare-Seru rows, retail's own skip)
/// and run it for every Magic cast. Monster casts are category 2, so
/// the retail monster banner is bit-identical; the player side gains
/// the same banner - the converted signature shows its spell's name
/// (the fix), and ordinary Seru casts now announce themselves the way
/// enemy casts always did (a deliberate presentation upgrade; no free
/// injection arena exists on a delilas image, every pool is carved).
const CAST_LABEL_SITE_VA: u32 = 0x801E_43D0;
/// Retail words at the site (byte-verified): `lbu v0,0x2(s5)` and, 8
/// bytes later past the `nop` delay filler, `sltiu v0,v0,0x3`.
const CAST_LABEL_SLOT_WORD: u32 = 0x92A2_0002;
const CAST_LABEL_SLTIU_WORD: u32 = 0x2C42_0003;

/// Install the label un-gate: the two category-test words over the
/// site's slot test (the `nop` between them and the `bne` after them
/// ride unchanged). See [`CAST_LABEL_SITE_VA`]. The routed spell rows
/// `0x79..=0x7B` keep their retail names - the sibling specials' own -
/// because the enemy-side rename this build once ran is retired (the
/// mirrored hero's signature is a physical attack now, so no enemy
/// casts these ids and the label is the row's only reader).
pub fn install_cast_label_gate(p: &mut DiscPatcher) -> Result<bool> {
    let entry = p.read_entry(BATTLE_OVERLAY_PROT).context("read PROT 898")?;
    let edits = [
        WordEdit {
            offset: (CAST_LABEL_SITE_VA - BATTLE_OVERLAY_BASE) as u64,
            expect: CAST_LABEL_SLOT_WORD,
            replace: m::lbu(V0, S3, 0x1DE),
        },
        WordEdit {
            offset: (CAST_LABEL_SITE_VA + 8 - BATTLE_OVERLAY_BASE) as u64,
            expect: CAST_LABEL_SLTIU_WORD,
            replace: m::sltiu(V0, V0, 2),
        },
    ];
    apply_word_edits(p, BATTLE_OVERLAY_PROT, &entry, &edits)
}

/// The `jal FUN_801EF9E4` word at the applier call site.
const APPLIER_JAL_RETAIL: u32 = 0x0C00_0000 | ((APPLIER_VA & 0x0FFF_FFFF) >> 2);

/// Write the stub into the SCUS injection gap ([`crate::shiny_seru::SCUS_GAP_VA`]) and redirect the applier
/// call through it. Claims the SCUS gap arena, so
/// the cast route is mutually exclusive with every other feature that
/// allocates the SCUS arenas (`--shiny-seru`, `--show-super-arts`); the
/// caller enforces that at option level. Returns `false` when the hook is
/// already installed byte-identically (idempotent skip).
pub fn install_cast_hook(p: &mut DiscPatcher, routes: &[CastRoute]) -> Result<bool> {
    use crate::shiny_seru::{SCUS_GAP_END_VA, SCUS_GAP_VA};
    const SCUS_NAME: &str = "SCUS_942.54";

    let stub_va = SCUS_GAP_VA;
    let bytes = assemble_hook(stub_va, routes);
    if stub_va + bytes.len() as u32 > SCUS_GAP_END_VA {
        bail!(
            "cast hook does not fit the SCUS gap ({} > {} bytes)",
            bytes.len(),
            SCUS_GAP_END_VA - SCUS_GAP_VA
        );
    }
    let scus = p.read_named_file(SCUS_NAME).context("read SCUS_942.54")?;
    let off = legaia_asset::item_names::file_offset_for_va(&scus, stub_va)
        .context("resolve SCUS gap file offset")?;
    let cur = &scus[off..off + bytes.len()];
    if cur == bytes.as_slice() {
        // Same routes already installed; make sure the redirect holds too.
        redirect_applier_call(p, stub_va)?;
        return Ok(false);
    }
    if cur.iter().any(|&b| b != 0) {
        bail!(
            "SCUS gap at {stub_va:#010x} is not free - another injected feature \
             (shiny-seru / show-super-arts) owns the arena"
        );
    }
    p.patch_named_file(SCUS_NAME, off as u64, &bytes)?;
    redirect_applier_call(p, stub_va)?;
    Ok(true)
}

/// Redirect the applier call through the stub. `stub_va` must hold the
/// bytes from [`assemble_hook`] (written by the caller into SCUS space).
pub fn redirect_applier_call(p: &mut DiscPatcher, stub_va: u32) -> Result<bool> {
    let off = u64::from(APPLIER_CALL_VA - BATTLE_OVERLAY_BASE);
    let entry = p.read_entry(BATTLE_OVERLAY_PROT)?;
    let cur = u32::from_le_bytes(entry[off as usize..off as usize + 4].try_into()?);
    let new = m::jal(stub_va);
    if cur == new {
        return Ok(false);
    }
    if cur != APPLIER_JAL_RETAIL {
        bail!(
            "applier call site {:#010x}: expected {:#010x}, found {:#010x}",
            APPLIER_CALL_VA,
            APPLIER_JAL_RETAIL,
            cur
        );
    }
    p.patch_prot_entry(BATTLE_OVERLAY_PROT, off, &new.to_le_bytes())?;
    Ok(true)
}

/// The party-init Block-reaction literal: `li v0, 0xB` at SCUS
/// `0x80054008` (`FUN_80053cb8` - `sb v0, 0x1F3(actor)` follows). Every
/// party actor's Block reaction id (`actor+0x1F3`) is seeded from this
/// one instruction, and every consumer in the corpus reads the seeded
/// VALUE back (the anim commit `FUN_8004AD80` at `0x8004B004`, the 0898
/// guard-input arm, the muscle-dome mirror) - no reader hardcodes `0xB`.
const BLOCK_REACTION_LI_VA: u32 = 0x8005_4008;
const BLOCK_REACTION_RETAIL: u32 = 0x2402_000B; // li v0, 0xB
const BLOCK_REACTION_PATCHED: u32 = 0x2402_0006; // li v0, 0x06

/// Re-home the party Block reaction onto action row `0x06`
/// (`party_swap::cast_stage::BLOCK_ROW_RELOCATED`).
///
/// The cast route hands rows `0x0A`/`0x0B` to the Megaton Press module,
/// and row `0x0B` is retail's Block row - so before the rows are
/// overwritten, the Block CLIP is re-homed byte-unmoved onto the
/// placeholder row `0x06` in **all four** player files
/// (`cast_stage::relocate_block_row`) and this one SCUS literal makes
/// every party actor's guard stage that row instead. Idempotent;
/// refuses an unknown word.
pub fn relocate_block_reaction(p: &mut DiscPatcher) -> Result<bool> {
    const SCUS_NAME: &str = "SCUS_942.54";
    let scus = p.read_named_file(SCUS_NAME).context("read SCUS_942.54")?;
    let off = legaia_asset::item_names::file_offset_for_va(&scus, BLOCK_REACTION_LI_VA)
        .context("resolve the Block-reaction literal's file offset")?;
    let cur = u32::from_le_bytes(scus[off..off + 4].try_into()?);
    if cur == BLOCK_REACTION_PATCHED {
        return Ok(false);
    }
    if cur != BLOCK_REACTION_RETAIL {
        bail!(
            "Block-reaction literal at {BLOCK_REACTION_LI_VA:#010x}: expected {BLOCK_REACTION_RETAIL:#010x}, found {cur:#010x}"
        );
    }
    p.patch_named_file(SCUS_NAME, off as u64, &BLOCK_REACTION_PATCHED.to_le_bytes())?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mips_sim::Cpu;

    const STUB_VA: u32 = 0x8007_7728; // SCUS_GAP - a plausible host
    const ACTOR_VA: u32 = 0x800F_0000;
    const RET_VA: u32 = 0x8003_0000; // fake return address for the stub call

    fn routes() -> Vec<CastRoute> {
        vec![
            CastRoute {
                char_index: 0,
                art_constant: 0x1C,
                spell_id: 0x7B,
            },
            CastRoute {
                char_index: 1,
                art_constant: 0x1F,
                spell_id: 0x79,
            },
            CastRoute {
                char_index: 2,
                art_constant: 0x1C,
                spell_id: 0x7A,
            },
        ]
    }

    /// Load the stub, a `jr ra; nop` fake applier, one actor with `queue`
    /// at `+0x1DF`, and enter the stub exactly as the redirected `jal`
    /// would (a0 = slot, a1 = char index, ra = return site).
    fn run(slot: u32, char_index: u32, queue: &[u8], super_fired: u32) -> Cpu {
        let mut cpu = Cpu::new();
        cpu.load(STUB_VA, &assemble_hook(STUB_VA, &routes()));
        let (tbl_va, done_va) = hook_layout(STUB_VA);
        cpu.load(ARENA1_QEDIT_VA, &assemble_delilas_arena(done_va, tbl_va));
        cpu.load_words(APPLIER_VA, &[m::jr(RA), m::nop()]);
        cpu.wr32(ACTOR_TABLE_VA + slot * 4, ACTOR_VA);
        cpu.wr32(SUPER_FIRED_VA, super_fired);
        cpu.wr8(ACTOR_VA + 0x1DE, 3); // Attack, as the command menu left it
        for (i, b) in queue.iter().enumerate() {
            cpu.wr8(ACTOR_VA + 0x1DF + i as u32, *b);
        }
        cpu.r[4] = slot; // a0
        cpu.r[5] = char_index; // a1
        cpu.r[31] = RET_VA; // ra
        cpu.r[29] = 0x8010_8000; // sp
        cpu.pc = STUB_VA;
        cpu.run_until(&[RET_VA]);
        assert_eq!(cpu.r[29], 0x8010_8000, "sp must balance");
        cpu
    }

    /// The R3000 load-delay law, as a gate: no assembled stub instruction
    /// may read a register in the slot right after the load that writes it
    /// (`mips_sim` does not model the delay, so only this scan catches it -
    /// the shipped hook once froze every battle's Begin on exactly this).
    #[test]
    fn no_load_delay_hazards_in_the_hook() {
        let bytes = assemble_hook(STUB_VA, &routes());
        let words: Vec<u32> = bytes
            .chunks_exact(4)
            .map(|c| u32::from_le_bytes(c.try_into().unwrap()))
            .collect();
        let loaded_reg = |w: u32| -> Option<u32> {
            // lb/lh/lwl/lw/lbu/lhu/lwr
            matches!(w >> 26, 0x20..=0x26).then_some((w >> 16) & 0x1F)
        };
        let reads = |w: u32, r: u32| -> bool {
            if r == 0 || w == 0 {
                return false;
            }
            let op = w >> 26;
            let rs = (w >> 21) & 0x1F;
            let rt = (w >> 16) & 0x1F;
            match op {
                0 => {
                    // SPECIAL: jr/jalr read rs; shifts read rt; ALU read both.
                    rs == r || ((w & 0x3F) > 0x08 && rt == r)
                }
                2 | 3 => false,                    // j / jal
                0x28..=0x2E => rs == r || rt == r, // stores read both
                _ => rs == r,                      // imm ops + loads read rs
            }
        };
        for (i, pair) in words.windows(2).enumerate() {
            if let Some(r) = loaded_reg(pair[0]) {
                assert!(
                    !reads(pair[1], r),
                    "load-delay hazard at word {i}: {:#010x} then {:#010x}",
                    pair[0],
                    pair[1]
                );
            }
        }
    }

    #[test]
    fn signature_constant_in_the_queue_becomes_the_cast() {
        let cpu = run(0, 0, &[0x0C, 0x0D, 0x19, 0x1C, 0x00], 0);
        assert_eq!(cpu.rd8(ACTOR_VA + 0x1DE), 2, "category flips to Magic");
        assert_eq!(cpu.rd8(ACTOR_VA + 0x1DF), 0x7B, "spell id installed");
    }

    #[test]
    fn a_super_wins_over_the_cast() {
        let cpu = run(0, 0, &[0x19, 0x1C, 0x00], 1);
        assert_eq!(cpu.rd8(ACTOR_VA + 0x1DE), 3, "category stays Attack");
        assert_eq!(cpu.rd8(ACTOR_VA + 0x1DF), 0x19, "queue untouched");
    }

    #[test]
    fn a_queue_without_the_constant_is_left_alone() {
        let cpu = run(0, 0, &[0x0C, 0x0D, 0x19, 0x1B, 0x00], 0);
        assert_eq!(cpu.rd8(ACTOR_VA + 0x1DE), 3);
        assert_eq!(cpu.rd8(ACTOR_VA + 0x1DF), 0x0C);
    }

    #[test]
    fn per_character_rows_resolve_independently() {
        // Noa's 0x1F on Noa's slot fires her mapped spell...
        let cpu = run(1, 1, &[0x19, 0x1F, 0x00], 0);
        assert_eq!(cpu.rd8(ACTOR_VA + 0x1DF), 0x79);
        // ...but Vahn's 0x1C on Noa's row does nothing.
        let cpu = run(1, 1, &[0x19, 0x1C, 0x00], 0);
        assert_eq!(cpu.rd8(ACTOR_VA + 0x1DE), 3);
    }

    #[test]
    fn terra_and_out_of_range_characters_are_inert() {
        let cpu = run(0, 3, &[0x1C, 0x00], 0);
        assert_eq!(cpu.rd8(ACTOR_VA + 0x1DE), 3, "Terra row is zeroed");
        let cpu = run(0, 9, &[0x1C, 0x00], 0);
        assert_eq!(cpu.rd8(ACTOR_VA + 0x1DE), 3, "bounds guard holds");
    }

    #[test]
    fn scan_stays_inside_the_queue_window() {
        // Constant only PAST the 0x14-byte scan bound: must not match.
        let mut queue = vec![0u8; 0x14];
        queue.push(0x1C);
        let cpu = run(0, 0, &queue, 0);
        assert_eq!(cpu.rd8(ACTOR_VA + 0x1DE), 3);
    }

    #[test]
    fn a_chained_queue_keeps_its_leading_art_and_defers() {
        // Somersault (0F 0E 19 27) chained into the signature combo
        // (0C 0D 0F 0F 19 1C): the signature's own windup directions
        // are deleted, everything else - category included - is kept
        // for the strike loop, where the morph converts at the
        // [starter][marker] fetch.
        let cpu = run(
            0,
            0,
            &[
                0x0F, 0x0E, 0x19, 0x27, 0x0C, 0x0D, 0x0F, 0x0F, 0x19, 0x1C, 0x00,
            ],
            0,
        );
        assert_eq!(cpu.rd8(ACTOR_VA + 0x1DE), 3, "category stays Attack");
        let want = [0x0F, 0x0E, 0x19, 0x27, 0x19, 0x1C, 0x00, 0x00];
        for (i, b) in want.iter().enumerate() {
            assert_eq!(
                cpu.rd8(ACTOR_VA + 0x1DF + i as u32),
                *b,
                "queue byte {i} after the windup deletion"
            );
        }
    }

    #[test]
    fn a_marker_without_a_starter_is_left_alone() {
        // A marker byte that is not part of a tokenized [starter][marker]
        // pair (a direction sits before it) is not a signature commit.
        let cpu = run(0, 0, &[0x0C, 0x1C, 0x00], 0);
        assert_eq!(cpu.rd8(ACTOR_VA + 0x1DE), 3);
        assert_eq!(cpu.rd8(ACTOR_VA + 0x1DF), 0x0C);
        assert_eq!(cpu.rd8(ACTOR_VA + 0x1E0), 0x1C);
    }

    #[test]
    fn a_pure_queue_consumes_the_marker() {
        let cpu = run(0, 0, &[0x0C, 0x0D, 0x0F, 0x0F, 0x19, 0x1C, 0x00], 0);
        assert_eq!(cpu.rd8(ACTOR_VA + 0x1DE), 2);
        assert_eq!(cpu.rd8(ACTOR_VA + 0x1DF), 0x7B);
        assert_eq!(cpu.rd8(ACTOR_VA + 0x1E4), 0, "marker consumed");
    }

    const CTX_VA: u32 = 0x800E_0000;

    /// Enter the strike-loop morph exactly as the 0898 detour does:
    /// `v1` = the fetched byte, `v0` = actor+cursor, `s3` = actor,
    /// `s5` = ctx+0x11, `ra` = the return site.
    fn run_morph(fetched: u8, cur: u32, slot: u8, queue: &[u8]) -> Cpu {
        let mut cpu = Cpu::new();
        let (tbl_va, done_va) = hook_layout(STUB_VA);
        cpu.load(STUB_VA, &assemble_hook(STUB_VA, &routes())); // route table
        cpu.load(ARENA1_QEDIT_VA, &assemble_delilas_arena(done_va, tbl_va));
        for (i, b) in queue.iter().enumerate() {
            cpu.wr8(ACTOR_VA + 0x1DF + i as u32, *b);
        }
        cpu.wr8(ACTOR_VA + 0x1DE, 3);
        cpu.wr8(ACTOR_VA + 0x1DC, 0x40); // recognizable busy-latch value
        cpu.wr8(CTX_VA + 0x13, slot);
        cpu.r[2] = ACTOR_VA + cur; // v0
        cpu.r[3] = fetched as u32; // v1
        cpu.r[19] = ACTOR_VA; // s3
        cpu.r[21] = CTX_VA + 0x11; // s5
        cpu.r[31] = RET_VA;
        cpu.pc = ARENA1_MORPH_VA;
        cpu.run_until(&[RET_VA]);
        cpu
    }

    #[test]
    fn the_morph_converts_at_the_starter_marker_fetch() {
        // Chained playout mid-queue: cursor 4 fetched the 0x19 whose
        // next byte is slot 0's marker.
        let cpu = run_morph(0x19, 4, 0, &[0x0F, 0x0E, 0x19, 0x27, 0x19, 0x1C, 0x00]);
        assert_eq!(cpu.rd8(ACTOR_VA + 0x1DE), 2, "category flips to Magic");
        assert_eq!(cpu.rd8(ACTOR_VA + 0x1DF), 0x7B, "spell over the queue head");
        assert_eq!(cpu.rd8(CTX_VA + 7), 0x28, "state -> Magic cast begin");
        assert_eq!(cpu.r[3], 0, "neutral stage id returned");
        assert_eq!(cpu.r[2], 0x40, "displaced latch load re-issued");
    }

    #[test]
    fn the_morph_ignores_an_ordinary_art() {
        let cpu = run_morph(0x19, 2, 0, &[0x0F, 0x0E, 0x19, 0x27, 0x19, 0x1C, 0x00]);
        assert_eq!(cpu.rd8(ACTOR_VA + 0x1DE), 3, "category untouched");
        assert_eq!(cpu.rd8(CTX_VA + 7), 0, "state untouched");
        assert_eq!(cpu.r[3], 0x19, "fetched byte staged unchanged");
        assert_eq!(cpu.r[2], 0x40, "latch load still re-issued");
    }

    #[test]
    fn the_morph_ignores_plain_swings_and_monster_slots() {
        let cpu = run_morph(0x0C, 0, 0, &[0x0C, 0x00]);
        assert_eq!(cpu.r[3], 0x0C);
        assert_eq!(cpu.rd8(CTX_VA + 7), 0);
        // A monster slot (>= 3) never routes.
        let cpu = run_morph(0x19, 0, 4, &[0x19, 0x1C, 0x00]);
        assert_eq!(cpu.rd8(CTX_VA + 7), 0);
        assert_eq!(cpu.r[3], 0x19);
    }

    #[test]
    fn the_morph_converts_a_pure_head_fetch_for_slot2() {
        // Che's route shares Vahn's marker; the row is the ACTIVE slot's.
        let cpu = run_morph(0x19, 0, 2, &[0x19, 0x1C, 0x00]);
        assert_eq!(cpu.rd8(ACTOR_VA + 0x1DF), 0x7A, "slot 2 spell");
        assert_eq!(cpu.rd8(CTX_VA + 7), 0x28);
    }

    #[test]
    fn no_load_delay_hazards_in_the_arena() {
        let (tbl_va, done_va) = hook_layout(STUB_VA);
        let bytes = assemble_delilas_arena(done_va, tbl_va);
        assert!(
            bytes.len() <= 0x100,
            "arena image must fit ARENA1 (got {} bytes)",
            bytes.len()
        );
        let words: Vec<u32> = bytes
            .chunks_exact(4)
            .map(|c| u32::from_le_bytes(c.try_into().unwrap()))
            .collect();
        let loaded_reg =
            |w: u32| -> Option<u32> { matches!(w >> 26, 0x20..=0x26).then_some((w >> 16) & 0x1F) };
        let reads = |w: u32, r: u32| -> bool {
            if r == 0 || w == 0 {
                return false;
            }
            let op = w >> 26;
            let rs = (w >> 21) & 0x1F;
            let rt = (w >> 16) & 0x1F;
            match op {
                0 => rs == r || ((w & 0x3F) > 0x08 && rt == r),
                2 | 3 => false,
                0x28..=0x2E => rs == r || rt == r,
                _ => rs == r,
            }
        };
        for (i, pair) in words.windows(2).enumerate() {
            if let Some(r) = loaded_reg(pair[0]) {
                assert!(
                    !reads(pair[1], r),
                    "load-delay hazard at arena word {i}: {:#010x} then {:#010x}",
                    pair[0],
                    pair[1]
                );
            }
        }
    }
}
