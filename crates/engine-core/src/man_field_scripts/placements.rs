//! Actor-placement classification (portals, dialog, props) + inline-dialog prologue.
//!
//! Extracted verbatim from `man_field_scripts.rs`.

use super::*;

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
