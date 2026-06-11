//! Seru-magic **summon scene-graph** part-record parser.
//!
//! A player Seru-magic cast (spell id `0x81..=0x8B`, e.g. Gimard's *Tail Fire*)
//! is staged by a per-summon code overlay, loaded into the shared overlay buffer
//! at link base [`SUMMON_OVERLAY_LINK_BASE`]. The overlay is a **move-VM
//! scene-graph**: its init function calls the SCUS actor-spawn helper
//! [`SPAWN_HELPER`] (`FUN_80021B04`) once per summon body part, passing a pointer
//! to a per-part **record** as its third argument (`a2`). Each record is
//!
//! ```text
//! +0x00  i16  model_sel   ; mesh selector: -1 = transform/pivot node (the mesh
//!                         ;   is bound by the move-VM anim-bank ops 0x00/0x04),
//!                         ;   >=0 = DAT_8007C018[model_sel + gp[0x754]]
//! +0x02  u16  flags
//! +0x04  ..   move-VM bytecode   ; ticked by FUN_80023070 (the move VM) to
//!                                ; animate the part each frame
//! ```
//!
//! `FUN_80021B04` stages each as an actor (`actor[+0x48] = record` move-buffer
//! base, `actor[+0x70] = 2` move-VM PC in u16 units → bytecode at `record+4`),
//! so the summon is a hierarchy of move-VM-driven parts.
//!
//! ## Two overlays, one buffer
//!
//! A summon cast uses **two** overlays that timeshare the buffer at
//! `0x801F69D8`: a *spawn stager* (e.g. **PROT 0905**, the spell-`0x83` slot
//! under the corrected loader index math — Gimard `0x81` arithmetics to 0903);
//! **PROT 0900** is a resident *transform / GTE-render* overlay that animates and
//! draws the spawned parts (`RotMatrixX/Y/Z` + prim emit) — it is the one byte-
//! resident in a mid-cast save state, *after* the stager has run. So the part
//! records live in the **stager file itself**, addressed by absolute pointers
//! (`lui 0x8020 / addiu`) that resolve in-file under the `0x801F69D8` link base.
//! (This corrects an earlier reading that placed the records "beyond the 0x5800
//! file" — that conflated the stager with the 900 render overlay's base.)
//!
//! ## The whole player-summon block
//!
//! The eleven player Seru-magic summons (`spell_id 0x81..=0x8B`) each ship a
//! stager overlay in [`PLAYER_SUMMON_STAGER_PROT`] (extraction PROT 0903..=0913,
//! resolved retail-side by `FUN_8003EC70(id - 0x79)`). [`parse`] recovers a
//! move-VM scene-graph from every one of them — 0905 is transform-node-dominated
//! with a handful of small library-mesh indices; the larger stagers (e.g. 0906,
//! 0911) carry many more parts, a good fraction of which classify as
//! [`SummonPartKind::Sentinel`] (node-mode markers in the record's first word, not
//! library indices). The structural recovery is pinned across the block by the
//! disc-gated `summon_overlay_block` sweep; the per-summon model-library base
//! (`gp[0x754]`) and the precise sentinel semantics are a live-trace follow-up.
//!
//! Recovery is by scanning the stager's `jal FUN_80021B04` call sites and
//! recovering the `a2` (record-pointer) each one passes — the records are
//! variable-length move-VM bytecode, not a fixed-stride table, so the call sites
//! are the authoritative enumeration. The `a2` register is followed with a tiny
//! `lui`/`addiu` emulator over each call site's preceding window.
//!
//! ## Where the effect magnitude lives (per-spell "power")
//!
//! This parser recovers the summon's *visual* scene-graph. The summon's combat
//! **effect** (the HP delta) is applied by the same stager function that spawns
//! the parts — each stager file carries exactly one `actor+0x14c` (HP) writer,
//! and the stager files split cleanly: **damage** stagers (files 0904/0912/0914, plus
//! 0915's second arm) compute the amount via the shared battle kernel
//! `FUN_801dd0ac` (`a0` = a per-summon move-type constant `0x10..0x12`, `a1 = 7`,
//! `a2` = target) and store `HP -= amount`; **heal** summons (PROT
//! 0903/0905/0910/0911/0913, plus 0915's first arm — file-content classes; which
//! spell id loads which file is re-pinned by the corrected loader math) apply `(power_byte<<5)+0xe0`
//! inline (`power_byte` from a `0x80084140`-based per-character table searched by
//! the cast spell-id `actor+0x1df`) and store `HP += amount`. For the summon path
//! (`FUN_801dd0ac` `attacker_slot == 7`) the roll is built from the attacker's
//! AGL (`+0x168`) and HP (`+0x14c`) plus the caster's AGL — i.e. **caster/summon
//! battle-state-derived, not a static per-spell scalar** (which is why no static
//! magic-power table exists). The render JT at `0x801F69D8` (PROT 0900) is
//! animation/GPU only and writes no HP. See `docs/formats/spell-table.md` and the
//! `FUN_801dd0ac` row in `docs/reference/functions.md`.

