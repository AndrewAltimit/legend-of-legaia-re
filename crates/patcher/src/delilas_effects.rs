//! Give a Delilas sibling's reskinned signature art the sibling's **own**
//! visual burst, by transplanting one move-VM effect prototype out of the
//! sibling's per-spell cast module into the battle overlay and naming it from
//! a spare `0x801F6324` prototype id.
//!
//! # Why this route exists
//!
//! Running the cast module itself is closed - it stores into PSX kernel RAM and
//! battle state `0x70` polls it with no bail-out (see
//! `docs/tooling/randomizer.md` § "Casting a sibling signature attack from a
//! party slot"). What that module *provides* is separable: its 24
//! `FUN_80050ED4` calls each hand the spawner a **move-VM part record** in the
//! summon part format (`[i16 model_sel][u16 flags][bytecode]`) - byte-for-byte
//! the same class of record the retail `0x801F6324` prototype table points at,
//! and the same class the game already spawns from a player art's own effect
//! script. Retail proto ids `50..=60` (Maximum Blow, Fire Tackle, Power Slash,
//! Super Javelin, Love You, Frost Breath, Hurricane Kick, Super Ironhead) are
//! all `model_sel = -1` submode-2 records reached exactly this way. So a record
//! copied into a resident cave and named by a spare id is not a new mechanism;
//! it is the mechanism the Super Arts already use, pointed at different data.
//!
//! # What the disc allows, measured
//!
//! **Spare ids.** The prototype table is 61 `u32`s at PROT 0898 file `0x27B0C`.
//! Every reader of it is one of six `lui 0x801f` sites, all inside PROT 0898
//! (`find-address-word-refs.py 801f6324`: 6 `lui`, 0 word / `jal` / `j` /
//! branch), and each one indexes with a byte lifted straight out of data -
//! `FUN_801DEA50` `@0x801df17c` from an action effect-script record's `+1`,
//! `@0x801df4b0` from a move-power record's `+0x0e` run, `FUN_801E09F8`
//! `@0x801e0ea4`/`0x801e1208`/`0x801e14a4` from `+0x0e`/`+0x12`/`+0x16`, and
//! `FUN_801E22C8` `@0x801e23a0` from a cue-group member byte. There is no
//! computed index anywhere; the one id that is **not** data-borne is the
//! literal `9` at `0x801df094` (used when a script record's id is `0` and the
//! actor's `+0x1d9` reads `0x11`). Censusing every carrier on the disc - 1811
//! monster action entries over 186 blocks, 286 player action entries, all 44
//! move-power records, all 13 cue groups - leaves exactly six ids that nothing
//! selects: [`SPARE_EFFECT_IDS`].
//!
//! **Cave.** Three of those six own their record exclusively (`37`, `38`, `47`);
//! the other three alias a live record (`14` → id 34/35/36's, `48`/`49` → id
//! 0's) and only their table *slot* is free. Records `37` and `38` are
//! **contiguous** - `0x801F5B64` and `0x801F5B90`, with live id `39` starting
//! at `0x801F5BBC` - which is the one run of consecutive dead record bytes in
//! the overlay: [`CAVE_LEN`] bytes.
//!
//! That is the whole budget. The battle overlay is packed to the byte, and this
//! module does not squat anywhere else. The `0x801F4E63..0x801F69D8` window's
//! zero runs are runtime-indexed (a previous feature squatted in the move-power
//! table's zeros and six move ids read the bytes as records - see
//! `crates/patcher/README.md`), and the 530 bytes that *look* free after
//! prototype id `44` are the two burst-arm trigger programs `0x801F5CF8` /
//! `0x801F5D90`, their two 130-byte stager records, and three further
//! `model_sel = -1` records packed behind them up to id `50`. Measured, not
//! assumed.
//!
//! **So one sibling per patched disc gets a transplant**, and the caller keeps
//! its existing behaviour for the others rather than shipping a maybe.
//!
//! # What is checked before anything is written
//!
//! [`plan`] refuses unless every one of these holds on the disc in hand:
//!
//! - all six spare slots still hold their retail prototype VAs
//!   ([`SPARE_RETAIL_PROTO`]), so a different build cannot be patched blind -
//!   with the one slot this module itself moves ([`CAVE_TAIL_ID`]) accepted at
//!   either value, so re-applying to a patched disc is a no-op and not a
//!   refusal;
//! - the chosen source record still parses as `model_sel = -1` at its measured
//!   VA in its module;
//! - the record's program is **self-contained** - [`program_len`] walks it with
//!   the move-VM's own operand widths and rejects any opcode that reaches
//!   outside the record ([`REJECTED_OPCODES`]);
//! - the program fits [`CAVE_LEN`].
//!
//! # Residual risk
//!
//! A transplanted record can look wrong; it cannot hang or corrupt memory. The
//! move VM bound-checks its opcode (`>= 0x47` ends the buffer), the stager
//! takes no pointer out of the record's data, the record contains no absolute
//! address (the module's whole record region scans clean for words in
//! `0x801C0000..0x80200000`), and the safety filter removes every opcode that
//! could name a resource the cast path stages - the `DAT_8007C018` mesh pool
//! (`model_sel >= 0`), the per-scene prescript stager (`0x25`), the VRAM copy
//! (`0x40`) and beam (`0x1E`) arms, the keyframe heap (`0x2C`/`0x30`), the
//! `DAT_8007BE60` face table (`0x21`), and the two unidentified escapes
//! (`0x20`'s `gp[0x714]` hook, and `0x17`/`0x2F`). What is left is a pure
//! parameter / colour tween node, whose worst case is texels that are not the
//! ones the enemy cast would have staged - a differently coloured burst, or
//! none.

