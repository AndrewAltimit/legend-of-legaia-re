//! **Arts AP-grant** hook: make selected Tactical Arts *grant* AP (Spirit)
//! instead of *costing* it.
//!
//! A community modding knob. Retail deducts an art's AP cost from the caster's
//! Spirit gauge (`actor[+0x170]`) inside the party arts queue-builder
//! `FUN_801EED1C` (PROT 0898, base `0x801CE818`), refusing the art when Spirit
//! is short. This feature detours three sites of that builder so a configured
//! art is admitted at any Spirit level and *adds* `amount` AP (clamped at the
//! native 100 cap) instead of paying a cost, without polluting the end-of-turn
//! refund accumulator.
//!
//! ## The four pinned sites (byte-verified against the extracted 0898 image)
//!
//! `FUN_801EED1C` is the **party** arts queue-builder (slot < 3; monster AI uses
//! `FUN_801E7320`), so the hook affects **player arts only** - enemies are
//! untouched. The art identity is register `s3` (the art-table row cursor, `li
//! s3,0xb` at `0x801ef2ec`); the 0-based config index is `s3 - 0x0B`, which
//! equals the character's arts-table **display index** (`0` = Miracle Art).
//!
//! | Site | VA | Stock word | Role |
//! |---|---|---|---|
//! | A affordability guard | `0x801EF410` | `0x94A20170` (`lhu v0,0x170(a1)`) | bypassed for a grant art (admit at 0 AP) |
//! | B per-art index | `0x801EF438` | `0x2665FFF5` (`addiu a1,s3,-0xb`) | pins the config index = `s3 - 0x0B` (not detoured, just the index proof) |
//! | C AP debit + accrual | `0x801EF490` | `0x94620170` (`lhu v0,0x170(v1)`) | grant art *adds* AP + skips the `+0x224` accrual |
//! | D end-of-turn refund | `0x801EF988` | `0x94620170` (`lhu v0,0x170(v1)`) | clamps the `Spirit += +0x224` refund at 100 |
//!
//! `actor[+0x170]` = Spirit/AP; `actor[+0x224]` = spent-AP accumulator (added
//! back at D at end of turn). See [`docs/subsystems/arts-command-gauge.md`].
//!
//! ## Config table + placement
//!
//! A 26-entry `i8` table `AP_GRANT[row]` (`row = s3 - 0x0B`, art rows `0x0B..=0x24`):
//! `0` = unmodified retail; `> 0` = grant that many AP (admit + no cost). The
//! battle overlay is packed (no dead space - see the RE record), so the three
//! detour routines + the table are injected into a **verified-dead SCUS arena**
//! (`shiny_seru::ARENA1_VA`, read-watch-confirmed dead). Because those bytes are
//! the same ones the shiny-Seru feature reuses, **AP-grant is mutually exclusive
//! with `--shiny-seru`** - enforced in the CLI and the web patcher.
//!
//! ## Combo targeting + the row-sharing caveat
//!
//! An art is targeted by its input **combo** (like [`crate::arts_power`]). The
//! combo resolves to its arts-table display index, which is the config **row**.
//! The row is a *shared* index across the three characters (the table is indexed
//! by `s3 - 0x0B`, not by character), so setting a row grants **every**
//! character's art at that same row. [`resolve`] returns the full set of arts a
//! grant touches so the caller can surface it. No Sony bytes are embedded; the
//! routines are the patcher's own code.

use anyhow::{Result, bail};

use legaia_art::arts_table::{self, ArtTableEntry};
use legaia_art::queue::{Character, Command};

use crate::mips::*;
use crate::shiny_seru::{ARENA1_END_VA, ARENA1_VA, Edit, OVERLAY_TABLE_RANGES, SCUS_TABLE_RANGES};

/// Number of config rows (`s3 - 0x0B` over art rows `0x0B..=0x24`).
pub const NUM_ROWS: usize = 26;
/// Native Spirit/AP cap the granted total is clamped at.
pub const AP_CAP: u16 = 100;

