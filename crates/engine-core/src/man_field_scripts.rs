//! Opcode-aware walk of a scene MAN's field-VM scripts.
//!
//! [`walk_partition1_scripts`] surveys partition 1 (the encounter hunt);
//! [`walk_partition_gflag_sites`] is the partition-agnostic companion that
//! collects global-flag writes (used for the opening prologue's partition-2
//! `GFLAG_SET 26` hand-off arm), both via the same [`LinearWalker`] decode.
//!
//! The record **header** is partition-specific. Partitions 0/1 use the
//! `[u8 N][N*2 locals][4-byte header]` prefix below. Partition 2 (the
//! cutscene-timeline records) instead opens with a Shift-JIS name and three
//! condition-list gates - see `partition2_record_script_offset` and
//! [`partition_record_span`], decoded from the dispatcher `FUN_8003BDE0`.
//!
//! Partition 1 of a scene MAN (the "actor-placement / scripts" partition)
//! holds one field-VM script per record:
//!
//! - record `0` is the scene-entry **system script** - the one
//!   [`crate::scene::Scene::field_man_entry_script`] resolves and
//!   `enter_field_scene` loads via `load_field_script_at`;
//! - records `1..` are per-actor **interaction scripts**, dispatched when
//!   the player interacts with the placed actor.
//!
//! Each record opens with the same `[u8 N][N*2 locals][4-byte header]`
//! prefix as the entry script, so the first opcode sits `1 + N*2 + 4`
//! bytes in (see [`legaia_asset::man_section::ManFile::scene_entry_script`]).
//!
//! This module pairs the MAN partition walk with the field-VM disassembler
//! ([`legaia_engine_vm::field_disasm`]) so callers get a faithful,
//! opcode-aware instruction stream per record instead of a byte scan. The
//! distinction matters for the scripted-encounter hunt: a naive search for
//! a "yield" byte (`0x37` / `0x41`) hits every yield opcode **and** every
//! operand / SJIS byte that happens to equal `0x37` / `0x41`. Walking the
//! opcode stream means an [`ArmSite`] is reported only at a real `Yield`
//! instruction boundary, and the inline record bytes are decoded with
//! [`EncounterRecord::parse`] - the same `+0x3` count / `+0x4` ids layout
//! the retail reader at `0x801DA620` consumes.
//!
//! ## What this can and cannot conclude
//!
//! Per [`crate::field::step`]'s own commentary there is **no dedicated
//! encounter opcode**: the arm ops (`0x37`/`0x41`, `0x38`, `0x43`, `0x47`,
//! `0x4C`) all share the yield-and-forward shape, and the *discriminator*
//! is the consuming entity-SM state, not the opcode. So a single
//! [`ArmSite`] whose inline window decodes as a valid `[count][ids]` record
//! is a *candidate*, not a proof. The value here is empirical: it surfaces
//! whether any P1 script carries an inline `[count=1][id=0x4F]` Tetsu
//! literal at a real yield boundary - which adjudicates the inline-literal
//! hypothesis against the indexed-formation-table hypothesis
//! (see [`crate::encounter_record::RIM_ELM_TRAINING_FORMATION_ID`]).

use legaia_asset::man_section::{ActorPlacement, ManFile};
use legaia_engine_vm::field_disasm::{
    EffectKind, FlagKind, InsnInfo, LinearWalker, MenuCtrlKind, YieldKind, scene_change_name,
};

use crate::encounter_record::EncounterRecord;
use crate::world::FieldCarrierConfig;

/// Inclusive `op0` range a genuine field-VM warp (`scene_transition`) uses.
///
/// The WARP opcode is `0x3E` with `op0 = map_id + 100`, and only **7** door-warp
/// destinations exist - `map_id 0..=6` (each selects a scene-*type* code overlay
/// at PROT `0x4d + map_id`; see [`crate::scene::DefaultMapIdResolver`] and
/// `docs/subsystems/asset-loader.md`). So a real warp's `op0` is `100..=106`.
///
/// This range matters for [`classify_placement`]: the per-actor walk is an
/// over-approximating linear disassembly that *desyncs* inside embedded message
/// text, and a desynced read can land on a `0x3E` whose following byte happens
/// to be `>= 100` - a phantom warp. Every observed phantom carries an `op0` far
/// outside this range (175 / 179 / 200, i.e. SJIS or dialog bytes) and rides the
/// `0x80` cross-context prefix, while every genuine corpus warp is the *base*
/// `0x3E` with `op0` in `100..=106`. So the kind decision requires both signals.
const WARP_OP0_RANGE: std::ops::RangeInclusive<u8> = 100..=106;

/// `true` when a decoded `WarpOrInteract` instruction is a *genuine* door-warp
/// (not a text-desync phantom): the base `0x3E` opcode (no `0x80` cross-context
/// prefix) carrying `op0` in [`WARP_OP0_RANGE`]. `op0` is the raw operand byte
/// (`map_id + 100`); `extended` is the disassembler's cross-context-target field
/// (`Some` iff the `0x80` prefix bit was set on the leading opcode byte).
fn is_genuine_warp(op0: u8, extended: Option<u8>) -> bool {
    extended.is_none() && WARP_OP0_RANGE.contains(&op0)
}

/// One field-VM `Yield` instruction in a partition-1 script, annotated with
/// the inline encounter-record decode of its trailing operand window.
#[derive(Debug, Clone)]
pub struct ArmSite {
    /// Absolute byte offset of the yield opcode in the MAN buffer.
    pub abs_pc: usize,
    /// Byte offset relative to the record's `script_start`.
    pub rel_pc: usize,
    /// The yield opcode (`0x37` / `0x41` standard, `0x47` wide).
    pub opcode: u8,
    /// `0x37`/`0x41` (standard) vs `0x47` (wide) yield encoding.
    pub wide: bool,
    /// The 8-byte window the retail reader would consume at this site
    /// (`man[abs_pc..abs_pc+8]`, zero-padded if the buffer ends early).
    pub window: [u8; 8],
    /// The inline record decoded from `window` (`+0x3` count, `+0x4` ids),
    /// when it parses as a valid `0..=4`-monster formation.
    pub record: Option<EncounterRecord>,
}

impl ArmSite {
    /// `true` when the inline window decodes as the lone Rim Elm Tetsu
    /// formation - `count == 1` and `monster_ids[0] == 0x4F`.
    pub fn matches_tetsu(&self) -> bool {
        matches!(
            self.record,
            Some(EncounterRecord { count: 1, monster_ids })
                if monster_ids[0] == crate::encounter_record::RIM_ELM_TRAINING_OPPONENT_ID
        )
    }
}

/// Per-record disassembly summary for one partition-1 field-VM script.
#[derive(Debug, Clone)]
pub struct ManScriptRecord {
    /// Partition-1 record index (`0` = scene-entry system script).
    pub index: usize,
    /// Absolute byte offset of the record's script block in the MAN buffer.
    pub script_start: usize,
    /// First-opcode offset relative to `script_start` (`1 + N*2 + 4`).
    pub pc0: usize,
    /// Number of bytes from `script_start` to the record's bounded end.
    pub body_len: usize,
    /// Number of instructions a linear walk decoded.
    pub insn_count: usize,
    /// Number of bytes the linear walk could not decode (recovered by
    /// advancing one byte).
    pub decode_errors: usize,
    /// Yield sites found in this record, with inline-record decodes.
    pub arm_sites: Vec<ArmSite>,
}

impl ManScriptRecord {
    /// Yield sites whose inline window decodes as a valid formation record.
    pub fn encounter_arm_candidates(&self) -> impl Iterator<Item = &ArmSite> {
        self.arm_sites.iter().filter(|s| s.record.is_some())
    }
}

/// Compute the tightest upper byte bound for a record body that starts at
/// `start`: the smallest record offset (across all three partitions) or
/// section start that is strictly greater than `start`, clamped to the MAN
/// length. This stops a record's walk from spilling into the next record's
/// or the encounter section's bytes.
fn record_end_bound(man_file: &ManFile, man_len: usize, start: usize) -> usize {
    let mut bound = man_len;
    let data = man_file.data_region_offset;
    for partition in &man_file.partitions {
        for &off in partition {
            let abs = data + off as usize;
            if abs > start && abs < bound {
                bound = abs;
            }
        }
    }
    // The encounter section (and its siblings) live in the same data region;
    // their length-prefix offsets are a hard ceiling for script bytes.
    for section in &man_file.sections {
        if section.offset > start && section.offset < bound {
            bound = section.offset;
        }
    }
    bound.min(man_len)
}