use anyhow::{Context, Result, bail};

use crate::delilas_party::Sibling;
use crate::disc::DiscPatcher;

/// PROT entry of the raw battle-action overlay (load base
/// [`BATTLE_OVERLAY_BASE`]).
pub const BATTLE_OVERLAY_ENTRY: usize = 898;

/// Load base of [`BATTLE_OVERLAY_ENTRY`] - the same constant
/// `legaia_asset::move_power::BATTLE_OVERLAY_BASE` derives from the move-power
/// table's pinned file offset.
pub const BATTLE_OVERLAY_BASE: u32 = 0x801C_E818;

/// Link base of the per-spell cast modules (the slot-B overlay slot).
pub const MODULE_LINK_BASE: u32 = 0x801F_69D8;

/// File offset of the `0x801F6324` prototype-pointer table inside
/// [`BATTLE_OVERLAY_ENTRY`].
pub const PROTO_TABLE_FILE_OFFSET: usize = 0x27B0C;

/// Entries in the prototype table.
pub const PROTO_TABLE_LEN: usize = 61;

/// The `0x801F6324` ids no producer on the retail disc selects.
///
/// Censused over every carrier the six table readers index with: all monster
/// and player action effect scripts, all move-power `+0x0e`/`+0x12`/`+0x16`
/// lists, the cue-group table, and the one code-resident literal (`9`).
pub const SPARE_EFFECT_IDS: [u8; 6] = [14, 37, 38, 47, 48, 49];

/// Retail prototype VA of each [`SPARE_EFFECT_IDS`] slot. Ids `48`/`49` alias
/// id `0`'s record and `14` aliases id `34`/`35`/`36`'s, so only ids `37`/`38`
/// own bytes this module may overwrite.
pub const SPARE_RETAIL_PROTO: [(u8, u32); 6] = [
    (14, 0x801F_5A04),
    (37, 0x801F_5B64),
    (38, 0x801F_5B90),
    (47, 0x801F_5688),
    (48, 0x801F_5484),
    (49, 0x801F_5484),
];

/// Start of the one contiguous run of dead prototype-record bytes: records `37`
/// and `38`, bounded by live record `39` at `0x801F5BBC`.
pub const CAVE_VA: u32 = 0x801F_5B64;

/// Bytes of [`CAVE_VA`] (`0x801F5BBC - 0x801F5B64`).
pub const CAVE_LEN: usize = 88;

/// The prototype id this module repoints at [`CAVE_VA`]. It already points
/// there on retail, so the table write is a no-op and only the record bytes
/// change - which keeps the edit as small as the finding allows.
pub const CAVE_EFFECT_ID: u8 = 37;

/// The second id the cave consumes. Its record is overwritten by the cave's
/// tail, so it must never be used afterwards; [`plan`] parks it on id `0`'s
/// record (`0x801F5484`, the one every monster hit-spark already shares) so a
/// stray selection degrades to a retail effect instead of decoding the cave's
/// middle as a record header.
pub const CAVE_TAIL_ID: u8 = 38;

/// Where [`CAVE_TAIL_ID`] is parked - id `0`'s record, which nothing this
/// module writes can disturb.
pub const CAVE_TAIL_PARK_VA: u32 = 0x801F_5484;

