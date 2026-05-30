//! Seru-summon scene-graph record table — the per-summon move-VM part list
//! embedded in each summon **code overlay**.
//!
//! A Seru-magic summon visual (e.g. Gimard *Tail Fire*) is a per-summon MIPS
//! code overlay loaded by the battle action SM `FUN_801E295C` state `0x29`:
//! it resolves spell id `0x81..=0x8b` via `PTR_801f6734[id-0x81]` and calls
//! `FUN_8003EC70(id-0x79, 0)`, which loads PROT entry `(id-0x79)+0x381`. So the
//! summon overlays are PROT `905..=915`; **PROT `905` = Gimard Tail Fire**
//! (`id 0x81`: `0x81-0x79 = 8`, `8+0x381 = 0x389 = 905`).
//!
//! The overlay is **raw MIPS code** (its first word is a function prologue
//! `addiu sp, sp, -N`), but a **record table is embedded as inline data**
//! between two functions: the summon scene-graph's part list. The overlay's
//! staging loop (link base `0x801F69D8`) walks the table and stages one
//! part-actor per record via `FUN_80021B04`, animating each by running the
//! record's move-VM bytecode through the move-table VM `FUN_80023070` (see
//! [`docs/subsystems/move-vm.md`]).
//!
//! ## Verified layout (PROT 905 / Gimard, byte-pinned against the disc)
//!
//! The leading function ends with a `jr ra` epilogue at file offset `0x1804`
//! (`+ 0x1808` delay slot); the record table begins at **`0x180C`** and runs
//! for **19 records** of **`0x58`** bytes each, ending exactly at `0x1E94`
//! (`0x180C + 19*0x58`) — where raw MIPS code resumes (the next function
//! prologue is at `0x201C`). Per record:
//!
//! ```text
//! +0x00  i16  model_sel    ; -1 = transform node (mesh bound by move-VM ops
//!                          ;      0x00/0x04); >= 0 = DAT_8007C018[model_sel + base]
//! +0x02  u16  flags        ; control flags (purpose per-summon)
//! +0x04  u8   bytecode[0x54] ; u16-aligned move-VM stream, self-terminating
//!                          ; (the VM stops on an opcode >= 0x47); PC starts here
//! ```
//!
//! The first three records carry `model_sel = -1` (transform nodes). The table
//! has **no count field and no terminator** — its length is the staging loop's
//! bound, recovered here from the disc by the clean MIPS-code boundary at
//! `0x1E94`. The offset and count are **pinned for PROT 905 only**; the sibling
//! summon overlays (`906..=915`) place their tables at their own offsets/counts
//! and are not yet individually verified.
//!
//! What this parser does **not** decide: the per-part initial world position /
//! render-slot transform (the `FUN_80021B04` arguments) and the spawn/teardown
//! lifecycle live in the overlay's staging code, which is not in the dumped
//! corpus (the `0x801F69D8`-base dumps are *other* overlays that alias the same
//! load address). Driving a faithful animated summon needs that staging code;
//! this module recovers the record table the driver consumes.

use serde::Serialize;

/// Per-record stride in the summon table (bytes).
pub const SUMMON_RECORD_STRIDE: usize = 0x58;
/// Header bytes before the move-VM bytecode (`i16 model_sel` + `u16 flags`).
pub const SUMMON_RECORD_HEADER: usize = 4;
/// Move-VM bytecode bytes per record (`stride - header`).
pub const SUMMON_BYTECODE_LEN: usize = SUMMON_RECORD_STRIDE - SUMMON_RECORD_HEADER;

/// PROT entry of the Gimard Tail Fire summon overlay (spell id `0x81`).
pub const GIMARD_PROT_INDEX: u32 = 905;
/// File offset of the record table within PROT 905.
pub const GIMARD_TABLE_OFFSET: usize = 0x180C;
/// Record count of the PROT 905 table (disc-pinned by the MIPS-code boundary).
pub const GIMARD_RECORD_COUNT: usize = 19;