use std::ops::Range;

/// SCUS actor-spawn helper `FUN_80021B04`. Each summon part is one call.
pub const SPAWN_HELPER: u32 = 0x8002_1B04;

/// CDNAME (`xxx_dat`) PROT entries that the retail summon path arithmetics over
/// for the player Seru-magic block `spell_id 0x81..=0x8B`: `FUN_8003EC70(id -
/// 0x79)` resolves `FUN_8003E8A8(id - 0x79 + 0x381)` against the raw in-RAM
/// TOC, which in extraction index space is entry `(id - 0x81) + 903`, i.e.
/// `903..=913` (Gimard *Tail Fire* = `0x81` → 903). The historical `905..=915`
/// range (Gimard → 905) carried the loader-math off-by-2 — the resolver indexes
/// the raw `PROT.DAT` head, 2 entries above extraction indexing (see
/// `docs/formats/prot.md` § In-RAM TOC).
///
/// **The whole block is capture-pinned**: every spell id `0x81..=0x8B` was
/// observed mid-cast loading its arithmetic slot (loader-B current id at
/// `0x8007BC4C`), with zero exceptions. PROT 0907 (the spell-`0x85` slot) is
/// **Nighto's stager** - its ASCII head title `Hell's Music` is the attack's
/// display name (the SCUS spell table carries the same string, and
/// `summon.dat`'s attack-name records list it exactly parallel to Gimard's
/// `Burning Attack`); the historical "Disco King dance-song" reading is
/// refuted (the dance overlay, PROT 0980, contains no slot-B loader
/// callsite; its music is sequenced BGM). The slot-B buffer
/// (`SUMMON_OVERLAY_LINK_BASE`) is still timeshared across the wider
/// `0900..0969` cluster (move-FX module, GAME OVER, summon-effect data), so
/// verify any entry OUTSIDE this range by its `FUN_80021B04` part-spawn calls
/// before treating it as a stager. See the disc-gated `summon_overlay_block`
/// sweep and `docs/reference/open-rev-eng-threads.md`.
pub const PLAYER_SUMMON_STAGER_PROT: std::ops::RangeInclusive<u32> = 903..=913;

/// Upper bound (exclusive) on a `model_sel` that indexes the small effect-model
/// library (`DAT_8007C018[model_sel + gp[0x754]]`; ~30 entries). A `model_sel`
/// at or above this is not a plain library index — it is one of the move-VM
/// node-mode sentinels (the `0x1000` / `0x4000` render-mode markers, or a
/// `0x8000`-class bit) the larger summons carry in the record's first word.
/// Mirrors `engine_core::summon::MAX_MESH_SEL`.
pub const LIBRARY_MESH_SEL_MAX: i16 = 0x100;

/// Link / load base of the per-summon overlay buffer (`*DAT_80010390`),
/// empirically pinned by byte-matching the resident overlay in a mid-cast save
/// state (`0x801F8000` ↔ file offset `0x1628`). Both the stager files and
/// the PROT 0900 render overlay are linked here.
pub const SUMMON_OVERLAY_LINK_BASE: u32 = 0x801F_69D8;

/// `model_sel` value marking a transform/pivot node (no direct mesh; the mesh is
/// bound by the move-VM anim-bank ops).
pub const MODEL_SEL_TRANSFORM_NODE: i16 = -1;

