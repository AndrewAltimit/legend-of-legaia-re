//! MAN partition record-offset helpers + narration / g-flag site collection.
//!
//! Extracted verbatim from `man_field_scripts.rs`.

use std::collections::BTreeMap;

use crate::scene::{ProtIndex, Scene};

use super::*;

/// Which flag bank a [`GFlagSite`] touches.
///
/// The two banks are distinct id spaces, so census consumers must not merge
/// them: a scratchpad bit `26` and a system flag `26` are unrelated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlagBank {
    /// The 32-bit scratchpad story-flag word `_DAT_1F800394` (the engine's
    /// [`crate::world::World::story_flags`]); reached by opcodes `0x2E`
    /// (`SET`) / `0x2F` (`CLEAR`). Flag numbers are bit indices `0..31`.
    Scratchpad,
    /// The wide SYSTEM-flag bitmap reached by the `0x50..=0x7F` op family
    /// (`0x5x` SET, `0x6x` CLEAR, `0x7x` TEST). The flag number is a `u16`
    /// (`(lead & 0x8F) << 8 | operand`); the engine's bit helpers live at
    /// [`crate::world::World::system_flag_set`] /
    /// [`crate::world::World::system_flag_test`]. This is the id space of the
    /// overworld progress gates (e.g. `0x193` / `0x482` / `0x2FC`).
    System,
}

/// One field-VM **flag write / test** found while walking a MAN partition's
/// records as field-VM scripts. Covers both the scratchpad global-flag ops
/// (`GFLAG_SET` `0x2E` / `GFLAG_CLEAR` `0x2F`) and the wide SYSTEM-flag ops
/// (`0x50..=0x7F`), annotated with the bank + flag number it touches.
///
/// The opening prologue's `opdeene` cutscene-timeline record ends with a
/// scratchpad `GFLAG_SET 26`, the write the `town01` hand-off gate
/// (`FUN_801D1344`) waits on - see [`crate::world::PROLOGUE_HANDOFF_FLAG`].
/// SYSTEM-flag setters (the overworld progress gates) typically live in a
/// *different* scene's MAN than the one that gates on them, which is what the
/// disc-wide [`system_flag_census`] surfaces.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GFlagSite {
    /// Absolute byte offset of the flag opcode in the MAN buffer.
    pub abs_pc: usize,
    /// Partition the carrying record lives in (`0..3`).
    pub partition: usize,
    /// Record index within the partition.
    pub record: usize,
    /// The opcode byte (scratchpad `0x2E`/`0x2F`, or a `0x50..=0x7F` system op).
    pub opcode: u8,
    /// `true` iff this is a SET op (scratchpad `0x2E` or system `0x5x`).
    /// `false` for CLEAR **and** TEST - use [`GFlagSite::kind`] to tell those
    /// apart. Kept for the prologue-arm consumers that only care about SET.
    pub set: bool,
    /// SET / CLEAR / TEST discriminator (carries TEST, which `set` cannot).
    pub kind: FlagKind,
    /// Which bank the op targets.
    pub bank: FlagBank,
    /// Low byte of the flag number. For [`FlagBank::Scratchpad`] this is the
    /// full bit index (`0..31`); for [`FlagBank::System`] it is truncated -
    /// use [`GFlagSite::flag`] for the full number.
    pub bit: u8,
    /// The full flag number: scratchpad bit index, or the `u16` system flag id.
    pub flag: u16,
}

/// The script `script_start` of `partition`'s record `index`, computed from
/// the partition's u24 record-offset table against the MAN data region.
/// `None` when the partition or index is out of range or the offset lands
/// past the buffer.
pub(crate) fn partition_record_offset(
    man_file: &ManFile,
    man_len: usize,
    partition: usize,
    index: usize,
) -> Option<usize> {
    let off = *man_file.partitions.get(partition)?.get(index)? as usize;
    let abs = man_file.data_region_offset.checked_add(off)?;
    (abs < man_len).then_some(abs)
}

