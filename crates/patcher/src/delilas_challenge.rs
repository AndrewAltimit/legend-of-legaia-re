//! **Delilas Challenge** - a new enrollment option at the Muscle Dome.
//!
//! The Muscle Dome entry clerk (scene `koin1`, the P1 interaction record that
//! carries the enter / who-enrolls / difficulty menus) gains a fourth option on
//! the "who will be entering" picker: **Delilas Challenge**. Picking it warps
//! into the dome arena and runs a brand-new 2-round contest course - Che & Lu
//! together (1v2), then Gi (1v1) - installed by the companion code injection
//! [`crate::delilas_dome`], which also pays 5000 coins for a full clear.
//! (The double-team fits the battle heap by streaming slim clones from
//! unreachable archive slots - the arithmetic and the mechanism are in the
//! `delilas_dome` module doc and `docs/subsystems/battle.md`.)
//!
//! ## Why the arena, not a scripted battle
//!
//! An earlier design launched a normal `3E FF` battle straight from `koin1`.
//! Live testing killed it: `koin1` is a town scene with no random encounters,
//! so it never installs the battle-effect / summon / player-magic asset
//! residency, and casting a spell (or opening the magic list) dereferences
//! unloaded buffers and freezes - which is exactly why the retail Muscle Dome
//! disables magic. And the battle heap's distinct-monster budget (~145 KB)
//! holds only one full Delilas block (~82 KB each) plus a normal-sized
//! partner - the double-team round only fits because the course streams
//! slim clones (see [`crate::delilas_dome`]). Routing the challenge through
//! the dome's arena fixes both: the arena stages its own rounds and disables
//! magic by design. This module is therefore only the **casino-menu half** -
//! the menu option and the warp; the course itself is [`crate::delilas_dome`].
//!
//! ## Mechanism - script + data, plus the companion code hook
//!
//! Retail's three difficulty arms (Beginner / Expert / Master) each do the
//! same thing: set the dome-active flag [`DOME_ACTIVE_FLAG`] (`0x509`), set
//! **one** of the course-unlock flags [`COURSE_UNLOCK_FLAGS`]
//! (`0x536`/`0x537`/`0x538`) while clearing the other two, then a short
//! BGM + wait flourish and the `3E 69` arena warp. The arena init reads those
//! flags to pick the course. The Delilas branch mirrors that arm exactly, but
//! requests **course 3** by setting the extra flag [`crate::delilas_dome::COURSE_FLAG`]
//! (`0x539`) that the [`crate::delilas_dome`] seed hook decodes. So:
//!
//! - **Menu.** The 3-option `0x28` "who will enter" picker grows to a 4-option
//!   `0x29` one (the picker arity ceiling - see `crates/mes/src/picker.rs`).
//!   The new option's branch is appended at the record's end. The two
//!   quick-path skip tests before the picker (flags `0x559`/`0x558`: once
//!   Noa or Gala has refused enrollment, retail skips the who-menu forever
//!   and auto-registers Vahn) are **retargeted at never-set flags**
//!   ([`NEVER_SET_FLAGS`]) so the menu always shows - without that, any save
//!   that ever picked Noa or Gala could never reach the challenge option.
//!   They are *not* NOPed: `0x21` (the byte an earlier build filled them
//!   with) is the field VM's **frame-yield/stop opcode**, not a no-op - the
//!   run-until-yield slice stops on it (`docs/subsystems/script-vm.md`), and
//!   eight of them in a row broke the clerk dialog mid-interaction (the
//!   "have to re-talk the NPC several times" live-test bug). Retargeting the
//!   flag id keeps the exact retail op shape with zero new semantics.
//! - **Gate.** The branch opens with a `0x70`-family SYSTEM-flag test of
//!   [`KORU_DEFEATED_FLAG`] (`0x378`) - the exact flag the game latches when
//!   Koru dies and the world-map ravine entrance flips from `nilboa` to
//!   `nilboa2` (`map03 P2[15]` sets it, `map03 P2[3]` selects the scene on it).
//!   Until then the clerk routes to its own decline flow.
//! - **Warp.** The available arm shows a short confirm picker (the challenge
//!   warps to the arena the moment it launches, so a misclick needs an out;
//!   "cancel" routes into the record's own decline flow), then mirrors a
//!   retail difficulty arm: set [`DOME_ACTIVE_FLAG`], clear the three
//!   [`COURSE_UNLOCK_FLAGS`], set the course-3 request flag, then the
//!   verbatim BGM ([`ARENA_ENTER_BGM`]) + wait ops and the verbatim `3E 69`
//!   warp ([`DOME_WARP_OP`]). Losing or winning is scored by the dome itself
//!   (a lost leg routes back through the arena hub - the battle-exit
//!   selector's `_DAT_8007BAC0 & 0x100` test, which is why the injected seed
//!   word carries bit `0x100`), so no marker/outcome bookkeeping lives here.
//! - **Reward announcement.** The post-contest award ceremony (the P2
//!   narration record P1[0] spawns on a non-gave-up contest end) reads
//!   "Contestant {name} is awarded / {n} tokens!" - and the Delilas course
//!   sits outside the token payout the `{ce}` counter substitutes, so a win
//!   read "0 tokens" while saying nothing about the items actually granted.
//!   The patch splices four SYSTEM-flag tests at the record's own branch
//!   point (right after its `76 CB` / `75 38` tests, before the message
//!   run - the actor-dialog SM consumes a `[1F..][24][1F..][48]` flow as
//!   one contiguous run, so ops may only sit at flow boundaries): any
//!   course-unlock flag set ([`COURSE_UNLOCK_FLAGS`] - a retail arm always
//!   sets its own, the Delilas arm clears all three) keeps the retail flow;
//!   otherwise the contest-won flag ([`CONTEST_WON_FLAG`] `0x50A`, set only
//!   by a winning settlement and untouched anywhere in `koin1`) routes to a
//!   complete parallel flow appended past the record: the shared box copied
//!   verbatim, a reward box ([`reward_box_lines`]), the verbatim close +
//!   settle ops, and a jump to the same park loop both retail arms converge
//!   on. A Delilas loss falls through into the retail flow.
//!
//! The dome fields whichever fighter the arena normally seats (retail's Muscle
//! Dome enrolls Vahn); routing a *chosen* party member into the arena's fighter
//! slot is an open RE thread, so this ships as the default-fighter contest.
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

