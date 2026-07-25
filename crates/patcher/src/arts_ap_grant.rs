//! **Arts AP override**: set what a Tactical Art does to the caster's AP
//! (Spirit) - *grant* AP instead of costing it, or charge a chosen flat cost
//! instead of retail's computed one.
//!
//! A community modding knob. Retail deducts an art's AP cost from the caster's
//! Spirit gauge (`actor[+0x170]`) inside the party arts queue-builder
//! `FUN_801EED1C` (PROT 0898, base `0x801CE818`), refusing the art when Spirit
//! is short. This feature detours three sites of that builder so a configured
//! art is either admitted at any Spirit level and *adds* `amount` AP (clamped
//! at the native 100 cap), or gated and charged at a flat `amount` of the
//! modder's choosing.
//!
//! ## Retail has no per-art AP cost byte
//!
//! Read off the disassembly, not the C: the cost the builder gates and charges
//! is **computed**, never loaded from an art record. `FUN_801EED1C` picks a
//! multiplier `t4` from three code immediates by how many art rows it has
//! already visited for this character (`[sp+0x40]`: `0` -> `0xB`, `1..3` ->
//! `0xA`, `>= 4` -> `6`; halved when the actor's `0x800` flag is set), then
//!
//! ```text
//! 801ef40c  _mult t4,s1        ; s1 = the art's command count
//! 801ef414  mflo  t7           ; t7 = required AP  -> the affordability gate
//! ...
//! 801ef474  mult  t4,v0        ; v0 = the same command count
//! 801ef478  mflo  a2           ; a2 = the charged AP
//! ```
//!
//! so `cost = multiplier x command_count`. Nothing on the disc carries a
//! per-art cost the runtime reads, which is why this feature is a code hook
//! rather than a table edit - and why a per-art cost only exists once the hook
//! introduces one.
//!
//! (The AP the builder debits at site C is added back at site D and re-charged
//! for real out of the spent accumulator `actor[+0x224]` by the battle-action
//! cleanup arm, `subu v0,v0,a0` at `0x801E5D74`. The in-builder debit exists so
//! a *chained* art is gated against what the earlier arts in the same run
//! already committed; the accumulator is the real spend. A cost override
//! therefore has to rewrite both, which the site-C routine does.)
//!
//! ## The four pinned sites (byte-verified against the extracted 0898 image)
//!
//! `FUN_801EED1C` is the **party** arts queue-builder (slot < 3; monster AI uses
//! `FUN_801E7320`), so the hook affects **player arts only** - enemies are
//! untouched.
//!
//! | Site | VA | Stock word | Role |
//! |---|---|---|---|
//! | A affordability guard | `0x801EF410` | `0x94A20170` (`lhu v0,0x170(a1)`) | admit at 0 AP (grant) or gate on the override cost |
//! | B per-art index | `0x801EF438` | `0x2665FFF5` (`addiu a1,s3,-0xb`) | pins the row index = `s3 - 0x0B` (not detoured, just the index proof) |
//! | C AP debit + accrual | `0x801EF490` | `0x94620170` (`lhu v0,0x170(v1)`) | grant art *adds* AP + skips the `+0x224` accrual; cost art debits + accrues the override |
//! | D end-of-turn refund | `0x801EF988` | `0x94620170` (`lhu v0,0x170(v1)`) | clamps the `Spirit += +0x224` refund at 100 |
//!
//! `actor[+0x170]` = Spirit/AP; `actor[+0x224]` = spent-AP accumulator.
//! See [`docs/subsystems/arts-command-gauge.md`].
//!
//! ## Config table: keyed by (character, row), not by row alone
//!
//! The art identity is register `s3` (the art-table row cursor, `li s3,0xb` at
//! `0x801ef2ec`); the 0-based row is `s3 - 0x0B` (site B, `addiu a1,s3,-0xb`),
//! which is the art's arts-table **display index** (`0` = Miracle Art). That row
//! alone is *shared* across the three characters, so the character has to come
//! from somewhere else: the builder already holds `t6 = &DAT_8007BD10[slot]`
//! (`addu t6,t9,t7` at `0x801ef30c`, read as `lbu v0,0x0(t6)` at three retail
//! sites), and `DAT_8007BD10[slot]` is the **1-based party-record id**
//! (`1` Vahn / `2` Noa / `3` Gala / `4` Terra). The routines replay that same
//! load, so the config index is
//!
//! ```text
//! index = (DAT_8007BD10[slot] - 1) * ROW_STRIDE + (s3 - 0x0B)
//! ```
//!
//! - a `[i8; NUM_CHARS * ROW_STRIDE]` table: `0` = unmodified retail,
//!   `> 0` = grant that many AP (admit + no cost), `< 0` = cost `-value` AP.
//! - `ROW_STRIDE` is 32 (a power of two, so the index is one `sll`; rows
//!   `NUM_ROWS..ROW_STRIDE` are unreachable padding that stays zero).
//! - **`0` cannot mean "free"** - it is the retail sentinel. The smallest
//!   configurable cost is `1`.
//!
//! A configured cost is **flat**: it replaces the product outright, so it does
//! not follow retail's `srl t4,t4,0x1` halving under the actor's `0x800` flag
//! (the menu renderer still halves what it *draws* in that state, since that
//! `sra` lives on the display side). That is the intended reading of "this art
//! costs N" - an override the modder typed should not silently halve.
//!
//! ## Placement
//!
//! The battle overlay is packed (no dead space - the move-power window
//! `0x801F4E63..0x801F69D8` is the only large zero run and is runtime-indexed),
//! so the routines + table are injected into **verified-dead SCUS arenas**: the
//! guard + debit routines in [`ARENA1_VA`], the refund routine in [`ARENA2_VA`],
//! and the config table in the rodata gap [`SCUS_GAP_VA`]. All three are the
//! regions the shiny-Seru feature reuses, so **arts AP override is mutually
//! exclusive with `--shiny-seru`** - enforced in the CLI and the web patcher.
//!
//! ## The menu's AP number is a second, independent source
//!
//! The number the pause menu's arts list shows is **not** the value the battle
//! path uses. It is the `+2` byte of the static SCUS arts-name table record
//! (`DAT_80075EC4 + n*0x14`, [`legaia_art::arts_table`]), read by exactly one
//! site in the whole image - `lbu a0,0x2(s2)` at `0x801D4524` in the menu
//! overlay's status-panel renderer `FUN_801D33D8` (PROT 0899), handed to the
//! 3-cell decimal drawer `FUN_80034b78`. Retail keeps it consistent by hand: for
//! all 45 arts the byte equals the builder's `multiplier x command_count`
//! exactly. Patching only the hook therefore leaves the menu showing the retail
//! number, so [`resolve`] also plans a same-size edit of that byte:
//!
//! - a **cost** override writes the configured cost, so the list reads true;
//! - a **grant** override writes `0`, which no retail art carries (the retail
//!   minimum is 18) and which the smallest configurable cost (`1`) cannot
//!   collide with - so `0` is the in-game marker for "this art gives AP back".
//!
//! `FUN_80034b78` renders digit sprites only (`u = digit*8`, `v = 0xD0`) and has
//! no sign path, so a literal `+`/`-` in that field would need an extra sprite
//! draw injected into 0899; that is deliberately not attempted here.
//!
//! No Sony bytes are embedded; the routines are the patcher's own code and the
//! display edit is a single computed byte.

