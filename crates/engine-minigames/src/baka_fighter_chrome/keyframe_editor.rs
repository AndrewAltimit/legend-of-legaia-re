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
/// The fighter cursor's wrap: retail tests it **unsigned** against this, so a
/// cursor stepped below zero wraps to `0`, and the later `< 0 -> 0x10` wrap
/// never fires.
pub const EDITOR_FIGHTER_COUNT: u16 = 0x11;
/// Fixed-point shift the editor applies to a frame cursor before handing it to
/// the keyframe lookup.
pub const EDITOR_FRAME_SHIFT: u32 = 4;
/// The match-timer value the editor writes back when the fighter cursor has
/// left the fighter the band was entered with: the cabinet's editor entry,
/// which re-seats the editor on the new fighter.
pub const EDITOR_REENTRY_PHASE: i32 = 0x190;
/// How far below the keyframe the marker effect is spawned (`addiu -0x32`).
pub const EDITOR_MARKER_DROP: i16 = 0x32;

/// What the editor did with a keyframe slot this frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditorKeyEdit {
    /// The edit-mode flag did not change across the dev menu.
    None,
    /// `FUN_801D57BC` - install (or rewrite) the slot for this key.
    Install,
    /// `FUN_801D58E0` - remove the slot for this key.
    Remove,
}

/// The editor's globals in the cabinet overlay's BSS (PROT 0976).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct EditorGlobals {
    /// `0x801DBF04` - the fighter cursor.
    pub fighter: i16,
    /// `0x801DBF08` - the fighter the band was entered with.
    pub fighter_at_entry: i16,
    /// `0x801DBF0A..0x801DBF0E` - the selected keyframe's offset being edited.
    pub offset: [i16; 3],
    /// `0x801DBF10` - whether the frame cursor sits on a keyframe (the edit
    /// mode the install / remove pair keys on).
    pub has_key: i16,
    /// `0x801DBF12` - the frame cursor, in whole frames.
    pub frame: i16,
    /// `0x801DBF14` - the fighter clip cursor copied back while playing.
    pub cursor_raw: i16,
    /// `0x801DBF16` - the action cursor.
    pub action: i16,
    /// `0x801DBF18` - the action the edit fields were loaded from.
    pub prev_action: i16,
    /// `0x801DBF1A` - the action's speed field being edited.
    pub speed: i16,
    /// `0x801DBF1C` - the action's power field being edited.
    pub power: i16,
    /// `0x801DBF1E` - the frame the keyframe offset was loaded at.
    pub prev_frame: i16,
    /// `0x801DBF44` - the cabinet's match timer.
    pub match_timer: i32,
    /// `0x801DC078` - the fighter's first action clip.
    pub action_base: i16,
    /// `0x801DC0FC` - the keyframe the marker was last spawned at.
    pub marker_key: i32,
    /// `0x801DC13C` - non-zero while the editor holds the clip still (the
    /// frame cursor drives the pose); zero while it plays.
    pub paused: i16,
    /// `0x801DC148` - the keyframe the frame cursor lands on, `-1` for none.
    pub key: i32,
    /// `0x801DC14C` - the keyframe the offset fields were loaded from.
    pub key_sel: i32,
}

/// The fields of the editor actor and of the fighter it edits that the tick
/// reads and writes.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct EditorActor {
    /// `+0x10`: bit 3 retires, bit 1 hides.
    pub flags: u32,
    /// `+0x14..+0x18`.
    pub pos: [i16; 3],
    /// `+0x5C` - the clip the actor plays.
    pub anim: i16,
    /// `+0x5E` - the clip it played last.
    pub anim_prev: i16,
    /// `+0x68` - the clip cursor, `<< 4` fixed point.
    pub cursor: i16,
    /// `+0x6A` - the clip rate.
    pub rate: i16,
}

/// One sub-keyframe of an action record: offset `+0x20..+0x24`, frame `+0x26`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct EditorKey {
    pub offset: [i16; 3],
    pub frame: i16,
}

/// The fields of one `0x60`-byte action record the editor edits.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EditorAction {
    /// `+0x04` - written back as a word, read as its low half.
    pub speed: i32,
    /// `+0x18` - the same.
    pub power: i32,
    /// The sub-keyframes, `+0x1C` of them.
    pub keys: Vec<EditorKey>,
}

