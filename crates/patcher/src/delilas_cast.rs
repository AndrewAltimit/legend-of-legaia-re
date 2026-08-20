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
use crate::mips::{self as m, A0, A1, RA, SP, T0, T1, T2, T3, T4, T5, T6, T7, V0, V1, ZERO};

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
        replace: m::lw(A0, m::S5, 0x102C), // a0 = cached finale entity
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
        replace: m::sw(ZERO, m::S5, 0x102C), // clear the cache
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

    // Idempotence: all-already-patched is a clean skip; a mix is an error.
    let already = edits
        .iter()
        .filter(|e| word(e.offset).map(|w| w == e.replace).unwrap_or(false))
        .count();
    if already == edits.len() {
        return Ok(false);
    }
    if already != 0 {
        bail!(
            "PROT 959 is partially patched ({already}/{} sites) - refusing",
            edits.len()
        );
    }
    for e in &edits {
        let w = word(e.offset)?;
        if w != e.expect {
            bail!(
                "PROT 959 +{:#x}: expected {:#010x}, found {:#010x} - not a known retail image",
                e.offset,
                e.expect,
                w
            );
        }
    }
    for e in &edits {
        p.patch_prot_entry(959, e.offset, &e.replace.to_le_bytes())?;
    }
    Ok(true)
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
    // slot advanced it)
    let hit = w.len();
    w.push(m::addiu(T7, ZERO, 2));
    w.push(m::sb(ZERO, T6, 0xFFFF)); // consume the matched byte (one-shot) -
    // BEFORE the spell write: a match at +0x1DF itself is the same cell
    w.push(m::sb(T7, V1, 0x1DE)); // category = Magic
    w.push(m::sb(T3, V1, 0x1DF)); // spell id
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
        let cpu = run(1, 1, &[0x1F, 0x00], 0);
        assert_eq!(cpu.rd8(ACTOR_VA + 0x1DF), 0x79);
        // ...but Vahn's 0x1C on Noa's row does nothing.
        let cpu = run(1, 1, &[0x1C, 0x00], 0);
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
}