/// `model_sel` sentinel: this record is a transform node (no direct mesh; the
/// mesh is bound at runtime by the record's move-VM anim-bank ops `0x00`/`0x04`).
pub const MODEL_SEL_TRANSFORM_NODE: i16 = -1;

/// One staged part-actor of a summon scene-graph.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SummonPartRecord {
    /// `-1` ([`MODEL_SEL_TRANSFORM_NODE`]) = transform node; `>= 0` indexes the
    /// global TMD pool (`DAT_8007C018[model_sel + base]`).
    pub model_sel: i16,
    /// Per-record control flags.
    pub flags: u16,
    /// The record's `0x54`-byte move-VM bytecode slot (u16-aligned; the move VM
    /// reads it from PC 0 and stops on an opcode `>= 0x47`).
    pub bytecode: Vec<u8>,
}

impl SummonPartRecord {
    /// `true` when this record is a transform node (`model_sel == -1`).
    pub fn is_transform_node(&self) -> bool {
        self.model_sel == MODEL_SEL_TRANSFORM_NODE
    }
}

/// A parsed summon overlay record table.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SummonOverlay {
    /// File offset the table was parsed from.
    pub table_offset: usize,
    /// The staged part records, in table order.
    pub records: Vec<SummonPartRecord>,
}

/// Parse `count` records of [`SUMMON_RECORD_STRIDE`] bytes starting at
/// `table_offset` in `overlay` (the raw PROT-entry bytes of a summon overlay).
///
/// Returns `None` when the table would run past the end of `overlay`.
pub fn parse_at(overlay: &[u8], table_offset: usize, count: usize) -> Option<SummonOverlay> {
    let end = table_offset.checked_add(count.checked_mul(SUMMON_RECORD_STRIDE)?)?;
    if end > overlay.len() {
        return None;
    }
    let mut records = Vec::with_capacity(count);
    for n in 0..count {
        let base = table_offset + n * SUMMON_RECORD_STRIDE;
        let model_sel = i16::from_le_bytes([overlay[base], overlay[base + 1]]);
        let flags = u16::from_le_bytes([overlay[base + 2], overlay[base + 3]]);
        let body = base + SUMMON_RECORD_HEADER;
        let bytecode = overlay[body..body + SUMMON_BYTECODE_LEN].to_vec();
        records.push(SummonPartRecord {
            model_sel,
            flags,
            bytecode,
        });
    }
    Some(SummonOverlay {
        table_offset,
        records,
    })
}

/// Parse the Gimard Tail Fire summon table (PROT 905) at its pinned offset and
/// count. `overlay` is the raw PROT 905 entry bytes.
pub fn parse_gimard(overlay: &[u8]) -> Option<SummonOverlay> {
    parse_at(overlay, GIMARD_TABLE_OFFSET, GIMARD_RECORD_COUNT)
}

