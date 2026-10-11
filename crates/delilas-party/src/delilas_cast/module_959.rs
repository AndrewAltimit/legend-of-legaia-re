//! Cast module 959 (PROT 0959): the damage, wipe, stage and teardown word edits
//! and their installer.

use super::{S4, WordEdit, apply_word_edits, move_};
use anyhow::{Context, Result};
use legaia_code_hooks::mips::{self as m, A0, S5, V0, V1, ZERO};
use legaia_disc_patch::disc::DiscPatcher;

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
pub(super) const MODULE_959_WIPE_EDIT: WordEdit = WordEdit {
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
///   the increment alone - `legaia_party_swap::cast_stage` gives the player
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
pub(super) const MODULE_959_TEARDOWN_EDITS: &[WordEdit] = &[
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
/// `legaia_party_swap::cast_stage`); `true` pins both stages to row `0x0A`
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