/// Opcodes a transplanted record may not contain, because each reaches outside
/// the record's own bytes.
///
/// | op | what it reaches |
/// |---|---|
/// | `0x17` | battle-overlay escape `FUN_801F30C4` (the radial burst arms) |
/// | `0x1E` | render-mode-4 VRAM beam (`LoadImage`/`StoreImage` scratch) |
/// | `0x20` | indirect call through `gp[0x714]`, an unidentified hook |
/// | `0x21` | per-id write into the `DAT_8007BE60` face table |
/// | `0x25` | child spawn out of the per-scene prescript stager `_DAT_8007B8D0` |
/// | `0x2C` | keyframe-buffer heap allocation (`FUN_80017888`) |
/// | `0x2F` | overlay extension VM `FUN_801D362C` |
/// | `0x30` | keyframe-buffer free |
/// | `0x40` | libgpu `MoveImage` VRAM-to-VRAM copy |
pub const REJECTED_OPCODES: [u16; 9] = [0x17, 0x1E, 0x20, 0x21, 0x25, 0x2C, 0x2F, 0x30, 0x40];

/// The record each sibling's signature burst is taken from: the module's PROT
/// entry and the record VA one of its `FUN_80050ED4` sites hands the spawner.
///
/// Every module's spawn sites cluster by phase; each pick below is from the
/// module's **main volley** - the run of consecutive spawn sites that fires the
/// attack's own burst - and is the largest member of that volley whose program
/// passes [`REJECTED_OPCODES`] and fits [`CAVE_LEN`]. The three modules are
/// `958` = Gi's Blazing Slash (action `0x79`), `959` = Che's Megaton Press
/// (`0x7A`), `960` = Lu's Plasma Strike (`0x7B`).
pub const SIGNATURE_RECORD: [(Sibling, usize, u32); 3] = [
    // Volley at module +0x23B8..+0x246C (7 consecutive spawns); this is the
    // largest that fits, 88 bytes live.
    (Sibling::Gi, 958, 0x801F_905C),
    // Volley at module +0x970..+0xA48 (11 consecutive spawns); 84 bytes live.
    (Sibling::Che, 959, 0x801F_8A34),
    // Volley at module +0x2B8..+0x348 (7 consecutive spawns); 84 bytes live.
    (Sibling::Lu, 960, 0x801F_8AB0),
];

/// Move-VM operand widths, in `u16` units, for the opcodes a self-contained
/// prototype record can hold (`docs/subsystems/move-vm.md`; the port's own
/// table is `legaia_engine_vm::move_vm::step`). `None` means the walker cannot
/// size the instruction and must refuse the record.
///
/// `0x08` (HALT) and `0x30` end the program; `0x0A` and `0x3D` are
/// count-prefixed and sized by [`program_len`] itself.
fn operand_words(op: u16) -> Option<usize> {
    Some(match op {
        0x00 | 0x01 | 0x04 | 0x05 | 0x07 => 4,
        0x02 | 0x03 | 0x06 => 2,
        0x08 => 0,
        0x09 => 2,
        0x0C => 6,
        0x0D..=0x12 => 2,
        0x13 => 0x10,
        0x14 => 5,
        0x15..=0x1D => 2,
        0x1E | 0x1F => 8,
        0x20 => 3,
        0x21 => 7,
        0x22 => 1,
        0x23 => 0xD,
        0x24 => 3,
        0x25 => 2,
        0x26 => 5,
        0x27 => 3,
        0x28..=0x2A => 2,
        0x2B => 4,
        0x2C => 5,
        0x2D | 0x2E => 4,
        0x31 | 0x32 => 2,
        0x33 => 1,
        0x34 => 9,
        0x35..=0x37 => 3,
        0x38 => 2,
        0x39 => 4,
        0x3A | 0x3B => 1,
        0x3E | 0x3F => 2,
        0x40 => 7,
        0x41 => 2,
        0x42 => 0xF,
        0x43 => 1,
        0x44 => 4,
        0x45 => 8,
        0x46 => 4,
        _ => return None,
    })
}

/// Why a record cannot be transplanted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reject {
    /// `record[+0]` is not `-1`, so the stager binds a `DAT_8007C018` mesh the
    /// battle may not hold in that slot.
    NotTransformNode(i16),
    /// The program holds an opcode that reaches outside the record.
    Opcode(u16),
    /// The walker cannot size an opcode, so the program's extent is unknown.
    Unsizeable(u16),
    /// The program runs off the end of the module without halting.
    NoHalt,
    /// The program is longer than the cave.
    TooLong(usize),
}

