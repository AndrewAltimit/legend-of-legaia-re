//! **Delilas Challenge** - a new enrollment option at the Muscle Dome.
//!
//! The Muscle Dome entry clerk (scene `koin1`, the P1 interaction record that
//! carries the enter / who-enrolls / difficulty menus) gains a fourth option on
//! the "who will be entering" picker: **Delilas Challenge**. Picking it offers
//! a solo (1 vs 3) or full-party (3 vs 3) fight against all three Delilas
//! siblings at once - Gi, Che and Lu ([`DELILAS_IDS`]) - a battle that never
//! exists in retail (the ravine duels are three consecutive *solo* fights, and
//! no retail formation seats three distinct boss ids).
//!
//! ## Mechanism - script + data, no code injection
//!
//! Everything rides on retail's own vocabulary:
//!
//! - **Formation.** `koin1`'s encounter section carries exactly one formation
//!   row (`count=1 ids=[4]`) that no region can roll (`rate+=0` everywhere) and
//!   no `3E FF` site references. It is rewritten in place - same size - to
//!   `hdr=[01,00,00] count=3 ids=[162,163,164]` (the `01` header byte is the
//!   boss-intro flag retail's own scripted rows carry).
//! - **Menu.** The 3-option `0x28` "who will enter" picker grows to a 4-option
//!   `0x29` one (the picker arity ceiling - see `crates/mes/src/picker.rs`).
//!   The new option's branch is appended at the record's end. The two
//!   quick-path skip tests before the picker (flags `0x559`/`0x558`: once
//!   Noa or Gala has refused enrollment, retail skips the who-menu forever
//!   and auto-registers Vahn) are NOPed so the menu always shows - without
//!   that, any save that ever picked Noa or Gala could never reach the
//!   challenge option.
//! - **Gate.** The branch opens with a `0x70`-family SYSTEM-flag test of
//!   [`KORU_DEFEATED_FLAG`] (`0x378`) - the exact flag the game latches when
//!   Koru dies and the world-map ravine entrance flips from `nilboa` to
//!   `nilboa2` (`map03 P2[15]` sets it, `map03 P2[3]` selects the scene on it).
//!   Until then the clerk politely refuses.
//! - **Solo party strip.** The solo arms use the `0x3D` PARTY_REMOVE ops the
//!   retail ravine duels themselves use to stage a one-member party, latch
//!   [`MARKER_SOLO`] (the group arm latches [`MARKER_GROUP`]), raise retail's
//!   scripted-loss latch [`SCRIPTED_LOSS_FLAG`], and launch via `3E FF 00`
//!   (the Tetsu-sparring idiom). The loss latch means a wipe returns to the
//!   venue instead of the continue screen; the boss formation header keeps
//!   the fight un-fleeable (`ctx+0x287`), so the outcome is strictly win or
//!   wipe.
//! - **Outcome + prize.** A scripted battle returns through a full scene
//!   reload, so a guarded block spliced at the top of `koin1`'s scene-entry
//!   script (P1 record 0) consumes the markers: it recomposes the party,
//!   scores the fight through retail's own battle-outcome flag
//!   ([`OUTCOME_SURVIVED_FLAG`], set/cleared by the MAIN INIT gate), grants
//!   the prize on a win ([`DelilasRewards`], default 3x / 1x Honey), and
//!   fully restores the party on a loss so nobody walks out of the venue at
//!   0 HP. The block is a no-op on every ordinary entry, and the markers
//!   never survive across a save point (set -> battle -> reload consumes
//!   them), so a save/reload can't strand them.
//!
//! ## Why this module carries its own relocation engine
//!
//! `man_edit::apply_insertions`'s ref scan does a clean fall-through decode and
//! stops at the first byte it can't decode. The clerk's record desyncs at its
//! very first `0x1F` dialogue byte, so that scan sees **zero** of the record's
//! straddling jumps, and the MES picker jump entries are data no opcode walk
//! ever visits. This module therefore walks the record **MES-aware** (text
//! segments skipped via the `legaia-mes` interpreter, pickers decoded via
//! `legaia_mes::picker`, opcodes via `legaia_asset::field_disasm`), collects
//! every relative-jump field - opcode deltas *and* picker entries, which share
//! the same "delta is relative to its own field" encoding - and rewrites each
//! one the insertions move. The rebuilt record is re-walked and required to
//! reproduce the original instruction stream before anything is written back.
//! (`man_edit` would additionally refuse the P1[0] splice outright: that
//! record carries a `0x45 0xC0` camera-apply, its blanket absolute-ref bail.
//! The apply's stored PC here is `0`, which sits before the splice point and
//! therefore never moves.)

use legaia_asset::field_disasm::{self, CameraKind, InsnInfo, InventoryCmpKind};
use legaia_asset::man_section::{self, ManFile};
use legaia_asset::scene_asset_table::{self, SceneAssetTable};
use legaia_mes::{Interpreter, MesEvent, parse_picker_at};

use crate::starting_bag::{sysflag_set, sysflag_test};

/// SYSTEM story flag latched by the Koru death event in Nivora Ravine
/// (`map03 P2[15]`); once set, the world-map ravine entrance resolves to
/// `nilboa2` (`map03 P2[3]`) and the Muscle Dome's own Master rounds 9+
/// unlock. The challenge branch tests exactly this flag.
pub const KORU_DEFEATED_FLAG: u16 = 0x378;

/// Transient SYSTEM flag marking "a **solo** Delilas Challenge is in flight,
/// the party was stripped to one member". Set immediately before the `3E FF`
/// battle op, consumed (cleared + party recomposed + outcome scored) by the
/// guarded block this patch splices into `koin1`'s scene-entry script. Chosen
/// from the same high band as `starting_bag::DEFAULT_GUARD_BIT` (a region
/// that reads zero across retail saves) but distinct from it, and absent from
/// both the field-VM and motion-VM disc-wide flag censuses.
pub const MARKER_SOLO: u16 = 0xD76;

/// Sibling of [`MARKER_SOLO`] for the full-party (3 vs 3) challenge - same
/// lifecycle, distinguishes the reward tier on scene re-entry. Also absent
/// from both disc-wide flag censuses.
pub const MARKER_GROUP: u16 = 0xD77;

/// Story-flag index 0 - retail's own **scripted-loss latch**. Raised by a
/// scene script right before a battle it is allowed to lose (the Rim Elm
/// ambush and the Tetsu spar both do exactly this: `50 00` before `3E FF`),
/// it makes MAIN INIT's back-from-battle game-over gate route a party wipe
/// back to the field like any other battle end, and MAIN INIT consumes the
/// latch itself - unconditionally, on both outcomes (`andi 0x7f` at
/// `0x8003B608`, the join point of the survived and loss-return paths). See
/// `docs/subsystems/battle.md` (party wipe + the game-over overlay, step 4).
pub const SCRIPTED_LOSS_FLAG: u16 = 0x000;