use anyhow::{Result, bail};

use legaia_art::arts_table::{self, RawArtRecord};
use legaia_art::queue::{Character, Command};

use crate::mips::*;
use crate::shiny_seru::{
    ARENA1_END_VA, ARENA1_VA, ARENA2_END_VA, ARENA2_VA, Edit, OVERLAY_TABLE_RANGES,
    SCUS_GAP_END_VA, SCUS_GAP_VA, SCUS_TABLE_RANGES,
};

/// Number of reachable config rows per character (`s3 - 0x0B` over art rows
/// `0x0B..=0x24`).
pub const NUM_ROWS: usize = 26;
/// Per-character block stride in the config table. A power of two so the
/// runtime index is a single `sll`; rows `NUM_ROWS..ROW_STRIDE` are padding.
pub const ROW_STRIDE: usize = 32;
/// `log2(ROW_STRIDE)` - the shift the injected routines use.
const ROW_SHIFT: u32 = 5;
/// Character blocks in the config table, one per `DAT_8007BD10` party-record id
/// (`1` Vahn / `2` Noa / `3` Gala / `4` Terra).
pub const NUM_CHARS: usize = 4;
/// Total config-table length in bytes.
pub const TABLE_LEN: usize = NUM_CHARS * ROW_STRIDE;
/// Native Spirit/AP cap: the granted total is clamped here, and it is also the
/// largest configurable cost.
pub const AP_CAP: u16 = 100;

/// The per-character, per-row config table the injected routines index.
pub type ApConfig = [i8; TABLE_LEN];

/// PROT entry index of the battle-action overlay (0898) hosting the detour sites.
pub const OVERLAY_PROT_INDEX: usize = legaia_asset::move_power::BATTLE_ACTION_OVERLAY_PROT_INDEX;
/// Load base VA of the slot-A overlays; overlay file offset = `va - BASE`.
pub const OVERLAY_BASE_VA: u32 = legaia_asset::move_power::BATTLE_OVERLAY_BASE;

// --- Pinned hook sites (VA, expected first word, return VA) ------------------