/// What the first word of a part record (`model_sel`) selects.
///
/// The PROT 0905 stager is dominated by [`Self::TransformNode`] parts
/// with a handful of small [`Self::LibraryMesh`] indices, so its records read
/// cleanly. The larger stagers (e.g. PROT 0906/0911) carry many records
/// whose first word is a [`Self::Sentinel`] node-mode marker rather than a plain
/// library index — the records are genuine (each is a statically-resolved
/// `FUN_80021B04` `a2` pointer, an explicit `lui`/`addiu` immediate that cannot
/// be mis-resolved), the move VM just drives those nodes through a different
/// render mode. Naming the kind keeps the parse output honest instead of
/// printing a sentinel as a bogus "mesh-sel".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SummonPartKind {
    /// `model_sel == -1`: a transform/pivot node; the mesh (if any) is bound by
    /// the move-VM anim-bank ops, not the record header.
    TransformNode,
    /// `0 <= model_sel < `[`LIBRARY_MESH_SEL_MAX`]: a plain effect-model-library
    /// index (`DAT_8007C018[model_sel + gp[0x754]]`).
    LibraryMesh,
    /// Any other value: a move-VM node-mode sentinel (`0x1000` / `0x4000` /
    /// `0x8000`-class), not a library index.
    Sentinel,
}

/// One staged summon part.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SummonPart {
    /// File offset of the record this `FUN_80021B04` call passed as `a2`.
    pub record_off: usize,
    /// `record[+0]` mesh selector ([`MODEL_SEL_TRANSFORM_NODE`] = transform node).
    pub model_sel: i16,
    /// `record[+2]` flags word.
    pub flags: u16,
    /// File-offset range of the part's move-VM bytecode (`record+4` up to the
    /// next part's record, bounded by the data region end).
    pub bytecode: Range<usize>,
}

impl SummonPart {
    /// `true` when this part is a transform/pivot node (`model_sel == -1`).
    pub fn is_transform_node(&self) -> bool {
        self.model_sel == MODEL_SEL_TRANSFORM_NODE
    }

    /// Classify the record's first word ([`model_sel`](Self::model_sel)).
    pub fn kind(&self) -> SummonPartKind {
        if self.model_sel == MODEL_SEL_TRANSFORM_NODE {
            SummonPartKind::TransformNode
        } else if (0..LIBRARY_MESH_SEL_MAX).contains(&self.model_sel) {
            SummonPartKind::LibraryMesh
        } else {
            SummonPartKind::Sentinel
        }
    }
}

/// A parsed summon scene-graph: the ordered set of parts the overlay stages.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SummonOverlay {
    /// Link base used to map absolute record pointers to file offsets.
    pub link_base: u32,
    /// Number of `FUN_80021B04` call sites found (parts spawned, including any
    /// whose record pointer couldn't be statically resolved).
    pub spawn_sites: usize,
    /// The recovered part records, sorted by file offset.
    pub parts: Vec<SummonPart>,
}

fn rd_u32(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])
}

/// `jal <addr>` instruction word for a kseg0 target.
fn jal_word(addr: u32) -> u32 {
    0x0c00_0000 | ((addr >> 2) & 0x03ff_ffff)
}

/// Recover the `a2` register value at the `jal` at `site` by emulating the
/// `lui`/`addiu` writes to `$a2` (register 6) over the preceding window. Returns
/// `None` if `a2` is last written by a non-immediate op (`move`/`addu`/`lw`),
/// i.e. loaded from a saved register the static window can't see.
fn resolve_a2(b: &[u8], site: usize, window_insns: usize) -> Option<u32> {
    let start = site.saturating_sub(window_insns * 4);
    let mut a2: Option<u32> = None;
    let mut o = start;
    while o + 4 <= site {
        let w = rd_u32(b, o);
        let op = w >> 26;
        let rs = (w >> 21) & 31;
        let rt = (w >> 16) & 31;
        let imm = w & 0xffff;
        if rt == 6 {
            match op {
                0x0f => a2 = Some(imm << 16), // lui $a2, imm
                0x09 if rs == 6 => {
                    // addiu $a2, $a2, imm (sign-extended)
                    let s = if imm & 0x8000 != 0 {
                        imm as i32 - 0x1_0000
                    } else {
                        imm as i32
                    };
                    a2 = a2.map(|v| (v as i32).wrapping_add(s) as u32);
                }
                0x09 if rs == 0 => {
                    // addiu $a2, $zero, imm (li)
                    let s = if imm & 0x8000 != 0 {
                        imm as i32 - 0x1_0000
                    } else {
                        imm as i32
                    };
                    a2 = Some(s as u32);
                }
                _ => a2 = None, // move / addu / lw / ... -> unknown
            }
        }
        o += 4;
    }
    a2
}