/// Story-flag index 1 - retail's **battle-outcome flag**, managed by the same
/// MAIN INIT back-from-battle gate: the survived path sets it (`ori 0x40` at
/// `0x8003B58C`), the wipe path clears it (`andi 0xbf` at `0x8003B5A0`),
/// unconditionally on every return from battle (`ghidra/scripts/funcs/`
/// `8003aeb0.txt`). Because the challenge battle is un-fleeable, testing this
/// one flag on scene re-entry is an exact won-vs-wiped discriminator.
pub const OUTCOME_SURVIVED_FLAG: u16 = 0x001;

/// Default prize item: **Honey** (`0x65`, permanent all-stats +4).
pub const DEFAULT_REWARD_ITEM: u8 = 0x65;

/// Default prize counts: 3x for a solo (1v3) win, 1x for a group (3v3) win.
pub const DEFAULT_SOLO_REWARD_COUNT: u8 = 3;
/// See [`DEFAULT_SOLO_REWARD_COUNT`].
pub const DEFAULT_GROUP_REWARD_COUNT: u8 = 1;

/// Victory prizes, per challenge mode. The defaults hand out Honey
/// ([`DEFAULT_REWARD_ITEM`]); callers may substitute any item id (e.g. a
/// custom-item pack's ids) without touching the script machinery.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DelilasRewards {
    /// Item granted after a solo (1 vs 3) win.
    pub solo_item: u8,
    /// How many of `solo_item` a solo win grants.
    pub solo_count: u8,
    /// Item granted after a full-party (3 vs 3) win.
    pub group_item: u8,
    /// How many of `group_item` a group win grants.
    pub group_count: u8,
}

impl Default for DelilasRewards {
    fn default() -> Self {
        Self {
            solo_item: DEFAULT_REWARD_ITEM,
            solo_count: DEFAULT_SOLO_REWARD_COUNT,
            group_item: DEFAULT_REWARD_ITEM,
            group_count: DEFAULT_GROUP_REWARD_COUNT,
        }
    }
}

/// 1-based `battle_data` (PROT 867) archive ids of the three Delilas siblings:
/// Gi (162), Che (163), Lu (164) - the same ids the retail ravine solo duels
/// stage one at a time.
pub const DELILAS_IDS: [u8; 3] = [162, 163, 164];

/// The picker option label shown on the grown enrollment menu.
pub const OPTION_LABEL: &str = "Delilas Challenge";

/// MAN asset type byte in a scene bundle's descriptor table.
const MAN_TYPE: u8 = 0x03;

/// `0x60`-family SYSTEM-flag CLEAR encoder (sibling of
/// [`crate::starting_bag::sysflag_set`]).
pub fn sysflag_clear(bit: u16) -> [u8; 2] {
    debug_assert!(bit <= 0x0FFF);
    [0x60 | ((bit >> 8) as u8 & 0x0F), (bit & 0xFF) as u8]
}

/// How a stored control-flow field encodes its destination.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RefKind {
    /// u16 LE delta at `field`; `target = (field + delta) & 0xFFFF`.
    Relative,
    /// u16 LE absolute record-relative PC at `field` (camera-apply).
    Absolute,
}

/// One control-flow field in a record (record-relative coordinates). Opcode
/// jump deltas and MES picker jump entries share the [`RefKind::Relative`]
/// shape exactly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct RefSite {
    /// Record-relative offset of the 2-byte field.
    field: usize,
    /// Record-relative destination.
    target: usize,
    kind: RefKind,
}

/// A decoded picker inside a walked record (record-relative coordinates).
#[derive(Debug, Clone)]
struct PickerSite {
    /// Offset of the open byte (`0x27`..`0x2A`, possibly `| 0x80`).
    open: usize,
    /// Option count.
    n: usize,
    /// Each option's 2-byte jump-entry field offset + decoded target.
    entries: Vec<RefSite>,
}

/// One decoded instruction from a walk: `(pc, opcode, size)`.
type WalkInsn = (usize, u8, usize);

/// Result of a MES-aware record walk.
struct Walk {
    refs: Vec<RefSite>,
    pickers: Vec<PickerSite>,
    insns: Vec<WalkInsn>,
    /// Offset where decoding stopped (start of any unreachable trailing
    /// bytes; equals the record length when the walk consumed everything).
    end: usize,
}

/// Everything located inside the decoded `koin1` MAN that the patch touches.
#[derive(Debug, Clone)]
pub struct DelilasSites {
    /// PROT entry index of the `koin1` scene bundle.
    pub entry_idx: usize,
    /// Byte offset of the compressed MAN stream within the entry.
    pub man_offset: usize,
    /// Offset of the MAN descriptor's `(type<<24)|size` word within the entry.
    pub man_descriptor_off: usize,
    /// Bytes the recompressed MAN must fit within (descriptor boundary).
    pub compressed_budget: usize,
    /// Decompressed MAN.
    pub decoded: Vec<u8>,
    /// `true` when the challenge is already present (idempotent re-run).
    pub already_applied: bool,

    /// Absolute offset of formation row 0's 8-byte record in `decoded`.
    formation_row: usize,
    /// Absolute start of the clerk record (P1[9] on a retail disc).
    rec_start: usize,
    /// First-opcode offset within the clerk record.
    rec_pc0: usize,
    /// Absolute one-past-end of the clerk record.
    rec_end: usize,
    /// Record-relative offset of the who-enrolls picker's open byte.
    who_open: usize,
    /// The who-enrolls picker's three existing jump entries.
    who_entries: Vec<RefSite>,
    /// Record-relative common-tail offset (where the difficulty arms and the
    /// decline path converge; the post-launch exit joins it).
    common_tail: usize,
    /// Record-relative decline-tail offset (the "We hope you come again
    /// soon!" flow every menu's refusal option targets; the locked-gate
    /// refusal jumps here so it costs zero new text). `0` when the patch is
    /// already applied (not needed, and no longer uniquely locatable).
    decline_tail: usize,
    /// `(pc, size)` of the two quick-path skip tests before the who-picker
    /// (retail flag `0x559`/`0x558` TESTs: once Noa or Gala has refused
    /// enrollment, the script skips the who-menu forever and auto-registers
    /// Vahn). The patch NOPs them out so the menu - and the challenge
    /// option - always shows. Empty when already applied.
    skip_tests: Vec<(usize, usize)>,
    /// Every control-flow field in the clerk record (opcode deltas + every
    /// picker's jump entries), record-relative.
    refs: Vec<RefSite>,
    /// Absolute start of the scene-entry record (P1[0]).
    entry_rec_start: usize,
    /// First-opcode offset within the scene-entry record.
    entry_rec_pc0: usize,
}