/// PROT entry index of the battle-action overlay (0898) hosting the detour sites.
pub const OVERLAY_PROT_INDEX: usize = legaia_asset::move_power::BATTLE_ACTION_OVERLAY_PROT_INDEX;
/// Load base VA of the slot-A overlays; overlay file offset = `va - BASE`.
pub const OVERLAY_BASE_VA: u32 = legaia_asset::move_power::BATTLE_OVERLAY_BASE;

// --- Pinned hook sites (VA, expected first word, return VA) ------------------

/// B: per-art index proof. `addiu a1,s3,-0xb` - confirms the config index is
/// `s3 - 0x0B`. Not detoured (read-only build fingerprint).
pub const HOOK_B_VA: u32 = 0x801E_F438;
pub(crate) const HOOK_B_W0: u32 = 0x2665_FFF5; // addiu a1,s3,-0xb

/// A: affordability guard. Detour replaces `lhu v0,0x170(a1)` + the following
/// `mflo t7`; returns to `0x801EF418` (`slt v0,v0,t7`).
pub const HOOK_A_VA: u32 = 0x801E_F410;
pub(crate) const HOOK_A_W0: u32 = 0x94A2_0170; // lhu v0,0x170(a1)
const RET_A_VA: u32 = 0x801E_F418;

/// C: AP debit + accrual. Detour replaces `lhu v0,0x170(v1)` + the following
/// `nop`. A grant art returns to `0x801EF4B8` (past the debit AND the `+0x224`
/// accrual `0x801EF4A0..0x801EF4B4`); a native art returns to `0x801EF498`
/// (`subu v0,v0,a2`, the stock debit).
pub const HOOK_C_VA: u32 = 0x801E_F490;
pub(crate) const HOOK_C_W0: u32 = 0x9462_0170; // lhu v0,0x170(v1)
const C_GRANT_RET_VA: u32 = 0x801E_F4B8;
const C_NATIVE_RET_VA: u32 = 0x801E_F498;

/// D: end-of-turn refund. Detour replaces `lhu v0,0x170(v1)` + the following
/// `nop`; the routine does the `Spirit += +0x224` add itself, clamps at 100,
/// stores, and returns to `0x801EF998` (past the stock `addu`/`sh`).
pub const HOOK_D_VA: u32 = 0x801E_F988;
pub(crate) const HOOK_D_W0: u32 = 0x9462_0170; // lhu v0,0x170(v1)
const RET_D_VA: u32 = 0x801E_F998;

// --- Routine assemblers ------------------------------------------------------

/// (A) Affordability-guard bypass. Replays the displaced `lhu v0,0x170(a1)`
/// (Spirit) + `mflo t7` (cost, `LO` preserved - the routine issues no
/// `mult`/`div`), and forces `v0 = 0x7FFF` for a grant art so the stock
/// `slt v0,v0,t7` at the return site reads "affordable". Native arts keep the
/// real Spirit. `disp = [lhu, mflo t7]`; `ret = 0x801EF418`.
pub(crate) fn assemble_guard(table_va: u32, disp: [u32; 2], ret: u32) -> Vec<u32> {
    const NATIVE: i32 = 12;
    vec![
        andi(T0, S3, 0xff),                 // 0  t0 = s3 & 0xff
        addiu(T0, T0, 0xFFF5),              // 1  t0 -= 0x0b  (row)
        sltiu(T1, T0, NUM_ROWS as u16),     // 2  row < 26?
        beq(T1, ZERO, (NATIVE - 4) as i16), // 3  row>=26 -> native
        disp[0],                            // 4  delay: v0 = Spirit (always)
        lui(T2, hi(table_va)),              // 5
        addu(T2, T2, T0),                   // 6  &AP_GRANT[row]
        lb(T2, T2, lo(table_va)),           // 7  g = (i8)AP_GRANT[row]
        nop(),                              // 8  load delay
        blez(T2, (NATIVE - 10) as i16),     // 9  g<=0 -> native
        nop(),                              // 10
        ori(V0, ZERO, 0x7FFF),              // 11 grant -> force affordable
        disp[1],                            // 12 NATIVE: mflo t7 (replay; LO intact)
        j(ret),                             // 13
        nop(),                              // 14
    ]
}