/// Walk every partition-1 record of `man_file` as a field-VM script and
/// return a per-record disassembly summary.
///
/// `man` is the decompressed MAN buffer the offsets index into.
pub fn walk_partition1_scripts(man_file: &ManFile, man: &[u8]) -> Vec<ManScriptRecord> {
    let n1 = man_file.header.partition_counts[1].max(0) as usize;
    let mut out = Vec::with_capacity(n1);
    for index in 0..n1 {
        let Some(script_start) = man_file.actor_placement_record_offset(index, man.len()) else {
            continue;
        };
        let n = *man.get(script_start).unwrap_or(&0) as usize;
        let pc0 = 1 + n * 2 + 4;
        let end = record_end_bound(man_file, man.len(), script_start);
        if script_start + pc0 >= end {
            // Degenerate / empty record body - record it with no sites.
            out.push(ManScriptRecord {
                index,
                script_start,
                pc0,
                body_len: end.saturating_sub(script_start),
                insn_count: 0,
                decode_errors: 0,
                arm_sites: Vec::new(),
            });
            continue;
        }
        let body = &man[script_start..end];
        let mut insn_count = 0usize;
        let mut decode_errors = 0usize;
        let mut arm_sites = Vec::new();
        for r in LinearWalker::new(body, pc0) {
            match r {
                Ok(insn) => {
                    insn_count += 1;
                    if let InsnInfo::Yield { kind } = insn.info {
                        let abs_pc = script_start + insn.pc;
                        let mut window = [0u8; 8];
                        for (i, slot) in window.iter_mut().enumerate() {
                            if let Some(&b) = man.get(abs_pc + i) {
                                *slot = b;
                            }
                        }
                        arm_sites.push(ArmSite {
                            abs_pc,
                            rel_pc: insn.pc,
                            opcode: insn.opcode,
                            wide: matches!(kind, YieldKind::Wide),
                            window,
                            record: EncounterRecord::parse(&window),
                        });
                    }
                }
                Err(_) => decode_errors += 1,
            }
        }
        out.push(ManScriptRecord {
            index,
            script_start,
            pc0,
            body_len: end - script_start,
            insn_count,
            decode_errors,
            arm_sites,
        });
    }
    out
}

/// The interactive role of a placed actor ([`ActorPlacement`]), inferred from
/// its per-entity field-VM script ([`classify_placements`]).
///
/// Retail has no static "entity kind" field: a placed actor's behaviour is
/// whatever its script does. This classifies by two signals:
///
/// - a **warp** (`0x3E` with `op0 >= 100`, retail `scene_transition`), found by
///   the linear opcode walk → a [`Portal`](Self::Portal) whose target map id is
///   `op0 - 100`;
/// - otherwise, an **inline dialog-text block** - a run of `0x1F`-lead /
///   `0x00`-terminated message segments embedded in the record - found
///   *structurally* (see [`first_inline_dialog_offset`]) → an
///   [`Npc`](Self::Npc) carrying that text;
/// - none of those → [`Plain`](Self::Plain) (a moving / animated / model-only
///   actor, e.g. the lead-actor slot or a decorative NPC).
///
/// ## Why dialog text is found structurally, not by opcode
///
/// A field-scene interaction record is dominated by its embedded message text,
/// and that text contains bytes that look like field-VM opcodes (a literal
/// `>` is `0x3E`, the `scene_transition`/interact opcode; a literal `?` is
/// `0x3F`, the named-scene-change opcode; ASCII punctuation hits `0x37`/`0x41`
/// yield bytes). A linear disassembly therefore *desyncs* inside the text and
/// reports phantom interact / scene-change ops with garbage operands. So the
/// message text is located by scanning for the `0x1F`-lead segment block
/// directly, and the (unreliable, for field scenes) opcode-decoded `interact_id`
/// is kept only as a best-effort hint. The warp scan is opcode-based but gated
/// (see `is_genuine_warp`): a *genuine* warp marks the actor a portal, and
/// genuine warp records carry no inline text block to confuse it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlacementKind {
    /// The script warps to another scene. `target_map` is the field-VM map id
    /// (`op0 - 100`), resolvable through the same `MapIdResolver` a
    /// `scene_transition` uses.
    Portal { target_map: u8 },
    /// The actor carries an inline dialog-text block and/or a field-interact
    /// op but never warps - a talk-to NPC / sign / event trigger.
    Npc {
        /// Best-effort `0x3E`-op interact selector from the opcode walk.
        /// Unreliable for text-heavy field records (the walk desyncs inside the
        /// message); the real message text is
        /// [`dialog_inline`](Self::Npc::dialog_inline).
        interact_id: Option<u8>,
        /// Record bytes from the start of the first inline `0x1F`-lead text
        /// segment through the record's bounded end - the actual message text;
        /// [`crate::dialog::OwnedDialogPanel::from_inline_dialog`] renders it
        /// (it re-finds the `0x1F` lead and types the first segment).
        dialog_inline: Option<Vec<u8>>,
    },
    /// No warp / dialog / interact opcode: a decorative or script-only actor
    /// (movement, animation, model preload, the lead-actor slot).
    Plain,
}

/// Find the byte offset of the first inline dialog-text segment in `body`,
/// searching from `from`.
///
/// A field-scene interaction record stores its message text as a run of
/// segments, each `0x1F <printable bytes> 0x00`. This returns the offset of the
/// first `0x1F` that introduces a segment whose body is non-trivial (≥3 bytes)
/// and overwhelmingly printable ASCII (≥3/4 of the bytes in `0x20..=0x7E`) - the
/// printable-ratio gate rejects a stray `0x1F` glyph byte that happens to sit in
/// opcode / move-script data. Returns `None` when no such segment exists (a
/// decorative or warp-only actor).
pub fn first_inline_dialog_offset(body: &[u8], from: usize) -> Option<usize> {
    let mut i = from.min(body.len());
    while i < body.len() {
        if body[i] == 0x1F {
            let text_start = i + 1;
            let mut j = text_start;
            while j < body.len() && body[j] != 0x00 {
                j += 1;
            }
            let raw = &body[text_start..j];
            let printable = raw.iter().filter(|&&b| (0x20..=0x7E).contains(&b)).count();
            if raw.len() >= 3 && printable * 4 >= raw.len() * 3 {
                return Some(i);
            }
            i = j + 1;
        } else {
            i += 1;
        }
    }
    None
}

/// Classify every partition-1 actor placement by scanning its field-VM script.
///
/// Pairs each [`ManFile::actor_placements`] entry with the
/// [`PlacementKind`] its script implies. The script is walked from the
/// placement's `script_pc0`, bounded by the same record-end ceiling
/// [`walk_partition1_scripts`] uses, so the scan never spills into the next
/// record or the encounter section.
pub fn classify_placements(man_file: &ManFile, man: &[u8]) -> Vec<(ActorPlacement, PlacementKind)> {
    man_file
        .actor_placements(man)
        .into_iter()
        .map(|p| {
            let kind = classify_placement(man_file, man, &p);
            (p, kind)
        })
        .collect()
}

/// One inline scene destination decoded from a `0x3F` named-scene-change op.
///
/// A field/overworld scene's controller script lists every place it can warp to
/// as a `0x3F` op that carries the destination scene **name** directly in the
/// bytecode (plus an `index` id and an entry tile). This is the disc-sourced
/// counterpart to [`crate::scene::DefaultMapIdResolver`]'s positional guess: the
/// destination names are *in the data*, not in an uncaptured overlay. (The
/// separate `0x3E` door-warp carries only a 7-id scene-*type* selector, whose
/// name resolution does still live in an uncaptured handler.)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SceneDestination {
    /// Destination CDNAME scene label (e.g. `"town0c"`, `"rikuroa"`).
    pub scene_name: String,
    /// The `i16` index operand the op carries (a story/entry id; *not* the
    /// `0x3E` door-warp `map_id` - distinct id space, observed past 100).
    pub index: i16,
    /// Entry tile X byte at the destination (`& 0x7F` tile, `& 0x80` half-tile).
    pub entry_x: u8,
    /// Entry tile Z byte at the destination (same encoding as `entry_x`).
    pub entry_z: u8,
}