/// Parse the summon part records out of a per-summon stager overlay's raw bytes
/// (e.g. PROT 0905), using `link_base` to map absolute record pointers to file
/// offsets.
///
/// Scans every `jal FUN_80021B04` call site, recovers the record pointer each
/// passes, keeps the ones that resolve in-file at or past the overlay's data
/// region, and bounds each record's move-VM bytecode by the next record start.
pub fn parse(bytes: &[u8], link_base: u32) -> SummonOverlay {
    let spawn = jal_word(SPAWN_HELPER);
    let mut sites = Vec::new();
    let mut o = 0usize;
    while o + 4 <= bytes.len() {
        if rd_u32(bytes, o) == spawn {
            sites.push(o);
        }
        o += 4;
    }

    // Recover each call site's record pointer, map to a file offset.
    let mut offs: Vec<usize> = Vec::new();
    for &s in &sites {
        if let Some(a2) = resolve_a2(bytes, s, 22) {
            let foff = a2.wrapping_sub(link_base) as usize;
            // Keep only pointers that land inside the file with room for a
            // record header; the spurious ones (a2 loaded from a saved reg the
            // window can't see) fall in the code region or out of range.
            if foff + 4 <= bytes.len() {
                offs.push(foff);
            }
        }
    }
    offs.sort_unstable();
    offs.dedup();

    // The records form a contiguous data region; the move-VM bytecode of each
    // runs up to the next record. Anchor the region at the first record that is
    // a transform node (`model_sel == -1`) — the dominant part kind — so a few
    // stray code-region pointers (resolved from a partial window) don't pull the
    // region start back into the overlay's code.
    let data_start = offs
        .iter()
        .copied()
        .find(|&f| f + 2 <= bytes.len() && i16::from_le_bytes([bytes[f], bytes[f + 1]]) == -1);

    let mut parts = Vec::new();
    if let Some(ds) = data_start {
        let in_region: Vec<usize> = offs.into_iter().filter(|&f| f >= ds).collect();
        for (i, &f) in in_region.iter().enumerate() {
            let model_sel = i16::from_le_bytes([bytes[f], bytes[f + 1]]);
            let flags = u16::from_le_bytes([bytes[f + 2], bytes[f + 3]]);
            let end = in_region.get(i + 1).copied().unwrap_or(bytes.len());
            parts.push(SummonPart {
                record_off: f,
                model_sel,
                flags,
                bytecode: (f + 4)..end.max(f + 4),
            });
        }
    }

    SummonOverlay {
        link_base,
        spawn_sites: sites.len(),
        parts,
    }
}