/// Retail's dome-contest-active SYSTEM flag (`0x509`). Every difficulty arm
/// sets it (`55 09`) before the arena warp; the Delilas arm mirrors that.
pub const DOME_ACTIVE_FLAG: u16 = 0x509;

/// The three course-unlock SYSTEM flags a retail difficulty arm toggles - it
/// sets one (Beginner `0x536` / Expert `0x537` / Master `0x538`) and clears
/// the other two, and the arena init reads them to pick the course. The
/// Delilas arm clears all three and instead sets
/// [`crate::delilas_dome::COURSE_FLAG`] so the injected seed hook selects the
/// 4th course.
pub const COURSE_UNLOCK_FLAGS: [u16; 3] = [0x536, 0x537, 0x538];

/// The arena-entry BGM op (`0x34`) copied verbatim from a retail difficulty
/// arm, so the Delilas warp plays the same music cue on entry.
pub const ARENA_ENTER_BGM: [u8; 7] = [0x34, 0x01, 0xFF, 0xFF, 0xFF, 0x1E, 0x00];
/// The two `0x4A` wait ops a retail difficulty arm runs before the warp,
/// copied verbatim (arena transition pacing).
pub const ARENA_ENTER_WAIT_A: [u8; 3] = [0x4A, 0x1E, 0x00];
/// See [`ARENA_ENTER_WAIT_A`].
pub const ARENA_ENTER_WAIT_B: [u8; 3] = [0x4A, 0x08, 0x00];

/// The Muscle-Dome arena warp op (`3E 69`, the 6-byte warp form of `0x3E`),
/// copied verbatim from a retail difficulty arm - it enters the dome arena
/// (game mode 24); the course is selected by the flags set just before it.
pub const DOME_WARP_OP: [u8; 6] = [0x3E, 0x69, 0x00, 0x00, 0x00, 0x00];

/// The two never-set SYSTEM flags the who-menu skip tests are retargeted at
/// (in order). Chosen from the high band that reads zero across retail saves
/// and absent from both the field-VM and motion-VM disc-wide flag censuses -
/// and nothing in this patch (or retail) ever sets them, so the retargeted
/// tests always fall through into the who-menu. Same 4-byte op shape as the
/// originals; only the op's flag-nibble + id byte change.
pub const NEVER_SET_FLAGS: [u16; 2] = [0xD76, 0xD77];

/// The picker option label shown on the grown enrollment menu.
pub const OPTION_LABEL: &str = "Delilas Challenge";

/// Retail's contest-won SYSTEM flag (`0x50A`): the arena settlement
/// (`FUN_801D0F60`) clears it at the top of every settlement and sets it
/// only on the cleared-latch (course exhausted AND survived) path - see
/// `docs/subsystems/minigame-muscle-dome.md`. Nothing in `koin1` touches it,
/// so at award-ceremony time it still says whether the contest was won.
pub const CONTEST_WON_FLAG: u16 = 0x50A;

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
    /// Vahn). The patch retargets them at [`NEVER_SET_FLAGS`] so the menu -
    /// and the challenge option - always shows. Empty when already applied.
    skip_tests: Vec<(usize, usize)>,
    /// Every control-flow field in the clerk record (opcode deltas + every
    /// picker's jump entries), record-relative.
    refs: Vec<RefSite>,

    /// The award-ceremony narration record (P2[9] on a retail disc) - the
    /// record P1[0] spawns on a non-gave-up contest end, carrying the
    /// "Contestant {name} is awarded {n} tokens!" box. `None` when the patch
    /// is already applied (not needed, and its pre-patch shape is gone).
    narr: Option<NarrationRecord>,
}