/// (C) Grant-instead-of-debit. Replays `lhu v0,0x170(v1)` (Spirit); a grant art
/// adds `g` (clamped at [`AP_CAP`]), stores, and returns PAST the `+0x224`
/// accrual so the refund never double-counts it. Native arts fall back to the
/// stock `subu v0,v0,a2` at `0x801EF498`. `disp = [lhu, nop]`.
pub(crate) fn assemble_debit(
    table_va: u32,
    disp: [u32; 2],
    grant_ret: u32,
    native_ret: u32,
) -> Vec<u32> {
    const NATIVE: i32 = 19;
    const STORE: i32 = 16;
    vec![
        andi(T0, S3, 0xff),                 // 0
        addiu(T0, T0, 0xFFF5),              // 1  row
        sltiu(T1, T0, NUM_ROWS as u16),     // 2
        beq(T1, ZERO, (NATIVE - 4) as i16), // 3  row>=26 -> native
        disp[0],                            // 4  delay: v0 = Spirit (always)
        lui(T2, hi(table_va)),              // 5
        addu(T2, T2, T0),                   // 6
        lb(T2, T2, lo(table_va)),           // 7  g
        nop(),                              // 8  load delay
        blez(T2, (NATIVE - 10) as i16),     // 9  g<=0 -> native debit
        nop(),                              // 10
        addu(V0, V0, T2),                   // 11 Spirit += g
        sltiu(T1, V0, AP_CAP + 1),          // 12 v0 <= 100?
        bne(T1, ZERO, (STORE - 14) as i16), // 13 in range -> store
        nop(),                              // 14
        ori(V0, ZERO, AP_CAP),              // 15 clamp to 100
        sh(V0, V1, 0x170),                  // 16 STORE: Spirit = v0
        j(grant_ret),                       // 17 -> 0x801EF4B8 (skip debit + accrual)
        nop(),                              // 18
        disp[1],                            // 19 NATIVE: nop (replay)
        j(native_ret),                      // 20 -> 0x801EF498 (stock subu)
        nop(),                              // 21
    ]
}

/// (D) Refund clamp. Replays `lhu v0,0x170(v1)`, adds the accumulated spent AP
/// (`a0`, loaded at `0x801EF984`), clamps at [`AP_CAP`], stores, and returns to
/// `0x801EF998` (skipping the stock unclamped `addu`/`sh`). `disp = [lhu, nop]`.
pub(crate) fn assemble_refund(disp: [u32; 2], ret: u32) -> Vec<u32> {
    const ST: i32 = 7;
    vec![
        disp[0],                        // 0  lhu v0,0x170(v1)
        disp[1],                        // 1  nop (load delay)
        addu(V0, V0, A0),               // 2  Spirit += accumulated
        sltiu(T0, V0, AP_CAP + 1),      // 3  v0 <= 100?
        bne(T0, ZERO, (ST - 5) as i16), // 4  in range -> store
        nop(),                          // 5
        ori(V0, ZERO, AP_CAP),          // 6  clamp
        sh(V0, V1, 0x170),              // 7  ST: store
        j(ret),                         // 8
        nop(),                          // 9
    ]
}

// --- Combo -> config-row resolution -----------------------------------------

/// One resolved grant: the config `row` (= arts-table index), the `amount`, the
/// combo the user targeted, and every art that occupies that row across the
/// three characters (the row is shared, so all of these are affected).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedGrant {
    pub row: u8,
    pub amount: u8,
    pub targeted_combo: Vec<Command>,
    /// `(character, art name, combo)` for every art at this row.
    pub shared: Vec<(Character, String, Vec<Command>)>,
}

/// Render a combo as `L/R/D/U` glyphs.
pub fn combo_str(combo: &[Command]) -> String {
    combo.iter().map(crate::arts_power::command_glyph).collect()
}