impl std::fmt::Display for Reject {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Reject::NotTransformNode(s) => write!(f, "model_sel {s} is not a transform node"),
            Reject::Opcode(op) => write!(f, "opcode {op:#04X} reaches outside the record"),
            Reject::Unsizeable(op) => write!(f, "opcode {op:#04X} has no known width"),
            Reject::NoHalt => write!(f, "program never halts"),
            Reject::TooLong(n) => write!(f, "program is {n} bytes, cave holds {CAVE_LEN}"),
        }
    }
}

/// Live byte length of the record at `off` in `bytes` - the header plus every
/// word up to and including the `0x08` HALT - or why it cannot be used.
///
/// `FUN_80021B04` seats the move-VM PC at u16 index `2` and nothing rewrites
/// it, so a record has exactly one entry point and the bytes past its halt are
/// never decoded. Backward branches (`0x19`/`0x1B` loop-back) only re-run words
/// the linear walk has already covered, so the linear extent is the whole live
/// extent.
pub fn program_len(bytes: &[u8], off: usize) -> std::result::Result<usize, Reject> {
    let word = |i: usize| -> Option<u16> {
        let b = off + 4 + i * 2;
        Some(u16::from_le_bytes([*bytes.get(b)?, *bytes.get(b + 1)?]))
    };
    let model_sel = i16::from_le_bytes([
        *bytes.get(off).ok_or(Reject::NoHalt)?,
        *bytes.get(off + 1).ok_or(Reject::NoHalt)?,
    ]);
    if model_sel != -1 {
        return Err(Reject::NotTransformNode(model_sel));
    }
    let mut pc = 0usize;
    // A record cannot be longer than the cave, so the walk is bounded well
    // below any runaway.
    for _ in 0..1024 {
        let op = word(pc).ok_or(Reject::NoHalt)?;
        if REJECTED_OPCODES.contains(&op) {
            return Err(Reject::Opcode(op));
        }
        if op == 0x08 {
            return Ok(4 + (pc + 1) * 2);
        }
        let size = match op {
            // Count-prefixed: `3 + count*3` / `3 + count*6`.
            0x0A => 3 + 3 * word(pc + 2).ok_or(Reject::NoHalt)? as usize,
            0x3D => 3 + 6 * word(pc + 2).ok_or(Reject::NoHalt)? as usize,
            _ => operand_words(op).ok_or(Reject::Unsizeable(op))?,
        };
        if size == 0 {
            return Err(Reject::Unsizeable(op));
        }
        pc += size;
    }
    Err(Reject::NoHalt)
}

/// One sibling's transplant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BurstPlan {
    /// Which sibling's signature art gets the burst.
    pub sibling: Sibling,
    /// The `0x801F6324` id to write into the art's effect-script records.
    pub effect_id: u8,
    /// PROT entry the record was taken from.
    pub source_entry: usize,
    /// Record VA inside that module.
    pub source_va: u32,
    /// The record bytes, header included, as they will sit at [`CAVE_VA`].
    pub record: Vec<u8>,
}

impl BurstPlan {
    /// Same-size in-place edits for [`BATTLE_OVERLAY_ENTRY`], as
    /// `(file offset, bytes)` pairs.
    pub fn edits(&self) -> Vec<(u64, Vec<u8>)> {
        let cave_off = (CAVE_VA - BATTLE_OVERLAY_BASE) as u64;
        vec![
            (cave_off, self.record.clone()),
            // The cave's own id already points at CAVE_VA on retail, so only
            // the consumed tail id has to move off the bytes we just wrote.
            (
                (PROTO_TABLE_FILE_OFFSET + CAVE_TAIL_ID as usize * 4) as u64,
                CAVE_TAIL_PARK_VA.to_le_bytes().to_vec(),
            ),
        ]
    }
}

/// Read the prototype table out of a battle-overlay image.
pub fn proto_table(overlay: &[u8]) -> Result<Vec<u32>> {
    let end = PROTO_TABLE_FILE_OFFSET + PROTO_TABLE_LEN * 4;
    let slice = overlay
        .get(PROTO_TABLE_FILE_OFFSET..end)
        .context("battle overlay is too short to hold the 0x801F6324 table")?;
    Ok(slice
        .chunks_exact(4)
        .map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect())
}