/// The located award-ceremony narration record. `start`/`end` are absolute
/// MAN offsets; everything else is record-relative.
///
/// The runtime constraint every coordinate serves: the actor-dialog SM
/// consumes a message flow - `[1F seg]... [24] [1F seg]... [48]` - as one
/// contiguous run, and VM control ops may only sit at flow boundaries (the
/// retail record's own `76 CB` / `75 38` tests run right before the first
/// segment; a live test with ops spliced *inside* the run cut the ceremony
/// short after the first box). So the win tests go at the retail branch
/// point, and the Delilas path is a complete parallel flow.
#[derive(Debug, Clone)]
struct NarrationRecord {
    /// Absolute start of the record.
    start: usize,
    /// First-opcode offset within the record.
    pc0: usize,
    /// Absolute one-past-end of the record.
    end: usize,
    /// The insertion seam: the `1F` of "That was a good fight." - the retail
    /// branch point (the `76 CB` repeat-clear test targets exactly this
    /// offset, and the `75 38` Master test falls through to it).
    seam: usize,
    /// The `0x24` box-break between the shared box and the tokens box; the
    /// bytes `[seam..box_break]` are the shared box's three segments,
    /// copied verbatim into the parallel flow.
    box_break: usize,
    /// The `0x48` box-close after the tokens text.
    close: usize,
    /// PC of the shared-tail `0x26` jump after the close; the bytes
    /// `[close..tail26]` (close + wait + camera ops) are copied verbatim.
    tail26: usize,
    /// The `0x26`'s decoded target: the record's yield + park loop both
    /// retail arms converge on. The parallel flow's final jump goes here.
    tail_target: usize,
    /// Every control-flow field in the record, record-relative.
    refs: Vec<RefSite>,
}

