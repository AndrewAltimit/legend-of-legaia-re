//! The cast-route hook itself: its assembly, the Delilas arena that hosts it,
//! and the strike-morph word it patches.

use super::{
    ACTOR_TABLE_VA, APPLIER_VA, BATTLE_OVERLAY_BASE, BATTLE_OVERLAY_PROT, CastRoute,
    SUPER_FIRED_VA, WordEdit, apply_word_edits,
};
use anyhow::{Context, Result, bail};
use legaia_code_hooks::mips::{
    self as m, A0, A1, RA, S3, S5, SP, T0, T1, T2, T3, T4, T5, T6, T7, T8, V0, V1, ZERO,
};
use legaia_disc_patch::disc::DiscPatcher;

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
pub(super) const ARENA1_QEDIT_VA: u32 = legaia_code_hooks::arenas::ARENA1_VA;
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
/// starter is left alone; a starter **at the queue base** converts to
/// the cast exactly as the one-piece hook did (consume marker,
/// category 2, spell at `+0x1DF`) - the immediate-cast feel of a bare
/// signature input is unchanged; a starter **anywhere else** defers
/// with the queue untouched, and the conversion happens at playout
/// when the strike loop fetches the `[starter][marker]` pair.
///
/// Why base-or-defer and not a windup back-scan: the retail matcher's
/// Hyper arm (`0x801EF4E8`) CONSUMES the matched arrow span, so a real
/// tokenized queue never carries windup directions before the
/// signature's `0x1A` - a bare input tokenizes to `[1A M]` at base and
/// a chained one to `[.. 19 <art> 1A M]`. The only queues with raw
/// directions before the starter are chains whose leading art the AP
/// admission gate dropped (`0x801EF424`: the Hyper's tier is charged
/// first, and an unaffordable completed match emits nothing), and
/// those directions are retail basic strikes the player entered -
/// deferring plays them out instead of silently eating them. The
/// earlier back-scan classified exactly those queues as "pure" and
/// cast instantly, which read as "my leading art was skipped".
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
    w.push(m::addiu(T0, T6, 0xFFFE)); // t0 = &starter (both branch paths)
    w.push(m::addiu(T8, ZERO, 0x19));
    let b_s19 = w.len();
    w.push(0); // beq t7, t8, QCHK
    w.push(m::addiu(T8, ZERO, 0x1A)); // delay
    let b_s1a = w.len();
    w.push(0); // bne t7, t8, QDONE
    w.push(m::nop());
    // QCHK: pure iff the starter sits at the queue base.
    let qchk = w.len();
    let b_defer = w.len();
    w.push(0); // bne t0, t4, QDONE (anything leading: defer untouched)
    w.push(m::nop());
    // PURE: the one-piece hook's conversion, verbatim.
    w.push(m::addiu(T7, ZERO, 2));
    w.push(m::sb(ZERO, T6, 0xFFFF)); // consume the matched marker
    w.push(m::sb(T7, V1, 0x1DE)); // category = Magic
    w.push(m::sb(T3, V1, 0x1DF)); // spell id (over the base starter)
    // QDONE:
    let qdone = w.len();
    w.push(m::j(stub_done_va));
    w.push(m::nop());

    w[b_s19] = m::beq(T7, T8, rel(b_s19, qchk));
    w[b_s1a] = m::bne(T7, T8, rel(b_s1a, qdone));
    w[b_defer] = m::bne(T0, T4, rel(b_defer, qdone));

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
const ARENA1_MORPH_WORD_INDEX: usize = 16;
/// VA of the strike-loop morph inside ARENA1.
pub(super) const ARENA1_MORPH_VA: u32 = ARENA1_QEDIT_VA + (ARENA1_MORPH_WORD_INDEX as u32) * 4;

/// The stub's route-table VA and DONE-label VA. Both are functions of
/// the fixed-shape hook alone (the table is appended after the last
/// code word; DONE is the epilogue's first word, 4 words from the end).
pub(super) fn hook_layout(stub_va: u32) -> (u32, u32) {
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
    use legaia_code_hooks::arenas::{ARENA1_END_VA, ARENA1_VA};
    const SCUS_NAME: &str = "SCUS_942.54";
    let (tbl_va, done_va) = hook_layout(legaia_code_hooks::arenas::SCUS_GAP_VA);
    let bytes = assemble_delilas_arena(done_va, tbl_va);
    if ARENA1_VA + bytes.len() as u32 > ARENA1_END_VA {
        bail!("delilas arena does not fit ARENA1 ({} bytes)", bytes.len());
    }
    let scus = p.read_named_file(SCUS_NAME).context("read SCUS_942.54")?;
    let off = legaia_game_tables::item_names::file_offset_for_va(&scus, ARENA1_VA)
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
