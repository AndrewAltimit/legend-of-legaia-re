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
//! under the corrected loader index math - Gimard `0x81` arithmetics to 0903);
//! **PROT 0900** is a resident *transform / GTE-render* overlay that animates and
//! draws the spawned parts (`RotMatrixX/Y/Z` + prim emit) - it is the one byte-
//! resident in a mid-cast save state, *after* the stager has run. So the part
//! records live in the **stager file itself**, addressed by absolute pointers
//! (`lui 0x8020 / addiu`) that resolve in-file under the `0x801F69D8` link base.
//! (This corrects an earlier reading that placed the records "beyond the 0x5800
//! file" - that conflated the stager with the 900 render overlay's base.)
//!
//! ## Two spawn helpers, one record format
//!
//! Stagers reach `FUN_80021B04` two ways: directly (`jal 0x80021B04`), or
//! through the thin pool wrapper [`POOL_SPAWN_HELPER`] (`FUN_80050ED4`), which
//! finds the first free slot of the 0x60-pointer pool at `DAT_801C90F0`, calls
//! `FUN_80021B04` with the same `(world_pos, src_pos, record, 0x1000)`
//! arguments, and stores the returned actor pointer in that slot (see
//! `ghidra/scripts/funcs/80050ed4.txt`). The player stagers are mostly direct;
//! the high-summon block ([`HIGH_SUMMON_STAGER_PROT`]) and the enemy boss
//! stagers ([`ENEMY_BOSS_STAGER_PROT`]) are mostly pooled. [`parse`] scans both
//! call forms; the record format is identical.
//!
//! ## Trim to the TOC-gap footprint before parsing
//!
//! Stager PROT entries **overlap on disc**: each entry's TOC-indexed size
//! over-reads into the following entries, so an extraction `.BIN` is really
//! `[this stager][the next stagers' head bytes...]`. Only the first
//! `(next_start_lba - start_lba) * 0x800` bytes are the entry's own content
//! (compute with [`unique_content_len`]). Spawn sites in the over-read tail
//! belong to *neighbouring* stagers: their record pointers are valid only for
//! that neighbour's own load at the shared link base, so resolved against the
//! wrong file window they dereference unrelated bytes and yield garbage
//! first-words. The six Cort mid-cast save states pin the boundary: each
//! slot-B resident image matches its stager file byte-for-byte exactly up to
//! the TOC-gap footprint and diverges after it (stale bytes of the previous
//! occupant), e.g. `0x2000` for PROT 0961, `0x4000` for PROT 0966.
//!
//! ## Record first words: nodes, meshes, and the `0x4000` render-mode sentinel
//!
//! Across every trimmed stager in the corpus (player 0903..=0913, evolved-Seru
//! 0914..=0923, high 0927..=0934, enemy 0938/0940/0944/0961/0962/0966) the
//! record first word is one of exactly three things: `-1` (transform node - the
//! dominant kind), a small library-mesh index (`< 0x20` observed), or
//! **`0x4000`** (render-mode nodes, in five stagers: the Sim-Seru trio
//! 0928/0929/0931 and the evolved-Seru casts 0916/0921). The historical
//! `0x1000` /
//! `0x8000`-class "sentinel" census was the over-read artifact above and
//! dissolves under trimming. This matches the spawn helper's own dispatch
//! (`FUN_80021B04`, `ghidra/scripts/funcs/80021b04.txt`): `model_sel < 0` →
//! no-mesh transform path (`actor[+0x56] = 0`, `actor[+0x5A] = 0`, draw-flag
//! bit 2 set), `== 0x4000` → render-mode node `actor[+0x5A] = 3`, `== 0x4001`
//! → render-mode node `actor[+0x5A] = 5` (special-cased by the helper but
//! unobserved in the trimmed corpus), any other non-negative value → library
//! mesh `DAT_8007C018[model_sel + gp[0x754]]` (`actor[+0x5A] = 1`).
//!
//! In the live Cort captures every pooled part-actor's `actor[+0x48]` record
//! pointer lands inside the trimmed record table and points at a `-1` record
//! (RAM first word == file first word); the spawn-time `+0x56`/`+0x5A` zeros
//! evolve post-spawn (`+0x56 = 4` / `+0x5A = 2` dominate mid-cast - the
//! move-VM anim-bank ops rebind the render mode after `FUN_80021B04` seats the
//! actor), with `actor[+0x64] = 0` throughout.
//!
//! Recovery is by scanning the stager's `jal FUN_80021B04` / `jal FUN_80050ED4`
//! call sites and recovering the `a2` (record-pointer) each one passes - the
//! records are variable-length move-VM bytecode, not a fixed-stride table, so
//! the call sites are the authoritative enumeration. The `a2` register is
//! followed with a tiny `lui`/`addiu` emulator over each call site's preceding
//! window.
//!
//! ## Where the effect magnitude lives (per-spell "power")
//!
//! This parser recovers the summon's *visual* scene-graph. The summon's combat
//! **effect** (the HP delta) is applied by the same stager function that spawns
//! the parts - each stager file carries exactly one `actor+0x14c` (HP) writer,
//! and the stager files split cleanly: **damage** stagers (files 0904/0912/0914, plus
//! 0915's second arm) compute the amount via the shared battle kernel
//! `FUN_801dd0ac` (`a0` = a per-summon move-type constant `0x10..0x12`, `a1 = 7`,
//! `a2` = target) and store `HP -= amount`; **heal** summons (PROT
//! 0903/0905/0910/0911/0913, plus 0915's first arm - file-content classes; which
//! spell id loads which file is re-pinned by the corrected loader math) apply `(power_byte<<5)+0xe0`
//! inline (`power_byte` from a `0x80084140`-based per-character table searched by
//! the cast spell-id `actor+0x1df`) and store `HP += amount`. For the summon path
//! (`FUN_801dd0ac` `attacker_slot == 7`) the roll is built from the attacker's
//! AGL (`+0x168`) and HP (`+0x14c`) plus the caster's AGL - i.e. **caster/summon
//! battle-state-derived, not a static per-spell scalar** (which is why no static
//! magic-power table exists). The render JT at `0x801F69D8` (PROT 0900) is
//! animation/GPU only and writes no HP. See `docs/formats/spell-table.md` and the
//! `FUN_801dd0ac` row in `docs/reference/functions.md`.