/// A `0x1F`-lead ASCII text segment (`0x00`-terminated).
fn seg(text: &str) -> Vec<u8> {
    debug_assert!(text.bytes().all(|b| (0x20..0x7F).contains(&b)));
    let mut v = Vec::with_capacity(text.len() + 2);
    v.push(0x1F);
    v.extend(text.bytes());
    v.push(0x00);
    v
}

/// The character-name substitution label `1F C1 <char> 00` (the exact bytes
/// the retail who-picker uses for Vahn / Noa / Gala).
fn name_label(char_id: u8) -> [u8; 4] {
    [0x1F, 0xC1, char_id, 0x00]
}

/// Skip one `0x1F` text segment starting at `pos` (record-relative) using the
/// MES interpreter; returns one past the terminator.
fn skip_segment(rec: &[u8], pos: usize) -> Result<usize, String> {
    debug_assert_eq!(rec.get(pos), Some(&0x1F));
    let mut interp = Interpreter::new_at(rec, pos + 1);
    loop {
        match interp.next_event() {
            Some(MesEvent::EndOfMessage(_)) => return Ok(interp.pc()),
            Some(_) => {}
            None => return Err(format!("unterminated text segment at +0x{pos:04X}")),
        }
    }
}

/// Extract an instruction's control-flow field, if any. Every relative-jump
/// op stores its u16 delta as the **last two bytes** of the instruction, and
/// the delta is relative to that field's own offset - the invariant
/// `docs/formats/man-relocation.md` documents. Camera-apply (`0x45 0xC0`)
/// stores an absolute PC there instead.
fn insn_ref(insn: &field_disasm::Insn) -> Option<RefSite> {
    let field = insn.pc + insn.size - 2;
    let rel = |target: usize| RefSite {
        field,
        target: target & 0xFFFF,
        kind: RefKind::Relative,
    };
    match &insn.info {
        InsnInfo::JmpRel { target, .. } | InsnInfo::CondJmp { target, .. } => Some(rel(*target)),
        InsnInfo::BBoxTest { skip_target, .. } => Some(rel(*skip_target)),
        InsnInfo::SystemFlag {
            target: Some(t), ..
        } => Some(rel(*t)),
        InsnInfo::InventoryCmp {
            kind:
                InventoryCmpKind::Compare { skip_target, .. }
                | InventoryCmpKind::PartyBank { skip_target, .. },
            ..
        } => Some(rel(*skip_target)),
        InsnInfo::Camera {
            kind: CameraKind::Apply { abs_target },
            ..
        } => Some(RefSite {
            field,
            target: *abs_target,
            kind: RefKind::Absolute,
        }),
        _ => None,
    }
}

/// MES-aware walk of one interaction record (`rec` = the record's bytes,
/// starting at `pc0`). Stops cleanly at the first undecodable byte (retail
/// records may carry unreachable trailing data).
fn walk_record(rec: &[u8], pc0: usize) -> Result<Walk, String> {
    let mut w = Walk {
        refs: Vec::new(),
        pickers: Vec::new(),
        insns: Vec::new(),
        end: pc0,
    };
    let mut pos = pc0;
    while pos < rec.len() {
        let b = rec[pos];
        if b == 0x1F {
            pos = skip_segment(rec, pos)?;
            continue;
        }
        let after_terminator = pos > 0 && rec[pos - 1] == 0x00;
        if after_terminator
            && matches!(b & 0x7F, 0x27..=0x2A)
            && let Some(p) = parse_picker_at(rec, pos)
        {
            let mut entries = Vec::with_capacity(p.n);
            let mut ok = true;
            for i in 0..p.n {
                match p.jump_target(i) {
                    Some(t) if t < rec.len() => entries.push(RefSite {
                        field: p.open + 1 + i * 2,
                        target: t & 0xFFFF,
                        kind: RefKind::Relative,
                    }),
                    _ => {
                        ok = false;
                        break;
                    }
                }
            }
            if ok {
                w.refs.extend(entries.iter().copied());
                w.pickers.push(PickerSite {
                    open: pos,
                    n: p.n,
                    entries,
                });
                pos = p.end;
                continue;
            }
        }
        match field_disasm::decode(rec, pos) {
            Ok(insn) if insn.size > 0 => {
                if let Some(r) = insn_ref(&insn) {
                    if r.kind == RefKind::Relative {
                        // Cross-check the "delta is the last two bytes,
                        // relative to itself" model against the decoder.
                        let delta = u16::from_le_bytes([rec[r.field], rec[r.field + 1]]) as usize;
                        if (r.field + delta) & 0xFFFF != r.target {
                            return Err(format!(
                                "op 0x{:02X} at +0x{pos:04X}: delta-field model mismatch",
                                insn.opcode
                            ));
                        }
                    }
                    if r.target >= rec.len() {
                        return Err(format!(
                            "op 0x{:02X} at +0x{pos:04X}: target +0x{:04X} out of record",
                            insn.opcode, r.target
                        ));
                    }
                    w.refs.push(r);
                }
                w.insns.push((pos, insn.opcode, insn.size));
                pos += insn.size;
            }
            _ => break,
        }
    }
    w.end = pos;
    Ok(w)
}

/// Normalized opcode-stream signature of a walked record for old-vs-new
/// verification, as `(pc, token)` pairs. Picker opens normalize to one token
/// (the patch legitimately widens `0x28` to `0x29`); text segments count as
/// one token each; Nop opcodes are skipped (the patch legitimately NOPs the
/// who-menu skip tests, and Nops carry no behaviour to preserve).
fn walk_signature(rec: &[u8], pc0: usize) -> Result<Vec<(usize, u8)>, String> {
    const TEXT: u8 = 0xFD;
    const PICKER: u8 = 0xFE;
    let mut sig = Vec::new();
    let mut pos = pc0;
    while pos < rec.len() {
        let b = rec[pos];
        if b == 0x1F {
            sig.push((pos, TEXT));
            pos = skip_segment(rec, pos)?;
            continue;
        }
        let after_terminator = pos > 0 && rec[pos - 1] == 0x00;
        if after_terminator
            && matches!(b & 0x7F, 0x27..=0x2A)
            && let Some(p) = parse_picker_at(rec, pos)
            && (0..p.n).all(|i| p.jump_target(i).is_some_and(|t| t < rec.len()))
        {
            sig.push((pos, PICKER));
            pos = p.end;
            continue;
        }
        match field_disasm::decode(rec, pos) {
            Ok(insn) if insn.size > 0 => {
                if !matches!(insn.opcode, 0x21 | 0x24 | 0x25 | 0x48) {
                    sig.push((pos, insn.opcode));
                }
                pos += insn.size;
            }
            _ => break,
        }
    }
    Ok(sig)
}