/// B: per-art index proof. `addiu a1,s3,-0xb` - confirms the config row is
/// `s3 - 0x0B`. Not detoured (read-only build fingerprint).
pub const HOOK_B_VA: u32 = 0x801E_F438;
pub(crate) const HOOK_B_W0: u32 = 0x2665_FFF5; // addiu a1,s3,-0xb

/// A: affordability guard. Detour replaces `lhu v0,0x170(a1)` + the following
/// `mflo t7`; returns to `0x801EF418` (`slt v0,v0,t7`).
pub const HOOK_A_VA: u32 = 0x801E_F410;
pub(crate) const HOOK_A_W0: u32 = 0x94A2_0170; // lhu v0,0x170(a1)
const RET_A_VA: u32 = 0x801E_F418;

/// C: AP debit + accrual. Detour replaces `lhu v0,0x170(v1)` + the following
/// `nop`. A grant or cost art returns to `0x801EF4B8` (past the stock debit AND
/// the `+0x224` accrual `0x801EF4A0..0x801EF4B4`, both of which the routine
/// performs itself for a cost); a native art returns to `0x801EF498`
/// (`subu v0,v0,a2`, the stock debit).
pub const HOOK_C_VA: u32 = 0x801E_F490;
pub(crate) const HOOK_C_W0: u32 = 0x9462_0170; // lhu v0,0x170(v1)
const C_OVERRIDE_RET_VA: u32 = 0x801E_F4B8;
const C_NATIVE_RET_VA: u32 = 0x801E_F498;

/// D: end-of-turn refund. Detour replaces `lhu v0,0x170(v1)` + the following
/// `nop`; the routine does the `Spirit += +0x224` add itself, clamps at 100,
/// stores, and returns to `0x801EF998` (past the stock `addu`/`sh`).
pub const HOOK_D_VA: u32 = 0x801E_F988;
pub(crate) const HOOK_D_W0: u32 = 0x9462_0170; // lhu v0,0x170(v1)
const RET_D_VA: u32 = 0x801E_F998;

/// The retail read of the acting slot's 1-based party-record id, replayed at
/// the head of both routines: `lbu t1,0x0(t6)` with `t6 = &DAT_8007BD10[slot]`
/// (built by `addu t6,t9,t7` at `0x801ef30c`, live across both sites).
fn load_char_id(dst: u32) -> u32 {
    lbu(dst, T6, 0)
}

// --- Routine assemblers ------------------------------------------------------

/// (A) Affordability guard. Replays the displaced `lhu v0,0x170(a1)` (Spirit)
/// and `mflo t7` (retail's computed cost; `LO` is preserved - the routine issues
/// no `mult`/`div`). A **grant** art forces `v0 = 0x7FFF` so the stock
/// `slt v0,v0,t7` at the return site reads "affordable"; a **cost** art replaces
/// `t7` with the configured cost and keeps the real Spirit, so the stock compare
/// gates on the override. Native arts are untouched.
/// `disp = [lhu, mflo t7]`; `ret = 0x801EF418`.
pub(crate) fn assemble_guard(table_va: u32, disp: [u32; 2], ret: u32) -> Vec<u32> {
    const GRANT: i32 = 21;
    const DONE: i32 = 22;
    vec![
        load_char_id(T1),                  // 0  t1 = DAT_8007BD10[slot] (1-based)
        andi(T0, S3, 0xff),                // 1  load delay: t0 = row cursor
        addiu(T0, T0, 0xFFF5),             // 2  t0 = row = s3 - 0x0b
        sltiu(T2, T0, NUM_ROWS as u16),    // 3  row < NUM_ROWS?
        beq(T2, ZERO, (DONE - 5) as i16),  // 4  out of range -> native
        disp[0],                           // 5  delay: v0 = Spirit (always)
        addiu(T1, T1, 0xFFFF),             // 6  t1 = character index (0-based)
        sltiu(T2, T1, NUM_CHARS as u16),   // 7  char < NUM_CHARS?
        beq(T2, ZERO, (DONE - 9) as i16),  // 8  out of range -> native
        sll(T1, T1, ROW_SHIFT),            // 9  delay: char * ROW_STRIDE
        addu(T0, T0, T1),                  // 10 config index
        lui(T2, hi(table_va)),             // 11
        addu(T2, T2, T0),                  // 12 &AP_CONFIG[index]
        lb(T2, T2, lo(table_va)),          // 13 g = (i8)AP_CONFIG[index]
        nop(),                             // 14 load delay
        beq(T2, ZERO, (DONE - 16) as i16), // 15 g == 0 -> native
        nop(),                             // 16
        bgez(T2, (GRANT - 18) as i16),     // 17 g > 0 -> grant
        nop(),                             // 18
        j(ret),                            // 19 cost: keep the real Spirit ...
        subu(T7, ZERO, T2),                // 20 delay: t7 = -g = the cost
        ori(V0, ZERO, 0x7FFF),             // 21 GRANT: force affordable
        j(ret),                            // 22 DONE
        disp[1],                           // 23 delay: mflo t7 (replay; LO intact)
    ]
}