/// One editor tick's effects beyond the state it rewrites.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EditorTick {
    /// The band gate failed: the editor actor and the fighter actor both
    /// took the retire bit and nothing else ran.
    pub retired: bool,
    /// The clip the editor actor's `+0x4C` was bound to, by the `+0x5C` it
    /// carried on entry.
    pub clip_anim: i16,
    /// Where the marker effect `FUN_80021B04` was spawned, when one was.
    pub marker: Option<[i16; 3]>,
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
/// retires both actors and returns. Inside it:
///
/// 1. a new action cursor reloads the record's speed (`+0x04`) and power
///    (`+0x18`) into the edit fields and forgets the loaded keyframe;
/// 2. the frame cursor is looked up through
///    [`crate::baka_fighter::keyframe_in_range`] at `frame << 4`;
/// 3. while the clip is held still and the frame cursor has moved, a keyframe
///    under it loads its offset into the edit fields (and shows the fighter),
///    and no keyframe hides the fighter;
/// 4. the fighter is seated at the editor's position plus the edited offset,
///    the debug text and the dev menu run (`menu`: retail's
///    `FUN_8002B984` is an empty stub, so nothing edits the fields), and a
///    change of the edit-mode flag across the menu installs or removes the
///    keyframe through [`anim_slot_install`] / [`anim_slot_delete`];
/// 5. the edit fields are written back into the record, the fighter cursor
///    wraps unsigned at [`EDITOR_FIGHTER_COUNT`] and the frame cursor wraps at
///    the bound clip's frame count less one (`frame >= count - 1` resets it);
/// 6. held still, the clip cursor follows the frame cursor at rate `0`;
///    playing, it runs at the edited speed, the frame cursor follows it, the
///    fighter hides and a marker effect spawns at the keyframe under it;
/// 7. a fighter cursor off the fighter the band opened on retires both actors
///    and rewinds the match timer to [`EDITOR_REENTRY_PHASE`]; the editor
///    actor's clip becomes `action_base + action + 1`, and a clip change
///    rewinds the frame cursor.
///
/// `clip_frames` gives the frame count (`+0x02`) of the clip the actor's
/// `+0x5C` names (`_DAT_8007B888`'s table below `0x400`, `_DAT_8007B840`'s
/// above).
pub fn editor_tick(
    g: &mut EditorGlobals,
    editor: &mut EditorActor,
    fighter: &mut EditorActor,
    actions: &mut [EditorAction],
    clip_frames: impl Fn(i16) -> u16,
    menu: impl FnOnce(&mut EditorGlobals),
) -> EditorTick {
    let clip_anim = editor.anim;
    let mut out = EditorTick {
        retired: false,
        clip_anim,
        marker: None,
        edit: EditorKeyEdit::None,
    };
    if !editor_band_open(g.match_timer) {
        editor.flags |= 8;
        fighter.flags |= 8;
        out.retired = true;
        return out;
    }
    let row = |action: i16| action as u16 as usize;
    if g.action != g.prev_action {
        g.prev_action = g.action;
        g.prev_frame = -1;
        g.key = -1;
        g.key_sel = -1;
        if let Some(r) = actions.get(row(g.action)) {
            g.speed = r.speed as i16;
            g.power = r.power as i16;
        }
    }
    let query = (g.frame as i32) << EDITOR_FRAME_SHIFT;
    g.key = actions.get(row(g.action)).map_or(-1, |r| {
        let frames: Vec<i16> = r.keys.iter().map(|k| k.frame).collect();
        crate::baka_fighter::keyframe_in_range(&frames, query, query).map_or(-1, |i| i as i32)
    });
    if g.frame != g.prev_frame && g.paused != 0 {
        g.prev_frame = g.frame;
        if g.key >= 0 {
            g.has_key = 1;
            g.key_sel = g.key;
            fighter.flags &= !2;
            g.offset = actions[row(g.action)].keys[g.key as usize].offset;
        } else {
            fighter.flags |= 2;
            g.has_key = 0;
        }
    }
    for i in 0..3 {
        fighter.pos[i] = editor.pos[i].wrapping_add(g.offset[i]);
    }
    let has_key = g.has_key;
    menu(g);
    let frame_count = clip_frames(clip_anim);
    if g.has_key != has_key {
        g.prev_frame = -1;
        let raw = (g.frame as i32) << EDITOR_FRAME_SHIFT;
        if let Some(r) = actions.get_mut(row(g.action)) {
            let mut frames: Vec<i16> = r.keys.iter().map(|k| k.frame).collect();
            if g.has_key != 0 {
                // Installing zeroes the slot's offset, new or re-used.
                let put = anim_slot_install(&mut frames, raw);
                if !put.full {
                    if put.reused {
                        r.keys[put.slot].offset = [0; 3];
                    } else {
                        r.keys.push(EditorKey {
                            offset: [0; 3],
                            frame: frames[put.slot],
                        });
                    }
                }
                out.edit = EditorKeyEdit::Install;
            } else {
                if let Some(slot) = anim_slot_delete(&mut frames, raw).slot {
                    r.keys.remove(slot);
                }
                out.edit = EditorKeyEdit::Remove;
            }
        }
    }
    if let Some(r) = actions.get_mut(row(g.prev_action)) {
        r.power = g.power as i32;
        r.speed = g.speed as i32;
        if g.key >= 0 && g.paused != 0 {
            if let Some(k) = r.keys.get_mut(g.key as usize) {
                k.offset = g.offset;
            }
        }
    }
    if g.fighter as u16 >= EDITOR_FIGHTER_COUNT {
        g.fighter = 0;
    }
    if g.frame as i32 >= frame_count as i32 - 1 {
        g.frame = 0;
    }
    if g.paused != 0 {
        editor.rate = 0;
        editor.cursor = g.frame.wrapping_shl(EDITOR_FRAME_SHIFT);
    } else {
        editor.rate = g.speed;
        let c = editor.cursor as i32;
        g.frame = ((if c < 0 { c + 0xF } else { c }) >> EDITOR_FRAME_SHIFT) as i16;
        g.cursor_raw = editor.cursor;
        fighter.flags |= 2;
        if g.key >= 0 {
            g.marker_key = g.key;
            if let Some(k) = actions
                .get(row(g.action))
                .and_then(|r| r.keys.get(g.key as usize))
            {
                out.marker = Some([
                    editor.pos[0].wrapping_add(k.offset[0]),
                    editor.pos[1].wrapping_add(k.offset[1]),
                    editor.pos[2]
                        .wrapping_add(k.offset[2])
                        .wrapping_sub(EDITOR_MARKER_DROP),
                ]);
            }
        }
    }
    if g.fighter < 0 {
        g.fighter = 0x10;
    }
    if g.fighter != g.fighter_at_entry {
        editor.flags |= 8;
        fighter.flags |= 8;
        g.match_timer = EDITOR_REENTRY_PHASE;
    }
    editor.anim = g.action_base.wrapping_add(g.action).wrapping_add(1);
    if editor.anim != editor.anim_prev {
        g.frame = 0;
    }
    editor.flags &= !2;
    out
}