/// First-opcode offset of a **partition-2 named-record** (the cutscene-timeline
/// records), relative to the record start in `body`.
///
/// Partition-2 records are not the partition-1 `[u8 N][N*2 locals][4-byte
/// header]` shape - they open with a Shift-JIS **name** and three
/// condition-list gates that the dispatcher `FUN_8003BDE0` walks before the
/// script proper:
///
/// ```text
/// [u8 name_len]                 ; name length in CHARACTERS
/// [name_len * 2 bytes]          ; SJIS name (no separate terminator)
/// [u8 C0][C0 bytes]             ; cond-block 0 (byte-granular; skipped)
/// [u8 C1][C1 * u16]             ; cond-block 1 (story-flag OR gate)
/// [u8 C2][C2 * u16]             ; cond-block 2 (story-flag AND gate)
/// <script…>                     ; first field-VM opcode
/// ```
///
/// So the entry offset is `1 + name_len*2 + (1+C0) + (1+C1*2) + (1+C2*2)`.
/// Returns `None` if a count byte lies past the record body. For `opdeene`'s
/// record 18 (`name_len=6` "Opening", all three blocks empty) this is `0x10`,
/// the `0x34` EFFECT op that opens the prologue timeline.
// REF: FUN_8003BDE0
pub(crate) fn partition2_record_script_offset(body: &[u8]) -> Option<usize> {
    let name_len = *body.first()? as usize;
    let mut cur = 1 + name_len * 2; // name field (chars * 2, no terminator)
    let c0 = *body.get(cur)? as usize;
    cur += 1 + c0; // cond-block 0: 1 byte per unit
    let c1 = *body.get(cur)? as usize;
    cur += 1 + c1 * 2; // cond-block 1: u16 per unit
    let c2 = *body.get(cur)? as usize;
    cur += 1 + c2 * 2; // cond-block 2: u16 per unit
    Some(cur)
}

/// The C1 / C2 story-flag gate lists of a **partition-2 named-record**
/// (see [`partition2_record_script_offset`] for the header shape).
///
/// Retail's record dispatcher `FUN_8003BDE0` tests each listed flag against
/// the story-flag bitmap at `DAT_80085758` (`bit = byte[flag >> 3] &
/// (0x80 >> (flag & 7))`): **C1 blocks the spawn if ANY listed flag is set**
/// (the one-shot mechanism - e.g. `town01`'s opening record lists `0x225`,
/// set once the opening has played); **C2 requires ALL listed flags set**.
/// Returns `None` when the header overruns the record body.
// REF: FUN_8003BDE0
pub fn partition2_record_gates(
    man_file: &ManFile,
    man: &[u8],
    index: usize,
) -> Option<(Vec<u16>, Vec<u16>)> {
    let script_start = partition_record_offset(man_file, man.len(), 2, index)?;
    let end = record_end_bound(man_file, man.len(), script_start);
    let body = man.get(script_start..end)?;
    let name_len = *body.first()? as usize;
    let mut cur = 1 + name_len * 2;
    let c0 = *body.get(cur)? as usize;
    cur += 1 + c0;
    let read_u16_list = |body: &[u8], cur: &mut usize| -> Option<Vec<u16>> {
        let n = *body.get(*cur)? as usize;
        *cur += 1;
        let mut out = Vec::with_capacity(n);
        for _ in 0..n {
            let lo = *body.get(*cur)?;
            let hi = *body.get(*cur + 1)?;
            *cur += 2;
            out.push(u16::from_le_bytes([lo, hi]));
        }
        Some(out)
    };
    let c1 = read_u16_list(body, &mut cur)?;
    let c2 = read_u16_list(body, &mut cur)?;
    Some((c1, c2))
}

/// The byte span of `partition`'s record `index` as a field-VM script:
/// `(script_start, pc0, body_len)`, where `script_start` is the absolute
/// MAN offset of the record, `pc0` the first-opcode offset relative to it,
/// and `body_len` the bounded body length (clamped so the walk does not spill
/// into the next record or a sibling section).
///
/// The header shape is partition-specific: partition 2 (the cutscene-timeline
/// records) uses the named-record header decoded by
/// `partition2_record_script_offset` (`FUN_8003BDE0`); the other partitions
/// use the `[u8 N][N*2 locals][4-byte header]` prefix (`pc0 = 1 + N*2 + 4`).
///
/// `None` when the partition / index is out of range, the offset lands past
/// the buffer, or the record's header already overruns its bound.
pub fn partition_record_span(
    man_file: &ManFile,
    man: &[u8],
    partition: usize,
    index: usize,
) -> Option<(usize, usize, usize)> {
    let script_start = partition_record_offset(man_file, man.len(), partition, index)?;
    let end = record_end_bound(man_file, man.len(), script_start);
    let body = man.get(script_start..end)?;
    let pc0 = if partition == 2 {
        partition2_record_script_offset(body)?
    } else {
        let n = *body.first().unwrap_or(&0) as usize;
        1 + n * 2 + 4
    };
    if script_start + pc0 >= end {
        return None;
    }
    Some((script_start, pc0, end - script_start))
}