/// (C) Grant / flat-cost instead of retail's computed debit. Replays
/// `lhu v0,0x170(v1)` (Spirit); a **grant** art adds `g` (clamped at [`AP_CAP`]),
/// a **cost** art subtracts `-g` and accrues that same `-g` into the spent
/// accumulator `+0x224` (the value the battle-action cleanup arm actually
/// charges). Both return PAST the stock debit and the stock accrual so neither
/// is double-counted. Native arts fall back to the stock `subu v0,v0,a2` at
/// `0x801EF498`. `disp = [lhu, nop]`.
pub(crate) fn assemble_debit(
    table_va: u32,
    disp: [u32; 2],
    override_ret: u32,
    native_ret: u32,
) -> Vec<u32> {
    const GRANT: i32 = 26;
    const STORE: i32 = 30;
    const NATIVE: i32 = 33;
    vec![
        load_char_id(T1),                    // 0  t1 = DAT_8007BD10[slot]
        andi(T0, S3, 0xff),                  // 1  load delay
        addiu(T0, T0, 0xFFF5),               // 2  row
        sltiu(T2, T0, NUM_ROWS as u16),      // 3
        beq(T2, ZERO, (NATIVE - 5) as i16),  // 4  row out of range -> native
        disp[0],                             // 5  delay: v0 = Spirit (always)
        addiu(T1, T1, 0xFFFF),               // 6  char index
        sltiu(T2, T1, NUM_CHARS as u16),     // 7
        beq(T2, ZERO, (NATIVE - 9) as i16),  // 8  char out of range -> native
        sll(T1, T1, ROW_SHIFT),              // 9  delay
        addu(T0, T0, T1),                    // 10 config index
        lui(T2, hi(table_va)),               // 11
        addu(T2, T2, T0),                    // 12
        lb(T2, T2, lo(table_va)),            // 13 g
        nop(),                               // 14 load delay
        beq(T2, ZERO, (NATIVE - 16) as i16), // 15 g == 0 -> native
        addu(V0, V0, T2),                    // 16 delay: Spirit += g (0 when native)
        bgez(T2, (GRANT - 18) as i16),       // 17 g > 0 -> grant
        nop(),                               // 18
        sh(V0, V1, 0x170),                   // 19 COST: store the debited gauge
        lbu(V0, V1, 0x224),                  // 20 spent accumulator
        subu(T1, ZERO, T2),                  // 21 load delay: t1 = the cost
        addu(V0, V0, T1),                    // 22
        sb(V0, V1, 0x224),                   // 23 accrue the real spend
        j(override_ret),                     // 24 -> 0x801EF4B8
        nop(),                               // 25
        sltiu(T1, V0, AP_CAP + 1),           // 26 GRANT: v0 <= 100?
        bne(T1, ZERO, (STORE - 28) as i16),  // 27 in range -> store
        nop(),                               // 28
        ori(V0, ZERO, AP_CAP),               // 29 clamp to 100
        sh(V0, V1, 0x170),                   // 30 STORE
        j(override_ret),                     // 31 -> 0x801EF4B8 (skip debit + accrual)
        nop(),                               // 32
        j(native_ret),                       // 33 NATIVE -> 0x801EF498 (stock subu)
        disp[1],                             // 34 delay: nop (replay)
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

// --- Spec -> config resolution ----------------------------------------------

/// What a targeted art does to the caster's AP after the patch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ApMode {
    /// Admit the art at any AP level and *add* this much (clamped at 100).
    Grant(u8),
    /// Gate on and charge exactly this much, replacing retail's
    /// `multiplier x command_count`.
    Cost(u8),
}

impl ApMode {
    /// The signed config byte this mode stores.
    fn config_byte(self) -> i8 {
        match self {
            ApMode::Grant(n) => n as i8,
            ApMode::Cost(n) => -(n as i16) as i8,
        }
    }
    /// The value the menu's arts list should show. A cost reads as itself; a
    /// grant reads as `0`, which no retail art and no configurable cost holds.
    fn display_ap(self) -> u8 {
        match self {
            ApMode::Grant(_) => 0,
            ApMode::Cost(n) => n,
        }
    }
    /// The configured magnitude.
    pub fn amount(self) -> u8 {
        match self {
            ApMode::Grant(n) | ApMode::Cost(n) => n,
        }
    }
    /// `true` for [`ApMode::Grant`].
    pub fn is_grant(self) -> bool {
        matches!(self, ApMode::Grant(_))
    }
}

/// One requested per-art AP override.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArtApSpec {
    /// `None` targets every character whose art list holds this combo (each
    /// gets its own config cell - nothing is shared).
    pub character: Option<Character>,
    /// The art's input combo (`L/R/D/U`), the matcher key.
    pub combo: Vec<Command>,
    pub mode: ApMode,
}