/// Recover the inline scene-destination table from a scene's MAN by decoding the
/// `0x3F` named-scene-change ops across its partition-1 scripts.
///
/// Returns one [`SceneDestination`] per distinct `(scene_name, index)` reached,
/// in first-seen order. Only ops whose inline name passes
/// [`scene_change_name`]'s clean-CDNAME-label gate are kept, so the
/// over-approximating walk's text-desync phantoms (a literal `?` = `0x3F` inside
/// a message, which decodes a bogus name) are dropped - exactly the desync
/// hazard the `0x3E` warp gate guards against. Genuine destinations recur with
/// stable indices across the controller's records; phantoms don't survive the
/// gate.
pub fn scene_destinations(man_file: &ManFile, man: &[u8]) -> Vec<SceneDestination> {
    // The `0x3F` destination table is a data blob the scene controller appends
    // *after* its small per-actor records (in `map01` it trails the last
    // partition-1 record, well past `record_end_bound`, which clips on the next
    // partition/section). So bound each partition-1 record by the **next
    // partition-1 record start** (man-end for the last record) rather than the
    // tight per-record ceiling, letting the final record's walk reach the table.
    // The clean-name gate + `(name, index)` dedup absorb the over-walk: a record
    // viewed from an earlier start re-sees the same ops, and desync junk past the
    // table fails the gate.
    let n1 = man_file.header.partition_counts[1].max(0) as usize;
    let mut starts: Vec<usize> = (0..n1)
        .filter_map(|i| man_file.actor_placement_record_offset(i, man.len()))
        .collect();
    starts.sort_unstable();
    let mut out: Vec<SceneDestination> = Vec::new();
    for (k, &start) in starts.iter().enumerate() {
        let end = starts.get(k + 1).copied().unwrap_or(man.len());
        let pc0 = {
            let locals = *man.get(start).unwrap_or(&0) as usize;
            1 + locals * 2 + 4
        };
        if start + pc0 >= end {
            continue;
        }
        let body = &man[start..end];
        for insn in LinearWalker::new(body, pc0).flatten() {
            let InsnInfo::SceneChange {
                index,
                entry_x,
                entry_z,
                ..
            } = insn.info
            else {
                continue;
            };
            let Some(scene_name) = scene_change_name(body, &insn) else {
                continue;
            };
            if out
                .iter()
                .any(|d| d.index == index && d.scene_name == scene_name)
            {
                continue;
            }
            out.push(SceneDestination {
                scene_name,
                index,
                entry_x,
                entry_z,
            });
        }
    }
    out
}

/// One inline FMV trigger decoded from a `0x4C 0xE2` op in a scene's scripts.
///
/// The field-VM FMV trigger carries its `fmv_id` as a literal `i16` operand
/// (`[4C, E2, lo, hi, _, _, _]` - it writes `_DAT_8007BA78` and pokes the
/// next game mode to `StrInit`), so the per-scene movie assignment is
/// disc-sourced script data, not a runtime value. See
/// [`docs/formats/str-fmv-table.md`] and the `MenuCtrlKind::FmvTrigger`
/// decoder in `legaia_asset::field_disasm`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SceneFmvTrigger {
    /// Partition-1 record index whose script carries the op.
    pub record: usize,
    /// Bytecode pc of the op within that record's walk.
    pub pc: usize,
    /// The literal `fmv_id` operand (the value written to `_DAT_8007BA78`).
    pub fmv_id: i16,
}

/// Recover every inline `0x4C 0xE2` FMV trigger from a scene's MAN by walking
/// its partition-1 scripts - the same record walk (and the same
/// over-approximation caveats) as [`scene_destinations`].
///
/// Phantom guard: a literal `4C E2` inside message text can desync-decode a
/// bogus trigger, so ops whose `fmv_id` falls outside the runtime FMV-state
/// table's 12 slots (`0..=11`; observed retail range `0..=8`) are dropped,
/// and `(record, fmv_id)` pairs re-seen from an earlier over-walk start are
/// deduped. Returns first-seen order.
pub fn scene_fmv_triggers(man_file: &ManFile, man: &[u8]) -> Vec<SceneFmvTrigger> {
    let n1 = man_file.header.partition_counts[1].max(0) as usize;
    let mut starts: Vec<usize> = (0..n1)
        .filter_map(|i| man_file.actor_placement_record_offset(i, man.len()))
        .collect();
    starts.sort_unstable();
    let mut out: Vec<SceneFmvTrigger> = Vec::new();
    for (k, &start) in starts.iter().enumerate() {
        let end = starts.get(k + 1).copied().unwrap_or(man.len());
        let pc0 = {
            let locals = *man.get(start).unwrap_or(&0) as usize;
            1 + locals * 2 + 4
        };
        if start + pc0 >= end {
            continue;
        }
        let body = &man[start..end];
        for insn in LinearWalker::new(body, pc0).flatten() {
            let InsnInfo::MenuCtrl {
                kind: MenuCtrlKind::FmvTrigger { fmv_id },
                ..
            } = insn.info
            else {
                continue;
            };
            if !(0..=11).contains(&fmv_id) {
                continue;
            }
            if out.iter().any(|t| t.record == k && t.fmv_id == fmv_id) {
                continue;
            }
            out.push(SceneFmvTrigger {
                record: k,
                pc: insn.pc,
                fmv_id,
            });
        }
    }
    out
}

/// One inline BGM start decoded from an op-`0x35` sub-`1` in a scene's scripts.
///
/// The field-VM BGM start carries its id as a literal `i16` operand
/// (`[35, lo, hi, 01]` - it writes `_DAT_8007BAC8`, resolved asynchronously
/// by `FUN_800243F0`: ids `< 2000` are scene-local PROT slots at
/// `scene_base + 6 + id`, ids `>= 2000` index the global BGM pool). So the
/// per-scene music assignment is disc-sourced script data - the same
/// pattern as [`SceneFmvTrigger`]. See `docs/subsystems/script-vm.md`
/// § BGM lookup table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SceneBgmStart {
    /// Partition-1 record index whose script carries the op.
    pub record: usize,
    /// Bytecode pc of the op within that record's walk.
    pub pc: usize,
    /// The literal BGM id operand (the value written to `_DAT_8007BAC8`).
    pub bgm_id: u16,
}

/// Recover every inline op-`0x35` sub-`1` BGM start from a scene's MAN by
/// walking its partition-1 scripts - the same record walk (and the same
/// over-approximation caveats) as [`scene_fmv_triggers`].
///
/// Phantom guard: a literal `0x35` inside message text can desync-decode a
/// bogus start, so ids outside the two documented spaces (scene-local
/// `0..2000` restricted to small slots `< 0x40`, or the global pool
/// `2000..2082` = the `music_01` bank) are dropped, and `(record, bgm_id)`
/// pairs re-seen from an earlier over-walk start are deduped. Returns
/// first-seen order.
pub fn scene_bgm_starts(man_file: &ManFile, man: &[u8]) -> Vec<SceneBgmStart> {
    let n1 = man_file.header.partition_counts[1].max(0) as usize;
    let mut starts: Vec<usize> = (0..n1)
        .filter_map(|i| man_file.actor_placement_record_offset(i, man.len()))
        .collect();
    starts.sort_unstable();
    let mut out: Vec<SceneBgmStart> = Vec::new();
    for (k, &start) in starts.iter().enumerate() {
        let end = starts.get(k + 1).copied().unwrap_or(man.len());
        let pc0 = {
            let locals = *man.get(start).unwrap_or(&0) as usize;
            1 + locals * 2 + 4
        };
        if start + pc0 >= end {
            continue;
        }
        let body = &man[start..end];
        for insn in LinearWalker::new(body, pc0).flatten() {
            let InsnInfo::Bgm { text_id, sub_op: 1 } = insn.info else {
                continue;
            };
            if !(text_id < 0x40 || (2000..2082).contains(&text_id)) {
                continue;
            }
            if out.iter().any(|t| t.record == k && t.bgm_id == text_id) {
                continue;
            }
            out.push(SceneBgmStart {
                record: k,
                pc: insn.pc,
                bgm_id: text_id,
            });
        }
    }
    out
}

/// One inline move-VM stager install decoded from an op-`0x34` sub-`3` in a
/// scene's scripts.
///
/// The field-VM "play 3D animation" op carries the prescript record id as a
/// literal byte operand (`[34, 3x, id]` - retail chains through the
/// installer `FUN_800252EC(id) = prescript_base + offsets[id]` into the
/// move-VM part stager `FUN_80021B04`; engine
/// `crate::world::World::spawn_field_stager`). So which prescript records
/// are **move-VM stagers** is disc-sourced script data - the operand census
/// that resolves the prescript bundle's dual-consumer split (see
/// `docs/reference/open-rev-eng-threads.md`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SceneStagerInstall {
    /// MAN partition the carrying script lives in (0 = house-door records,
    /// 1 = actor placements / system script, 2 = cutscene timelines).
    pub partition: usize,
    /// Record index within that partition.
    pub record: usize,
    /// Bytecode pc of the op within that record's walk.
    pub pc: usize,
    /// The literal prescript-record id operand (the `FUN_800252EC` id).
    pub stager_id: u8,
}