/// Collect every inline cutscene-narration page in `partition`'s records, in
/// record-then-page order.
///
/// Each record's bounded body is handed to
/// [`legaia_asset::cutscene_text::parse_narration`], which finds the narration
/// op + `0x1F`/`0x00` page framing structurally. The opening prologue scene
/// (`opdeene`) carries its narration in the cutscene-timeline partition
/// (partition 2); this returns those subtitle pages as plain text for the
/// runtime presenter ([`crate::cutscene_narration::CutsceneNarration`]).
pub fn collect_partition_narration(
    man_file: &ManFile,
    man: &[u8],
    partition: usize,
) -> Vec<String> {
    let count = man_file
        .header
        .partition_counts
        .get(partition)
        .copied()
        .unwrap_or(0)
        .max(0) as usize;
    let mut pages = Vec::new();
    for index in 0..count {
        let Some((script_start, _pc0, body_len)) =
            partition_record_span(man_file, man, partition, index)
        else {
            continue;
        };
        let body = &man[script_start..script_start + body_len];
        for block in legaia_asset::cutscene_text::parse_narration(body) {
            pages.extend(block.pages.into_iter().map(|p| p.text));
        }
    }
    pages
}

/// Walk every record of `partition` (`0..3`) as a field-VM script and
/// collect its flag write/test sites: the scratchpad global-flag ops
/// (`GFLAG_SET` `0x2E` / `GFLAG_CLEAR` `0x2F`) **and** the wide SYSTEM-flag
/// ops (`0x50..=0x7F`, SET/CLEAR/TEST). Each site is tagged with its
/// [`FlagBank`] and full flag number, so callers can tell a scratchpad bit
/// from a system flag that share a low byte.
///
/// This is the partition-agnostic companion to [`walk_partition1_scripts`]:
/// the encounter hunt cares about partition 1's yield sites, the opening
/// prologue cares about partition 2's cutscene-timeline `GFLAG_SET`, and the
/// overworld progress-gate hunt cares about SYSTEM-flag setters across every
/// partition. All share the same `[u8 N][N*2 locals][4-byte header]` record
/// prefix and the same opcode-aware [`LinearWalker`] decode, so a site is
/// reported only at a real instruction boundary - not at an operand / SJIS
/// byte that happens to equal a flag opcode.
///
/// Prologue-arm consumers filter on `s.set && s.bit == 26` over the scratchpad
/// bank; TEST sites (`set == false`) and system-bank sites are ignored by that
/// filter, so the extra sites are additive.
pub fn walk_partition_gflag_sites(
    man_file: &ManFile,
    man: &[u8],
    partition: usize,
) -> Vec<GFlagSite> {
    let count = man_file
        .header
        .partition_counts
        .get(partition)
        .copied()
        .unwrap_or(0)
        .max(0) as usize;
    let mut out = Vec::new();
    for index in 0..count {
        let Some(script_start) = partition_record_offset(man_file, man.len(), partition, index)
        else {
            continue;
        };
        let n = *man.get(script_start).unwrap_or(&0) as usize;
        let pc0 = 1 + n * 2 + 4;
        let end = record_end_bound(man_file, man.len(), script_start);
        if script_start + pc0 >= end {
            continue;
        }
        let body = &man[script_start..end];
        for insn in LinearWalker::new(body, pc0).flatten() {
            match insn.info {
                // Scratchpad global flag (`0x2E` set / `0x2F` clear). The VM
                // has no scratchpad TEST op reaching this variant, but guard
                // anyway so `set`/`kind` stay coherent.
                InsnInfo::GFlag { kind, bit } => out.push(GFlagSite {
                    abs_pc: script_start + insn.pc,
                    partition,
                    record: index,
                    opcode: insn.opcode,
                    set: kind == FlagKind::Set,
                    kind,
                    bank: FlagBank::Scratchpad,
                    bit,
                    flag: u16::from(bit),
                }),
                // Wide SYSTEM-flag bank (`0x5x` set / `0x6x` clear / `0x7x`
                // test). `idx` is the full `u16` flag number; `bit` keeps the
                // low byte for the scratchpad-shaped consumers.
                InsnInfo::SystemFlag { kind, idx, .. } => out.push(GFlagSite {
                    abs_pc: script_start + insn.pc,
                    partition,
                    record: index,
                    opcode: insn.opcode,
                    set: kind == FlagKind::Set,
                    kind,
                    bank: FlagBank::System,
                    bit: (idx & 0xFF) as u8,
                    flag: idx,
                }),
                _ => {}
            }
        }
    }
    out
}