/// Locate a summon record table by structure: scan for a function `jr ra`
/// epilogue (`0x03E00008`) whose data two words later (past the `jr` + delay
/// slot) begins a plausible record run — the first record a transform node
/// (`model_sel == -1`) followed by at least two more records whose `model_sel`
/// is in the plausible part range. Returns the table offset, or `None` if no
/// such epilogue is found in the first `scan_limit` bytes.
///
/// This recovers PROT 905's table offset (`0x180C`) from the disc — the naive
/// "first `jr ra`" misses (several functions precede the table), so the
/// candidate must validate against the record shape. The record **count** is
/// not derivable structurally without the staging loop bound, so callers pair
/// this with a known count (e.g. [`GIMARD_RECORD_COUNT`]).
pub fn locate_table_offset(overlay: &[u8], scan_limit: usize) -> Option<usize> {
    let limit = scan_limit.min(overlay.len().saturating_sub(4));
    let read_model_sel = |o: usize| -> Option<i16> {
        overlay
            .get(o..o + 2)
            .map(|b| i16::from_le_bytes([b[0], b[1]]))
    };
    // A record's `model_sel` is plausible when small (-1 transform node, or a
    // pool index / special marker) — code reinterpreted as a record yields
    // large arbitrary values.
    let plausible = |m: i16| (-1..=0x4001).contains(&m);
    let mut off = 0;
    while off <= limit {
        let word = u32::from_le_bytes([
            overlay[off],
            overlay[off + 1],
            overlay[off + 2],
            overlay[off + 3],
        ]);
        if word == 0x03E0_0008 {
            let table = off + 8;
            let r0 = read_model_sel(table);
            let r1 = read_model_sel(table + SUMMON_RECORD_STRIDE);
            let r2 = read_model_sel(table + 2 * SUMMON_RECORD_STRIDE);
            if r0 == Some(MODEL_SEL_TRANSFORM_NODE)
                && r1.is_some_and(plausible)
                && r2.is_some_and(plausible)
            {
                return Some(table);
            }
        }
        off += 4;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_at_reads_header_and_bytecode_slot() {
        // Two synthetic records: a transform node then a mesh node.
        let mut buf = vec![0u8; 0x100];
        let off = 0x10;
        // record 0: model_sel = -1, flags = 0, bytecode starts 0x13 0x00 ...
        buf[off..off + 2].copy_from_slice(&(-1i16).to_le_bytes());
        buf[off + 2..off + 4].copy_from_slice(&0u16.to_le_bytes());
        buf[off + 4] = 0x13;
        // record 1: model_sel = 25, flags = 0x0008
        let r1 = off + SUMMON_RECORD_STRIDE;
        buf[r1..r1 + 2].copy_from_slice(&25i16.to_le_bytes());
        buf[r1 + 2..r1 + 4].copy_from_slice(&0x0008u16.to_le_bytes());

        let parsed = parse_at(&buf, off, 2).expect("fits");
        assert_eq!(parsed.records.len(), 2);
        assert!(parsed.records[0].is_transform_node());
        assert_eq!(parsed.records[0].bytecode.len(), SUMMON_BYTECODE_LEN);
        assert_eq!(parsed.records[0].bytecode[0], 0x13);
        assert_eq!(parsed.records[1].model_sel, 25);
        assert_eq!(parsed.records[1].flags, 0x0008);
        assert!(!parsed.records[1].is_transform_node());
    }

    #[test]
    fn parse_at_rejects_overrun() {
        let buf = vec![0u8; 0x40];
        assert!(parse_at(&buf, 0x10, 2).is_none());
    }

    #[test]
    fn locate_table_offset_finds_validated_table_after_jr_ra() {
        // An earlier `jr ra` whose data is NOT a record run must be rejected;
        // only the epilogue followed by a transform-node record wins.
        let mut buf = vec![0u8; 0x10 + 4 * SUMMON_RECORD_STRIDE];
        // Decoy epilogue at 0x00 -> candidate 0x08 has model_sel = 0x4242 (code-like).
        buf[0x00..0x04].copy_from_slice(&0x03E0_0008u32.to_le_bytes());
        buf[0x08..0x0A].copy_from_slice(&0x4242i16.to_le_bytes());
        // Real epilogue at 0x08 -> table at 0x10: rec0 = -1, rec1/2 plausible.
        buf[0x08..0x0C].copy_from_slice(&0x03E0_0008u32.to_le_bytes());
        buf[0x10..0x12].copy_from_slice(&(-1i16).to_le_bytes());
        buf[0x10 + SUMMON_RECORD_STRIDE..0x10 + SUMMON_RECORD_STRIDE + 2]
            .copy_from_slice(&25i16.to_le_bytes());
        buf[0x10 + 2 * SUMMON_RECORD_STRIDE..0x10 + 2 * SUMMON_RECORD_STRIDE + 2]
            .copy_from_slice(&0i16.to_le_bytes());
        assert_eq!(locate_table_offset(&buf, buf.len()), Some(0x10));
    }
}