/// The dialog-box lines announcing a Delilas full-clear reward, one line per
/// reward ("You won 1 Ra-Seru Tear," / "1 Nature's Elixir and" / "1 Fury
/// Bloom!"). One box holds at most three rows, and every granted reward is a
/// single item, so the count is the literal `1`.
pub fn reward_box_lines(rewards: &[&str]) -> Result<Vec<String>, String> {
    if rewards.is_empty() || rewards.len() > 3 {
        return Err(format!(
            "reward box holds 1..=3 lines, got {}",
            rewards.len()
        ));
    }
    let n = rewards.len();
    let lines: Vec<String> = rewards
        .iter()
        .enumerate()
        .map(|(i, name)| {
            let prefix = if i == 0 { "You won " } else { "" };
            let suffix = if i + 1 == n {
                "!"
            } else if i + 2 == n {
                " and"
            } else {
                ","
            };
            format!("{prefix}1 {name}{suffix}")
        })
        .collect();
    // The widest retail line in this record is 33 characters; stay inside it.
    if let Some(l) = lines.iter().find(|l| l.len() > 33) {
        return Err(format!("reward line too wide for the dialog box: {l:?}"));
    }
    Ok(lines)
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
/// one token each; the 1-byte reserved opcodes are skipped. The retargeted
/// who-menu skip tests (opcode byte changes with the flag nibble) are masked
/// out by PC at the compare site, not here.
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

/// First occurrence of `needle` in `hay`.
fn find_sub(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

/// Locate the award-ceremony narration record: the partition-2 record P1[0]
/// spawns on a non-gave-up contest end, whose plain path reads "That was a
/// good fight. / Well done. / We hope you enter again." and then opens the
/// "Contestant {name} is awarded / {n} tokens!" box.
///
/// Partition-2 records open with a Shift-JIS actor-name header whose exact
/// prologue length varies, so `pc0` is recovered empirically: the first
/// offset after the name terminator from which the MES-aware walk consumes
/// the record exactly and lands on the byte-scanned anchors (the `0x24`
/// box-break and the `0x48` box-close) as instruction boundaries.
fn locate_narration(man: &[u8], mf: &ManFile) -> Result<NarrationRecord, String> {
    const SEAM_TEXT: &[u8] = b"\x1FThat was a good fight.\x00";
    const BOX_OPEN: &[u8] = b"\x1FContestant ";
    const TOKENS_TAIL: &[u8] = b" tokens!\x00";
    let dro = mf.data_region_offset;
    let mut found = None;
    for &off in &mf.partitions[2] {
        let start = dro + off as usize;
        let end = record_end(mf, man, start);
        let Some(rec) = man.get(start..end) else {
            continue;
        };
        let Some(seam) = find_sub(rec, SEAM_TEXT) else {
            continue;
        };
        let Some(open) = find_sub(rec, BOX_OPEN) else {
            continue;
        };
        let Some(tok) = find_sub(rec, TOKENS_TAIL) else {
            continue;
        };
        let close = tok + TOKENS_TAIL.len();
        if open <= seam || rec[open - 1] != 0x24 || rec.get(close) != Some(&0x48) {
            continue;
        }
        if found.is_some() {
            return Err("two P2 records carry the award-ceremony tokens box".into());
        }
        found = Some((start, end, seam, open - 1, close));
    }
    let Some((start, end, seam, box_break, close)) = found else {
        return Err("no P2 record carries the award-ceremony tokens box".into());
    };
    let rec = &man[start..end];
    if find_sub(&rec[seam + 1..], SEAM_TEXT).is_some() {
        return Err("the shared-box text appears twice in the narration record".into());
    }

    // Recover pc0: first candidate past the actor-name terminator whose walk
    // consumes the record exactly and hits the anchors on a boundary.
    let name_nul = rec
        .iter()
        .position(|&b| b == 0)
        .ok_or("narration record has no name terminator")?;
    let mut chosen = None;
    for pc0 in name_nul + 1..(name_nul + 12).min(seam) {
        if let Ok(w) = walk_record(rec, pc0) {
            let on_boundary =
                |pc: usize, op: u8| w.insns.iter().any(|&(p, o, _)| p == pc && o == op);
            if w.end == rec.len() && on_boundary(box_break, 0x24) && on_boundary(close, 0x48) {
                chosen = Some((pc0, w));
                break;
            }
        }
    }
    let Some((pc0, walk)) = chosen else {
        return Err("no pc0 walks the narration record onto its anchors".into());
    };

    // The shared-tail jump: the first `0x26` at/after the box-close. The
    // bytes between (the close + wait + camera ops) are what the parallel
    // flow copies verbatim, so no ref field may sit inside them.
    let Some(&(tail26, _, _)) = walk
        .insns
        .iter()
        .find(|&&(pc, op, _)| pc >= close && op == 0x26)
    else {
        return Err("no shared-tail jump after the tokens box-close".into());
    };
    let Some(tail_target) = walk
        .refs
        .iter()
        .find(|r| r.kind == RefKind::Relative && r.field == tail26 + 1)
        .map(|r| r.target)
    else {
        return Err("shared-tail jump carries no decoded target".into());
    };
    if walk
        .refs
        .iter()
        .any(|r| (seam..box_break).contains(&r.field) || (close..tail26).contains(&r.field))
    {
        return Err("a ref field sits inside a to-be-copied narration region".into());
    }

    // Shape lock against a desynced walk: exactly two pre-seam refs may
    // reach at/past the seam - the `76 CB` repeat-clear test targeting
    // exactly the seam (it must land on the spliced win tests) and the
    // `75 38` Master test targeting past it. Any phantom ref a mis-decoded
    // prologue produced would trip this before the build could relocate it.
    let mut crossers: Vec<&RefSite> = walk
        .refs
        .iter()
        .filter(|r| r.field < seam && r.target >= seam)
        .collect();
    crossers.sort_by_key(|r| r.field);
    let [repeat, master] = crossers[..] else {
        return Err(format!(
            "expected two seam-crossing narration refs, found {}",
            crossers.len()
        ));
    };
    if repeat.field < 2
        || rec[repeat.field - 2..repeat.field] != [0x76, 0xCB]
        || repeat.target != seam
    {
        return Err("first seam-crosser is not the repeat-clear test onto the seam".into());
    }
    if master.field < 2
        || rec[master.field - 2..master.field] != [0x75, 0x38]
        || master.target <= seam
    {
        return Err("second seam-crosser is not the Master test past the seam".into());
    }
    if walk
        .refs
        .iter()
        .any(|r| r.target == seam && r.field != repeat.field)
    {
        return Err("an unexpected narration ref targets the seam".into());
    }
    Ok(NarrationRecord {
        start,
        pc0,
        end,
        seam,
        box_break,
        close,
        tail26,
        tail_target,
        refs: walk.refs,
    })
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
        // The who-picker is 3-wide on a retail disc and 4-wide once the
        // Delilas option is spliced in - the applied-state signal.
        let already_applied = who.n == 4;

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
        // would strand the challenge option. The build retargets them at
        // never-set flags (same op shape, always falls through).
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

        // Award-ceremony narration record (not needed - and its pre-patch
        // shape no longer present - once the patch is applied).
        let narr = if already_applied {
            None
        } else {
            Some(locate_narration(&decoded, &mf)?)
        };

        Ok(Self {
            entry_idx,
            man_offset,
            man_descriptor_off,
            compressed_budget,
            decoded,
            already_applied,
            rec_start,
            rec_pc0,
            rec_end,
            who_open: who.open,
            who_entries: who.entries.clone(),
            common_tail,
            decline_tail,
            skip_tests,
            refs: walk.refs,
            narr,
        })
    }

    /// Build the patched MAN: the enrollment menu + warp in the clerk record,
    /// and the win-path reward announcement in the award-ceremony narration
    /// record (`rewards` = the full-clear item names, one dialog row each -
    /// see [`reward_box_lines`]). Returns the new decompressed MAN plus the
    /// number of inserted bytes. Errors if any verification step fails; never
    /// writes.
    pub fn build(&self, rewards: &[&str]) -> Result<(Vec<u8>, usize), String> {
        if self.already_applied {
            return Err("already applied".into());
        }
        let man = &self.decoded;
        let rec_len = self.rec_end - self.rec_start;
        let rec = &man[self.rec_start..self.rec_end];
        // The skip-test retarget changes those ops' flag nibble (0x75 -> 0x7D),
        // so the signature compare masks them out ON BOTH SIDES by PC. The PCs
        // are stable across the splice: every skip test sits before the first
        // insertion seam (asserted below).
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
        if skip_pcs.iter().any(|&pc| pc >= entry_ins_at) {
            return Err("skip test unexpectedly at/after the first insertion seam".into());
        }

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
        let branch = build_branch(branch_start, common_tail_new, decline_tail_new)?;

        // --- Narration-record insertion plan (record-relative offsets). ---
        // Runtime constraint (live-tested the hard way): the actor-dialog SM
        // consumes a message flow `[1F seg]... [24] [1F seg]... [48]` as one
        // contiguous run - VM control ops spliced *inside* the run cut the
        // ceremony short after the first box. So the win tests sit at the
        // retail branch point (right after the record's own `76 CB`/`75 38`
        // tests, before the flow's first segment), and the Delilas path is a
        // complete parallel flow appended past the record: a verbatim copy
        // of the shared box, the reward box, a verbatim copy of the
        // close-and-settle ops, and a jump to the same yield + park loop
        // both retail arms converge on.
        let narr = self
            .narr
            .as_ref()
            .ok_or("narration sites absent on an unapplied image")?;
        let narr_len = narr.end - narr.start;
        let narr_rec = &man[narr.start..narr.end];
        let narr_sig_old = walk_signature(narr_rec, narr.pc0)?;
        let tests_len = 4 * (COURSE_UNLOCK_FLAGS.len() + 1);
        // Content moved by the splice: everything from the seam on. Jump
        // TARGETS use the strict form so the `76 CB` test's jump onto the
        // seam lands on the spliced tests (locate_narration pins that it is
        // the only ref targeting the seam).
        let narr_shift = |x: usize| if x >= narr.seam { tests_len } else { 0 };
        let narr_shift_target = |x: usize| if x > narr.seam { tests_len } else { 0 };
        let retail_new = narr.seam + tests_len; // the relocated first segment
        let block_new = narr_len + tests_len; // the appended parallel flow

        // The win tests: any retail course-unlock flag set means the
        // last-enrolled course was a retail one (each retail arm sets its
        // own; the Delilas arm clears all three), so the retail flow stands.
        // Otherwise the contest-won flag - set only by a winning settlement
        // - routes to the parallel flow. A Delilas loss falls through into
        // the retail flow (awarded 0 tokens, the retail loss reading).
        let mut tests = Vec::with_capacity(tests_len);
        for (i, &flag) in COURSE_UNLOCK_FLAGS.iter().enumerate() {
            let field = narr.seam + i * 4 + 2;
            tests.extend_from_slice(&sysflag_test(flag, retail_new.wrapping_sub(field) as u16));
        }
        let won_field = narr.seam + COURSE_UNLOCK_FLAGS.len() * 4 + 2;
        tests.extend_from_slice(&sysflag_test(
            CONTEST_WON_FLAG,
            block_new.wrapping_sub(won_field) as u16,
        ));
        if tests.len() != tests_len {
            return Err("win-test block has an unexpected size".into());
        }

        // The parallel flow: shared-box copy, box-break, one row per reward,
        // then the verbatim close-and-settle ops and the park-loop jump (a
        // backward hop via the wrapped `0x26` - retail's own idiom, e.g.
        // this record's `26 FE FF` park loop).
        let lines = reward_box_lines(rewards)?;
        let mut item_box = Vec::new();
        item_box.extend_from_slice(&narr_rec[narr.seam..narr.box_break]);
        item_box.push(0x24);
        for l in &lines {
            item_box.extend(seg(l));
        }
        item_box.extend_from_slice(&narr_rec[narr.close..narr.tail26]);
        item_box.push(0x26);
        let jfield_local = item_box.len();
        item_box.extend_from_slice(&[0, 0]);
        let jd = (narr.tail_target + narr_shift_target(narr.tail_target))
            .wrapping_sub(block_new + jfield_local) as u16;
        item_box[jfield_local..jfield_local + 2].copy_from_slice(&jd.to_le_bytes());

        // --- Same-size rewrites, applied to a copy at OLD offsets. ---
        let mut out = man.clone();

        // Who-picker: open byte 0x28 -> 0x29 (preserve a stored 0x80 bit).
        let open_abs = self.rec_start + self.who_open;
        if out[open_abs] & 0x7F != 0x28 {
            return Err("who-picker open byte is not 0x28".into());
        }
        out[open_abs] = (out[open_abs] & 0x80) | 0x29;

        // Retarget the who-menu skip tests (flags 0x559/0x558) at never-set
        // flags so the enrollment menu (and with it the challenge option)
        // always shows. NOT NOPed: `0x21` is the field VM's frame-yield/stop
        // opcode, and a run of them broke the clerk dialog mid-interaction
        // (see the module doc). Only the op's flag nibble + id byte change;
        // the delta field stays live and relocates like any other ref.
        for (i, &(pc, size)) in self.skip_tests.iter().enumerate() {
            if size != 4 || i >= NEVER_SET_FLAGS.len() {
                return Err("unexpected skip-test shape".into());
            }
            let flag = NEVER_SET_FLAGS[i];
            let abs = self.rec_start + pc;
            out[abs] = 0x70 | ((flag >> 8) as u8 & 0x0F);
            out[abs + 1] = (flag & 0xFF) as u8;
        }

        // Every collected control-flow field: rewrite with post-shift values.
        for r in &self.refs {
            let f_new = r.field + shift(r.field);
            let t_new = r.target + shift(r.target);
            let value: u16 = match r.kind {
                RefKind::Relative => (t_new.wrapping_sub(f_new)) as u16,
                RefKind::Absolute => t_new as u16,
            };
            let abs = self.rec_start + r.field;
            out[abs..abs + 2].copy_from_slice(&value.to_le_bytes());
        }

        // Same for the narration record's control-flow fields (targets use
        // the strict shift so a jump onto the seam lands on the win tests).
        for r in &narr.refs {
            let f_new = r.field + narr_shift(r.field);
            let t_new = r.target + narr_shift_target(r.target);
            let value: u16 = match r.kind {
                RefKind::Relative => (t_new.wrapping_sub(f_new)) as u16,
                RefKind::Absolute => t_new as u16,
            };
            let abs = narr.start + r.field;
            out[abs..abs + 2].copy_from_slice(&value.to_le_bytes());
        }

        // --- Splice the three insertions (absolute offsets, ascending). ---
        // The new 4th jump entry lands AT the seam, so in the new layout its
        // own field offset is exactly `entry_ins_at`.
        let opt3_delta = (branch_start.wrapping_sub(entry_ins_at)) as u16;
        let inserts: [(usize, Vec<u8>); 5] = [
            (
                self.rec_start + entry_ins_at,
                opt3_delta.to_le_bytes().to_vec(),
            ),
            (self.rec_start + label_ins_at, label_bytes.clone()),
            (self.rec_start + branch_ins_at, branch.clone()),
            (narr.start + narr.seam, tests.clone()),
            (narr.end, item_box.clone()),
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
        // The rebuilt MAN must re-parse (partition tables + sections intact).
        man_section::parse(&new_man).map_err(|e| format!("rebuilt MAN: {e}"))?;
        // Shift for points strictly inside the clerk record (its own start
        // moves only by insertions in earlier records).
        let n_start = self.rec_start + abs_shift(self.rec_start.saturating_sub(1));
        let n_len = rec_len + pre_branch_shift + branch.len();
        let n_rec = &new_man[n_start..n_start + n_len];
        // The rebuilt clerk record must decode as the original stream (the
        // retargeted skip tests masked by PC on both sides - their PCs sit
        // before every insertion seam, so old and new PCs coincide).
        let new_sig: Vec<u8> = walk_signature(n_rec, self.rec_pc0)?
            .into_iter()
            .filter(|(pc, _)| !skip_pcs.contains(pc))
            .map(|(_, t)| t)
            .collect();
        if new_sig[..expected_sig.len().min(new_sig.len())] != expected_sig[..] {
            return Err("rebuilt record diverges from the original stream".into());
        }
        // The retargeted skip tests decode at their original PCs with the
        // never-set flag ids (and no 0x21 yield bytes were introduced there).
        for (i, &pc) in skip_pcs.iter().enumerate() {
            let flag = NEVER_SET_FLAGS[i];
            let want = [0x70 | ((flag >> 8) as u8 & 0x0F), (flag & 0xFF) as u8];
            if n_rec[pc..pc + 2] != want {
                return Err("skip-test retarget missing from the rebuilt record".into());
            }
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
        // The branch itself must decode: the Koru gate opens it, it requests
        // course 3, and the arena warp op is present. The branch walks cleanly
        // to the end of the record (no stranded bytes).
        let b = &n_rec[branch_start..branch_start + branch.len()];
        let gate = sysflag_test(KORU_DEFEATED_FLAG, 0);
        if b[..2] != gate[..2] {
            return Err("branch does not open with the Koru gate".into());
        }
        let course = sysflag_set(crate::delilas_dome::COURSE_FLAG);
        if !b.windows(2).any(|w| w == course) {
            return Err("branch does not request the Delilas course".into());
        }
        if !b.windows(DOME_WARP_OP.len()).any(|w| w == DOME_WARP_OP) {
            return Err("branch lost its arena warp op".into());
        }
        // Walking the whole rebuilt record must consume every byte (no decode
        // desync introduced by the splice).
        let walk = walk_record(n_rec, self.rec_pc0)?;
        if walk.end != n_rec.len() {
            return Err(format!(
                "rebuilt record walk stalled at +0x{:04X} (len 0x{:04X})",
                walk.end,
                n_rec.len()
            ));
        }

        // --- Narration-record verification. ---
        let narr_start_new = narr.start + abs_shift(narr.start.saturating_sub(1));
        let narr_len_new = narr_len + tests_len + item_box.len();
        let n_narr = &new_man[narr_start_new..narr_start_new + narr_len_new];
        // The inserted bytes landed verbatim at their new-layout homes, the
        // relocated flow still opens with its first segment, and the retail
        // close survives for the loss path.
        if n_narr[narr.seam..narr.seam + tests_len] != tests[..] {
            return Err("win tests missing from the rebuilt narration record".into());
        }
        if n_narr[block_new..block_new + item_box.len()] != item_box[..] {
            return Err("parallel flow missing from the rebuilt narration record".into());
        }
        if n_narr.get(retail_new) != Some(&0x1F) {
            return Err("retail flow no longer starts with its first segment".into());
        }
        if n_narr.get(narr.close + tests_len) != Some(&0x48) {
            return Err("retail box-close missing from the rebuilt record".into());
        }
        // The `76 CB` repeat-clear jump must now land exactly on the first
        // win test (target unshifted onto the spliced block).
        {
            let repeat = narr
                .refs
                .iter()
                .find(|r| r.target == narr.seam)
                .ok_or("repeat-clear ref lost")?;
            let f_new = repeat.field + narr_shift(repeat.field);
            let d = u16::from_le_bytes([n_narr[f_new], n_narr[f_new + 1]]) as usize;
            if (f_new + d) & 0xFFFF != narr.seam {
                return Err("repeat-clear jump does not land on the win tests".into());
            }
        }
        // The rebuilt narration record decodes as the original stream with
        // exactly the win tests + parallel flow added: every byte consumed,
        // and the original signature reproduced (position-exact) once the
        // inserted instructions are filtered out.
        let narr_walk = walk_record(n_narr, narr.pc0)?;
        if narr_walk.end != n_narr.len() {
            return Err(format!(
                "rebuilt narration walk stalled at +0x{:04X} (len 0x{:04X})",
                narr_walk.end,
                n_narr.len()
            ));
        }
        let new_narr_sig: Vec<(usize, u8)> = walk_signature(n_narr, narr.pc0)?
            .into_iter()
            .filter(|&(pc, _)| !(narr.seam..narr.seam + tests_len).contains(&pc) && pc < block_new)
            .collect();
        let expected_narr_sig: Vec<(usize, u8)> = narr_sig_old
            .into_iter()
            .map(|(pc, t)| (pc + narr_shift(pc), t))
            .collect();
        if new_narr_sig != expected_narr_sig {
            return Err("rebuilt narration record diverges from the original stream".into());
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

/// Build the Delilas branch bytecode: gate on the Koru flag, confirm, then
/// request the injected Delilas dome course and warp into the arena the same
/// way retail's own difficulty arms do. `base` is the branch's record-relative
/// start in the NEW layout; `common_tail` is the shared post-warp exit (NEW
/// layout) and `decline_tail` the refusal target (NEW layout) both the locked
/// gate and the confirm's cancel route into. All internal jumps are emitted
/// position-correct.
///
/// ```text
///        if KORU_DEFEATED -> AVAIL          ; else fall to the decline flow
///        jmp DECLINE_TAIL
/// AVAIL: text  "Face Che and Lu, then Gi."
///        0x27 picker: [0] -> WARP, [1] -> CANCEL
///        label "Bring them on!"  label "On second thought, forget it."
/// WARP:  55 09                              ; SET dome-active (0x509)
///        65 36 65 37 65 38                  ; CLEAR the course-unlock flags
///        55 39                              ; SET course-3 request (0x539)
///        34 01 FF FF FF 1E 00               ; arena-entry BGM (verbatim)
///        4A 1E 00  4A 08 00                 ; arena transition waits (verbatim)
///        3E 69 00 00 00 00                  ; arena warp (verbatim)
///        jmp COMMON_TAIL
/// CANCEL: jmp DECLINE_TAIL
/// ```
///
/// The confirm exists because the warp fires the moment the arm runs - with
/// no picker, selecting the menu option launches the contest instantly and a
/// misclick has no out. Cancel routes into the record's own decline flow
/// (through a local trampoline jump: every retail picker entry observed jumps
/// forward, so the backward hop is taken by the proven `0x26` op instead).
fn build_branch(base: usize, common_tail: usize, decline_tail: usize) -> Result<Vec<u8>, String> {
    #[derive(Clone, Copy, PartialEq)]
    enum Label {
        Avail,
        Warp,
        Cancel,
        CommonTail,
        DeclineTail,
    }
    let mut b: Vec<u8> = Vec::with_capacity(128);
    let mut fixups: Vec<(usize, Label)> = Vec::new(); // (local field off, dest)
    let mut labels: Vec<(Label, usize)> = Vec::new();
    let jmp = |b: &mut Vec<u8>, fixups: &mut Vec<(usize, Label)>, l: Label| {
        b.push(0x26);
        b.extend_from_slice(&[0, 0]);
        fixups.push((b.len() - 2, l));
    };

    // Gate: if the Koru flag is set, jump to AVAIL; else the locked-gate
    // refusal is a bare jump into the record's own decline flow ("We hope
    // you come again soon!") - zero new text.
    b.extend_from_slice(&sysflag_test(KORU_DEFEATED_FLAG, 0));
    fixups.push((b.len() - 2, Label::Avail));
    jmp(&mut b, &mut fixups, Label::DeclineTail);

    // AVAIL: confirm picker (immediate-labels form, entries then labels).
    labels.push((Label::Avail, b.len()));
    b.extend(seg("Face Che and Lu, then Gi."));
    b.push(0x27);
    b.extend_from_slice(&[0, 0]);
    fixups.push((b.len() - 2, Label::Warp));
    b.extend_from_slice(&[0, 0]);
    fixups.push((b.len() - 2, Label::Cancel));
    // The cancel label repeats the record's own decline-option line verbatim
    // ("On second thought, forget it." sits twice in this record, inside the
    // LZS window) so it folds to a few match tokens instead of novel text -
    // budget headroom the reward-announcement box needs.
    b.extend(seg("Bring them on!"));
    b.extend(seg("On second thought, forget it."));

    // WARP: mirror a retail difficulty arm, but request course 3 (the
    // injected Delilas dome course) via the extra flag the delilas_dome seed
    // hook decodes. Set the dome-active flag, clear the three course-unlock
    // flags so retail's own seed logic picks nothing, and set the course-3
    // request flag. Then the verbatim BGM + wait + warp ops.
    labels.push((Label::Warp, b.len()));
    b.extend_from_slice(&sysflag_set(DOME_ACTIVE_FLAG));
    for &flag in &COURSE_UNLOCK_FLAGS {
        b.extend_from_slice(&sysflag_clear(flag));
    }
    b.extend_from_slice(&sysflag_set(crate::delilas_dome::COURSE_FLAG));
    b.extend_from_slice(&ARENA_ENTER_BGM);
    b.extend_from_slice(&ARENA_ENTER_WAIT_A);
    b.extend_from_slice(&ARENA_ENTER_WAIT_B);
    b.extend_from_slice(&DOME_WARP_OP);
    jmp(&mut b, &mut fixups, Label::CommonTail);

    // CANCEL: trampoline into the decline flow.
    labels.push((Label::Cancel, b.len()));
    jmp(&mut b, &mut fixups, Label::DeclineTail);

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
    Ok(b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sysflag_clear_encoding() {
        assert_eq!(sysflag_clear(0xD76), [0x6D, 0x76]);
        assert_eq!(sysflag_clear(0x378), [0x63, 0x78]);
        // The retail difficulty arms' own flag ops (dome-active + course clear).
        assert_eq!(sysflag_set(DOME_ACTIVE_FLAG), [0x55, 0x09]);
        assert_eq!(sysflag_clear(COURSE_UNLOCK_FLAGS[0]), [0x65, 0x36]);
        assert_eq!(sysflag_set(crate::delilas_dome::COURSE_FLAG), [0x55, 0x39]);
        // The narration win test reads the settlement's contest-won flag.
        assert_eq!(sysflag_test(CONTEST_WON_FLAG, 0)[..2], [0x75, 0x0A]);
    }

    #[test]
    fn reward_box_lines_shapes() {
        // The custom-items announcement: three rows, Oxford-free "and" on
        // the middle row, terminal bang on the last.
        assert_eq!(
            reward_box_lines(&crate::custom_items::REWARD_ANNOUNCE_NAMES).unwrap(),
            [
                "You won 1 Ra-Seru Tear,",
                "1 Nature's Elixir and",
                "1 Fury Bloom!"
            ]
        );
        // The Honey fallback: one row.
        assert_eq!(
            reward_box_lines(&[crate::custom_items::HONEY_ANNOUNCE_NAME]).unwrap(),
            ["You won 1 Honey!"]
        );
        // Bounds: an empty or 4-item list refuses, as does an over-wide name.
        assert!(reward_box_lines(&[]).is_err());
        assert!(reward_box_lines(&["a", "b", "c", "d"]).is_err());
        assert!(reward_box_lines(&["An Impossibly Long Item Name Overflow"]).is_err());
        // Every produced line stays inside the record's widest retail row.
        for line in reward_box_lines(&crate::custom_items::REWARD_ANNOUNCE_NAMES).unwrap() {
            assert!(line.len() <= 33, "{line:?}");
        }
    }

    #[test]
    fn branch_is_a_gated_confirmed_arena_warp() {
        let base = 0x0FF4;
        let tail = 0x0FDF;
        let decline = 0x0F5A;
        let b = build_branch(base, tail, decline).unwrap();

        // Opens with the Koru gate (op 0x73, operand 0x78) -> AVAIL.
        assert_eq!(&b[..2], &[0x73, 0x78]);
        let d = u16::from_le_bytes([b[2], b[3]]) as usize;
        let avail = (base + 2 + d) & 0xFFFF;
        assert!(avail > base && avail < base + b.len());
        // The gate-fail path is a bare jump to the decline tail.
        assert_eq!(b[4], 0x26);
        let dd = u16::from_le_bytes([b[5], b[6]]) as usize;
        assert_eq!((base + 5 + dd) & 0xFFFF, decline);
        // AVAIL is the confirm prompt (a text segment).
        assert_eq!(b[avail - base], 0x1F);

        // The dome-course request mirrors a retail arm: dome-active set once,
        // each course-unlock flag cleared once, course-3 requested once, the
        // BGM + warp verbatim, warp exactly once. No scripted battle op.
        assert_eq!(
            b.windows(2)
                .filter(|w| *w == sysflag_set(DOME_ACTIVE_FLAG))
                .count(),
            1
        );
        for &flag in &COURSE_UNLOCK_FLAGS {
            assert_eq!(
                b.windows(2).filter(|w| *w == sysflag_clear(flag)).count(),
                1
            );
        }
        let course = sysflag_set(crate::delilas_dome::COURSE_FLAG);
        assert_eq!(b.windows(2).filter(|w| *w == course).count(), 1);
        assert_eq!(
            b.windows(DOME_WARP_OP.len())
                .filter(|w| *w == DOME_WARP_OP)
                .count(),
            1
        );
        assert_eq!(b.windows(7).filter(|w| *w == ARENA_ENTER_BGM).count(), 1);
        assert!(!b.windows(3).any(|w| w == [0x3E, 0xFF, 0x00]));

        // Placed at `base` in a synthetic record, the branch walks cleanly to
        // the end (every byte decoded) and carries exactly the confirm picker.
        let mut rec = vec![0x21u8; base]; // pad decodes as 1-byte ops in the walk
        rec[base - 1] = 0x00; // the gate op follows a terminator in the real record
        rec.extend_from_slice(&b);
        let w = walk_record(&rec, base).unwrap();
        assert_eq!(w.end, rec.len(), "branch walk must consume every byte");
        assert_eq!(w.pickers.len(), 1, "exactly the confirm picker");
        assert_eq!(w.pickers[0].n, 2);
        // Entry 0 (fight) lands on the dome-active set; entry 1 (cancel) on a
        // local trampoline jump that exits to the decline tail.
        let fight = w.pickers[0].entries[0].target;
        assert_eq!(
            &rec[fight..fight + 2],
            &sysflag_set(DOME_ACTIVE_FLAG),
            "confirm-yes must land on the warp arm"
        );
        let cancel = w.pickers[0].entries[1].target;
        assert_eq!(rec[cancel], 0x26, "cancel lands on the trampoline jump");
        assert!(cancel >= base && cancel < base + b.len());
        // The warp op is a 6-byte warp form (not the 2-byte interact form).
        let warp_at = base + b.windows(6).position(|w| w == DOME_WARP_OP).unwrap();
        assert!(
            w.insns
                .iter()
                .any(|&(pc, op, sz)| pc == warp_at && op == 0x3E && sz == 6)
        );
    }
}