/// Resolve `(combo, amount)` grants into the 26-entry config table + the
/// per-row resolution. A combo maps to its arts-table display index (the config
/// row); a combo shared across characters at different indices sets each such
/// row. Errors on an unknown combo, an out-of-range amount/index, or a row set
/// to two conflicting amounts.
pub fn resolve(
    scus: &[u8],
    grants: &[(Vec<Command>, u8)],
) -> Result<([i8; NUM_ROWS], Vec<ResolvedGrant>)> {
    let entries: Vec<ArtTableEntry> = arts_table::parse_from_scus(scus)
        .ok_or_else(|| anyhow::anyhow!("parse arts-name table"))?;
    let mut config = [0i8; NUM_ROWS];
    let mut resolved: Vec<ResolvedGrant> = Vec::new();
    for (combo, amount) in grants {
        if *amount == 0 {
            bail!(
                "AP-grant amount for {} must be >= 1 (0 leaves the art at retail)",
                combo_str(combo)
            );
        }
        if u16::from(*amount) > AP_CAP {
            bail!(
                "AP-grant amount {amount} for {} exceeds the {AP_CAP} AP cap",
                combo_str(combo)
            );
        }
        let rows: std::collections::BTreeSet<u8> = entries
            .iter()
            .filter(|e| e.commands == *combo)
            .map(|e| e.index)
            .collect();
        if rows.is_empty() {
            bail!(
                "no Tactical Art has combo {} (nothing to AP-grant)",
                combo_str(combo)
            );
        }
        for &row in &rows {
            if usize::from(row) >= NUM_ROWS {
                bail!(
                    "art index {row} for combo {} is outside the {NUM_ROWS}-row config space",
                    combo_str(combo)
                );
            }
            let prev = config[usize::from(row)];
            if prev != 0 && prev != *amount as i8 {
                bail!(
                    "AP-grant row {row} set to conflicting amounts ({prev} and {amount}); \
                     it is a shared row - pick one value"
                );
            }
            config[usize::from(row)] = *amount as i8;
            let shared = entries
                .iter()
                .filter(|e| e.index == row)
                .map(|e| (e.character, e.name.clone(), e.commands.clone()))
                .collect();
            resolved.push(ResolvedGrant {
                row,
                amount: *amount,
                targeted_combo: combo.clone(),
                shared,
            });
        }
    }
    Ok((config, resolved))
}

// --- The planned injection ---------------------------------------------------

/// A planned arts-AP-grant injection: all the same-size writes + the resolution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtsApGrantInjection {
    pub edits: Vec<Edit>,
    pub resolved: Vec<ResolvedGrant>,
    /// Chosen SCUS arena VAs (for the oracle to pin the exact landing).
    pub guard_va: u32,
    pub debit_va: u32,
    pub refund_va: u32,
    pub table_va: u32,
}

fn words_to_bytes(w: &[u32]) -> Vec<u8> {
    w.iter().flat_map(|x| x.to_le_bytes()).collect()
}

/// Read a little-endian `u32` from an overlay at `va - OVERLAY_BASE_VA`.
fn ov_hook(overlay: &[u8], va: u32, expect_w0: u32) -> Result<(usize, [u32; 2])> {
    let off = (va - OVERLAY_BASE_VA) as usize;
    let w0 = read_word(overlay, off)?;
    let w1 = read_word(overlay, off + 4)?;
    if w0 != expect_w0 {
        bail!("0898 hook {va:#x} = {w0:#010x}, expected {expect_w0:#010x} (unrecognized build)");
    }
    Ok((off, [w0, w1]))
}

/// Refuse if `[va, va+len)` overlaps a known live data table (zero bytes there
/// are indexed at runtime).
fn assert_not_in_tables(va: u32, len: u32, ranges: &[(u32, u32)], what: &str) -> Result<()> {
    let end = va.saturating_add(len);
    for &(a, b) in ranges {
        if va < b && a < end {
            bail!(
                "arts-ap-grant {what} region {va:#x}..+{len} overlaps live table {a:#x}..{b:#x} - refusing"
            );
        }
    }
    Ok(())
}

/// Confirm `[off, off+len)` in `scus` is all-zero dead space.
fn assert_zero(scus: &[u8], off: usize, len: usize, va: u32) -> Result<()> {
    let region = scus
        .get(off..off + len)
        .ok_or_else(|| anyhow::anyhow!("arena {va:#x}..+{len} past end of SCUS"))?;
    if region.iter().any(|&b| b != 0) {
        bail!("arena {va:#x}..+{len} is not all-zero dead space (build / collision) - refusing");
    }
    Ok(())
}

