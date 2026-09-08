//! The dance floor's **step-marker flipbook** - the per-frame tick every
//! marker tile actor on the Sol disco floor runs.
//!
//! REF: FUN_801D2A10 (the marker floor pass that spawns these actors),
//!      FUN_80020DE0 (the allocator), FUN_80024E08 (the set-model primitive
//!      the tick calls), FUN_800204F8 (the clip selector it gates on)
//!
//! # What spawns it
//!
//! The dance overlay (PROT 0980, slot-A base `0x801CE818`) carries exactly
//! one spawn descriptor whose `+0x08` handler word is `0x801D0640`: the
//! record at `0x801D4314` (file `0x5AFC`), and exactly one site materialises
//! it - `0x801D2C24`, inside the step-marker floor pass `FUN_801D2A10`. That
//! arm is already ported as `legaia_engine_core::minigame_floor::marker_template`:
//! a cell whose step record resolves to clip `6..=9` takes the **marker**
//! template and gets `clip - 6` stamped into the new actor's `+0x50`. So
//! `+0x50` is the marker **class**, `0..=3`, and it is the row selector of
//! the script table below.
//!
//! # The table is a mesh flipbook, not a facing script
//!
//! Retail reads a halfword pair `[value, duration]` out of
//! `0x801D44CC + class * 0x80 + cursor * 2` and hands `value +
//! _DAT_8007B6F8` to `FUN_80024E08`. `FUN_80024E08` is the **set-model**
//! primitive - it writes `actor+0x64`, clears the clip cursor `+0x5C` and
//! re-stages the actor through `FUN_80020F88` - and `_DAT_8007B6F8` is the
//! field actor **pack bias** (`legaia_asset::field_objects::FIELD_ACTOR_PACK_BIAS`).
//! So the halfword is a scene-pool **mesh index**, and the tick is a
//! flipbook that swaps the marker's mesh every `duration` ticks. Reading it
//! as a yaw / facing angle misses that the value is biased by the pack base,
//! which no rotation would be.
//!
//! # The table's shape, from the bytes
//!
//! Four rows of `0x80` bytes at `0x801D44CC..0x801D46CC` in
//! `extracted/overlays/overlay_dance_0980.bin` (file `0x5CB4`). Each row is
//! 25 `[mesh, duration]` pairs closed by a `[-1, -1]` sentinel and padded
//! with zeros; row `0x200` onward is unrelated data, which is what bounds the
//! table at four rows. The four rows are the same 25-step loop at four
//! different phase offsets, so the four marker classes flip through one
//! routine out of step with each other.

/// Runtime VA of the marker script table in the dance overlay.
pub const MARKER_SCRIPT_VA: u32 = 0x801D_44CC;

/// Bytes per class row.
pub const MARKER_SCRIPT_ROW_BYTES: usize = 0x80;

/// Number of class rows (marker clips `6..=9`).
pub const MARKER_SCRIPT_ROWS: usize = 4;

/// Sentinel that wraps a row's cursor back to `0`. Retail's test is
/// `bgez` on the **next** pair's first halfword, i.e. "wrap when the entry
/// after this one reads negative".
pub const MARKER_SCRIPT_END: i16 = -1;

/// One class row of the marker script: `(mesh index, duration in ticks)`
/// pairs, sentinel excluded.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MarkerScript {
    rows: [Vec<(i16, i16)>; MARKER_SCRIPT_ROWS],
}

impl MarkerScript {
    /// Parse the four rows out of a based dance-overlay image.
    ///
    /// `base_va` is the VA of `overlay[0]` (`0x801CE818` for the static
    /// extraction). Returns `None` when the table does not fit.
    pub fn from_overlay(overlay: &[u8], base_va: u32) -> Option<Self> {
        let off = MARKER_SCRIPT_VA.checked_sub(base_va)? as usize;
        let end = off.checked_add(MARKER_SCRIPT_ROW_BYTES * MARKER_SCRIPT_ROWS)?;
        if end > overlay.len() {
            return None;
        }
        let mut rows: [Vec<(i16, i16)>; MARKER_SCRIPT_ROWS] = Default::default();
        for (class, row) in rows.iter_mut().enumerate() {
            let base = off + class * MARKER_SCRIPT_ROW_BYTES;
            for pair in 0..MARKER_SCRIPT_ROW_BYTES / 4 {
                let p = base + pair * 4;
                let mesh = i16::from_le_bytes([overlay[p], overlay[p + 1]]);
                let dur = i16::from_le_bytes([overlay[p + 2], overlay[p + 3]]);
                if mesh < 0 {
                    break;
                }
                row.push((mesh, dur));
            }
        }
        (!rows.iter().all(|r| r.is_empty())).then_some(MarkerScript { rows })
    }

    /// Build a script from explicit rows (tests, and hosts with no overlay).
    pub fn from_rows(rows: [Vec<(i16, i16)>; MARKER_SCRIPT_ROWS]) -> Self {
        MarkerScript { rows }
    }

    /// The `(mesh, duration)` pair at `cursor` of `class`, `None` past the
    /// row's sentinel.
    pub fn entry(&self, class: usize, cursor: usize) -> Option<(i16, i16)> {
        self.rows.get(class)?.get(cursor).copied()
    }

    /// How many live steps a class row carries.
    pub fn steps(&self, class: usize) -> usize {
        self.rows.get(class).map(|r| r.len()).unwrap_or(0)
    }
}

/// One step-marker tile actor's flipbook state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MarkerActor {
    /// `+0x50` - marker class (`clip - 6`), the script row.
    pub class: u16,
    /// `+0x9C` - script cursor, in **halfwords**; a step consumes two.
    pub cursor: u16,
    /// `+0x54` - ticks left on the current mesh.
    pub timer: i16,
    /// `actor+0x64` - the mesh index currently staged, once the first step
    /// has run.
    pub mesh: Option<i16>,
}