use std::ops::Range;

/// SCUS actor-spawn helper `FUN_80021B04`. Each summon part is one call.
pub const SPAWN_HELPER: u32 = 0x8002_1B04;

/// SCUS pool-tracked spawn wrapper `FUN_80050ED4`: stores the first free slot
/// of the 0x60-pointer pool at `DAT_801C90F0` and forwards `(world_pos,
/// src_pos, record, 0x1000)` unchanged to [`SPAWN_HELPER`]. The dominant call
/// form in the high-summon and enemy boss stagers; [`parse`] scans it
/// alongside the direct calls. `see ghidra/scripts/funcs/80050ed4.txt`.
pub const POOL_SPAWN_HELPER: u32 = 0x8005_0ED4;

/// CDNAME (`xxx_dat`) PROT entries that the retail summon path arithmetics over
/// for the player Seru-magic block `spell_id 0x81..=0x8B`: `FUN_8003EC70(id -
/// 0x79)` resolves `FUN_8003E8A8(id - 0x79 + 0x381)` against the raw in-RAM
/// TOC, which in extraction index space is entry `(id - 0x81) + 903`, i.e.
/// `903..=913` (Gimard *Tail Fire* = `0x81` → 903). The historical `905..=915`
/// range (Gimard → 905) carried the loader-math off-by-2 - the resolver indexes
/// the raw `PROT.DAT` head, 2 entries above extraction indexing (see
/// `docs/formats/prot.md` § In-RAM TOC).
///
/// **The whole block is capture-pinned**: every spell id `0x81..=0x8B` was
/// observed mid-cast loading its arithmetic slot (loader-B current id at
/// `0x8007BC4C`), with zero exceptions. The `0x82..=0x8B` legs carry a committed
/// regression oracle - one mid-cast state each byte-pins the loader-B id and the
/// slot-B-resident stager (disc+library-gated `summon_binding_base_high`); the
/// `0x81` Gimard leg is PCSX-side. PROT 0907 (the spell-`0x85` slot) is
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