impl DelilasSites {
    /// Locate every site inside a single PROT entry, or `None` if the entry
    /// isn't the `koin1` scene bundle (wrong shape at any step).
    pub fn locate(entry: &[u8], entry_idx: usize) -> Option<Self> {
        let table = scene_asset_table::detect(entry)?;
        let man_desc_idx = table.used().iter().position(|d| d.type_byte == MAN_TYPE)?;
        let man = table.used()[man_desc_idx];
        if man.size == 0 || man.data_offset == 0 {
            return None;
        }
        let man_offset = man.data_offset as usize;
        let body = entry.get(man_offset..)?;
        let (decoded, _consumed) = legaia_lzs::decompress_tracked(body, man.size as usize).ok()?;
        if decoded.len() != man.size as usize {
            return None;
        }
        let man_descriptor_off = SceneAssetTable::size_word_offset(man_desc_idx);
        let budget = crate::man_compressed_budget(&table, man_offset, entry.len());
        Self::locate_in_man(decoded, entry_idx, man_offset, man_descriptor_off, budget).ok()
    }

    /// The MAN-level half of [`Self::locate`], separated for testability.
    fn locate_in_man(
        decoded: Vec<u8>,
        entry_idx: usize,
        man_offset: usize,
        man_descriptor_off: usize,
        compressed_budget: usize,
    ) -> Result<Self, String> {
        let mf = man_section::parse(&decoded).map_err(|e| format!("MAN parse: {e}"))?;

        // Formation row 0: the encounter section's first 8-byte record.
        let enc = mf.sections[0];
        let enc_bytes = decoded
            .get(enc.body_offset()..enc.end_offset())
            .ok_or("encounter section out of bounds")?;
        let es = man_section::parse_encounter_section(enc_bytes)
            .map_err(|e| format!("encounter section: {e}"))?;
        if es.formation_stride != 8 || es.formation_count == 0 {
            return Err("unexpected formation table shape".into());
        }
        let formation_row = enc.body_offset() + 4;
        let row = &decoded[formation_row..formation_row + 8];
        let retail_row = row[..4] == [0, 0, 0, 1] && row[4] == 4;
        let applied_row = row[..4] == [1, 0, 0, 3] && row[4..7] == DELILAS_IDS;
        if !retail_row && !applied_row {
            return Err(format!("formation row 0 has unexpected bytes {row:02X?}"));
        }

        // Clerk record: the P1 record whose picker labels are the three
        // character-name substitution tokens.
        let dro = mf.data_region_offset;
        let mut found = None;
        for &off in &mf.partitions[1] {
            let start = dro + off as usize;
            let end = record_end(&mf, &decoded, start);
            let Some(pc0) = p01_pc0(&decoded, start) else {
                continue;
            };
            if start + pc0 >= end {
                continue;
            }
            let rec = &decoded[start..end];
            let Ok(walk) = walk_record(rec, pc0) else {
                continue;
            };
            let who = walk.pickers.iter().position(|p| {
                (p.n == 3 || p.n == 4)
                    && rec[p.open + 1 + p.n * 2..].starts_with(&name_label(0))
                    && rec[p.open + 1 + p.n * 2 + 4..].starts_with(&name_label(1))
                    && rec[p.open + 1 + p.n * 2 + 8..].starts_with(&name_label(2))
            });
            if let Some(wi) = who {
                found = Some((start, pc0, end, walk, wi));
                break;
            }
        }
        let Some((rec_start, rec_pc0, rec_end, walk, who_idx)) = found else {
            return Err("no P1 record carries the who-enrolls picker".into());
        };
        let who = walk.pickers[who_idx].clone();
        let already_applied = who.n == 4 && applied_row;
        if (who.n == 4) != applied_row {
            return Err("inconsistent partial application (picker vs formation)".into());
        }

        // Common tail: the shared JmpRel target right after each `0x3E 0x69`
        // Muscle-Dome warp op, taken from the *walked* instruction stream.
        let rec = &decoded[rec_start..rec_end];
        let mut tails: Vec<usize> = Vec::new();
        for (i, &(pc, op, size)) in walk.insns.iter().enumerate() {
            if op == 0x3E && rec[pc + 1] == 0x69 {
                let Some(&(jpc, jop, _)) = walk.insns.get(i + 1) else {
                    continue;
                };
                if jop == 0x26 && jpc == pc + size {
                    let d = u16::from_le_bytes([rec[jpc + 1], rec[jpc + 2]]) as usize;
                    tails.push((jpc + 1 + d) & 0xFFFF);
                }
            }
        }
        tails.sort_unstable();
        tails.dedup();
        let [common_tail] = tails[..] else {
            return Err(format!(
                "expected one shared dome-warp tail, found {tails:?}"
            ));
        };
        if common_tail >= walk.end {
            return Err("common tail outside the walked region".into());
        }

        // The quick-path skip tests: two SYSTEM-flag TESTs of `0x559`/`0x558`
        // before the who-picker, both jumping to the same auto-register-Vahn
        // block. Retail latches those flags the first time Noa / Gala refuse
        // enrollment, after which the who-menu never shows again - which
        // would strand the challenge option. They are NOPed by the build.
        let skip_tests: Vec<(usize, usize)> = if already_applied {
            Vec::new()
        } else {
            let mut sites: Vec<(usize, usize, usize)> = Vec::new();
            for &(pc, op, size) in &walk.insns {
                if pc < who.open
                    && (op & 0xF0) == 0x70
                    && size == 4
                    && matches!(rec[pc + 1], 0x58 | 0x59)
                {
                    let d = u16::from_le_bytes([rec[pc + 2], rec[pc + 3]]) as usize;
                    sites.push((pc, size, (pc + 2 + d) & 0xFFFF));
                }
            }
            let [(pc_a, sz_a, t_a), (pc_b, sz_b, t_b)] = sites[..] else {
                return Err(format!(
                    "expected two who-menu skip tests, found {}",
                    sites.len()
                ));
            };
            if t_a != t_b {
                return Err("who-menu skip tests disagree on their target".into());
            }
            vec![(pc_a, sz_a), (pc_b, sz_b)]
        };

        // Decline tail: the last option of the difficulty picker (the only
        // 4-option picker on an unpatched disc) - the record's shared
        // "On second thought, forget it." exit.
        let decline_tail = if already_applied {
            0
        } else {
            let four: Vec<&PickerSite> = walk.pickers.iter().filter(|p| p.n == 4).collect();
            let [diff] = four[..] else {
                return Err(format!(
                    "expected one 4-option picker pre-patch, found {}",
                    four.len()
                ));
            };
            let t = diff.entries[3].target;
            if t >= walk.end {
                return Err("decline tail outside the walked region".into());
            }
            t
        };

        // Scene-entry record P1[0].
        let entry_rec_start = dro + *mf.partitions[1].first().ok_or("empty partition 1")? as usize;
        let entry_rec_pc0 =
            p01_pc0(&decoded, entry_rec_start).ok_or("P1[0] header out of bounds")?;

        Ok(Self {
            entry_idx,
            man_offset,
            man_descriptor_off,
            compressed_budget,
            decoded,
            already_applied,
            formation_row,
            rec_start,
            rec_pc0,
            rec_end,
            who_open: who.open,
            who_entries: who.entries.clone(),
            common_tail,
            decline_tail,
            skip_tests,
            refs: walk.refs,
            entry_rec_start,
            entry_rec_pc0,
        })
    }