/// One resolved override: the exact art it lands on, plus the display-byte edit
/// that keeps the pause menu's arts list honest.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedArtAp {
    pub character: Character,
    /// Config row = the art's arts-table display index.
    pub row: u8,
    pub name: String,
    pub combo: Vec<Command>,
    pub mode: ApMode,
    /// File offset of the record's `+2` display-AP byte inside `SCUS_942.54`.
    pub display_ap_off: usize,
    /// The display AP the record held before the patch.
    pub previous_display_ap: u8,
    /// The display AP written (`0` marks a grant).
    pub display_ap: u8,
}

/// Render a combo as `L/R/D/U` glyphs.
pub fn combo_str(combo: &[Command]) -> String {
    combo.iter().map(crate::arts_power::command_glyph).collect()
}

/// Config-table index for a `(character, row)` pair.
fn config_index(character: Character, row: u8) -> usize {
    (character as usize) * ROW_STRIDE + usize::from(row)
}

/// Resolve `specs` into the config table + the per-art resolution. Errors on an
/// unknown combo, an out-of-range amount or row, or one art configured twice
/// with conflicting values.
pub fn resolve(scus: &[u8], specs: &[ArtApSpec]) -> Result<(ApConfig, Vec<ResolvedArtAp>)> {
    let records: Vec<RawArtRecord> = arts_table::raw_records_from_scus(scus)
        .ok_or_else(|| anyhow::anyhow!("parse arts-name table"))?;
    let names: Vec<(Character, u8, String)> = arts_table::parse_from_scus(scus)
        .ok_or_else(|| anyhow::anyhow!("parse arts-name table"))?
        .into_iter()
        .map(|e| (e.character, e.index, e.name))
        .collect();
    let mut config: ApConfig = [0i8; TABLE_LEN];
    let mut resolved: Vec<ResolvedArtAp> = Vec::new();
    for spec in specs {
        let amount = spec.mode.amount();
        let what = if spec.mode.is_grant() {
            "AP-grant"
        } else {
            "AP-cost"
        };
        if amount == 0 {
            bail!(
                "{what} amount for {} must be >= 1 (0 is the table's \"leave at retail\" value)",
                combo_str(&spec.combo)
            );
        }
        if u16::from(amount) > AP_CAP {
            bail!(
                "{what} amount {amount} for {} exceeds the {AP_CAP} AP cap",
                combo_str(&spec.combo)
            );
        }
        let matches: Vec<&RawArtRecord> = records
            .iter()
            .filter(|r| r.commands == spec.combo)
            .filter(|r| spec.character.is_none_or(|c| c == r.character))
            .collect();
        if matches.is_empty() {
            match spec.character {
                Some(c) => bail!(
                    "{c:?} has no Tactical Art with combo {} (nothing to override)",
                    combo_str(&spec.combo)
                ),
                None => bail!(
                    "no Tactical Art has combo {} (nothing to override)",
                    combo_str(&spec.combo)
                ),
            }
        }
        for rec in matches {
            if usize::from(rec.index) >= NUM_ROWS {
                bail!(
                    "art index {} for combo {} is outside the {NUM_ROWS}-row config space",
                    rec.index,
                    combo_str(&spec.combo)
                );
            }
            let idx = config_index(rec.character, rec.index);
            let byte = spec.mode.config_byte();
            let prev = config[idx];
            if prev != 0 && prev != byte {
                bail!(
                    "{:?}'s art at row {} is configured twice with conflicting AP values",
                    rec.character,
                    rec.index
                );
            }
            config[idx] = byte;
            resolved.push(ResolvedArtAp {
                character: rec.character,
                row: rec.index,
                name: names
                    .iter()
                    .find(|(c, i, _)| *c == rec.character && *i == rec.index)
                    .map(|(_, _, n)| n.clone())
                    .unwrap_or_default(),
                combo: rec.commands.clone(),
                mode: spec.mode,
                display_ap_off: rec.record_file_offset + 2,
                previous_display_ap: rec.ap,
                display_ap: spec.mode.display_ap(),
            });
        }
    }
    Ok((config, resolved))
}

// --- The planned injection ---------------------------------------------------