/// Recover every inline op-`0x34` sub-`3` stager install from a scene's MAN
/// by walking all three partitions' scripts ([`partition_record_span`]
/// bounds each record; partition 2's Shift-JIS-name header is handled
/// there) - the same over-approximation caveats as [`scene_fmv_triggers`] /
/// [`scene_bgm_starts`]. Ids are raw operands (bound them against the
/// scene's prescript record count at the call site). Returns partition /
/// record / pc order.
pub fn scene_stager_installs(man_file: &ManFile, man: &[u8]) -> Vec<SceneStagerInstall> {
    let mut out: Vec<SceneStagerInstall> = Vec::new();
    for partition in 0..=2 {
        let count = man_file
            .header
            .partition_counts
            .get(partition)
            .copied()
            .unwrap_or(0)
            .max(0) as usize;
        for record in 0..count {
            let Some((start, pc0, len)) = partition_record_span(man_file, man, partition, record)
            else {
                continue;
            };
            let body = &man[start..start + len];
            for insn in LinearWalker::new(body, pc0).flatten() {
                let InsnInfo::Effect {
                    kind: EffectKind::AnimTrigger { arg },
                    ..
                } = insn.info
                else {
                    continue;
                };
                if out
                    .iter()
                    .any(|t| t.partition == partition && t.record == record && t.pc == insn.pc)
                {
                    continue;
                }
                out.push(SceneStagerInstall {
                    partition,
                    record,
                    pc: insn.pc,
                    stager_id: arg,
                });
            }
        }
    }
    out
}

/// Classify a single placement by scanning its script. See [`PlacementKind`].
pub fn classify_placement(man_file: &ManFile, man: &[u8], p: &ActorPlacement) -> PlacementKind {
    let start = p.record_offset;
    let end = record_end_bound(man_file, man.len(), start);
    if start + p.script_pc0 >= end {
        return PlacementKind::Plain;
    }
    let body = &man[start..end];

    // Opcode-walk pass: a *genuine* door-warp wins outright (the actor is a
    // portal). A real warp is the base `0x3E op0 ...` with `op0` in the 7-id
    // door-warp range ([`WARP_OP0_RANGE`]). The over-approximating linear walk
    // can still desync inside embedded message / SJIS text and land on a `0x3E`
    // whose next byte is `>= 100` - but every such phantom in the corpus rides
    // the `0x80` cross-context prefix and carries an out-of-range `op0`
    // (175 / 179 / 200), so [`is_genuine_warp`] rejects it. The decoded interact
    // / dialog hints, likewise, are unreliable on text-heavy field records (the
    // walk desyncs inside the message), so they are best-effort only - the real
    // dialog text is recovered structurally below.
    let mut interact_id = None;
    for insn in LinearWalker::new(body, p.script_pc0).flatten() {
        match insn.info {
            // A warp wins outright - but only when it is a *genuine* door-warp,
            // not a text-desync phantom (see [`is_genuine_warp`]). A phantom
            // warp (cross-context `0x80` prefix and/or `op0` outside the 7-id
            // door-warp range) is dropped here so the actor falls through to the
            // structural dialog pass, which classifies a text-bearing record as
            // an [`Npc`] (e.g. `geremi`'s talk NPC) rather than a portal to a
            // non-existent map.
            InsnInfo::WarpOrInteract {
                op0, is_warp: true, ..
            } if is_genuine_warp(op0, insn.extended) => {
                return PlacementKind::Portal {
                    target_map: op0 - 100,
                };
            }
            InsnInfo::WarpOrInteract {
                op1,
                is_warp: false,
                ..
            } => {
                interact_id.get_or_insert(op1);
            }
            // NB: `0x3F` (`InsnInfo::SceneChange`) is deliberately *not* read as a
            // dialog hint here - it is the named scene-change opcode, not a dialog
            // op (field dialogue is the `0x4C` nibble-5 path). Its inline string is
            // a destination scene name, recovered by the scene-destination resolver,
            // not NPC message text.
            _ => {}
        }
    }

    // Structural pass: the message text is a run of `0x1F`-lead segments. Carry
    // the record bytes from the first segment's `0x1F` through the record end;
    // `from_inline_dialog` re-finds the lead and types the first segment.
    let dialog_inline = first_inline_dialog_offset(body, p.script_pc0).map(|o| body[o..].to_vec());

    if dialog_inline.is_some() || interact_id.is_some() {
        PlacementKind::Npc {
            interact_id,
            dialog_inline,
        }
    } else {
        PlacementKind::Plain
    }
}

/// The prologue-aware form of a talk NPC's inline interaction script.
///
/// [`classify_placement`]'s [`PlacementKind::Npc::dialog_inline`] is the record
/// truncated to start at the first `0x1F` text segment - enough for the
/// simplified renderer, but it discards the **interaction prologue**: the
/// field-VM bytecode between the record's `script_pc0` and that first segment
/// (story-flag `SysFlag.Test` / `JmpRel` chains, `CFlag.Set`, NPC move-to-tile).
/// Retail runs that prologue first; its `SysFlag.Test` branches are how the box
/// *selects which segment to start at* per story state. This struct carries the
/// untruncated record so the opt-in field-VM dialogue runner can execute it.
///
/// `entry_pc` and `first_segment` are byte offsets **into `body`** (the record
/// from `record_offset` to its bounded end). The runner steps the VM from
/// `entry_pc`; if the prologue reaches a text segment it opens there, otherwise
/// it falls back to `first_segment` (the old start), so it is never worse than
/// the truncated path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InlineDialogPrologue {
    /// The full interaction record body (`man[record_offset..record_end]`).
    pub body: Vec<u8>,
    /// Offset of the interaction-script entry (`script_pc0`) within `body`.
    pub entry_pc: usize,
    /// Offset of the first `0x1F` text segment within `body`.
    pub first_segment: usize,
}

/// Recover the [`InlineDialogPrologue`] for placement `p`, or `None` when the
/// record carries no inline text segment (a decorative / warp-only actor). The
/// `body`/`entry_pc`/`first_segment` are derived from the same bounds
/// [`classify_placement`] uses, so `body[first_segment..]` equals that
/// placement's `dialog_inline` byte-for-byte.
pub fn placement_inline_prologue(
    man_file: &ManFile,
    man: &[u8],
    p: &ActorPlacement,
) -> Option<InlineDialogPrologue> {
    let start = p.record_offset;
    let end = record_end_bound(man_file, man.len(), start);
    if start + p.script_pc0 >= end {
        return None;
    }
    let body = &man[start..end];
    let first_segment = first_inline_dialog_offset(body, p.script_pc0)?;
    Some(InlineDialogPrologue {
        body: body.to_vec(),
        entry_pc: p.script_pc0,
        first_segment,
    })
}

/// The parked-actor sentinel tile `(0x7F, 0x7F)`: a placement (or a move
/// target) at this tile is off-field - retail parks despawned/conditional
/// actors there (the `0x7F,0x7F` parked-sentinel decode `FUN_8003A1E4`
/// consumes). Move ops targeting it are despawns, not walks.
pub const PARKED_SENTINEL_TILE: (u8, u8) = (0x7F, 0x7F);

/// Decode a placement-script grid-coordinate byte to a world coordinate -
/// the same `(b & 0x7F) * 0x80 + 0x40` (+`0x40` when bit 7 is set) formula
/// the field VM applies to op `0x23` / `0x4C 0x51` position bytes (see the
/// `grid_to_world` decode in `legaia_engine_vm::field`).
pub fn grid_byte_to_world(b: u8) -> i16 {
    let base = (b & 0x7F) as i16 * 0x80 + 0x40;
    if b & 0x80 != 0 { base + 0x40 } else { base }
}

/// Locality radius (world units) for an autonomous NPC-route waypoint. A
/// placement's pre-text script mixes its local walk legs with story-flag-gated
/// relocations to other parts of the scene (the linear walk sees every branch);
/// only waypoints within this radius of the spawn anchor are kept as the
/// patrol route. 6 tiles = the observed span of authored local walks.
pub const NPC_ROUTE_LOCALITY: i32 = 0x300;

