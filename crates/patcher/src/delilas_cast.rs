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
//! 2. **The cast modules** (PROT 958/959/960, link base `0x801F69D8`): the
//!    per-tick damage/HP writes hardcode battle-actor `table[0]`
//!    (`lw rX, -0x6C90($fp)`, party seat 0) even though the tick prologue
//!    already derives the true target (`table[caster[+0x1DD]]`) into a saved
//!    register that is never clobbered. Each such load becomes
//!    `move rX, <target reg>`. The finale's "victim at 0 HP" arm - which in
//!    retail declares a party wipe, because the victim of a boss cast is a
//!    hero - is neutralised to the alive path so killing a monster with the
//!    special falls through to the ordinary end-of-action liveness sweep.
//!
//! Verified live for PROT 959 (Megaton Press): with the five moves applied,
//! the module damages the chosen monster and leaves party seat 0 untouched.
//!
//! No Sony bytes ship here: every patched word is derived from (and verified
//! against) the user's own disc before writing.

#![allow(dead_code)] // wired behind the delilas-party apply chain

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

/// Cast-module link base (slot-B overlay window).
const MODULE_BASE_VA: u32 = 0x801F_69D8;

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

/// PROT 959 finale (phase 0x11, `0x801F8158`): `bnez <victim hp>, alive`
/// becomes an unconditional branch to the alive path, so a monster victim
/// at 0 HP no longer trips the retail party-wipe shortcut (`BD2C = 5`).
/// The wipe/defeat special-case exists because retail's victim is always a
/// hero; real deaths on either side are still caught by the end-of-action
/// liveness sweep (state `0x5A`).
const MODULE_959_WIPE_EDIT: WordEdit = WordEdit {
    offset: 0x1780,
    expect: 0x1460_0013,  // bnez v1, +0x13
    replace: 0x1000_0013, // b +0x13
};

/// Apply the PROT 959 module patches.
pub fn patch_module_959(p: &mut DiscPatcher) -> Result<bool> {
    let entry = p.read_entry(959).context("read PROT 959")?;
    let mut edits: Vec<WordEdit> = MODULE_959_DAMAGE_EDITS.to_vec();
    edits.push(MODULE_959_WIPE_EDIT);

    let word = |off: u64| -> Result<u32> {
        let off = off as usize;
        Ok(u32::from_le_bytes(
            entry
                .get(off..off + 4)
                .with_context(|| format!("PROT 959 short at +{off:#x}"))?
                .try_into()?,
        ))
    };

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
    // HIT:
    let hit = w.len();
    w.push(m::addiu(T7, ZERO, 2));
    w.push(m::sb(T7, V1, 0x1DE)); // category = Magic
    w.push(m::sb(T3, V1, 0x1DF)); // spell id
    // DONE:
    let done = w.len();
    w.push(m::lw(RA, SP, 0x14));
    w.push(m::jr(RA));
    w.push(m::addiu(SP, SP, 0x18));

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