/// One SYSTEM-flag site recovered by [`system_flag_census`], carrying the
/// scene it lives in plus the partition/record/op that touches the flag.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlagCensusSite {
    /// CDNAME scene name whose MAN carries the op.
    pub scene_name: String,
    /// Partition the carrying record lives in (`0..3`).
    pub partition: usize,
    /// Record index within the partition.
    pub record: usize,
    /// The opcode byte (a `0x50..=0x7F` system op).
    pub opcode: u8,
    /// SET / CLEAR / TEST discriminator.
    pub kind: FlagKind,
}

/// Disc-wide SYSTEM-flag census: walk every scene's MAN across all three
/// partitions and map each SYSTEM flag number to the list of sites (scene +
/// partition + record + op + kind) that set / clear / test it.
///
/// This is the tool the overworld progress-gate RE needs: a gate like
/// `system_flag_test(0x193)` lives in one scene, but the *setter* that opens
/// it almost always lives in a different scene's MAN. Only the SYSTEM bank
/// (`0x50..=0x7F` ops) is reported - the scratchpad bank is a separate 32-bit
/// id space with its own tooling ([`walk_partition_gflag_sites`]).
///
/// Scenes that fail to load or have no MAN are skipped silently (the census is
/// best-effort over the whole CDNAME scene set). The returned map is sorted by
/// flag number; each flag's site list preserves scene / partition / record
/// discovery order.
pub fn system_flag_census<I, S>(index: &ProtIndex, scenes: I) -> BTreeMap<u16, Vec<FlagCensusSite>>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut out: BTreeMap<u16, Vec<FlagCensusSite>> = BTreeMap::new();
    for name in scenes {
        let name = name.as_ref();
        let Ok(scene) = Scene::load(index, name) else {
            continue;
        };
        let Ok(Some(man)) = scene.field_man_payload(index) else {
            continue;
        };
        let Ok(man_file) = legaia_asset::man_section::parse(&man) else {
            continue;
        };
        for partition in 0..3 {
            for site in walk_partition_gflag_sites(&man_file, &man, partition) {
                if site.bank != FlagBank::System {
                    continue;
                }
                out.entry(site.flag).or_default().push(FlagCensusSite {
                    scene_name: name.to_string(),
                    partition: site.partition,
                    record: site.record,
                    opcode: site.opcode,
                    kind: site.kind,
                });
            }
        }
    }
    out
}