/// The pre-text region of a placement's script: the record bytes from
/// `script_pc0` up to (exclusive) the first inline `0x1F` text segment, or the
/// record's bounded end when it carries no text. This is the same region the
/// interaction-prologue runner executes - real field-VM bytecode, free of the
/// text-desync hazard the full-record walk has.
fn placement_pretext_region<'a>(
    man_file: &ManFile,
    man: &'a [u8],
    p: &ActorPlacement,
) -> Option<(&'a [u8], usize)> {
    let start = p.record_offset;
    let end = record_end_bound(man_file, man.len(), start);
    if start + p.script_pc0 >= end {
        return None;
    }
    let body = &man[start..end];
    let walk_end = first_inline_dialog_offset(body, p.script_pc0).unwrap_or(body.len());
    Some((&body[..walk_end], p.script_pc0))
}

/// Recover placement `p`'s **autonomous walk route**: the ordered list of
/// `(world_x, world_z)` waypoints its own pre-text script bytecode walks the
/// actor through. The carrier ops are the `0x4C 0x51` NPC move-to-tile
/// instructions ([`MenuCtrlKind::Nibble5NpcRun`]) in the actor's own context
/// (no `0x80` cross-context prefix) - the same ops retail's per-actor script
/// channel feeds into the NPC run/glide path. Dropped: cross-context targets
/// (another actor's walk), the [`PARKED_SENTINEL_TILE`] despawn, waypoints
/// beyond [`NPC_ROUTE_LOCALITY`] of the spawn anchor (story-flag-gated
/// relocations the linear walk can't condition), and consecutive duplicates
/// (facing/wait re-issues of the same tile).
///
/// What this does NOT model: the per-actor field-VM channel that paces these
/// ops with yields and story-flag branches - the engine consumer drives the
/// kept waypoints as a loop through the motion VM instead. See
/// `docs/subsystems/motion-vm.md`.
pub fn placement_motion_route(
    man_file: &ManFile,
    man: &[u8],
    p: &ActorPlacement,
) -> Vec<(i16, i16)> {
    let Some((region, pc0)) = placement_pretext_region(man_file, man, p) else {
        return Vec::new();
    };
    let mut out: Vec<(i16, i16)> = Vec::new();
    for insn in LinearWalker::new(region, pc0).flatten() {
        let InsnInfo::MenuCtrl {
            kind: MenuCtrlKind::Nibble5NpcRun { x_enc, z_enc, .. },
            ..
        } = insn.info
        else {
            continue;
        };
        if insn.extended.is_some() {
            continue; // cross-context: drives another channel, not this actor
        }
        if (x_enc & 0x7F, z_enc & 0x7F) == PARKED_SENTINEL_TILE {
            continue; // park/despawn, not a walk target
        }
        let (wx, wz) = (grid_byte_to_world(x_enc), grid_byte_to_world(z_enc));
        let (dx, dz) = (
            (wx as i32 - p.world_x as i32).abs(),
            (wz as i32 - p.world_z as i32).abs(),
        );
        if dx.max(dz) > NPC_ROUTE_LOCALITY {
            continue; // story-gated relocation, not a local patrol leg
        }
        if out.last() == Some(&(wx, wz)) {
            continue;
        }
        out.push((wx, wz));
    }
    out
}

/// The field-VM **player system channel** id (`0xF8`): a cross-context op
/// prefixed `op | 0x80, 0xF8` targets the player actor (retail resolves it to
/// `_DAT_8007c364`). See `docs/subsystems/script-vm.md`.
pub const PLAYER_CHANNEL: u8 = 0xF8;

/// A walk-touch event a placement's script fires when the player's movement
/// collides with the placed actor's body (retail: the locomotion's per-step
/// touch dispatch posts `FUN_801d5b5c` on the mutual `+0x98` collision
/// partner, which runs the touched entity's script - no button press).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WalkTouchEvent {
    /// The script door-warps (`0x3E`, `op0 = map_id + 100`): walking into the
    /// placement leaves the scene through the 7-id scene-type selector.
    Warp { target_map: u8 },
    /// The script teleports the **player** (cross-context `0x23 | 0x80` into
    /// the [`PLAYER_CHANNEL`]): walking into the placement snaps the player to
    /// `(world_x, world_z)` - the cave-guard throw-back / intra-scene door
    /// mechanism.
    PlayerMoveTo { world_x: i16, world_z: i16 },
}

/// Classify placement `p`'s walk-touch behaviour, if any. `None` for parked
/// placements (no touchable body until a script un-parks them - not modelled)
/// and for placements whose script carries neither a genuine door-warp nor a
/// player-channel teleport in its pre-text region.
pub fn placement_walk_touch_event(
    man_file: &ManFile,
    man: &[u8],
    p: &ActorPlacement,
) -> Option<WalkTouchEvent> {
    if (p.tile_x, p.tile_z) == PARKED_SENTINEL_TILE {
        return None;
    }
    if let PlacementKind::Portal { target_map } = classify_placement(man_file, man, p) {
        return Some(WalkTouchEvent::Warp { target_map });
    }
    let (region, pc0) = placement_pretext_region(man_file, man, p)?;
    for insn in LinearWalker::new(region, pc0).flatten() {
        let InsnInfo::MoveTo { xb, zb } = insn.info else {
            continue;
        };
        if insn.extended != Some(PLAYER_CHANNEL) {
            continue; // own-context snap (the actor's own reposition)
        }
        if (xb & 0x7F, zb & 0x7F) == PARKED_SENTINEL_TILE {
            continue;
        }
        return Some(WalkTouchEvent::PlayerMoveTo {
            world_x: grid_byte_to_world(xb),
            world_z: grid_byte_to_world(zb),
        });
    }
    None
}

/// `true` when `p` is the Rim Elm sparring partner: the partition-1 placement
/// pinned at [`RIM_ELM_SPARRING_CARRIER_TILE`] carrying
/// [`RIM_ELM_SPARRING_CARRIER_MODEL`] (the NPC whose talk-menu installs the
/// opening lone-Tetsu training fight). See [`crate::encounter_record`].
pub fn is_rim_elm_sparring_carrier(p: &ActorPlacement) -> bool {
    (p.tile_x, p.tile_z) == crate::encounter_record::RIM_ELM_SPARRING_CARRIER_TILE
        && p.model_index == crate::encounter_record::RIM_ELM_SPARRING_CARRIER_MODEL
}

/// A field carrier derived from one MAN partition-1 placement: the placement it
/// came from plus the [`FieldCarrierConfig`] its identity / script implies.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DerivedFieldCarrier {
    /// Partition-1 record index of the source placement (retail actor record).
    pub placement_index: usize,
    /// Source placement tile (column, row).
    pub tile: (u8, u8),
    /// Source placement model byte.
    pub model: u8,
    /// The carrier role to install for this placement.
    pub config: FieldCarrierConfig,
}

/// Derive field-carrier configs **directly from a scene MAN's actor
/// placements**, instead of hand-building them.
///
/// Each interactable placement ([`PlacementKind::Npc`]) becomes a carrier:
///
/// - the pinned Rim Elm sparring partner ([`is_rim_elm_sparring_carrier`]) maps
///   to [`FieldCarrierConfig::ScriptedEncounter`] for the training formation
///   ([`crate::encounter_record::RIM_ELM_TRAINING_FORMATION_ID`]);
/// - every other talk-to NPC maps to [`FieldCarrierConfig::Npc`] keyed by its
///   partition-1 record index (the retail interaction-script selector).
///
/// Decorative ([`PlacementKind::Plain`]) and warp ([`PlacementKind::Portal`])
/// placements carry no engageable carrier SM and are skipped; each
/// [`DerivedFieldCarrier`] keeps its `placement_index` so a caller can map a
/// carrier-Vec index back to the MAN actor.
///
/// The formation **index** the sparring carrier launches (`= 4`) is still a
/// pinned constant: a town01 field interaction record selects its formation by
/// index, not via an inline `[count][ids]` literal (proven by the partition-1
/// script walk), so the selection bytecode is not yet decoded. What this
/// derives from the MAN is the carrier's *identity and placement* - which actor
/// is the carrier, where it stands, and that the scene actually contains it -
/// rather than fabricating a standalone carrier with no MAN linkage.
pub fn derive_field_carriers(man_file: &ManFile, man: &[u8]) -> Vec<DerivedFieldCarrier> {
    classify_placements(man_file, man)
        .into_iter()
        .filter_map(|(p, kind)| {
            let config = if is_rim_elm_sparring_carrier(&p) {
                FieldCarrierConfig::ScriptedEncounter {
                    formation_id: crate::encounter_record::RIM_ELM_TRAINING_FORMATION_ID,
                }
            } else if matches!(kind, PlacementKind::Npc { .. }) {
                FieldCarrierConfig::Npc {
                    interact_id: p.index as u8,
                }
            } else {
                return None;
            };
            Some(DerivedFieldCarrier {
                placement_index: p.index,
                tile: (p.tile_x, p.tile_z),
                model: p.model_index,
                config,
            })
        })
        .collect()
}