/// A planned arts-AP-override injection: all the same-size writes + the
/// resolution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtsApGrantInjection {
    pub edits: Vec<Edit>,
    pub resolved: Vec<ResolvedArtAp>,
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
                "arts-ap-override {what} region {va:#x}..+{len} overlaps live table {a:#x}..{b:#x} - refusing"
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
    /// Plan all edits for the resolved `config`. Needs the `SCUS_942.54` image
    /// (arena host + zero/table guards + the arts-name display bytes) and the
    /// raw 0898 overlay entry (detour fingerprints + replay words). Refuses -
    /// without touching anything - if the build isn't the recognized US layout,
    /// a region isn't dead, or a routine overruns / overlaps a live table.
    pub fn plan(
        scus: &[u8],
        ov0898: &[u8],
        config: ApConfig,
        resolved: Vec<ResolvedArtAp>,
    ) -> Result<Self> {
        // Fingerprint + capture the replay words at each detour site. Site B is
        // read (not detoured) to confirm the `s3 - 0x0B` row formula holds.
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
        // The character discriminator both routines replay is retail's own read
        // of `DAT_8007BD10[slot]` through `t6`; refuse a build whose builder
        // does not hold that pointer (`lbu v0,0x0(t6)` at 0x801EF340).
        const CHAR_READ_VA: u32 = 0x801E_F340;
        let char_read = read_word(ov0898, (CHAR_READ_VA - OVERLAY_BASE_VA) as usize)?;
        if char_read != lbu(V0, T6, 0) {
            bail!(
                "0898 {CHAR_READ_VA:#x} = {char_read:#010x}, expected lbu v0,0x0(t6) \
                 (the party-record id read the config index keys on) - unrecognized build"
            );
        }
        for (va, name) in [
            (HOOK_A_VA, "site-A"),
            (HOOK_C_VA, "site-C"),
            (HOOK_D_VA, "site-D"),
        ] {
            assert_not_in_tables(va, 8, OVERLAY_TABLE_RANGES, name)?;
        }

        // Fixed-length routines. Guard + debit share arena 1, the refund lives
        // in arena 2, and the config table sits in the SCUS rodata gap - the
        // per-character table no longer fits alongside the routines.
        let table_va = SCUS_GAP_VA;
        let guard = assemble_guard(table_va, a.1, RET_A_VA);
        let debit = assemble_debit(table_va, c.1, C_OVERRIDE_RET_VA, C_NATIVE_RET_VA);
        let refund = assemble_refund(d.1, RET_D_VA);

        let guard_va = ARENA1_VA;
        let debit_va = guard_va + (guard.len() * 4) as u32;
        let arena1_end = debit_va + (debit.len() * 4) as u32;
        let refund_va = ARENA2_VA;
        let arena2_end = refund_va + (refund.len() * 4) as u32;

        // Every routine VA is a `j` target - must be 4-byte aligned.
        for (va, what) in [
            (guard_va, "guard"),
            (debit_va, "debit"),
            (refund_va, "refund"),
        ] {
            if va & 3 != 0 {
                bail!("arts-ap-override {what} routine VA {va:#x} is not 4-byte aligned");
            }
        }
        if arena1_end > ARENA1_END_VA {
            bail!(
                "arts-ap-override guard + debit ({} B) overrun arena 1 {ARENA1_VA:#x}..{ARENA1_END_VA:#x}",
                arena1_end - ARENA1_VA
            );
        }
        if arena2_end > ARENA2_END_VA {
            bail!(
                "arts-ap-override refund ({} B) overruns arena 2 {ARENA2_VA:#x}..{ARENA2_END_VA:#x}",
                arena2_end - ARENA2_VA
            );
        }
        if table_va + TABLE_LEN as u32 > SCUS_GAP_END_VA {
            bail!(
                "arts-ap-override config table ({TABLE_LEN} B) overruns the SCUS gap \
                 {SCUS_GAP_VA:#x}..{SCUS_GAP_END_VA:#x}"
            );
        }
        for (va, len, what) in [
            (ARENA1_VA, arena1_end - ARENA1_VA, "arena1"),
            (ARENA2_VA, arena2_end - ARENA2_VA, "arena2"),
            (table_va, TABLE_LEN as u32, "config table"),
        ] {
            assert_not_in_tables(va, len, SCUS_TABLE_RANGES, what)?;
        }

        // Resolve VAs to SCUS file offsets + confirm every hosted span is
        // all-zero dead space (necessary; the regions are also read-watch-
        // verified unreferenced on a live battle - the part a static check
        // can't prove).
        let scus_off = |va: u32| -> Result<usize> {
            legaia_asset::item_names::file_offset_for_va(scus, va)
                .ok_or_else(|| anyhow::anyhow!("can't resolve SCUS VA {va:#x}"))
        };
        for (va, len) in [
            (ARENA1_VA, arena1_end - ARENA1_VA),
            (ARENA2_VA, arena2_end - ARENA2_VA),
            (table_va, TABLE_LEN as u32),
        ] {
            assert_zero(scus, scus_off(va)?, len as usize, va)?;
        }

        let config_bytes: Vec<u8> = config.iter().map(|&v| v as u8).collect();
        let detour = |target_va: u32| -> Vec<u8> { words_to_bytes(&[j(target_va), nop()]) };

        let mut edits = vec![
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
            // Routines + config table into the SCUS dead regions.
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
        // The menu arts list reads its own byte (`record+2`); keep it honest.
        for r in &resolved {
            edits.push(Edit {
                prot_index: None,
                file_off: r.display_ap_off,
                bytes: vec![r.display_ap],
            });
        }

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

    /// Follow a branch at word `i` and return the word index it lands on.
    fn br(words: &[u32], i: usize) -> i32 {
        i as i32 + 1 + ((words[i] & 0xffff) as i16 as i32)
    }

    #[test]
    fn hook_words_match_documented_disassembly() {
        assert_eq!(HOOK_A_W0, lhu(V0, A1, 0x170));
        assert_eq!(HOOK_C_W0, lhu(V0, V1, 0x170));
        assert_eq!(HOOK_D_W0, lhu(V0, V1, 0x170));
        // Site B is the index proof, not a detour: addiu a1,s3,-0xb = 0x2665FFF5.
        assert_eq!(addiu(A1, S3, 0xFFF5), 0x2665_FFF5);
        // The char discriminator is retail's own `lbu v0,0x0(t6)`.
        assert_eq!(lbu(V0, T6, 0), 0x91C2_0000);
    }

    #[test]
    fn guard_routine_shape() {
        let disp = [HOOK_A_W0, mflo(T7)];
        let r = assemble_guard(SCUS_GAP_VA, disp, RET_A_VA);
        assert_eq!(r.len(), 24);
        assert_eq!(r[0], lbu(T1, T6, 0), "replays the party-record id read");
        assert_eq!(r[2], addiu(T0, T0, 0xFFF5), "row = s3 - 0xb");
        assert_eq!(r[5], HOOK_A_W0, "replays the Spirit load in the beq delay");
        assert_eq!(r[9], sll(T1, T1, ROW_SHIFT), "char * ROW_STRIDE");
        assert_eq!(r[13], lb(T2, T2, lo(SCUS_GAP_VA)), "signed config load");
        assert_eq!(r[20], subu(T7, ZERO, T2), "cost path: t7 = -g");
        assert_eq!(r[21], ori(V0, ZERO, 0x7FFF), "grant path: force affordable");
        assert_eq!(r[23], mflo(T7), "replays mflo t7 (LO preserved)");
        // Every guard branch lands where the comments claim.
        assert_eq!(br(&r, 4), 22, "row guard -> DONE");
        assert_eq!(br(&r, 8), 22, "char guard -> DONE");
        assert_eq!(br(&r, 15), 22, "g == 0 -> DONE");
        assert_eq!(br(&r, 17), 21, "g > 0 -> GRANT");
        for i in [19, 22] {
            assert_eq!(op(r[i]), 0x02, "word {i} closes with j");
            assert_eq!((r[i] & 0x03ff_ffff) << 2, RET_A_VA & 0x0fff_ffff);
        }
        // The guard issues no mult/div before replaying mflo t7 (LO hazard).
        assert!(
            !r[..23]
                .iter()
                .any(|&w| (w & 0x3f) == 0x18 || (w & 0x3f) == 0x19)
        );
    }

    #[test]
    fn debit_routine_grants_costs_and_falls_back() {
        let disp = [HOOK_C_W0, nop()];
        let r = assemble_debit(SCUS_GAP_VA, disp, C_OVERRIDE_RET_VA, C_NATIVE_RET_VA);
        assert_eq!(r.len(), 35);
        assert_eq!(r[0], lbu(T1, T6, 0));
        assert_eq!(r[5], HOOK_C_W0, "replays Spirit load");
        assert_eq!(r[16], addu(V0, V0, T2), "Spirit += g (grant) / -= cost");
        // Cost arm: store, then accrue the same magnitude into +0x224.
        assert_eq!(r[19], sh(V0, V1, 0x170));
        assert_eq!(r[20], lbu(V0, V1, 0x224));
        assert_eq!(
            r[21],
            subu(T1, ZERO, T2),
            "t1 = the cost, in the load delay"
        );
        assert_eq!(r[23], sb(V0, V1, 0x224));
        // Grant arm: clamp then store.
        assert_eq!(r[26], sltiu(T1, V0, 101));
        assert_eq!(r[29], ori(V0, ZERO, 100));
        assert_eq!(r[30], sh(V0, V1, 0x170));
        assert_eq!(br(&r, 4), 33, "row guard -> NATIVE");
        assert_eq!(br(&r, 8), 33, "char guard -> NATIVE");
        assert_eq!(br(&r, 15), 33, "g == 0 -> NATIVE");
        assert_eq!(br(&r, 17), 26, "g > 0 -> GRANT");
        assert_eq!(br(&r, 27), 30, "in range -> STORE");
        // Both override arms jump PAST the stock debit + accrual.
        for i in [24, 31] {
            assert_eq!((r[i] & 0x03ff_ffff) << 2, C_OVERRIDE_RET_VA & 0x0fff_ffff);
        }
        assert_eq!((r[33] & 0x03ff_ffff) << 2, C_NATIVE_RET_VA & 0x0fff_ffff);
        assert_eq!(r[34], nop(), "native path replays the displaced nop");
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
        assert_eq!(br(&r, 4), 7, "bne -> ST");
    }

    #[test]
    fn config_index_is_per_character() {
        // Vahn / Noa / Gala at the same row land in three distinct cells - the
        // whole point of the (character, row) keying.
        let a = config_index(Character::Vahn, 4);
        let b = config_index(Character::Noa, 4);
        let c = config_index(Character::Gala, 4);
        assert_ne!(a, b);
        assert_ne!(b, c);
        for i in [a, b, c] {
            assert!(i < TABLE_LEN);
        }
        assert_eq!(b - a, ROW_STRIDE);
        // The runtime index formula the routines assemble agrees.
        assert_eq!(config_index(Character::Gala, 25), 2 * ROW_STRIDE + 25);
        const { assert!(NUM_ROWS <= ROW_STRIDE, "rows fit their block") };
        assert_eq!(ROW_STRIDE, 1usize << ROW_SHIFT);
    }

    #[test]
    fn mode_encodes_sign_and_display() {
        assert_eq!(ApMode::Grant(20).config_byte(), 20);
        assert_eq!(ApMode::Cost(20).config_byte(), -20);
        assert_eq!(ApMode::Cost(100).config_byte(), -100);
        // A grant shows 0 in the menu list; a cost shows itself.
        assert_eq!(ApMode::Grant(20).display_ap(), 0);
        assert_eq!(ApMode::Cost(7).display_ap(), 7);
        assert!(ApMode::Grant(1).is_grant());
        assert!(!ApMode::Cost(1).is_grant());
    }

    #[test]
    fn routines_and_table_fit_their_regions() {
        let guard = assemble_guard(0, [HOOK_A_W0, mflo(T7)], RET_A_VA);
        let debit = assemble_debit(0, [HOOK_C_W0, nop()], C_OVERRIDE_RET_VA, C_NATIVE_RET_VA);
        let refund = assemble_refund([HOOK_D_W0, nop()], RET_D_VA);
        let arena1 = ((guard.len() + debit.len()) * 4) as u32;
        assert!(
            ARENA1_VA + arena1 <= ARENA1_END_VA,
            "guard + debit ({arena1} B) fit arena 1"
        );
        assert!(ARENA2_VA + (refund.len() * 4) as u32 <= ARENA2_END_VA);
        assert!(SCUS_GAP_VA + TABLE_LEN as u32 <= SCUS_GAP_END_VA);
        // All routine VAs 4-byte aligned.
        for va in [ARENA1_VA, ARENA1_VA + (guard.len() * 4) as u32, ARENA2_VA] {
            assert_eq!(va & 3, 0);
        }
    }

    #[test]
    fn regions_and_sites_are_outside_live_tables() {
        for (va, len, what) in [
            (ARENA1_VA, ARENA1_END_VA - ARENA1_VA, "arena1"),
            (ARENA2_VA, ARENA2_END_VA - ARENA2_VA, "arena2"),
            (SCUS_GAP_VA, TABLE_LEN as u32, "table"),
        ] {
            assert!(assert_not_in_tables(va, len, SCUS_TABLE_RANGES, what).is_ok());
        }
        for va in [HOOK_A_VA, HOOK_B_VA, HOOK_C_VA, HOOK_D_VA] {
            assert!(assert_not_in_tables(va, 8, OVERLAY_TABLE_RANGES, "site").is_ok());
        }
        // The guard refuses a region overlapping a live table (move-power window
        // in 0898; the font/name tables in SCUS).
        assert!(assert_not_in_tables(0x801F_5000, 8, OVERLAY_TABLE_RANGES, "x").is_err());
        assert!(assert_not_in_tables(0x8007_4400, 8, SCUS_TABLE_RANGES, "x").is_err());
        // ... and the steal table immediately after the config gap.
        assert!(assert_not_in_tables(SCUS_GAP_END_VA, 8, SCUS_TABLE_RANGES, "x").is_err());
    }
}