/// One field-VM op-`0x49` (`STATE_RESUME`) site recovered by
/// [`op49_window_census`], with its operand bytes interpreted under the
/// **flag-window descriptor** layout the field-overlay picker widget
/// `FUN_801EF014` consumes through `_DAT_8007B450` (system-actor handler id
/// `0x23` in the `PTR_FUN_801f33b4` table, dispatcher `FUN_801F159C`):
///
/// ```text
/// [0]    opcode 0x49          (descriptor pointer targets byte [1])
/// [1]    sub-op
/// [2]    count               ; window width in flags (`+1` from _DAT_8007B450)
/// [3]    default_index       ; state-0 fallback selection (`+2`)
/// [4]    rows                ; widget row geometry (`+3`)
/// [5..6] base_flag (u16 LE)  ; first flag of the window (`+4..5`, read via
///                            ; the u16 loader FUN_8003CE9C)
/// ```
///
/// The picker's writes land on `DAT_80085758` system flags
/// `base_flag + i` for `i` in `0..count` (state-0 window CLEAR loop via
/// `FUN_8003CE34`) plus `base_flag + default_index` (the state-0 fallback
/// `_DAT_8007BB88` seed the state-1 confirm SET `FUN_8003CE08` can land on) -
/// so a site's covered flag set is `[base, base+count) ∪ {base+default}`.
///
/// Every op-`0x49` site is reported regardless of sub-op: which sub-op arms
/// handler `0x23` is runtime state (`actor+0x50`), so the census interprets
/// the descriptor window at **all** sub-ops as the conservative superset.
/// `in_footprint` records whether the 6 descriptor bytes lie inside the
/// instruction's own decoded operand footprint (sub-ops narrower than 5
/// operand bytes would make the picker read into the following instruction's
/// bytes - still resident MAN bytes at runtime, so they are interpreted too).
// REF: FUN_801EF014
// REF: FUN_801F159C
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Op49WindowSite {
    /// CDNAME scene name whose MAN carries the op.
    pub scene_name: String,
    /// Partition the carrying record lives in.
    pub partition: usize,
    /// Record index within the partition.
    pub record: usize,
    /// Absolute byte offset of the `0x49` opcode in the MAN buffer.
    pub abs_pc: usize,
    /// The sub-op byte (`_DAT_8007B450` target).
    pub sub_op: u8,
    /// Descriptor `+1`: window width in flags.
    pub count: u8,
    /// Descriptor `+2`: default selection index.
    pub default_index: u8,
    /// Descriptor `+3`: widget row-geometry byte.
    pub rows: u8,
    /// Descriptor `+4..5` (u16 LE): first flag id of the window.
    pub base_flag: u16,
    /// `true` iff all 6 descriptor bytes sit inside the instruction's own
    /// decoded footprint (see the type docs).
    pub in_footprint: bool,
}

impl Op49WindowSite {
    /// The window's inclusive flag span `[base, base+count-1]`, or `None`
    /// when `count == 0` (the CLEAR loop never runs; only the
    /// `base + default_index` fallback remains reachable).
    pub fn window(&self) -> Option<(u32, u32)> {
        (self.count > 0).then(|| {
            let base = u32::from(self.base_flag);
            (base, base + u32::from(self.count) - 1)
        })
    }

    /// `true` iff `flag` is in the site's covered flag set
    /// `[base, base+count) ∪ {base + default_index}`.
    pub fn covers(&self, flag: u16) -> bool {
        self.min_distance(flag) == 0
    }

    /// Minimum absolute distance from `flag` to the site's covered flag set
    /// (`0` = contained). Computed in `u32` so `base + count` cannot wrap.
    pub fn min_distance(&self, flag: u16) -> u32 {
        let flag = u32::from(flag);
        let base = u32::from(self.base_flag);
        let fallback = base + u32::from(self.default_index);
        let mut best = flag.abs_diff(fallback);
        if let Some((lo, hi)) = self.window() {
            // Distance to the inclusive span [lo, hi]: 0 when inside.
            let d = if flag < lo {
                lo - flag
            } else {
                flag.saturating_sub(hi)
            };
            best = best.min(d);
        }
        best
    }
}