    /// Build the patched MAN with the given victory prizes. Returns the new
    /// decompressed MAN plus the number of inserted bytes. Errors if any
    /// verification step fails; never writes.
    pub fn build(&self, rewards: &DelilasRewards) -> Result<(Vec<u8>, usize), String> {
        if self.already_applied {
            return Err("already applied".into());
        }
        let man = &self.decoded;
        let rec_len = self.rec_end - self.rec_start;
        let rec = &man[self.rec_start..self.rec_end];
        let skip_pcs: Vec<usize> = self.skip_tests.iter().map(|&(pc, _)| pc).collect();
        let expected_sig: Vec<u8> = walk_signature(rec, self.rec_pc0)?
            .into_iter()
            .filter(|(pc, _)| !skip_pcs.contains(pc))
            .map(|(_, t)| t)
            .collect();

        // --- Record-internal insertion plan (record-relative offsets). ---
        let entry_ins_at = self.who_open + 1 + 3 * 2; // after the 3 entries
        let label_ins_at = entry_ins_at + 12; // after the 3 name labels
        let label_bytes = seg(OPTION_LABEL);
        let branch_ins_at = rec_len; // record end
        let pre_branch_shift = 2 + label_bytes.len();

        // Record-relative shift for a point in the OLD record layout.
        let shift = |x: usize| -> usize {
            let mut s = 0;
            if x >= entry_ins_at {
                s += 2;
            }
            if x >= label_ins_at {
                s += label_bytes.len();
            }
            s
        };
        // No pre-existing jump may land exactly on an insertion seam - the
        // "before or after the new bytes" question would be ambiguous.
        for r in &self.refs {
            if r.target == entry_ins_at || r.target == label_ins_at {
                return Err(format!(
                    "jump target +0x{:04X} sits exactly on an insertion seam",
                    r.target
                ));
            }
        }

        let branch_start = branch_ins_at + pre_branch_shift;
        let common_tail_new = self.common_tail + shift(self.common_tail);
        let decline_tail_new = self.decline_tail + shift(self.decline_tail);
        let (branch, bmeta) = build_branch(branch_start, common_tail_new, decline_tail_new)?;

        // --- Same-size rewrites, applied to a copy at OLD offsets. ---
        let mut out = man.clone();

        // Formation row 0 -> the Delilas trio (boss-intro header byte).
        let fr = self.formation_row;
        out[fr..fr + 8].copy_from_slice(&[
            0x01,
            0x00,
            0x00,
            0x03,
            DELILAS_IDS[0],
            DELILAS_IDS[1],
            DELILAS_IDS[2],
            0x00,
        ]);

        // Who-picker: open byte 0x28 -> 0x29 (preserve a stored 0x80 bit).
        let open_abs = self.rec_start + self.who_open;
        if out[open_abs] & 0x7F != 0x28 {
            return Err("who-picker open byte is not 0x28".into());
        }
        out[open_abs] = (out[open_abs] & 0x80) | 0x29;

        // NOP the who-menu skip tests so the enrollment menu (and with it the
        // challenge option) always shows; their delta fields die with them.
        for &(pc, size) in &self.skip_tests {
            let abs = self.rec_start + pc;
            out[abs..abs + size].fill(0x21);
        }
        let dead_fields: Vec<usize> = self.skip_tests.iter().map(|&(pc, _)| pc + 2).collect();

        // Every collected control-flow field: rewrite with post-shift values.
        for r in &self.refs {
            if dead_fields.contains(&r.field) {
                continue;
            }
            let f_new = r.field + shift(r.field);
            let t_new = r.target + shift(r.target);
            let value: u16 = match r.kind {
                RefKind::Relative => (t_new.wrapping_sub(f_new)) as u16,
                RefKind::Absolute => t_new as u16,
            };
            let abs = self.rec_start + r.field;
            out[abs..abs + 2].copy_from_slice(&value.to_le_bytes());
        }

        // --- Splice the four insertions (absolute offsets, ascending). ---
        // The new 4th jump entry lands AT the seam, so in the new layout its
        // own field offset is exactly `entry_ins_at`.
        let opt3_delta = (branch_start.wrapping_sub(entry_ins_at)) as u16;
        let p10_block = build_restore_block(rewards);
        let inserts: [(usize, Vec<u8>); 4] = [
            (self.entry_rec_start + self.entry_rec_pc0, p10_block),
            (
                self.rec_start + entry_ins_at,
                opt3_delta.to_le_bytes().to_vec(),
            ),
            (self.rec_start + label_ins_at, label_bytes.clone()),
            (self.rec_start + branch_ins_at, branch.clone()),
        ];
        if !inserts.windows(2).all(|w| w[0].0 <= w[1].0) {
            return Err("insertions out of order".into());
        }
        let mf = man_section::parse(man).map_err(|e| format!("MAN reparse: {e}"))?;
        if inserts.iter().any(|(at, _)| *at >= mf.sections[0].offset) {
            return Err("insertion would land inside the section chain".into());
        }

        let grown: usize = inserts.iter().map(|(_, b)| b.len()).sum();
        let mut new_man = Vec::with_capacity(out.len() + grown);
        let mut cursor = 0usize;
        for (at, bytes) in &inserts {
            new_man.extend_from_slice(&out[cursor..*at]);
            new_man.extend_from_slice(bytes);
            cursor = *at;
        }
        new_man.extend_from_slice(&out[cursor..]);

        // --- Header fixups: partition tables + u24_at_28. ---
        let abs_shift = |x: usize| -> usize {
            inserts
                .iter()
                .map(|(at, b)| if *at <= x { b.len() } else { 0 })
                .sum()
        };
        let dro = mf.data_region_offset;
        let mut table_pos = man_section::RECORDS_BEGIN_OFFSET;
        for part in &mf.partitions {
            for &off in part {
                let old_start = dro + off as usize;
                let new_off = off as usize + abs_shift(old_start);
                write_u24(&mut new_man, table_pos, new_off as u32)?;
                table_pos += 3;
            }
        }
        let new_u24 = mf.header.u24_at_28 as usize + grown;
        write_u24(&mut new_man, man_section::U24_AT_28_OFFSET, new_u24 as u32)?;

        // --- Verification. ---
        let nmf = man_section::parse(&new_man).map_err(|e| format!("rebuilt MAN: {e}"))?;
        // Shift for points strictly inside the clerk record (its own start
        // moves only by insertions in earlier records).
        let n_start = self.rec_start + abs_shift(self.rec_start.saturating_sub(1));
        let n_len = rec_len + pre_branch_shift + branch.len();
        let n_rec = &new_man[n_start..n_start + n_len];
        // The rebuilt clerk record must decode as the original stream.
        let new_sig: Vec<u8> = walk_signature(n_rec, self.rec_pc0)?
            .into_iter()
            .map(|(_, t)| t)
            .collect();
        if new_sig[..expected_sig.len().min(new_sig.len())] != expected_sig[..] {
            return Err("rebuilt record diverges from the original stream".into());
        }
        // The who-picker must re-parse as a 4-option picker: options 0..2
        // relocated intact, option 3 targeting the branch.
        let who_new =
            parse_picker_at(n_rec, self.who_open).ok_or("rebuilt who-picker does not parse")?;
        if who_new.n != 4 {
            return Err("rebuilt who-picker is not 4-wide".into());
        }
        if who_new.jump_target(3) != Some(branch_start) {
            return Err("4th option does not target the branch".into());
        }
        for (i, e) in self.who_entries.iter().enumerate() {
            if who_new.jump_target(i) != Some(e.target + shift(e.target)) {
                return Err(format!("option {i} target mis-relocated"));
            }
        }
        // The branch itself must decode: its two pickers parse with the
        // authored arity and in-branch targets, the gate opens it, and the
        // battle op is present.
        let b = &n_rec[branch_start..branch_start + branch.len()];
        let gate = sysflag_test(KORU_DEFEATED_FLAG, 0);
        if b[..2] != gate[..2] {
            return Err("branch does not open with the Koru gate".into());
        }
        if !b.windows(3).any(|w| w == [0x3E, 0xFF, 0x00]) {
            return Err("branch lost its battle op".into());
        }
        for (open_local, n) in [(bmeta.picker_mode, 2), (bmeta.picker_fighter, 3)] {
            let p = parse_picker_at(n_rec, branch_start + open_local)
                .ok_or("branch picker does not parse")?;
            if p.n != n {
                return Err("branch picker has the wrong arity".into());
            }
            for i in 0..p.n {
                let t = p
                    .jump_target(i)
                    .ok_or("branch picker target unresolvable")?;
                if t < branch_start || t >= branch_start + branch.len() {
                    return Err("branch picker target escapes the branch".into());
                }
            }
        }
        // Encounter row must decode as the trio.
        let enc = nmf.sections[0];
        let es =
            man_section::parse_encounter_section(&new_man[enc.body_offset()..enc.end_offset()])
                .map_err(|e| format!("rebuilt encounter section: {e}"))?;
        if es.formation_stride != 8 {
            return Err("rebuilt formation stride changed".into());
        }
        let nfr = enc.body_offset() + 4;
        if new_man[nfr..nfr + 7] != [1, 0, 0, 3, DELILAS_IDS[0], DELILAS_IDS[1], DELILAS_IDS[2]] {
            return Err("rebuilt formation row 0 wrong".into());
        }
        // The restore block must decode at the top of P1[0].
        let n_entry_start =
            self.entry_rec_start + abs_shift(self.entry_rec_start.saturating_sub(1));
        let blk_at = n_entry_start + self.entry_rec_pc0;
        if new_man[blk_at..blk_at + 2] != sysflag_test(MARKER_SOLO, 0)[..2] {
            return Err("restore block missing from the entry script".into());
        }
        Ok((new_man, grown))
    }
}