/// Refuse unless every spare slot still holds its retail prototype VA. This is
/// the build guard: the census behind [`SPARE_EFFECT_IDS`] was taken on the
/// retail USA disc, and a table that has moved is a table the census does not
/// describe.
pub fn verify_spare_ids(overlay: &[u8]) -> Result<()> {
    let table = proto_table(overlay)?;
    for (id, want) in SPARE_RETAIL_PROTO {
        let got = table[id as usize];
        // The cave's tail id is the one slot this module itself moves, so a
        // disc that has already been patched must still pass - re-applying is
        // a no-op, not a refusal.
        if id == CAVE_TAIL_ID && got == CAVE_TAIL_PARK_VA {
            continue;
        }
        if got != want {
            bail!(
                "0x801F6324[{id}] is {got:#010X}, retail has {want:#010X} - \
                 this disc is not the build the spare-id census was taken on"
            );
        }
    }
    Ok(())
}

/// Plan the transplant for `sibling`, or `Ok(None)` when the disc has no room
/// left (the cave holds exactly one record, so only the first sibling asked
/// gets one).
///
/// `cave_taken` is the caller's record of whether the cave has already been
/// spent this run.
pub fn plan(disc: &DiscPatcher, sibling: Sibling, cave_taken: bool) -> Result<Option<BurstPlan>> {
    if cave_taken {
        return Ok(None);
    }
    let overlay = disc
        .read_entry(BATTLE_OVERLAY_ENTRY)
        .context("read the battle overlay for the effect-prototype transplant")?;
    verify_spare_ids(&overlay)?;

    let (_, entry, va) = SIGNATURE_RECORD
        .iter()
        .copied()
        .find(|(s, _, _)| *s == sibling)
        .expect("every sibling has a signature record");

    let module = disc
        .read_entry(entry)
        .with_context(|| format!("read cast module PROT {entry}"))?;
    let off = va
        .checked_sub(MODULE_LINK_BASE)
        .map(|o| o as usize)
        .filter(|o| *o < module.len())
        .with_context(|| format!("record {va:#010X} is outside PROT {entry}"))?;

    let len = match program_len(&module, off) {
        Ok(n) => n,
        Err(why) => bail!("PROT {entry} record {va:#010X}: {why}"),
    };
    if len > CAVE_LEN {
        bail!("PROT {entry} record {va:#010X}: {}", Reject::TooLong(len));
    }

    // Pad to the cave so the trailing bytes are a clean `0x08` HALT rather than
    // whatever record 38 left there: any future reader that lands mid-cave
    // stops immediately instead of decoding stale operands.
    let mut record = module[off..off + len].to_vec();
    record.resize(CAVE_LEN, 0);
    for w in record[len..].chunks_exact_mut(2) {
        w.copy_from_slice(&0x0008u16.to_le_bytes());
    }

    Ok(Some(BurstPlan {
        sibling,
        effect_id: CAVE_EFFECT_ID,
        source_entry: entry,
        source_va: va,
        record,
    }))
}