/// Disc-wide **op-`0x49` flag-window census**: walk every scene MAN's
/// field-VM bytecode (every partition record, decoded with the opcode-aware
/// [`LinearWalker`] - real instruction boundaries, not raw byte pairs) and
/// report every op-`0x49` site with its operand window interpreted under the
/// [`Op49WindowSite`] flag-window descriptor layout.
///
/// This is the residual static probe for the spine flags whose writers are
/// corpus-negative as LITERAL operands (`0x142` dolk-clear / `0x482` Drake
/// mist-walls, plus the same-family orphans `0x1BE` / `0x225`): a flag-window
/// site writes `base + offset`, so a window whose arithmetic covers a target
/// flag would explain the write with **no literal** anywhere in the corpus.
/// Consumers check containment / near-miss with [`Op49WindowSite::covers`] /
/// [`Op49WindowSite::min_distance`].
///
/// Same contract as [`system_flag_census`] / [`motion_flag_census`]: scenes
/// without a resolvable MAN are skipped (best-effort over the CDNAME scene
/// set, all bundle forms incl. scripted-table + v12-embedded via
/// [`crate::scene::Scene::field_man_payload`]); site order preserves scene /
/// partition / record discovery order. Descriptor bytes are read from the
/// full MAN buffer (the retail picker reads through `_DAT_8007B450` into the
/// resident MAN, not the instruction footprint); sites whose descriptor
/// window would run past the MAN end are skipped (nothing resident to read).
// REF: FUN_801EF014
pub fn op49_window_census<I, S>(index: &ProtIndex, scenes: I) -> Vec<Op49WindowSite>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut out = Vec::new();
    for name in scenes {
        let name = name.as_ref();
        let Ok(scene) = Scene::load(index, name) else {
            continue;
        };
        let Ok(Some(man)) = scene.field_man_payload(index) else {
            continue;
        };
        let Ok(man_file) = legaia_asset::man_section::parse(&man) else {
            continue;
        };
        let partition_count = man_file.header.partition_counts.len();
        for partition in 0..partition_count {
            let records = man_file
                .header
                .partition_counts
                .get(partition)
                .copied()
                .unwrap_or(0)
                .max(0) as usize;
            for record in 0..records {
                let Some((script_start, pc0, body_len)) =
                    partition_record_span(&man_file, &man, partition, record)
                else {
                    continue;
                };
                let body = &man[script_start..script_start + body_len];
                for insn in LinearWalker::new(body, pc0).flatten() {
                    let InsnInfo::StateResume { sub_op, .. } = insn.info else {
                        continue;
                    };
                    // Header size: 1 byte, or 2 with the 0x80 cross-context
                    // prefix. The descriptor pointer (`_DAT_8007B450`)
                    // targets the sub-op byte right after the header.
                    let hs = if insn.extended.is_some() { 2 } else { 1 };
                    let desc = script_start + insn.pc + hs;
                    // Descriptor bytes +1..+5 from the sub-op byte, read
                    // from the full resident MAN (see fn docs).
                    let Some(win) = man.get(desc + 1..desc + 6) else {
                        continue;
                    };
                    out.push(Op49WindowSite {
                        scene_name: name.to_string(),
                        partition,
                        record,
                        abs_pc: script_start + insn.pc,
                        sub_op,
                        count: win[0],
                        default_index: win[1],
                        rows: win[2],
                        base_flag: u16::from_le_bytes([win[3], win[4]]),
                        in_footprint: insn.size >= hs + 6,
                    });
                }
            }
        }
    }
    out
}

/// One motion-VM system-flag site recovered by [`motion_flag_census`]:
/// the scene plus the [`legaia_asset::man_motion::MotionFlagSite`] the
/// scene's MAN tail-section 1 carries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MotionCensusSite {
    /// CDNAME scene name whose MAN motion section carries the op.
    pub scene_name: String,
    /// The section-1 record / variant / gate / offset / kind detail.
    pub site: legaia_asset::man_motion::MotionFlagSite,
    /// The record's actor bindings (who the stream runs on).
    pub bindings: Vec<legaia_asset::man_motion::MotionBinding>,
}

/// Disc-wide **motion-VM** flag census - the sibling of
/// [`system_flag_census`] for the *second* bytecode VM that writes the
/// `DAT_80085758` system story-flag bank: `FUN_80038158` op `0x07` (SET) /
/// `0x08` (CLEAR), whose scripts live in each scene MAN's tail **section 1**
/// (installed on actors by `FUN_8003A9D4` at scene entry; see
/// [`legaia_asset::man_motion`]). The MAN field-VM census is structurally
/// blind to these writes - they are a different opcode space in a different
/// carrier section.
///
/// Same contract as [`system_flag_census`]: scenes without a resolvable MAN
/// (or with a terminator section 1) are skipped, the map is sorted by flag
/// id, and site order preserves scene / record discovery order.
pub fn motion_flag_census<I, S>(
    index: &ProtIndex,
    scenes: I,
) -> BTreeMap<u16, Vec<MotionCensusSite>>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    use legaia_asset::man_motion;
    let mut out: BTreeMap<u16, Vec<MotionCensusSite>> = BTreeMap::new();
    for name in scenes {
        let name = name.as_ref();
        let Ok(scene) = Scene::load(index, name) else {
            continue;
        };
        let Ok(Some(man)) = scene.field_man_payload(index) else {
            continue;
        };
        let Ok(man_file) = legaia_asset::man_section::parse(&man) else {
            continue;
        };
        let records = man_motion::motion_records(&man, &man_file);
        for site in man_motion::motion_flag_sites(&man, &man_file) {
            let bindings = records
                .get(site.record)
                .map(|r| r.bindings.clone())
                .unwrap_or_default();
            out.entry(site.flag).or_default().push(MotionCensusSite {
                scene_name: name.to_string(),
                site,
                bindings,
            });
        }
    }
    out
}