/// One-past-end of the record starting at `start` (bounded by the next record
/// start or the first section).
fn record_end(mf: &ManFile, man: &[u8], start: usize) -> usize {
    let dro = mf.data_region_offset;
    let mut end = man.len();
    for part in &mf.partitions {
        for &off in part {
            let s = dro + off as usize;
            if s > start && s < end {
                end = s;
            }
        }
    }
    for s in &mf.sections {
        if s.offset > start && s.offset < end {
            end = s.offset;
        }
    }
    end
}

/// Partition-0/1 record first-opcode offset: `[u8 locals][locals*2][4]`.
fn p01_pc0(man: &[u8], start: usize) -> Option<usize> {
    let locals = *man.get(start)? as usize;
    Some(1 + locals * 2 + 4)
}

fn write_u24(buf: &mut [u8], at: usize, v: u32) -> Result<(), String> {
    if v > 0x00FF_FFFF {
        return Err(format!("u24 overflow: 0x{v:X}"));
    }
    let b = v.to_le_bytes();
    buf[at] = b[0];
    buf[at + 1] = b[1];
    buf[at + 2] = b[2];
    Ok(())
}

/// Branch-local offsets of the structures [`DelilasSites::build`] verifies.
struct BranchMeta {
    /// Local offset of the solo/group `0x27` open byte.
    picker_mode: usize,
    /// Local offset of the fighter `0x28` open byte.
    picker_fighter: usize,
}