/// Apply a plan to the disc image. Returns one human-readable note per plan.
pub fn apply(disc: &mut DiscPatcher, plan: &BurstPlan) -> Result<String> {
    for (off, bytes) in plan.edits() {
        disc.patch_prot_entry(BATTLE_OVERLAY_ENTRY, off, &bytes)
            .with_context(|| {
                format!(
                    "write the {} signature-burst transplant at battle-overlay +{off:#X}",
                    plan.sibling.display_name()
                )
            })?;
    }
    Ok(format!(
        "{}: signature burst transplanted from PROT {} record {:#010X} \
         into effect prototype {} ({} bytes)",
        plan.sibling.display_name(),
        plan.source_entry,
        plan.source_va,
        plan.effect_id,
        plan.record.len(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `[i16 model_sel][u16 flags]` then the given opcode words.
    fn record(model_sel: i16, words: &[u16]) -> Vec<u8> {
        let mut v = model_sel.to_le_bytes().to_vec();
        v.extend_from_slice(&0u16.to_le_bytes());
        for w in words {
            v.extend_from_slice(&w.to_le_bytes());
        }
        v
    }

    #[test]
    fn halt_bounds_the_live_extent() {
        // WAIT_SET 0 / HALT -> header + 3 words.
        let r = record(-1, &[0x09, 0x0000, 0x08, 0xDEAD, 0xBEEF]);
        assert_eq!(program_len(&r, 0), Ok(4 + 3 * 2));
    }

    #[test]
    fn a_library_mesh_record_is_refused() {
        let r = record(27, &[0x08]);
        assert_eq!(program_len(&r, 0), Err(Reject::NotTransformNode(27)));
    }

    #[test]
    fn every_rejected_opcode_is_refused() {
        for op in REJECTED_OPCODES {
            let r = record(-1, &[op, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x08]);
            assert_eq!(program_len(&r, 0), Err(Reject::Opcode(op)));
        }
    }

    #[test]
    fn a_program_that_never_halts_is_refused() {
        let r = record(-1, &[0x09, 0x0000, 0x09, 0x0000]);
        assert_eq!(program_len(&r, 0), Err(Reject::NoHalt));
    }

    #[test]
    fn the_count_prefixed_opcodes_size_themselves() {
        // 0x0A with count 2 -> 3 + 2*3 = 9 words, then HALT.
        let mut words = vec![0x0A, 0x0001, 0x0002];
        words.extend(std::iter::repeat_n(0u16, 6));
        words.push(0x08);
        let r = record(-1, &words);
        assert_eq!(program_len(&r, 0), Ok(4 + 10 * 2));
    }

    #[test]
    fn the_op_0x13_block_is_sixteen_words() {
        // 0x13 is the parameter/colour-tween seat every signature record opens
        // with; mis-sizing it walks the operands as opcodes.
        let mut words = vec![0x13];
        words.extend(std::iter::repeat_n(0u16, 15));
        words.push(0x08);
        let r = record(-1, &words);
        assert_eq!(program_len(&r, 0), Ok(4 + 17 * 2));
    }

    #[test]
    fn the_cave_spans_exactly_records_37_and_38() {
        let id37 = SPARE_RETAIL_PROTO[1];
        let id38 = SPARE_RETAIL_PROTO[2];
        assert_eq!(id37, (37, CAVE_VA));
        // Record 39 starts at 0x801F5BBC, which is where the cave ends.
        assert_eq!(id38.1 - CAVE_VA, 44);
        assert_eq!(CAVE_VA + CAVE_LEN as u32, 0x801F_5BBC);
    }

    #[test]
    fn the_parked_tail_id_lands_on_a_record_the_cave_cannot_touch() {
        // The parked record sits below the cave, so nothing this module
        // writes can reach it.
        assert_eq!(CAVE_TAIL_PARK_VA, SPARE_RETAIL_PROTO[4].1);
        assert_eq!(CAVE_TAIL_PARK_VA.min(CAVE_VA), CAVE_TAIL_PARK_VA);
    }

    #[test]
    fn the_proto_table_offset_matches_the_pinned_va() {
        assert_eq!(
            BATTLE_OVERLAY_BASE as usize + PROTO_TABLE_FILE_OFFSET,
            0x801F_6324
        );
        assert_eq!(
            BATTLE_OVERLAY_BASE,
            legaia_asset::move_power::BATTLE_OVERLAY_BASE
        );
        assert_eq!(
            PROTO_TABLE_FILE_OFFSET,
            legaia_asset::move_power::EFFECT_PROTO_TABLE_FILE_OFFSET
        );
        assert_eq!(
            PROTO_TABLE_LEN,
            legaia_asset::move_power::EFFECT_AUX_TABLE_LEN
        );
    }

    #[test]
    fn the_guard_accepts_a_disc_this_module_has_already_patched() {
        let mut overlay = vec![0u8; PROTO_TABLE_FILE_OFFSET + PROTO_TABLE_LEN * 4];
        for (id, va) in SPARE_RETAIL_PROTO {
            let b = PROTO_TABLE_FILE_OFFSET + id as usize * 4;
            overlay[b..b + 4].copy_from_slice(&va.to_le_bytes());
        }
        assert!(verify_spare_ids(&overlay).is_ok());
        let b = PROTO_TABLE_FILE_OFFSET + CAVE_TAIL_ID as usize * 4;
        overlay[b..b + 4].copy_from_slice(&CAVE_TAIL_PARK_VA.to_le_bytes());
        assert!(
            verify_spare_ids(&overlay).is_ok(),
            "re-patch must be a no-op"
        );
        // Any other movement is still a refusal.
        overlay[b..b + 4].copy_from_slice(&0xDEAD_BEEFu32.to_le_bytes());
        assert!(verify_spare_ids(&overlay).is_err());
    }

    #[test]
    fn every_sibling_has_exactly_one_signature_record() {
        for s in [Sibling::Gi, Sibling::Che, Sibling::Lu] {
            assert_eq!(
                SIGNATURE_RECORD.iter().filter(|(x, _, _)| *x == s).count(),
                1
            );
        }
    }
}
