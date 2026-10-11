//! Splitting an inline dialogue operand into its text segments.

use super::*;

/// Decode every `0x1F`-lead text segment in a field-VM inline dialog buffer
/// to its glyph bytes.
///
/// An interaction record's inline text is a flat **pool** of segments, each
/// introduced by a `0x1F` lead byte and terminated by an MES end byte
/// (`0x00..=0x1E`). Empirically (decoding real town01 placements) this pool is
/// the NPC's *entire* dialogue line set - every line across every story-state
/// branch of that NPC's conversation, with interspersed option labels (e.g.
/// `"Yes"` / `"No"`). It is **not** a single box's "prompt then option labels":
/// most segments are consecutive speech lines, and the field-VM script (gated
/// on story flags via `COND_JMP`) selects which segment to start at, how many
/// lines fill one box, and which are selectable options.
///
/// **There is no separate "box-geometry header" format.** The bytes between
/// the placement's `script_pc0` and the first `0x1F` lead are normal field-VM
/// bytecode - `CFlag` / `SysFlag.Test` / `JmpRel` / `Nop` / `0x4C 0x51`
/// NPC-move-to-tile - that runs as the NPC's interaction prologue (face the
/// player, set conversation flags, walk to the talk position, branch on story
/// flags). It is consumed by the field-VM dispatcher `FUN_801DE840` from
/// state 0 of the per-actor dialog SM `FUN_80039b7c`, which loops until the
/// dispatcher leaves the actor's PC on a byte where `& 0x7F < 0x20` (a
/// `0x1F` lead or a `0x21` terminator) and then transitions into the pager
/// `FUN_801D84D0`. The "select which segment to start at" mechanism is the
/// story-flag-gated `SysFlag.Test` branches themselves: the script
/// `JmpRel`s past unwanted segments to the desired one. See
/// `field_actor_placements_disc::dialog_prefix_decodes_as_field_vm_bytecode`
/// for the disc-gated proof. An earlier note pointed at `func_0x8001ebec` as
/// the renderer - that is **wrong**; the disassembly shows it is a
/// per-character TMD-pose copier indexed by the slot-4 freeze flag, not the
/// dialog box renderer.
///
/// The page/option grouping is pinned: the pager types consecutive `0x1F`
/// lines as one page, terminated by a post-page control byte
/// ([`legaia_mes::Dispatch`]), in a window of [`legaia_mes::LINES_PER_BOX`]
/// (`_DAT_801F2740` = 3) rows that scrolls when a page runs longer
/// ([`crate::dialog_window`]); `0xC0..=0xCF` escapes inside a line are 2-byte
/// (so a `0x00` argument doesn't end the line early). See
/// [`legaia_mes::pack_page`] / [`legaia_mes::pack_box`] and the disc-gated
/// `field_dialog_boxpack_disc` regression.
///
/// This function still returns the raw, ungrouped segment pool - the simplest
/// faithful view of the record's whole line set. Callers that want boxes use
/// the `pack_box` decoder on the same bytes.
/// The field-VM bytecode preceding the first `0x1F` is skipped (its bytes
/// can fall in the glyph range, so it is not interpreted as text).
pub fn decode_inline_segments(inline: &[u8]) -> Vec<Vec<u8>> {
    let mut segments = Vec::new();
    let mut cursor = 0usize;
    while let Some(rel) = inline[cursor..].iter().position(|&b| b == 0x1F) {
        let start = cursor + rel + 1;
        let mut interp = Interpreter::new_at(inline, start);
        let mut glyphs = Vec::new();
        loop {
            match interp.next_event() {
                Some(MesEvent::Glyph(g)) | Some(MesEvent::SkipTwo(g)) => glyphs.push(g),
                Some(MesEvent::WideGlyph(_op, arg)) => glyphs.push(arg),
                Some(MesEvent::EndOfMessage(_)) | None => break,
                // Page-break / spacing / substitution stay inside the current
                // segment: a segment ends only at its MES terminator, never at
                // an intermediate control byte.
                Some(_) => {}
            }
        }
        // Resume scanning just past this segment's terminator. `pc()` sits
        // after the end byte; clamp to `start` so a `0x1F` immediately followed
        // by a terminator (empty segment) still makes forward progress.
        cursor = interp.pc().max(start);
        segments.push(glyphs);
    }
    segments
}