/// Evolved-Seru player cast block (`spell_id 0x8C..=0x95`), the contiguous
/// continuation of [`PLAYER_SUMMON_STAGER_PROT`] under the *same* linear loader
/// arithmetic: `extraction = (id - 0x81) + 903`, so `0x8C → 914 .. 0x95 → 923`.
/// Each entry, trimmed to its TOC-gap footprint ([`unique_content_len`]), parses
/// as a move-VM stager (4..67 spawn sites, non-trivial scene-graphs) - the same
/// structure the base, high, and enemy blocks carry. This pins the *structural*
/// half statically: the evolved-Seru casts ride the stager mechanism, not the
/// resident `0900` move-FX module. Several legs are now capture-pinned too - a
/// mid-cast state per cast holds loader-B id `spell − 0x79` with the stager
/// 100% byte-resident at slot B (`evolved_summon_binding`); the remaining legs
/// ride the same bracketed run (`0x8B → 913` and `0x99 → 927` bracket the gap).
///
/// **Two entries carry `0x4000` render-mode nodes** ([`RENDER_NODE_MODE_A`]) -
/// `0x8E` → 916 (4 records) and `0x93` → 921 (6) - the only such records found
/// outside the Sim-Seru high stagers (0928/0929/0931). Pinned by the disc-gated
/// `summon_overlay_block` sweep.
pub const EVOLVED_SUMMON_STAGER_PROT: std::ops::RangeInclusive<u32> = 914..=923;

/// High-summon (evil-Seru creature) stager block, capture-pinned: action ids
/// `0x99..=0xA0` (Juggernaut / Palma / Mule / Horn / Jedo / Meta / Terra /
/// Ozma) load extraction PROT 0927..=0934 through the same loader-B path. All
/// eight legs carry a committed regression oracle (one mid-cast state each
/// byte-pins the loader-B id + slot-B-resident stager; disc+library-gated
/// `summon_binding_base_high`), including the `0x4000`-node carriers Palma 0928
/// / Mule 0929 / Jedo 0931.
pub const HIGH_SUMMON_STAGER_PROT: std::ops::RangeInclusive<u32> = 927..=934;

/// Enemy boss (final-boss Cort) special-attack stagers, capture-pinned by the
/// six catalogued mid-cast save states (`cort_*_mid_cast` in
/// `scripts/scenarios.toml`): the loader-B current id at `0x8007BC4C` resolves
/// `extraction = 895 + id` - Mystic Circle `0x2B` → 0938, Mystic Shield `0x2D`
/// → 0940, Guilty Cross `0x31` → 0944, evolved-form Final Crisis `0x42` → 0961
/// and Ultra Charge `0x43` → 0962, Evil Seru Magic `0x47` → 0966 (distinct
/// from the player-side Juggernaut stager 0927). Each parses as a stager
/// under [`SUMMON_OVERLAY_LINK_BASE`] once trimmed via [`unique_content_len`];
/// pinned by the disc-gated `enemy_stager_real` test.
pub const ENEMY_BOSS_STAGER_PROT: [u32; 6] = [938, 940, 944, 961, 962, 966];