/// Build the Delilas branch bytecode. `base` is the branch's record-relative
/// start offset in the NEW layout; `common_tail` is the record-relative offset
/// (NEW layout) of the shared exit both the refusal and the post-battle path
/// jump to. All internal jumps are emitted position-correct.
fn build_branch(
    base: usize,
    common_tail: usize,
    decline_tail: usize,
) -> Result<(Vec<u8>, BranchMeta), String> {
    #[derive(Clone, Copy, PartialEq)]
    enum Label {
        Avail,
        Solo,
        Vahn,
        Noa,
        Gala,
        Group,
        Launch,
        CommonTail,
        DeclineTail,
    }
    let mut b: Vec<u8> = Vec::with_capacity(224);
    let mut fixups: Vec<(usize, Label)> = Vec::new(); // (local field off, dest)
    let mut labels: Vec<(Label, usize)> = Vec::new();
    let jmp = |b: &mut Vec<u8>, fixups: &mut Vec<(usize, Label)>, l: Label| {
        b.push(0x26);
        b.extend_from_slice(&[0, 0]);
        fixups.push((b.len() - 2, l));
    };

    // Gate: if the Koru flag is set, jump to AVAIL; else the locked-gate
    // refusal is a bare jump into the record's own decline flow ("We hope
    // you come again soon!") - zero new text, and the koin1 MAN recompresses
    // into a zero-slack footprint, so every glyph here costs real bytes.
    b.extend_from_slice(&sysflag_test(KORU_DEFEATED_FLAG, 0));
    fixups.push((b.len() - 2, Label::Avail));
    jmp(&mut b, &mut fixups, Label::DeclineTail);
    // Available: prompt + solo/group picker (immediate-labels form). The
    // prompt reuses a retail line of this very record verbatim so the LZS
    // window folds it to a few bytes.
    labels.push((Label::Avail, b.len()));
    b.extend(seg("Which do you want to enter?"));
    let picker_mode = b.len();
    b.push(0x27);
    b.extend_from_slice(&[0, 0]);
    fixups.push((b.len() - 2, Label::Solo));
    b.extend_from_slice(&[0, 0]);
    fixups.push((b.len() - 2, Label::Group));
    // Capitalized to tail-match the "Delilas Challenge" option label a few
    // hundred bytes earlier in the LZS window.
    b.extend(seg("Solo Challenge"));
    b.extend(seg("Group Challenge"));
    // Solo: fighter picker. Prompt lines reuse this record's retail text
    // verbatim so the LZS window folds them to a few control bytes.
    labels.push((Label::Solo, b.len()));
    b.extend(seg("First, tell me which one of you"));
    b.extend(seg("will be entering."));
    let picker_fighter = b.len();
    b.push(0x28);
    for l in [Label::Vahn, Label::Noa, Label::Gala] {
        b.extend_from_slice(&[0, 0]);
        fixups.push((b.len() - 2, l));
    }
    b.extend_from_slice(&name_label(0));
    b.extend_from_slice(&name_label(1));
    b.extend_from_slice(&name_label(2));
    // Solo arms: strip the party to the chosen fighter (the retail ravine
    // duels' own PARTY_REMOVE idiom) and latch the solo marker.
    let solo = sysflag_set(MARKER_SOLO);
    labels.push((Label::Vahn, b.len()));
    b.extend_from_slice(&[0x3D, 0x01, 0x3D, 0x02]);
    b.extend_from_slice(&solo);
    jmp(&mut b, &mut fixups, Label::Launch);
    labels.push((Label::Noa, b.len()));
    b.extend_from_slice(&[0x3D, 0x00, 0x3D, 0x02]);
    b.extend_from_slice(&solo);
    jmp(&mut b, &mut fixups, Label::Launch);
    labels.push((Label::Gala, b.len()));
    b.extend_from_slice(&[0x3D, 0x00, 0x3D, 0x01]);
    b.extend_from_slice(&solo);
    jmp(&mut b, &mut fixups, Label::Launch);
    // Group arm: full party, latch the group marker, fall into LAUNCH.
    labels.push((Label::Group, b.len()));
    b.extend_from_slice(&sysflag_set(MARKER_GROUP));
    labels.push((Label::Launch, b.len()));
    // Raise retail's scripted-loss latch so a wipe returns to the venue
    // instead of the continue screen (the Tetsu-spar `50 00` idiom), fight
    // formation row 0, then rejoin the record's common tail. No BGM op: the
    // Tetsu spar launches bare too - battle init cues the battle music
    // itself (and the zero-slack MAN wants the 4 bytes back).
    b.extend_from_slice(&sysflag_set(SCRIPTED_LOSS_FLAG));
    b.extend_from_slice(&[0x3E, 0xFF, 0x00]);
    jmp(&mut b, &mut fixups, Label::CommonTail);

    let resolve = |l: Label| -> Result<usize, String> {
        match l {
            Label::CommonTail => return Ok(common_tail),
            Label::DeclineTail => return Ok(decline_tail),
            _ => {}
        }
        labels
            .iter()
            .find(|(k, _)| *k == l)
            .map(|(_, off)| base + off)
            .ok_or_else(|| "unresolved branch label".to_string())
    };
    for (field_local, label) in fixups {
        let field = base + field_local;
        let target = resolve(label)?;
        let delta = (target.wrapping_sub(field)) as u16;
        b[field_local..field_local + 2].copy_from_slice(&delta.to_le_bytes());
    }
    Ok((
        b,
        BranchMeta {
            picker_mode,
            picker_fighter,
        },
    ))
}

