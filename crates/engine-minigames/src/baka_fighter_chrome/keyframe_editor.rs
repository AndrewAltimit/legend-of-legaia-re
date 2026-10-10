//! The developer action-table keyframe editor.

use super::*;

// ---------------------------------------------------------------------------
// The developer action-table keyframe editor
// ---------------------------------------------------------------------------

/// First value of the match timer `DAT_801DBF44` that opens the editor band.
pub const EDITOR_PHASE_BASE: i32 = 400;
/// Width of that band - retail tests `DAT_801DBF44 - 400` **unsigned** against
/// this, so the editor runs in `400..=499` and nowhere else. The live-round
/// band is `DAT_801DBF44 == 100`, well below it.
pub const EDITOR_PHASE_SPAN: u32 = 100;
/// Number of entries in the per-character action table the editor cycles.
pub const EDITOR_ACTION_COUNT: u16 = 0x11;
/// Fixed-point shift the editor applies to a frame cursor before handing it to
/// the keyframe lookup.
pub const EDITOR_FRAME_SHIFT: u32 = 4;

/// What the editor decided to do with the selected keyframe this frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditorKeyEdit {
    /// The frame cursor did not move; nothing is installed or removed.
    None,
    /// `FUN_801D57BC` - install (or rewrite) the slot for this key.
    Install,
    /// `FUN_801D58E0` - remove the slot for this key.
    Remove,
}

/// One editor tick's decisions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EditorTick {
    /// The band gate failed: the editor actor and the fighter actor both
    /// take the retire bit and nothing else runs.
    pub retired: bool,
    /// The action cursor after wrapping.
    pub action: u16,
    /// The frame cursor after wrapping.
    pub frame: i16,
    /// The keyframe the current frame cursor lands on, if any.
    pub keyframe: Option<usize>,
    /// Whether the selected action changed this frame, which is what reloads
    /// the record's speed / power into the edit fields.
    pub action_changed: bool,
    pub edit: EditorKeyEdit,
}

/// Whether the editor band is open for a given match-timer value.
///
/// REF: FUN_801d4fc8 (`0x801D5018..0x801D5028`)
pub fn editor_band_open(match_timer: i32) -> bool {
    (match_timer.wrapping_sub(EDITOR_PHASE_BASE) as u32) < EDITOR_PHASE_SPAN
}

// REPLACED-BY: nothing is owed a port - retail never reaches it. The editor
// is *linked* rather than dead: nothing `jal`s `0x801D4FC8`, but its address
// is the callback word of the `0x18`-byte actor prototype at `0x801D766C`,
// which the `0x190` arm spawns (`lui`+`addiu` at `0x801D1D68`).
// The whole editor is a development screen behind a flag the shipped disc
// never raises. The band `0x190` / `0x191` is written in exactly one place,
// the developer menu arm (`0x801D19DC`, cabinet state `0xC8`), and the cabinet
// enters `0xC8` only when `_DAT_8007B868` is non-zero (`lw v1,-0x4798(v1)` at
// `0x801D08E8`, `beqz` to the pause menu `0xBF` otherwise). A disc-wide
// `find-gp-relative-refs.py --va 0x8007b868 --prot` sweep (SCUS, every based
// overlay image and every raw PROT entry) finds exactly two stores to that
// word: the boot store of `FUN_8002B92C`'s result (`0x80015F18`), a stub whose
// whole body is `jr ra; move v0, zero`, and a bit-clear (`and ~2` at
// `0x8001E008`). So retail boots it to zero and only ever clears it, and no
// shipped path reaches the editor or its two slot helpers.
/// PORT: FUN_801d4fc8 - the **developer action-table keyframe editor** tick.
///
/// It is not the fight's pose path. Outside the [`editor_band_open`] window it
/// retires both actors and returns; inside it, it draws the action / frame
/// cursors, reloads the record's speed (`+0x04`) and power (`+0x18`) into the
/// edit fields whenever the selected action changes, looks the current frame
/// cursor up through [`crate::baka_fighter::keyframe_in_range`], and - when
/// the frame cursor has moved - installs the keyframe through
/// [`anim_slot_install`] or removes it through its sibling `FUN_801D58E0`,
/// picked by the edit-mode flag. It then writes the edited values back into
/// the action record, wraps the action cursor at [`EDITOR_ACTION_COUNT`] and
/// the frame cursor at `record[+2] - 1`, spawns a marker effect at the
/// keyframe position and re-poses the fighter.
///
/// The frame cursor is handed to the lookup shifted left by
/// [`EDITOR_FRAME_SHIFT`], which is the fixed point the lookup expects on its
/// query rather than on the record.
#[allow(clippy::too_many_arguments)]
pub fn editor_tick(
    match_timer: i32,
    action: u16,
    prev_action: u16,
    frame: i16,
    prev_frame: i16,
    edit_mode: bool,
    keyframe_count: i16,
    frame_indices: &[i16],
    slots: &mut Vec<i16>,
) -> EditorTick {
    if !editor_band_open(match_timer) {
        return EditorTick {
            retired: true,
            action,
            frame,
            keyframe: None,
            action_changed: false,
            edit: EditorKeyEdit::None,
        };
    }
    let query = (frame as i32) << EDITOR_FRAME_SHIFT;
    let keyframe = crate::baka_fighter::keyframe_in_range(frame_indices, query, query);

    let mut edit = EditorKeyEdit::None;
    if frame != prev_frame {
        if edit_mode {
            anim_slot_install(slots, query);
            edit = EditorKeyEdit::Install;
        } else {
            anim_slot_delete(slots, query);
            edit = EditorKeyEdit::Remove;
        }
    }

    let action = if action >= EDITOR_ACTION_COUNT {
        0
    } else {
        action
    };
    let frame = if frame > keyframe_count.saturating_sub(1) {
        0
    } else {
        frame
    };

    EditorTick {
        retired: false,
        action,
        frame,
        keyframe,
        action_changed: action != prev_action,
        edit,
    }
}