/// Parse summon-format part records at an explicit, caller-provided set of file
/// offsets, bounding each record's move-VM bytecode by the next offset in sorted
/// order. Use this when the record *locations* come from somewhere other than the
/// `jal FUN_80021B04` scan — notably the battle-action move-power effect-prototype
/// pointer table (`0x801f6324`), whose `0x01..=0x63` entries spawn records in this
/// exact format through the same stager (see
/// [`crate::move_power`] / `docs/formats/move-power.md`).
///
/// `offsets` are file offsets into `bytes`; out-of-range or sub-header offsets are
/// dropped. The returned parts are sorted by offset and deduped.
pub fn parse_records_at(bytes: &[u8], offsets: &[usize]) -> Vec<SummonPart> {
    let mut offs: Vec<usize> = offsets
        .iter()
        .copied()
        .filter(|&f| f + 4 <= bytes.len())
        .collect();
    offs.sort_unstable();
    offs.dedup();

    let mut parts = Vec::with_capacity(offs.len());
    for (i, &f) in offs.iter().enumerate() {
        let model_sel = i16::from_le_bytes([bytes[f], bytes[f + 1]]);
        let flags = u16::from_le_bytes([bytes[f + 2], bytes[f + 3]]);
        let end = offs.get(i + 1).copied().unwrap_or(bytes.len());
        parts.push(SummonPart {
            record_off: f,
            model_sel,
            flags,
            bytecode: (f + 4)..end.max(f + 4),
        });
    }
    parts
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jal_word_encodes_spawn_helper() {
        // jal 0x80021B04 -> 0x0C0086C1
        assert_eq!(jal_word(SPAWN_HELPER), 0x0c00_86c1);
    }

    #[test]
    fn resolve_a2_follows_lui_addiu() {
        // lui $a2, 0x8020 ; addiu $a2, $a2, -0x7dc4 ; jal (at +8)
        let mut b = vec![0u8; 12];
        b[0..4].copy_from_slice(&0x3c06_8020u32.to_le_bytes()); // lui $a2, 0x8020
        // addiu $a2, $a2, -0x7DC4  (imm low half = 0x823C): 0x09<<26 | rs6 | rt6 | imm
        let addiu = (0x09u32 << 26) | (6 << 21) | (6 << 16) | 0x823c;
        b[4..8].copy_from_slice(&addiu.to_le_bytes());
        let a2 = resolve_a2(&b, 8, 4).expect("a2 resolves");
        assert_eq!(a2, 0x801f_823c);
    }

    #[test]
    fn parse_synthetic_two_part_overlay() {
        // Build a tiny overlay: 2 spawn sites each loading a record pointer, then
        // a small data region with two `-1` records.
        let base = 0x801F_0000u32;
        let mut b = vec![0u8; 0x400];
        let rec0 = 0x300usize;
        let rec1 = 0x340usize;
        // record 0: model_sel=-1, flags=0, bytecode 0x13 ...
        b[rec0..rec0 + 2].copy_from_slice(&(-1i16).to_le_bytes());
        b[rec0 + 4] = 0x13;
        b[rec1..rec1 + 2].copy_from_slice(&(-1i16).to_le_bytes());
        b[rec1 + 4] = 0x13;
        // site 0 @ 0x10: lui/addiu a2 = base+rec0 ; jal at 0x18
        let put = |b: &mut [u8], o: usize, w: u32| b[o..o + 4].copy_from_slice(&w.to_le_bytes());
        let load_a2 = |b: &mut [u8], at: usize, addr: u32| {
            let hi = (addr >> 16) + ((addr >> 15) & 1); // round for sign-extension
            put(b, at, (0x0fu32 << 26) | (6 << 16) | (hi & 0xffff));
            let lo = (addr.wrapping_sub(hi << 16)) & 0xffff;
            put(b, at + 4, (0x09u32 << 26) | (6 << 21) | (6 << 16) | lo);
        };
        load_a2(&mut b, 0x10, base + rec0 as u32);
        put(&mut b, 0x18, jal_word(SPAWN_HELPER));
        load_a2(&mut b, 0x20, base + rec1 as u32);
        put(&mut b, 0x28, jal_word(SPAWN_HELPER));

        let ov = parse(&b, base);
        assert_eq!(ov.spawn_sites, 2);
        assert_eq!(ov.parts.len(), 2);
        assert_eq!(ov.parts[0].record_off, rec0);
        assert!(ov.parts[0].is_transform_node());
        assert_eq!(ov.parts[1].record_off, rec1);
        // first record's bytecode runs up to the second record
        assert_eq!(ov.parts[0].bytecode, (rec0 + 4)..rec1);
    }

    #[test]
    fn parse_records_at_bounds_each_record_by_the_next() {
        // Two packed records at explicit offsets (the move-power 0x801f6324 path).
        let mut b = vec![0u8; 0x80];
        let (r0, r1) = (0x20usize, 0x30usize);
        b[r0..r0 + 2].copy_from_slice(&2i16.to_le_bytes()); // model_sel = 2 (library)
        b[r0 + 4] = 0x0c; // some move-VM op
        b[r1..r1 + 2].copy_from_slice(&(-1i16).to_le_bytes()); // transform node
        // Offsets handed out of order + a duplicate + an out-of-range one.
        let parts = parse_records_at(&b, &[r1, r0, r0, 0x1000]);
        assert_eq!(parts.len(), 2, "sorted + deduped + range-filtered");
        assert_eq!(parts[0].record_off, r0);
        assert_eq!(parts[0].model_sel, 2);
        assert_eq!(parts[0].kind(), SummonPartKind::LibraryMesh);
        assert_eq!(
            parts[0].bytecode,
            (r0 + 4)..r1,
            "bytecode runs to next record"
        );
        assert_eq!(parts[1].record_off, r1);
        assert!(parts[1].is_transform_node());
        assert_eq!(parts[1].bytecode, (r1 + 4)..b.len(), "last runs to EOF");
    }
}