/// The guarded outcome block spliced at the top of `koin1`'s scene-entry
/// script. A scripted battle returns through a full scene reload, so this is
/// where the challenge is scored. Position-independent: every jump is local
/// (the outcome flag is global state, not an offset).
///
/// ```text
///       if MARKER_SOLO  -> SOLO
///       if MARKER_GROUP -> GRP
///       jmp END
/// SOLO: clear MARKER_SOLO
///       3D 00/01/02  3C 00/01/02      ; recompose Vahn / Noa / Gala
///       if OUTCOME_SURVIVED -> WIN3, else jmp LOSS
/// WIN3: grant solo surplus            ; falls into WIN1
/// WIN1: grant group prize; jmp END
/// GRP:  clear MARKER_GROUP
///       if OUTCOME_SURVIVED -> WIN1   ; falls into LOSS
/// LOSS: 4C 82 0/1/2                   ; full restore - nobody leaves the
///                                     ; venue at 0 HP (no game over)
/// END:
/// ```
///
/// The split between WIN3 and WIN1 assumes the default same-item rewards
/// (solo = surplus + shared tail); distinct items emit separate grant runs.
fn build_restore_block(rewards: &DelilasRewards) -> Vec<u8> {
    #[derive(Clone, Copy, PartialEq)]
    enum L {
        Solo,
        Win3,
        Grp,
        Loss,
        Win1,
        End,
    }
    let mut b: Vec<u8> = Vec::with_capacity(64);
    let mut fixups: Vec<(usize, L)> = Vec::new();
    let mut labels: Vec<(L, usize)> = Vec::new();
    let jmp = |b: &mut Vec<u8>, fixups: &mut Vec<(usize, L)>, l: L| {
        b.push(0x26);
        b.extend_from_slice(&[0, 0]);
        fixups.push((b.len() - 2, l));
    };
    let survived = |b: &mut Vec<u8>, fixups: &mut Vec<(usize, L)>, l: L| {
        b.extend_from_slice(&sysflag_test(OUTCOME_SURVIVED_FLAG, 0));
        fixups.push((b.len() - 2, l));
    };

    b.extend_from_slice(&sysflag_test(MARKER_SOLO, 0));
    fixups.push((b.len() - 2, L::Solo));
    b.extend_from_slice(&sysflag_test(MARKER_GROUP, 0));
    fixups.push((b.len() - 2, L::Grp));
    jmp(&mut b, &mut fixups, L::End);

    labels.push((L::Solo, b.len()));
    b.extend_from_slice(&sysflag_clear(MARKER_SOLO));
    b.extend_from_slice(&[0x3D, 0x00, 0x3D, 0x01, 0x3D, 0x02]);
    b.extend_from_slice(&[0x3C, 0x00, 0x3C, 0x01, 0x3C, 0x02]);
    survived(&mut b, &mut fixups, L::Win3);
    jmp(&mut b, &mut fixups, L::Loss);
    labels.push((L::Win3, b.len()));
    let shared_tail =
        rewards.solo_item == rewards.group_item && rewards.solo_count >= rewards.group_count;
    if shared_tail {
        for _ in 0..rewards.solo_count - rewards.group_count {
            b.extend_from_slice(&[0x39, rewards.solo_item]);
        }
        // ...falls into WIN1.
    } else {
        for _ in 0..rewards.solo_count {
            b.extend_from_slice(&[0x39, rewards.solo_item]);
        }
        jmp(&mut b, &mut fixups, L::End);
    }
    labels.push((L::Win1, b.len()));
    for _ in 0..rewards.group_count {
        b.extend_from_slice(&[0x39, rewards.group_item]);
    }
    jmp(&mut b, &mut fixups, L::End);

    labels.push((L::Grp, b.len()));
    b.extend_from_slice(&sysflag_clear(MARKER_GROUP));
    survived(&mut b, &mut fixups, L::Win1);
    // ...falls into LOSS, which falls into END.
    labels.push((L::Loss, b.len()));
    b.extend_from_slice(&[0x4C, 0x82, 0x00, 0x4C, 0x82, 0x01, 0x4C, 0x82, 0x02]);
    labels.push((L::End, b.len()));

    for (field, label) in fixups {
        let target = labels
            .iter()
            .find(|(k, _)| *k == label)
            .map(|(_, off)| *off)
            .expect("restore-block label");
        let delta = (target.wrapping_sub(field)) as u16;
        b[field..field + 2].copy_from_slice(&delta.to_le_bytes());
    }
    b
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn restore_block_shape() {
        let rewards = DelilasRewards::default();
        let b = build_restore_block(&rewards);
        // Opens with the two marker tests, then the not-fought skip.
        assert_eq!(&b[..2], &[0x7D, 0x76]);
        assert_eq!(&b[4..6], &[0x7D, 0x77]);
        assert_eq!(b[8], 0x26);
        // Marker clears present for both arms.
        assert_eq!(b.windows(2).filter(|w| *w == [0x6D, 0x76]).count(), 1);
        assert_eq!(b.windows(2).filter(|w| *w == [0x6D, 0x77]).count(), 1);
        // Party recompose.
        assert!(b.windows(12).any(|w| w
            == [
                0x3D, 0x00, 0x3D, 0x01, 0x3D, 0x02, 0x3C, 0x00, 0x3C, 0x01, 0x3C, 0x02
            ]));
        // One battle-outcome test per arm (retail's survived flag, index 1).
        let outcome = sysflag_test(OUTCOME_SURVIVED_FLAG, 0);
        assert_eq!(
            b.windows(2).filter(|w| *w == &outcome[..2]).count(),
            2,
            "solo + group outcome tests"
        );
        // Prize grants: the shared tail emits solo_count give ops in total
        // (surplus in WIN3 + the group grant in WIN1 the solo path falls
        // into), which the group path enters at WIN1.
        let give = [0x39, DEFAULT_REWARD_ITEM];
        assert_eq!(
            b.windows(2).filter(|w| *w == give).count(),
            DEFAULT_SOLO_REWARD_COUNT as usize
        );
        // Loss path: three full restores.
        assert!(
            b.windows(9)
                .any(|w| w == [0x4C, 0x82, 0x00, 0x4C, 0x82, 0x01, 0x4C, 0x82, 0x02])
        );
        // The whole block decodes as field-VM ops, and every collected jump
        // stays inside the block (END = one past the block, where the host
        // record's original first instruction sits).
        let base = 4;
        let mut rec = vec![0x21u8; base];
        rec.extend_from_slice(&b);
        rec.push(0x21);
        let w = walk_record(&rec, base).unwrap();
        assert_eq!(w.end, rec.len());
        for r in &w.refs {
            assert!(r.target >= base && r.target <= base + b.len());
        }
    }

    #[test]
    fn sysflag_clear_encoding() {
        assert_eq!(sysflag_clear(0xD76), [0x6D, 0x76]);
        assert_eq!(sysflag_clear(0x378), [0x63, 0x78]);
    }

    #[test]
    fn branch_assembles_with_correct_jumps() {
        let base = 0x0FF4;
        let tail = 0x0FDF;
        let (b, meta) = build_branch(base, tail, 0x0F5A).unwrap();
        // Opens with the Koru gate (op 0x73, operand 0x78).
        assert_eq!(b[0], 0x73);
        assert_eq!(b[1], 0x78);
        // Gate target: field at local +2 -> AVAIL, inside the branch, at a
        // text segment.
        let d = u16::from_le_bytes([b[2], b[3]]) as usize;
        let avail = (base + 2 + d) & 0xFFFF;
        assert!(avail > base && avail < base + b.len());
        assert_eq!(b[avail - base], 0x1F);
        // Battle op present exactly once, against formation row 0.
        let battles = b.windows(3).filter(|w| *w == [0x3E, 0xFF, 0x00]).count();
        assert_eq!(battles, 1);
        // Solo marker latch in each of the three solo arms, one group latch,
        // and one scripted-loss latch before the battle op.
        let solo = sysflag_set(MARKER_SOLO);
        assert_eq!(b.windows(2).filter(|w| *w == solo).count(), 3);
        let group = sysflag_set(MARKER_GROUP);
        assert_eq!(b.windows(2).filter(|w| *w == group).count(), 1);
        let loss = sysflag_set(SCRIPTED_LOSS_FLAG);
        assert_eq!(b.windows(2).filter(|w| *w == loss).count(), 1);
        let battle_at = b.windows(3).position(|w| w == [0x3E, 0xFF, 0x00]).unwrap();
        let loss_at = b.windows(2).position(|w| *w == loss).unwrap();
        assert!(loss_at < battle_at, "loss latch must precede the battle op");
        // Place the branch at `base` in a synthetic record and walk it: the
        // walk must consume every byte and see the two pickers.
        let mut rec = vec![0x21u8; base];
        rec[base - 1] = 0x00; // gate op follows a terminator in the real record
        rec.extend_from_slice(&b);
        let w = walk_record(&rec, base).unwrap();
        assert_eq!(w.end, rec.len(), "branch walk must consume every byte");
        assert_eq!(w.pickers.len(), 2, "solo/group + fighter pickers");
        assert_eq!(w.pickers[0].open, base + meta.picker_mode);
        assert_eq!(w.pickers[0].n, 2);
        assert_eq!(w.pickers[1].open, base + meta.picker_fighter);
        assert_eq!(w.pickers[1].n, 3);
        // Every picker target stays inside the branch.
        for p in &w.pickers {
            for e in &p.entries {
                assert!(e.target >= base && e.target < base + b.len());
            }
        }
    }
}