/// One field-VM **global-flag write** (`GFLAG_SET` / `GFLAG_CLEAR`, opcodes
/// `0x2E` / `0x2F`) found while walking a MAN partition's records as
/// field-VM scripts, annotated with the scratchpad flag bit it touches.
///
/// The global-flag bank is `_DAT_1F800394` (the engine's
/// [`crate::world::World::story_flags`]); op `0x2E` sets `1 << bit`, op
/// `0x2F` clears it. The opening prologue's `opdeene` cutscene-timeline
/// record ends with `GFLAG_SET 26`, the write the `town01` hand-off gate
/// (`FUN_801D1344`) waits on - see
/// [`crate::world::PROLOGUE_HANDOFF_FLAG`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GFlagSite {
    /// Absolute byte offset of the `GFLAG` opcode in the MAN buffer.
    pub abs_pc: usize,
    /// Partition the carrying record lives in (`0..3`).
    pub partition: usize,
    /// Record index within the partition.
    pub record: usize,
    /// The opcode byte (`0x2E` set, `0x2F` clear).
    pub opcode: u8,
    /// `true` for `GFLAG_SET` (`0x2E`), `false` for `GFLAG_CLEAR` (`0x2F`).
    pub set: bool,
    /// Scratchpad flag bit the op touches (`0..31`).
    pub bit: u8,
}