/// Advance one marker actor and return the mesh index it staged this frame,
/// if it stepped.
///
/// PORT: FUN_801D0640 (`0x801D0640..0x801D0710` - the flipbook; the tail from
/// `0x801D0710` is the clip-selector gate, reported as
/// [`MarkerStep::run_clip_selector`])
///
/// The timer counts **down** by the frame delta and the step fires when it
/// goes negative, so a `duration` of `0` still holds the mesh for one frame
/// at `frame_delta` `1`. `pack_bias` is retail's `_DAT_8007B6F8`
/// (`legaia_asset::field_objects::FIELD_ACTOR_PACK_BIAS`), added to the table
/// value before it reaches the set-model primitive.
///
/// `clip_cursor` is the actor's `+0x5C` and `flags` its `+0x10`: retail runs
/// the clip selector `FUN_800204F8` when `+0x5C > 0` **or** `+0x10 & 0x1000`,
/// which is reported rather than performed because the clip player is the
/// host's.
pub fn step_marker(
    actor: &mut MarkerActor,
    script: &MarkerScript,
    frame_delta: u8,
    pack_bias: i16,
    clip_cursor: i16,
    flags: u32,
) -> MarkerStep {
    let mut step = MarkerStep {
        run_clip_selector: clip_cursor > 0 || flags & MARKER_CLIP_FLAG != 0,
        ..Default::default()
    };
    actor.timer = (actor.timer as u16).wrapping_sub(u16::from(frame_delta)) as i16;
    if actor.timer >= 0 {
        return step;
    }
    let class = actor.class as usize;
    let pair = (actor.cursor / 2) as usize;
    let Some((mesh, dur)) = script.entry(class, pair) else {
        // Retail would read past the row; the port parks the actor instead
        // of staging whatever follows the table.
        actor.cursor = 0;
        return step;
    };
    let staged = mesh.wrapping_add(pack_bias);
    actor.mesh = Some(staged);
    actor.timer = dur;
    actor.cursor = actor.cursor.wrapping_add(2);
    if script.entry(class, (actor.cursor / 2) as usize).is_none() {
        actor.cursor = 0;
    }
    step.staged_mesh = Some(staged);
    step
}

/// Actor flag `+0x10 & 0x1000` - the second half of the clip-selector gate.
pub const MARKER_CLIP_FLAG: u32 = 0x1000;

/// What one [`step_marker`] call did.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MarkerStep {
    /// The mesh index handed to the set-model primitive, when the flipbook
    /// advanced this frame.
    pub staged_mesh: Option<i16>,
    /// `true` when retail would call the clip selector `FUN_800204F8` for
    /// this actor this frame.
    pub run_clip_selector: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn script() -> MarkerScript {
        MarkerScript::from_rows([
            vec![(27, 12), (20, 12), (22, 6)],
            vec![(20, 12), (22, 6), (27, 12)],
            vec![(22, 6), (27, 12), (20, 12)],
            vec![(27, 6)],
        ])
    }

    #[test]
    fn the_flipbook_cycles_its_row_with_the_tabled_durations() {
        let s = script();
        let mut a = MarkerActor::default();
        // First tick: timer 0 - 1 = -1 < 0, so the first entry stages
        // immediately.
        let first = step_marker(&mut a, &s, 1, 5, 0, 0);
        assert_eq!(first.staged_mesh, Some(27 + 5));
        assert_eq!(a.timer, 12);
        // It then holds for the tabled duration before the next swap.
        for _ in 0..12 {
            assert_eq!(step_marker(&mut a, &s, 1, 5, 0, 0).staged_mesh, None);
        }
        assert_eq!(
            step_marker(&mut a, &s, 1, 5, 0, 0).staged_mesh,
            Some(20 + 5)
        );
    }

    #[test]
    fn the_cursor_wraps_at_the_sentinel() {
        let s = script();
        let mut a = MarkerActor::default();
        let mut staged = Vec::new();
        for _ in 0..64 {
            if let Some(m) = step_marker(&mut a, &s, 8, 0, 0, 0).staged_mesh {
                staged.push(m);
            }
        }
        assert!(staged.len() > 3, "the row must repeat: {staged:?}");
        assert_eq!(&staged[..3], &[27, 20, 22]);
        assert_eq!(staged[3], 27, "wrapped back to the row head");
    }

    #[test]
    fn the_class_selects_the_row() {
        let s = script();
        let mut a = MarkerActor {
            class: 2,
            ..Default::default()
        };
        assert_eq!(step_marker(&mut a, &s, 1, 0, 0, 0).staged_mesh, Some(22));
    }

    #[test]
    fn the_pack_bias_lands_on_the_staged_mesh() {
        // The value is a pool index, not an angle: it is biased by the field
        // actor pack base before it reaches the set-model primitive.
        let s = script();
        let mut a = MarkerActor::default();
        let bias = legaia_asset::field_objects::FIELD_ACTOR_PACK_BIAS as i16;
        assert_eq!(
            step_marker(&mut a, &s, 1, bias, 0, 0).staged_mesh,
            Some(27 + bias)
        );
    }

    #[test]
    fn the_clip_selector_gate_is_either_of_two_conditions() {
        let s = script();
        let mut a = MarkerActor::default();
        assert!(!step_marker(&mut a, &s, 0, 0, 0, 0).run_clip_selector);
        assert!(step_marker(&mut a, &s, 0, 0, 1, 0).run_clip_selector);
        assert!(step_marker(&mut a, &s, 0, 0, 0, MARKER_CLIP_FLAG).run_clip_selector);
    }
}