/// Upper bound (exclusive) on a `model_sel` that indexes the small effect-model
/// library (`DAT_8007C018[model_sel + gp[0x754]]`; ~30 entries). A `model_sel`
/// at or above this is not a plain library index - across the trimmed corpus
/// the only such values are the [`RENDER_NODE_MODE_A`] (`0x4000`) records
/// (`FUN_80021B04` also special-cases `0x4001`, unobserved on disc). Any other
/// out-of-band first word indicates the input wasn't trimmed to its TOC-gap
/// footprint (see [`unique_content_len`]). Mirrors
/// `engine_core::summon::MAX_MESH_SEL`.
pub const LIBRARY_MESH_SEL_MAX: i16 = 0x100;

/// `model_sel == 0x4000`: a special render-mode node - `FUN_80021B04` seats the
/// part-actor with `actor[+0x5A] = 3`, `actor[+0x56] = 0`, draw-flag bit 2.
pub const RENDER_NODE_MODE_A: i16 = 0x4000;

/// `model_sel == 0x4001`: the second special render-mode node
/// (`actor[+0x5A] = 5`). Handled by `FUN_80021B04` but unobserved in the
/// trimmed stager corpus.
pub const RENDER_NODE_MODE_B: i16 = 0x4001;

/// Unique-content length of a stager PROT entry: the byte distance to the next
/// TOC entry's start LBA, capped at the extraction footprint. Stager entries'
/// indexed sizes over-read into their neighbours (the extraction `.BIN`s
/// overlap on disc), so [`parse`] input must be trimmed to this length - the
/// over-read tail is the *next* stagers' content, whose spawn-site record
/// pointers are only valid for their own load at the shared link base.
/// Capture-pinned: each Cort mid-cast save's slot-B resident image matches the
/// stager file exactly up to this boundary and diverges after it.
pub fn unique_content_len(file_len: usize, start_lba: u32, next_start_lba: u32) -> usize {
    let gap_bytes = (next_start_lba.saturating_sub(start_lba) as usize) * 0x800;
    if gap_bytes == 0 {
        return file_len;
    }
    file_len.min(gap_bytes)
}

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
/// On a stager trimmed to its TOC-gap footprint ([`unique_content_len`]) the
/// corpus carries exactly three first words: `-1` ([`Self::TransformNode`],
/// dominant), small library indices ([`Self::LibraryMesh`]), and `0x4000`
/// ([`Self::Sentinel`] - the `FUN_80021B04` render-mode-3 node;
/// [`RENDER_NODE_MODE_A`]). Any *other* [`Self::Sentinel`] value is the
/// signature of an untrimmed over-read window - the record offset belongs to a
/// neighbouring stager's load and dereferences unrelated bytes here (the
/// historical `0x1000` / `0x8000`-class "sentinel" census was exactly this).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SummonPartKind {
    /// `model_sel == -1`: a transform/pivot node; the mesh (if any) is bound by
    /// the move-VM anim-bank ops, not the record header.
    TransformNode,
    /// `0 <= model_sel < `[`LIBRARY_MESH_SEL_MAX`]: a plain effect-model-library
    /// index (`DAT_8007C018[model_sel + gp[0x754]]`).
    LibraryMesh,
    /// `0x4000`/`0x4001` render-mode nodes ([`RENDER_NODE_MODE_A`] /
    /// [`RENDER_NODE_MODE_B`]) - or, for any other value, an over-read artifact
    /// (see the enum-level docs).
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

    /// `true` when this part is one of the two `FUN_80021B04` special
    /// render-mode nodes (`model_sel == 0x4000` → `actor[+0x5A] = 3`,
    /// `0x4001` → `actor[+0x5A] = 5`).
    pub fn is_render_mode_node(&self) -> bool {
        self.model_sel == RENDER_NODE_MODE_A || self.model_sel == RENDER_NODE_MODE_B
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
    /// Number of `FUN_80021B04` + `FUN_80050ED4` call sites found (parts
    /// spawned, including any whose record pointer couldn't be statically
    /// resolved).
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
/// offsets. **Trim the input to its TOC-gap footprint first**
/// ([`unique_content_len`]) - the extraction `.BIN`s over-read into the
/// following stagers, and the over-read tail's spawn sites resolve record
/// pointers that are only valid for the neighbour's own load.
///
/// Scans every `jal FUN_80021B04` and `jal FUN_80050ED4` (pool-wrapper) call
/// site, recovers the record pointer each passes, keeps the ones that resolve
/// in-file at or past the overlay's data region, and bounds each record's
/// move-VM bytecode by the next record start.
pub fn parse(bytes: &[u8], link_base: u32) -> SummonOverlay {
    let spawn = jal_word(SPAWN_HELPER);
    let pooled = jal_word(POOL_SPAWN_HELPER);
    let mut sites = Vec::new();
    let mut o = 0usize;
    while o + 4 <= bytes.len() {
        let w = rd_u32(bytes, o);
        if w == spawn || w == pooled {
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
    // a transform node (`model_sel == -1`) - the dominant part kind - so a few
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
/// `jal FUN_80021B04` scan - notably the battle-action move-power effect-prototype
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
        // jal 0x80050ED4 -> 0x0C0143B5
        assert_eq!(jal_word(POOL_SPAWN_HELPER), 0x0c01_43b5);
    }

    #[test]
    fn unique_content_len_trims_to_toc_gap() {
        // PROT 0961: file 0x7800, LBA 47579, next entry (0962) at 47583 ->
        // 4 sectors of unique content.
        assert_eq!(unique_content_len(0x7800, 47579, 47583), 0x2000);
        // Entry without a following over-read (gap covers the whole file).
        assert_eq!(unique_content_len(0x1800, 100, 200), 0x1800);
        // Degenerate (no next entry): keep the file as-is.
        assert_eq!(unique_content_len(0x1800, 100, 0), 0x1800);
    }

    #[test]
    fn parse_scans_pooled_spawn_sites_too() {
        // Same shape as `parse_synthetic_two_part_overlay`, but the second
        // part is spawned through the FUN_80050ED4 pool wrapper.
        let base = 0x801F_0000u32;
        let mut b = vec![0u8; 0x400];
        let (rec0, rec1) = (0x300usize, 0x340usize);
        b[rec0..rec0 + 2].copy_from_slice(&(-1i16).to_le_bytes());
        b[rec1..rec1 + 2].copy_from_slice(&(-1i16).to_le_bytes());
        let put = |b: &mut [u8], o: usize, w: u32| b[o..o + 4].copy_from_slice(&w.to_le_bytes());
        let load_a2 = |b: &mut [u8], at: usize, addr: u32| {
            let hi = (addr >> 16) + ((addr >> 15) & 1);
            put(b, at, (0x0fu32 << 26) | (6 << 16) | (hi & 0xffff));
            let lo = (addr.wrapping_sub(hi << 16)) & 0xffff;
            put(b, at + 4, (0x09u32 << 26) | (6 << 21) | (6 << 16) | lo);
        };
        load_a2(&mut b, 0x10, base + rec0 as u32);
        put(&mut b, 0x18, jal_word(SPAWN_HELPER));
        load_a2(&mut b, 0x20, base + rec1 as u32);
        put(&mut b, 0x28, jal_word(POOL_SPAWN_HELPER));

        let ov = parse(&b, base);
        assert_eq!(ov.spawn_sites, 2, "both call forms are spawn sites");
        assert_eq!(ov.parts.len(), 2);
        assert_eq!(ov.parts[1].record_off, rec1);
    }

    #[test]
    fn render_mode_node_classifies_as_sentinel() {
        let p = SummonPart {
            record_off: 0,
            model_sel: RENDER_NODE_MODE_A,
            flags: 0,
            bytecode: 4..8,
        };
        assert!(p.is_render_mode_node());
        assert_eq!(p.kind(), SummonPartKind::Sentinel);
        let q = SummonPart {
            model_sel: RENDER_NODE_MODE_B,
            ..p.clone()
        };
        assert!(q.is_render_mode_node());
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