/// The script `script_start` of `partition`'s record `index`, computed from
/// the partition's u24 record-offset table against the MAN data region.
/// `None` when the partition or index is out of range or the offset lands
/// past the buffer.
fn partition_record_offset(
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
fn partition2_record_script_offset(body: &[u8]) -> Option<usize> {
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
/// collect its global-flag write sites (`GFLAG_SET` / `GFLAG_CLEAR`).
///
/// This is the partition-agnostic companion to [`walk_partition1_scripts`]:
/// the encounter hunt cares about partition 1's yield sites, the opening
/// prologue cares about partition 2's cutscene-timeline `GFLAG_SET`. Both
/// share the same `[u8 N][N*2 locals][4-byte header]` record prefix and the
/// same opcode-aware [`LinearWalker`] decode, so a `GFLAG` site is reported
/// only at a real instruction boundary - not at an operand / SJIS byte that
/// happens to equal `0x2E`.
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
            if let InsnInfo::GFlag { kind, bit } = insn.info
                && kind != FlagKind::Test
            {
                out.push(GFlagSite {
                    abs_pc: script_start + insn.pc,
                    partition,
                    record: index,
                    opcode: insn.opcode,
                    set: kind == FlagKind::Set,
                    bit,
                });
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use legaia_asset::man_section::{ManFile, ManHeader};

    /// Build a minimal one-partition-1-record MAN whose single record is a
    /// field-VM script: `[N=0][4-byte header][0x37 yield with inline
    /// count=1 id=0x4F][...]`. Exercises the record-walk + arm-site decode
    /// without disc data.
    fn synthetic_man_with_tetsu_arm() -> (ManFile, Vec<u8>) {
        // data_region_offset is arbitrary for the synthetic test; pick a
        // small value and lay the record body right after it.
        let data_region_offset = 0x40usize;
        let p1_0 = 0u32; // record 0 sits at the start of the data region.
        let script_start = data_region_offset + p1_0 as usize;

        // Record prefix: N=0 -> pc0 = 1 + 0 + 4 = 5.
        // Then a 0x37 yield whose inline window is [0x37][s0][s1][count=1][0x4F].
        let mut man = vec![0u8; script_start];
        man.push(0x00); // N = 0
        man.extend_from_slice(&[0xAA, 0xBB, 0xCC, 0xDD]); // 4-byte header
        // pc0 = 5: the yield opcode + inline record.
        man.push(0x37); // +0 yield opcode
        man.push(0x11); // +1 reserved
        man.push(0x22); // +2 reserved
        man.push(0x01); // +3 count = 1
        man.push(0x4F); // +4 monster id = Tetsu
        man.push(0x00); // +5 padding so the window has 8 bytes
        man.push(0x00);
        man.push(0x00);

        let header = ManHeader {
            status_flags: 0,
            low_flag: false,
            depth_lut: [0; 16],
            partition_counts: [0, 1, 0],
            u24_at_28: 0,
        };
        let man_file = ManFile {
            header,
            partitions: [vec![], vec![p1_0], vec![]],
            data_region_offset,
            // Sections all point past the script so they don't bound it.
            sections: std::array::from_fn(|_| legaia_asset::man_section::SectionRef {
                offset: man.len(),
                length: 0,
            }),
        };
        (man_file, man)
    }

    #[test]
    fn walks_partition1_and_decodes_inline_tetsu_arm() {
        let (man_file, man) = synthetic_man_with_tetsu_arm();
        let records = walk_partition1_scripts(&man_file, &man);
        assert_eq!(records.len(), 1);
        let rec = &records[0];
        assert_eq!(rec.index, 0);
        assert_eq!(rec.pc0, 5);
        assert_eq!(rec.arm_sites.len(), 1, "one yield site");
        let site = &rec.arm_sites[0];
        assert_eq!(site.opcode, 0x37);
        assert!(!site.wide);
        let record = site.record.expect("inline window decodes");
        assert_eq!(record.count, 1);
        assert_eq!(record.monster_ids[0], 0x4F);
        assert!(site.matches_tetsu());
    }

    /// Build a MAN with two partition-1 records: record 0 (the scene
    /// controller, skipped by `actor_placements`) and record 1 (a placed actor
    /// whose `[N=0][model][actions][tx][tz]` header is followed by `script`).
    fn man_with_placement_script(script: &[u8]) -> (ManFile, Vec<u8>) {
        let data_region_offset = 0x40usize;
        // Record 0: a minimal controller (`N=0`, header, halt).
        let rec0: &[u8] = &[0x00, 0, 0, 0, 0, 0x21];
        // Record 1: N=0, model=5, actions=0, tile (3,4), then the script.
        let mut rec1 = vec![0x00, 0x05, 0x00, 0x03, 0x04];
        rec1.extend_from_slice(script);

        let off0 = 0u32;
        let off1 = rec0.len() as u32;
        let mut man = vec![0u8; data_region_offset];
        man.extend_from_slice(rec0);
        man.extend_from_slice(&rec1);

        let header = ManHeader {
            status_flags: 0,
            low_flag: false,
            depth_lut: [0; 16],
            partition_counts: [0, 2, 0],
            u24_at_28: 0,
        };
        let man_file = ManFile {
            header,
            partitions: [vec![], vec![off0, off1], vec![]],
            data_region_offset,
            sections: std::array::from_fn(|_| legaia_asset::man_section::SectionRef {
                offset: man.len(),
                length: 0,
            }),
        };
        (man_file, man)
    }

    #[test]
    fn classify_warp_script_is_a_portal() {
        // `0x3E` with op0 = 103 is a genuine door-warp to map id 103 - 100 = 3
        // (within the 7-id `WARP_OP0_RANGE`).
        let (mf, man) = man_with_placement_script(&[0x3E, 103, 0, 0, 0, 0]);
        let placements = mf.actor_placements(&man);
        assert_eq!(placements.len(), 1, "record 0 is the controller");
        assert_eq!(
            classify_placement(&mf, &man, &placements[0]),
            PlacementKind::Portal { target_map: 3 }
        );
    }

    #[test]
    fn is_genuine_warp_gate() {
        // Base opcode, in-range op0 -> genuine (map_id 0..=6 -> op0 100..=106).
        assert!(is_genuine_warp(100, None)); // map_id 0
        assert!(is_genuine_warp(106, None)); // map_id 6
        // Out-of-range op0 (the desync phantoms: 175 / 179 / 200) -> rejected.
        assert!(!is_genuine_warp(107, None));
        assert!(!is_genuine_warp(200, None));
        // Cross-context `0x80`-prefixed warp -> rejected even with in-range op0.
        assert!(!is_genuine_warp(103, Some(0xF8)));
    }

    #[test]
    fn classify_out_of_range_warp_is_not_a_portal() {
        // `0x3E` with op0 = 200 decodes as `is_warp` (op0 >= 100) but lands far
        // outside the 7-id door-warp range - the signature of a text-desynced
        // read (corpus: `geremi` op0=200, `other7` op0=175/179). With no inline
        // text after it, the actor is Plain, never a phantom portal to map 100.
        let (mf, man) = man_with_placement_script(&[0x3E, 200, 0, 0, 0, 0, 0x21]);
        let placements = mf.actor_placements(&man);
        assert_eq!(
            classify_placement(&mf, &man, &placements[0]),
            PlacementKind::Plain,
            "an out-of-range pseudo-warp must not classify as a portal"
        );
    }

    #[test]
    fn scene_destinations_decodes_named_warp() {
        // A script with a 0x3F named scene-change to "dolk" (index 60, entry
        // tile bytes 0x10/0x20, dir 0x30) followed by a halt.
        let mut script = vec![0x3Fu8, 60, 0, 4];
        script.extend_from_slice(b"dolk");
        script.extend_from_slice(&[0x10, 0x20, 0x30, 0x21]);
        let (mf, man) = man_with_placement_script(&script);
        let dests = scene_destinations(&mf, &man);
        assert_eq!(
            dests,
            vec![SceneDestination {
                scene_name: "dolk".to_string(),
                index: 60,
                entry_x: 0x10,
                entry_z: 0x20,
            }]
        );
    }

    #[test]
    fn scene_destinations_rejects_text_desync_name() {
        // A 0x3F whose "name" is uppercase/punctuation (a literal '?' inside
        // message text) is not a clean CDNAME label and is dropped.
        let mut script = vec![0x3Fu8, 0, 0, 4];
        script.extend_from_slice(b"Hi! ");
        script.extend_from_slice(&[0x00, 0x00, 0x00, 0x21]);
        let (mf, man) = man_with_placement_script(&script);
        assert!(scene_destinations(&mf, &man).is_empty());
    }

    #[test]
    fn classify_interact_script_is_an_npc() {
        // `0x3E` with op0 < 100 is a field interact at index op1.
        let (mf, man) = man_with_placement_script(&[0x3E, 0x05, 0x07, 0x21]);
        let placements = mf.actor_placements(&man);
        assert_eq!(
            classify_placement(&mf, &man, &placements[0]),
            PlacementKind::Npc {
                interact_id: Some(0x07),
                dialog_inline: None,
            }
        );
    }

    #[test]
    fn classify_plain_script_has_no_interaction() {
        // A bare halt: no warp / dialog / interact.
        let (mf, man) = man_with_placement_script(&[0x21]);
        let placements = mf.actor_placements(&man);
        assert_eq!(
            classify_placement(&mf, &man, &placements[0]),
            PlacementKind::Plain
        );
    }

    #[test]
    fn first_inline_dialog_offset_finds_a_printable_segment() {
        // `[noise][0x1F "Hello" 0x00]` -> offset of the 0x1F.
        let body = [0x21u8, 0x25, 0x1F, b'H', b'e', b'l', b'l', b'o', 0x00, 0x21];
        assert_eq!(first_inline_dialog_offset(&body, 0), Some(2));
    }

    #[test]
    fn first_inline_dialog_offset_rejects_a_stray_marker() {
        // A 0x1F followed by non-printable / too-short data is not a segment.
        let body = [0x1Fu8, 0x01, 0x02, 0x00, 0x1F, 0xAB, 0x00];
        assert_eq!(first_inline_dialog_offset(&body, 0), None);
    }

    #[test]
    fn classify_inline_text_with_phantom_warp_byte_is_an_npc() {
        // A talk-NPC record whose message contains a literal '>' (0x3E, the
        // warp/interact opcode). The structural pass finds the 0x1F text block;
        // the desync gate ignores the '>' byte because it sits inside the text,
        // so the actor classifies as an Npc carrying the inline message - NOT a
        // phantom portal.
        let mut script = vec![0x25u8]; // a benign leading op
        script.extend_from_slice(&[0x1F]); // text-segment lead
        script.extend_from_slice(b"<Go north>"); // contains 0x3E ('>')
        script.push(0x00); // terminator
        let (mf, man) = man_with_placement_script(&script);
        let placements = mf.actor_placements(&man);
        let kind = classify_placement(&mf, &man, &placements[0]);
        match kind {
            PlacementKind::Npc { dialog_inline, .. } => {
                let inline = dialog_inline.expect("inline text captured");
                // Renders the segment text (after the 0x1F lead).
                let panel = crate::dialog::OwnedDialogPanel::from_inline_dialog(&inline);
                assert!(panel.is_some(), "inline buffer is renderable");
            }
            other => panic!("expected Npc, got {other:?}"),
        }
    }

    #[test]
    fn classify_warp_wins_over_a_preceding_dialog() {
        // A talk-then-warp script (interact first, warp after) classifies as a
        // portal - the warp is the defining behaviour.
        let (mf, man) = man_with_placement_script(&[0x3E, 0x01, 0x09, 0x3E, 105, 0, 0, 0, 0]);
        let placements = mf.actor_placements(&man);
        assert_eq!(
            classify_placement(&mf, &man, &placements[0]),
            PlacementKind::Portal { target_map: 5 }
        );
    }

    /// Build a minimal one-partition-2-record MAN whose single record is a
    /// field-VM script ending in `GFLAG_SET 26` (op `0x2E`, operand `0x1A`) -
    /// the opening prologue's `town01` hand-off arm.
    fn synthetic_man_with_gflag_set_26() -> (ManFile, Vec<u8>) {
        let data_region_offset = 0x40usize;
        let p2_0 = 0u32;
        let script_start = data_region_offset + p2_0 as usize;

        // Record prefix: N=0 -> pc0 = 5. Then GFLAG_SET 26.
        let mut man = vec![0u8; script_start];
        man.push(0x00); // N = 0
        man.extend_from_slice(&[0xAA, 0xBB, 0xCC, 0xDD]); // 4-byte header
        man.push(0x2E); // GFLAG_SET
        man.push(0x1A); // bit 26
        man.push(0x48); // a trailing no-op so the walk has a clean boundary

        let header = ManHeader {
            status_flags: 0,
            low_flag: false,
            depth_lut: [0; 16],
            partition_counts: [0, 0, 1],
            u24_at_28: 0,
        };
        let man_file = ManFile {
            header,
            partitions: [vec![], vec![], vec![p2_0]],
            data_region_offset,
            sections: std::array::from_fn(|_| legaia_asset::man_section::SectionRef {
                offset: man.len(),
                length: 0,
            }),
        };
        (man_file, man)
    }

    #[test]
    fn walks_partition2_and_finds_gflag_set_26() {
        let (man_file, man) = synthetic_man_with_gflag_set_26();
        let sites = walk_partition_gflag_sites(&man_file, &man, 2);
        assert_eq!(sites.len(), 1, "one GFLAG site");
        let site = sites[0];
        assert_eq!(site.partition, 2);
        assert_eq!(site.record, 0);
        assert_eq!(site.opcode, 0x2E);
        assert!(site.set);
        assert_eq!(site.bit, 26);
        // The other partitions carry no records, hence no sites.
        assert!(walk_partition_gflag_sites(&man_file, &man, 0).is_empty());
        assert!(walk_partition_gflag_sites(&man_file, &man, 1).is_empty());
    }

    #[test]
    fn partition2_named_record_script_offset_matches_the_formula() {
        // name_len=6 (12 SJIS bytes), all three cond-blocks empty -> 0x10,
        // the opdeene record-18 shape.
        let mut body = vec![0x06];
        body.extend_from_slice(&[0xAA; 12]); // 6 SJIS chars
        body.extend_from_slice(&[0x00, 0x00, 0x00]); // C0=C1=C2=0
        body.push(0x34); // first opcode
        assert_eq!(partition2_record_script_offset(&body), Some(0x10));

        // Non-empty blocks: name_len=2 (4 bytes), C0=3 (3 bytes), C1=1 (2
        // bytes), C2=2 (4 bytes) -> 1 + 4 + (1+3) + (1+2) + (1+4) = 17.
        let mut body = vec![0x02, 0xAA, 0xAA, 0xAA, 0xAA];
        body.push(0x03); // C0 = 3
        body.extend_from_slice(&[0x11, 0x22, 0x33]);
        body.push(0x01); // C1 = 1 u16
        body.extend_from_slice(&[0x44, 0x55]);
        body.push(0x02); // C2 = 2 u16
        body.extend_from_slice(&[0x66, 0x77, 0x88, 0x99]);
        body.push(0x21); // first opcode
        assert_eq!(partition2_record_script_offset(&body), Some(17));
        assert_eq!(body[17], 0x21);

        // A count byte past the end returns None rather than panicking.
        assert_eq!(partition2_record_script_offset(&[0x06]), None);
    }

    /// Build a MAN whose partition 1 is `[controller, records...]`. Each
    /// `records[i]` is a full placement record body
    /// (`[N=0][model][actions][tx][tz][script...]`); `records[0]` is the
    /// scene controller (skipped by `actor_placements`).
    fn man_with_placements(records: &[Vec<u8>]) -> (ManFile, Vec<u8>) {
        let data_region_offset = 0x40usize;
        let mut man = vec![0u8; data_region_offset];
        let mut offsets = Vec::new();
        for rec in records {
            offsets.push((man.len() - data_region_offset) as u32);
            man.extend_from_slice(rec);
        }
        let header = ManHeader {
            status_flags: 0,
            low_flag: false,
            depth_lut: [0; 16],
            partition_counts: [0, records.len() as i16, 0],
            u24_at_28: 0,
        };
        let man_file = ManFile {
            header,
            partitions: [vec![], offsets, vec![]],
            data_region_offset,
            sections: std::array::from_fn(|_| legaia_asset::man_section::SectionRef {
                offset: man.len(),
                length: 0,
            }),
        };
        (man_file, man)
    }

    #[test]
    fn derive_field_carriers_maps_sparring_carrier_and_npcs() {
        use crate::encounter_record::{
            RIM_ELM_SPARRING_CARRIER_MODEL, RIM_ELM_SPARRING_CARRIER_TILE,
            RIM_ELM_TRAINING_FORMATION_ID,
        };
        let (tx, tz) = RIM_ELM_SPARRING_CARRIER_TILE;
        // controller (idx 0), sparring carrier (idx 1, pinned tile/model + dialog),
        // a plain talk NPC (idx 2, dialog), a portal (idx 3), a decorative actor
        // (idx 4, halt only).
        let controller = vec![0x00, 0x00, 0x00, 0x00, 0x00, 0x21];
        let mut sparring = vec![0x00, RIM_ELM_SPARRING_CARRIER_MODEL, 0x00, tx, tz];
        sparring.extend_from_slice(&[0x1F, b's', b'p', b'a', b'r', 0x00]);
        let mut npc = vec![0x00, 0x10, 0x00, 10, 12];
        npc.extend_from_slice(&[0x1F, b'h', b'i', b'!', 0x00]);
        let portal = vec![0x00, 0x11, 0x00, 5, 5, 0x3E, 103, 0, 0, 0, 0];
        let decorative = vec![0x00, 0x12, 0x00, 6, 6, 0x21];
        let (mf, man) = man_with_placements(&[controller, sparring, npc, portal, decorative]);

        let carriers = derive_field_carriers(&mf, &man);
        // Portal + decorative carry no engageable carrier; only the sparring
        // partner and the talk NPC survive.
        assert_eq!(carriers.len(), 2, "portal + decorative are skipped");

        // The sparring carrier is first and maps to the training formation.
        assert_eq!(carriers[0].placement_index, 1);
        assert_eq!(carriers[0].tile, RIM_ELM_SPARRING_CARRIER_TILE);
        assert_eq!(carriers[0].model, RIM_ELM_SPARRING_CARRIER_MODEL);
        assert_eq!(
            carriers[0].config,
            FieldCarrierConfig::ScriptedEncounter {
                formation_id: RIM_ELM_TRAINING_FORMATION_ID
            }
        );

        // The plain talk NPC maps to an Npc carrier keyed by its record index.
        assert_eq!(carriers[1].placement_index, 2);
        assert_eq!(
            carriers[1].config,
            FieldCarrierConfig::Npc { interact_id: 2 }
        );
    }

    #[test]
    fn placement_motion_route_keeps_local_own_context_runs_only() {
        // Placement at tile (10, 10) -> world (1344, 1344). Script:
        //   NPC_RUN -> (11, 10)        kept (local)
        //   NPC_RUN -> (11, 10)        dropped (consecutive duplicate)
        //   NPC_RUN -> (10, 11)        kept (local)
        //   NPC_RUN -> (127, 127)      dropped (park sentinel)
        //   NPC_RUN -> (60, 60)        dropped (beyond NPC_ROUTE_LOCALITY)
        //   cross-context NPC_RUN      dropped (drives another channel)
        let script = [
            0x4C, 0x51, 11, 10, 0, 5, //
            0x4C, 0x51, 11, 10, 3, 5, //
            0x4C, 0x51, 10, 11, 0, 5, //
            0x4C, 0x51, 0x7F, 0x7F, 0, 5, //
            0x4C, 0x51, 60, 60, 0, 5, //
            0xCC, 0x07, 0x51, 11, 11, 0, 5, // 0x4C | 0x80 prefix, target 0x07
            0x21,
        ];
        let (mf, man) = man_with_placement_script(&script);
        let placements = mf.actor_placements(&man);
        // Re-anchor the placement world position for the test: the helper
        // places it at tile (3, 4); use a placement-local route instead.
        let mut p = placements[0].clone();
        p.world_x = 10 * 0x80 + 0x40;
        p.world_z = 10 * 0x80 + 0x40;
        let route = placement_motion_route(&mf, &man, &p);
        assert_eq!(
            route,
            vec![
                (grid_byte_to_world(11), grid_byte_to_world(10)),
                (grid_byte_to_world(10), grid_byte_to_world(11)),
            ]
        );
    }

    #[test]
    fn grid_byte_to_world_decodes_half_tiles() {
        assert_eq!(grid_byte_to_world(0), 0x40);
        assert_eq!(grid_byte_to_world(10), 10 * 0x80 + 0x40);
        assert_eq!(grid_byte_to_world(10 | 0x80), 10 * 0x80 + 0x80);
    }

    #[test]
    fn walk_touch_event_classifies_portal_and_player_moveto() {
        // A genuine door-warp placement -> Warp.
        let (mf, man) = man_with_placement_script(&[0x3E, 103, 0, 0, 0, 0]);
        let placements = mf.actor_placements(&man);
        assert_eq!(
            placement_walk_touch_event(&mf, &man, &placements[0]),
            Some(WalkTouchEvent::Warp { target_map: 3 })
        );

        // A cross-context player-channel MOVE_TO (`0xA3 0xF8 xb zb`) ->
        // PlayerMoveTo at the decoded world coords.
        let (mf, man) = man_with_placement_script(&[0xA3, 0xF8, 20, 30, 0x21]);
        let placements = mf.actor_placements(&man);
        assert_eq!(
            placement_walk_touch_event(&mf, &man, &placements[0]),
            Some(WalkTouchEvent::PlayerMoveTo {
                world_x: grid_byte_to_world(20),
                world_z: grid_byte_to_world(30),
            })
        );

        // An own-context MOVE_TO (the actor repositioning itself) is NOT a
        // walk-touch event; neither is a bare halt.
        let (mf, man) = man_with_placement_script(&[0x23, 20, 30, 0x21]);
        let placements = mf.actor_placements(&man);
        assert_eq!(placement_walk_touch_event(&mf, &man, &placements[0]), None);
        let (mf, man) = man_with_placement_script(&[0x21]);
        let placements = mf.actor_placements(&man);
        assert_eq!(placement_walk_touch_event(&mf, &man, &placements[0]), None);
    }

    #[test]
    fn parked_placement_carries_no_walk_touch() {
        // Same warp script, but the placement itself parks at the (127, 127)
        // sentinel tile - no touchable body, so no walk-touch event.
        let (mf, man) = man_with_placement_script(&[0x3E, 103, 0, 0, 0, 0]);
        let placements = mf.actor_placements(&man);
        let mut p = placements[0].clone();
        p.tile_x = 0x7F;
        p.tile_z = 0x7F;
        assert_eq!(placement_walk_touch_event(&mf, &man, &p), None);
    }

    #[test]
    fn empty_partition1_yields_no_records() {
        let header = ManHeader {
            status_flags: 0,
            low_flag: false,
            depth_lut: [0; 16],
            partition_counts: [0, 0, 0],
            u24_at_28: 0,
        };
        let man_file = ManFile {
            header,
            partitions: [vec![], vec![], vec![]],
            data_region_offset: 0x2B,
            sections: std::array::from_fn(|_| legaia_asset::man_section::SectionRef {
                offset: 0x2B,
                length: 0,
            }),
        };
        let man = vec![0u8; 0x80];
        assert!(walk_partition1_scripts(&man_file, &man).is_empty());
    }
}