impl ArtsApGrantInjection {
    /// Plan all edits for the resolved `config` (26 `i8`s). Needs the
    /// `SCUS_942.54` image (arena host + zero/table guards) and the raw 0898
    /// overlay entry (detour fingerprints + replay words). Refuses - without
    /// touching anything - if the build isn't the recognized US layout, the
    /// arena isn't dead, or the routines overrun/overlap a live table.
    pub fn plan(
        scus: &[u8],
        ov0898: &[u8],
        config: [i8; NUM_ROWS],
        resolved: Vec<ResolvedGrant>,
    ) -> Result<Self> {
        // Fingerprint + capture the replay words at each detour site. Site B is
        // read (not detoured) to confirm the `s3 - 0x0B` index formula holds.
        let a = ov_hook(ov0898, HOOK_A_VA, HOOK_A_W0)?;
        ov_hook(ov0898, HOOK_B_VA, HOOK_B_W0)?;
        let c = ov_hook(ov0898, HOOK_C_VA, HOOK_C_W0)?;
        let d = ov_hook(ov0898, HOOK_D_VA, HOOK_D_W0)?;
        // The displaced second words are structural: A's is `mflo t7`, C/D's are
        // `nop`. A wrong build (or a shifted overlay) is refused, not corrupted.
        if a.1[1] != mflo(T7) {
            bail!(
                "0898 site A +4 = {:#010x}, expected mflo t7 (unrecognized build)",
                a.1[1]
            );
        }
        if c.1[1] != nop() || d.1[1] != nop() {
            bail!("0898 site C/D +4 is not the expected nop (unrecognized build)");
        }
        for (va, name) in [
            (HOOK_A_VA, "site-A"),
            (HOOK_C_VA, "site-C"),
            (HOOK_D_VA, "site-D"),
        ] {
            assert_not_in_tables(va, 8, OVERLAY_TABLE_RANGES, name)?;
        }

        // Fixed-length routines: assemble once to size the table, then place.
        let table_va = ARENA1_VA
            + ((assemble_guard(0, a.1, RET_A_VA).len()
                + assemble_debit(0, c.1, C_GRANT_RET_VA, C_NATIVE_RET_VA).len()
                + assemble_refund(d.1, RET_D_VA).len())
                * 4) as u32;
        let guard = assemble_guard(table_va, a.1, RET_A_VA);
        let debit = assemble_debit(table_va, c.1, C_GRANT_RET_VA, C_NATIVE_RET_VA);
        let refund = assemble_refund(d.1, RET_D_VA);

        let guard_va = ARENA1_VA;
        let debit_va = guard_va + (guard.len() * 4) as u32;
        let refund_va = debit_va + (debit.len() * 4) as u32;
        let computed_table_va = refund_va + (refund.len() * 4) as u32;
        debug_assert_eq!(computed_table_va, table_va, "table VA follows the routines");

        // Every routine VA is a `j` target - must be 4-byte aligned.
        for (va, what) in [
            (guard_va, "guard"),
            (debit_va, "debit"),
            (refund_va, "refund"),
        ] {
            if va & 3 != 0 {
                bail!("arts-ap-grant {what} routine VA {va:#x} is not 4-byte aligned");
            }
        }
        let used_end = table_va + NUM_ROWS as u32;
        if used_end > ARENA1_END_VA {
            bail!(
                "arts-ap-grant routines + table ({} B) overrun the arena {ARENA1_VA:#x}..{ARENA1_END_VA:#x}",
                used_end - ARENA1_VA
            );
        }
        assert_not_in_tables(ARENA1_VA, used_end - ARENA1_VA, SCUS_TABLE_RANGES, "arena")?;

        // Resolve arena VAs to SCUS file offsets + confirm the whole span is
        // all-zero dead space (necessary; the arena is also read-watch-verified
        // unreferenced on a live battle - the part a static check can't prove).
        let scus_off = |va: u32| -> Result<usize> {
            legaia_asset::item_names::file_offset_for_va(scus, va)
                .ok_or_else(|| anyhow::anyhow!("can't resolve SCUS VA {va:#x}"))
        };
        let arena_off = scus_off(ARENA1_VA)?;
        assert_zero(scus, arena_off, (used_end - ARENA1_VA) as usize, ARENA1_VA)?;

        let config_bytes: Vec<u8> = config.iter().map(|&v| v as u8).collect();
        let detour = |target_va: u32| -> Vec<u8> { words_to_bytes(&[j(target_va), nop()]) };

        let edits = vec![
            // Detours into the 0898 overlay ([j routine, nop] over the two words).
            Edit {
                prot_index: Some(OVERLAY_PROT_INDEX),
                file_off: a.0,
                bytes: detour(guard_va),
            },
            Edit {
                prot_index: Some(OVERLAY_PROT_INDEX),
                file_off: c.0,
                bytes: detour(debit_va),
            },
            Edit {
                prot_index: Some(OVERLAY_PROT_INDEX),
                file_off: d.0,
                bytes: detour(refund_va),
            },
            // Routines + config table into the SCUS arena.
            Edit {
                prot_index: None,
                file_off: scus_off(guard_va)?,
                bytes: words_to_bytes(&guard),
            },
            Edit {
                prot_index: None,
                file_off: scus_off(debit_va)?,
                bytes: words_to_bytes(&debit),
            },
            Edit {
                prot_index: None,
                file_off: scus_off(refund_va)?,
                bytes: words_to_bytes(&refund),
            },
            Edit {
                prot_index: None,
                file_off: scus_off(table_va)?,
                bytes: config_bytes,
            },
        ];

        Ok(Self {
            edits,
            resolved,
            guard_va,
            debit_va,
            refund_va,
            table_va,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn op(w: u32) -> u32 {
        w >> 26
    }

    #[test]
    fn hook_words_match_documented_disassembly() {
        assert_eq!(HOOK_A_W0, lhu(V0, A1, 0x170));
        assert_eq!(HOOK_C_W0, lhu(V0, V1, 0x170));
        assert_eq!(HOOK_D_W0, lhu(V0, V1, 0x170));
        // Site B is the index proof, not a detour: addiu a1,s3,-0xb = 0x2665FFF5.
        assert_eq!(addiu(A1, S3, 0xFFF5), 0x2665_FFF5);
    }

    #[test]
    fn guard_routine_shape() {
        let disp = [HOOK_A_W0, mflo(T7)];
        let r = assemble_guard(0x8007_AEBC, disp, RET_A_VA);
        assert_eq!(r.len(), 15);
        assert_eq!(r[0], andi(T0, S3, 0xff));
        assert_eq!(r[1], addiu(T0, T0, 0xFFF5), "row = s3 - 0xb");
        assert_eq!(
            r[4], HOOK_A_W0,
            "replays the Spirit load in the beq delay slot"
        );
        assert_eq!(r[7], lb(T2, T2, lo(0x8007_AEBC)), "signed config load");
        assert_eq!(r[11], ori(V0, ZERO, 0x7FFF), "force affordable for a grant");
        assert_eq!(r[12], mflo(T7), "replays mflo t7 (LO preserved)");
        // beq idx3 and blez idx9 both skip to NATIVE (idx12).
        assert_eq!(3 + 1 + ((r[3] & 0xffff) as i16 as i32), 12);
        assert_eq!(9 + 1 + ((r[9] & 0xffff) as i16 as i32), 12);
        assert_eq!(op(r[13]), 0x02, "closes with j");
        assert_eq!((r[13] & 0x03ff_ffff) << 2, RET_A_VA & 0x0fff_ffff);
        // The guard issues no mult/div before replaying mflo t7 (LO hazard).
        assert!(
            !r[..12]
                .iter()
                .any(|&w| w == multu(T9, T6) || (w & 0x3f) == 0x1b)
        );
    }

    #[test]
    fn debit_routine_grants_and_skips_accrual() {
        let disp = [HOOK_C_W0, nop()];
        let r = assemble_debit(0x8007_AEBC, disp, C_GRANT_RET_VA, C_NATIVE_RET_VA);
        assert_eq!(r.len(), 22);
        assert_eq!(r[4], HOOK_C_W0, "replays Spirit load");
        assert_eq!(r[11], addu(V0, V0, T2), "Spirit += g");
        assert_eq!(r[12], sltiu(T1, V0, 101), "clamp test");
        assert_eq!(r[15], ori(V0, ZERO, 100), "clamp value");
        assert_eq!(r[16], sh(V0, V1, 0x170), "store granted Spirit");
        // Grant path jumps PAST the +0x224 accrual.
        assert_eq!(op(r[17]), 0x02);
        assert_eq!((r[17] & 0x03ff_ffff) << 2, C_GRANT_RET_VA & 0x0fff_ffff);
        // Native path replays the nop and jumps to the stock subu.
        assert_eq!(r[19], nop());
        assert_eq!((r[20] & 0x03ff_ffff) << 2, C_NATIVE_RET_VA & 0x0fff_ffff);
        // beq idx3 / blez idx9 -> NATIVE (idx19); bne idx13 -> STORE (idx16).
        assert_eq!(3 + 1 + ((r[3] & 0xffff) as i16 as i32), 19);
        assert_eq!(9 + 1 + ((r[9] & 0xffff) as i16 as i32), 19);
        assert_eq!(13 + 1 + ((r[13] & 0xffff) as i16 as i32), 16);
    }

    #[test]
    fn refund_routine_clamps() {
        let disp = [HOOK_D_W0, nop()];
        let r = assemble_refund(disp, RET_D_VA);
        assert_eq!(r.len(), 10);
        assert_eq!(r[0], HOOK_D_W0);
        assert_eq!(r[2], addu(V0, V0, A0), "+= accumulated");
        assert_eq!(r[6], ori(V0, ZERO, 100), "clamp value");
        assert_eq!(r[7], sh(V0, V1, 0x170));
        assert_eq!((r[8] & 0x03ff_ffff) << 2, RET_D_VA & 0x0fff_ffff);
        assert_eq!(4 + 1 + ((r[4] & 0xffff) as i16 as i32), 7, "bne -> ST");
    }

    #[test]
    fn arena_and_sites_are_outside_live_tables() {
        let used = ((assemble_guard(0, [0, 0], 0).len()
            + assemble_debit(0, [0, 0], 0, 0).len()
            + assemble_refund([0, 0], 0).len())
            * 4
            + NUM_ROWS) as u32;
        assert!(assert_not_in_tables(ARENA1_VA, used, SCUS_TABLE_RANGES, "arena").is_ok());
        for va in [HOOK_A_VA, HOOK_B_VA, HOOK_C_VA, HOOK_D_VA] {
            assert!(assert_not_in_tables(va, 8, OVERLAY_TABLE_RANGES, "site").is_ok());
        }
        // The guard refuses a region overlapping a live table (move-power window
        // in 0898; the font/name tables in SCUS).
        assert!(assert_not_in_tables(0x801F_5000, 8, OVERLAY_TABLE_RANGES, "x").is_err());
        assert!(assert_not_in_tables(0x8007_4400, 8, SCUS_TABLE_RANGES, "x").is_err());
    }

    #[test]
    fn routines_plus_table_fit_arena1() {
        let guard = assemble_guard(0, [HOOK_A_W0, mflo(T7)], RET_A_VA);
        let debit = assemble_debit(0, [HOOK_C_W0, nop()], C_GRANT_RET_VA, C_NATIVE_RET_VA);
        let refund = assemble_refund([HOOK_D_W0, nop()], RET_D_VA);
        let bytes = (guard.len() + debit.len() + refund.len()) * 4 + NUM_ROWS;
        assert!(
            ARENA1_VA + bytes as u32 <= ARENA1_END_VA,
            "routines + table ({bytes} B) fit the 256B arena"
        );
        // All routine VAs 4-byte aligned.
        let debit_va = ARENA1_VA + (guard.len() * 4) as u32;
        let refund_va = debit_va + (debit.len() * 4) as u32;
        for va in [ARENA1_VA, debit_va, refund_va] {
            assert_eq!(va & 3, 0);
        }
    }
}
