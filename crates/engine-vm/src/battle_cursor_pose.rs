//! Four leaves of the battle overlay's shared "slot B" library: the
//! living-actor cursor step, the two pose-slot copy helpers, and the
//! tracked-widget pool teardown.
//!
//! PORT: FUN_801D32BC
//! PORT: FUN_801D57E8
//! PORT: FUN_801D5778
//! PORT: FUN_801D9AE8
//!
//! NOT WIRED: all four are pure kernels with no host root yet - the engine's
//! round boundary (`engine-core::battle_round::BattleRound::boundary`) keeps
//! the active-actor cursor with the host, the engine's battle HUD has no
//! pose-slot array, and it has no tracked-widget pool.
//!
//! ## Why one module for four functions
//!
//! All four sit in the address band that every "slot B" overlay image carries
//! verbatim - `overlay_battle_action`, `overlay_magic_capture`,
//! `overlay_magic_level_up` and `overlay_muscle_dome` disassemble
//! byte-identically at each of these VAs. (The `overlay_0897` image does
//! **not**: at `0x801D57E8` it has a mid-function fragment with no prologue,
//! which is a VA collision, not the same routine. Read the battle-side dumps
//! for these four, never the 0897 ones.)
//!
//! Provenance: `see ghidra/scripts/funcs/overlay_battle_action_801d32bc.txt`,
//! `overlay_battle_action_801d57e8.txt`, `overlay_battle_action_801d5778.txt`,
//! `overlay_battle_action_801d9ae8.txt`. Behaviour notes in
//! `docs/subsystems/battle-action.md` and `docs/reference/functions.md`.

// ---------------------------------------------------------------------------
// FUN_801D32BC - living-actor cursor step
// ---------------------------------------------------------------------------

/// The status-word mask that makes an actor un-selectable: `+0x16E & 0xF84`.
///
/// Same mask the action-select scans use (`FUN_801DB81C` / `FUN_801DBA04`,
/// ported in `battle_action::pool_ops`). `FUN_801D32BC` applies **only** this
/// and the liveness halfword - unlike those two it does *not* consult the
/// per-slot action-state byte, which is what makes it a distinct routine.
pub const UNSELECTABLE_STATUS_MASK: u16 = 0xf84;

/// Direction argument of `FUN_801D32BC`. Retail passes the raw word; any
/// value other than `0` or `1` returns with the context untouched, which is
/// why [`step_actor_cursor`] takes the enum and the caller decides.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CursorStep {
    /// `FUN_801D32BC(0)` - scan upward from `cursor + 1`.
    Forward,
    /// `FUN_801D32BC(1)` - scan downward from `cursor - 1`.
    Backward,
}

/// The four battle-context cursor bytes `FUN_801D32BC` rewrites.
///
/// | field | ctx offset | role |
/// |---|---|---|
/// | `active` | `+0x13` | active-actor index; also the scan's start point |
/// | `latest` | `+0x20` | the index the last step landed on |
/// | `previous` | `+0x21` | the index before that (`+0x21 = +0x20` on every step) |
/// | `depth` | `+0x1F` | signed step counter, `+1` forward / `-1` backward |
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ActorCursor {
    /// ctx `+0x13`.
    pub active: u8,
    /// ctx `+0x20`.
    pub latest: u8,
    /// ctx `+0x21`.
    pub previous: u8,
    /// ctx `+0x1F`.
    pub depth: u8,
}

/// One battle actor as this cursor reads it - the two halfwords the scan
/// predicate touches and nothing else.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CursorActor {
    /// `+0x14C` liveness halfword. Zero = skip.
    pub liveness: u16,
    /// `+0x16E` status word. Any bit in [`UNSELECTABLE_STATUS_MASK`] = skip.
    pub status: u16,
}

impl CursorActor {
    /// The retail predicate: `+0x14C != 0 && (+0x16E & 0xF84) == 0`.
    pub fn selectable(&self) -> bool {
        self.liveness != 0 && (self.status & UNSELECTABLE_STATUS_MASK) == 0
    }
}

/// Step the active-actor cursor to the next / previous selectable slot.
/// `FUN_801D32BC`.
///
/// The scan walks the 8-slot battle actor pointer table at `DAT_801C9370`.
/// Forward it starts at `active + 1` and stops on the first selectable slot
/// **or** the first index `>= actor_count` (retail's `sltiu` bound is the
/// actor count byte at ctx `+0x00`). Backward it starts at `active - 1` and
/// stops on the first selectable slot **or** when the index wraps to `0xFF`.
///
/// Retail keeps the cursor in a 32-bit register and masks it to 8 bits both
/// for the bound test and for the store, so the whole walk is exact `u8`
/// wrapping arithmetic - reproduced here.
///
/// The bookkeeping writes happen on **every** call, including the exhausted
/// case: `+0x21` takes the old `+0x20`, `+0x20` and `+0x13` take the landing
/// index (which may be out of range), and `+0x1F` moves one step. Retail does
/// not clamp any of them.
///
/// PORT: FUN_801D32BC
pub fn step_actor_cursor(
    cursor: &mut ActorCursor,
    dir: CursorStep,
    actor_count: u8,
    actors: &[CursorActor],
) {
    let selectable = |i: u8| -> bool {
        actors
            .get(i as usize)
            .map(CursorActor::selectable)
            .unwrap_or(false)
    };

    let landing = match dir {
        CursorStep::Forward => {
            let mut n = cursor.active.wrapping_add(1);
            while n < actor_count {
                if selectable(n) {
                    break;
                }
                n = n.wrapping_add(1);
            }
            n
        }
        CursorStep::Backward => {
            let mut n = cursor.active.wrapping_sub(1);
            if n != 0xff {
                loop {
                    if selectable(n) {
                        break;
                    }
                    n = n.wrapping_sub(1);
                    if n == 0xff {
                        break;
                    }
                }
            }
            n
        }
    };

    cursor.previous = cursor.latest;
    cursor.latest = landing;
    cursor.active = landing;
    cursor.depth = match dir {
        CursorStep::Forward => cursor.depth.wrapping_add(1),
        CursorStep::Backward => cursor.depth.wrapping_sub(1),
    };
}

// ---------------------------------------------------------------------------
// FUN_801D57E8 / FUN_801D5778 - pose-slot copy helpers
// ---------------------------------------------------------------------------

/// Byte stride of one record in the battle pose-slot array at `0x80076C10`.
pub const POSE_SLOT_STRIDE: usize = 0x18;

/// The horizontal bias `FUN_801D5778` subtracts while re-mapping `+0x0A`.
/// `0x140` = 320 = the PSX display width, i.e. the copy lands the clone one
/// full screen to the left of its source.
pub const POSE_SLOT_X_BIAS: u16 = 0x140;

/// One 24-byte record of the pose-slot array at `0x80076C10`.
///
/// The two helpers only ever touch `+0x02`, `+0x04`, `+0x06`, `+0x0A`, `+0x0C`
/// and `+0x14`; `+0x00`, `+0x08`, `+0x0E`, `+0x10` and `+0x12` are carried
/// here so the record round-trips at its retail width but are deliberately
/// left alone by both copies. `+0x14` holds the acting actor's `+0x1BC`
/// animation-descriptor pointer, modelled here as an opaque handle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PoseSlot {
    /// `+0x00` - never written by either helper.
    pub f00: u16,
    /// `+0x02`.
    pub f02: u16,
    /// `+0x04`.
    pub f04: u16,
    /// `+0x06`.
    pub f06: u16,
    /// `+0x08` - never written by either helper.
    pub f08: u16,
    /// `+0x0A`.
    pub f0a: u16,
    /// `+0x0C`.
    pub f0c: u16,
    /// `+0x0E` - never written by either helper.
    pub f0e: u16,
    /// `+0x10` - never written by either helper.
    pub f10: u16,
    /// `+0x12` - never written by either helper.
    pub f12: u16,
    /// `+0x14` - animation-descriptor handle.
    pub anim: u32,
}

/// Straight partial clone of one pose slot into another. `FUN_801D57E8`.
///
/// Copies five halfwords (`+0x02`, `+0x04`, `+0x06`, `+0x0A`, `+0x0C`) plus
/// the word at `+0x14`. It is **not** a `memcpy`: the destination's `+0x00`,
/// `+0x08`, `+0x0E`, `+0x10` and `+0x12` survive.
///
/// Out-of-range indices are a no-op here; retail would address past the array
/// (both indices are scaled `* 0x18` with no bound check).
///
/// PORT: FUN_801D57E8
pub fn pose_slot_copy(slots: &mut [PoseSlot], dst: usize, src: usize) {
    if dst >= slots.len() || src >= slots.len() {
        return;
    }
    let s = slots[src];
    let d = &mut slots[dst];
    d.f02 = s.f02;
    d.f04 = s.f04;
    d.f06 = s.f06;
    d.f0a = s.f0a;
    d.f0c = s.f0c;
    d.anim = s.anim;
}

/// Re-mapped clone of one pose slot into another. `FUN_801D5778`.
///
/// Same array and stride as [`pose_slot_copy`], but three of the six moves are
/// permuted and one is biased:
///
/// ```text
///   dst[+0x02] = src[+0x0A]
///   dst[+0x04] = src[+0x0C]
///   dst[+0x06] = src[+0x06]
///   dst[+0x0A] = src[+0x0A] - 0x140
///   dst[+0x0C] = src[+0x0C]
///   dst[+0x14] = src[+0x14]
/// ```
///
/// Note `dst[+0x02]` and `dst[+0x04]` take the *source's* `+0x0A` / `+0x0C`,
/// so the pair that was the second point of the source record becomes the
/// first point of the clone, and the clone's own second point is the same
/// value shifted one screen width. The subtraction is `addiu`, i.e. wrapping
/// 16-bit.
///
/// PORT: FUN_801D5778
pub fn pose_slot_copy_remapped(slots: &mut [PoseSlot], dst: usize, src: usize) {
    if dst >= slots.len() || src >= slots.len() {
        return;
    }
    let s = slots[src];
    let d = &mut slots[dst];
    d.f02 = s.f0a;
    d.f04 = s.f0c;
    d.f06 = s.f06;
    d.f0a = s.f0a.wrapping_sub(POSE_SLOT_X_BIAS);
    d.f0c = s.f0c;
    d.anim = s.anim;
}

// ---------------------------------------------------------------------------
// FUN_801D9AE8 - tracked-widget pool teardown
// ---------------------------------------------------------------------------

/// Number of tracked-widget slots the teardown walks (`sltiu ..., 0x28`).
pub const WIDGET_SLOTS: usize = 0x28;

/// Number of words the teardown zeroes at `DAT_801C8FA0` after the walk.
pub const WIDGET_SCRATCH_WORDS: usize = 0x10;

/// One tracked-widget slot. Retail splits the record across two arrays with
/// different strides - the `0xC`-stride record at ctx `+0x11B4` (whose `+0x00`
/// is `tag` and `+0x03` is `live`) and the flat pointer array at ctx `+0x1074`
/// - so the pairing by slot index is what this struct captures.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct WidgetSlot {
    /// ctx `+0x11B7 + slot * 0xC`. Non-zero = this slot is tracked.
    pub live: u8,
    /// ctx `+0x11B4 + slot * 0xC`. Cleared alongside `live`.
    pub tag: u8,
    /// ctx `+0x1074 + slot * 4`. `None` models retail's null pointer.
    pub widget: Option<WidgetHandle>,
}

/// The part of a tracked widget the teardown reads: the halfword at `+0x08`,
/// which is the actor id handed to the shared effect trigger.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct WidgetHandle {
    /// Widget `+0x08` - the actor id passed to `FUN_800319A8`.
    pub actor_id: u16,
}

/// Release every tracked battle-UI widget and clear the per-battle scratch.
/// `FUN_801D9AE8`.
///
/// Walks all `0x28` slots. A slot is released only when **both** its `live`
/// flag is non-zero and its pointer is non-null - a live flag with a null
/// pointer is left set, which is retail behaviour, not an oversight of this
/// port. Each released slot yields its widget's `+0x08` actor id; the caller
/// feeds those to the shared effect trigger `FUN_800319A8` (the engine's
/// `Host::actor_effect`) in slot order. Afterwards the pointer, `live` and
/// `tag` are all zeroed, and the 16-word scratch at `DAT_801C8FA0` is wiped
/// unconditionally.
///
/// The same body is compiled into the battle overlay a second time at
/// `0x801F02D0` (see `docs/reference/functions.md`).
///
/// PORT: FUN_801D9AE8
/// REF: FUN_800319A8 (the per-widget release the returned ids feed)
pub fn release_widget_pool(
    slots: &mut [WidgetSlot],
    scratch: &mut [u32; WIDGET_SCRATCH_WORDS],
) -> Vec<u16> {
    let mut released = Vec::new();
    for slot in slots.iter_mut().take(WIDGET_SLOTS) {
        if slot.live != 0
            && let Some(handle) = slot.widget
        {
            released.push(handle.actor_id);
            slot.widget = None;
            slot.live = 0;
            slot.tag = 0;
        }
    }
    scratch.fill(0);
    released
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pool(spec: &[(u16, u16)]) -> Vec<CursorActor> {
        spec.iter()
            .map(|&(liveness, status)| CursorActor { liveness, status })
            .collect()
    }

    #[test]
    fn forward_skips_dead_and_afflicted() {
        // Slot 1 dead, slot 2 afflicted (0x004 is inside the 0xF84 mask),
        // slot 3 selectable.
        let actors = pool(&[(1, 0), (0, 0), (1, 0x004), (1, 0), (1, 0)]);
        let mut cursor = ActorCursor {
            active: 0,
            latest: 9,
            previous: 0,
            depth: 4,
        };
        step_actor_cursor(&mut cursor, CursorStep::Forward, 5, &actors);
        assert_eq!(cursor.active, 3);
        assert_eq!(cursor.latest, 3);
        assert_eq!(cursor.previous, 9, "+0x21 takes the old +0x20");
        assert_eq!(cursor.depth, 5);
    }

    #[test]
    fn forward_exhausted_lands_past_the_bound() {
        let actors = pool(&[(1, 0), (0, 0), (0, 0)]);
        let mut cursor = ActorCursor {
            active: 0,
            ..Default::default()
        };
        step_actor_cursor(&mut cursor, CursorStep::Forward, 3, &actors);
        // Retail does not clamp: the landing index is the first one that
        // failed the bound test.
        assert_eq!(cursor.active, 3);
    }

    #[test]
    fn backward_stops_at_the_ff_wrap() {
        let actors = pool(&[(0, 0), (0, 0), (1, 0)]);
        let mut cursor = ActorCursor {
            active: 2,
            depth: 1,
            ..Default::default()
        };
        step_actor_cursor(&mut cursor, CursorStep::Backward, 3, &actors);
        assert_eq!(cursor.active, 0xff, "walked off the bottom");
        assert_eq!(cursor.depth, 0);
    }

    #[test]
    fn backward_from_zero_is_an_immediate_stop() {
        let actors = pool(&[(1, 0), (1, 0)]);
        let mut cursor = ActorCursor {
            active: 0,
            ..Default::default()
        };
        step_actor_cursor(&mut cursor, CursorStep::Backward, 2, &actors);
        // `active - 1` is already 0xFF, so the scan body never runs.
        assert_eq!(cursor.active, 0xff);
        assert_eq!(cursor.depth, 0xff, "+0x1F still moves");
    }

    #[test]
    fn backward_finds_the_nearest_selectable_below() {
        let actors = pool(&[(1, 0), (1, 0x080), (1, 0)]);
        let mut cursor = ActorCursor {
            active: 2,
            ..Default::default()
        };
        step_actor_cursor(&mut cursor, CursorStep::Backward, 3, &actors);
        assert_eq!(cursor.active, 0, "slot 1 carries a 0xF84 bit");
    }

    #[test]
    fn pose_copy_is_partial() {
        let mut slots = vec![PoseSlot::default(); 4];
        slots[1] = PoseSlot {
            f00: 0x1111,
            f02: 0x2222,
            f04: 0x3333,
            f06: 0x4444,
            f08: 0x5555,
            f0a: 0x6666,
            f0c: 0x7777,
            f0e: 0x8888,
            f10: 0x9999,
            f12: 0xaaaa,
            anim: 0xdead_beef,
        };
        slots[0].f00 = 0xffff;
        slots[0].f08 = 0xeeee;
        slots[0].f0e = 0xdddd;
        pose_slot_copy(&mut slots, 0, 1);
        assert_eq!(slots[0].f02, 0x2222);
        assert_eq!(slots[0].f04, 0x3333);
        assert_eq!(slots[0].f06, 0x4444);
        assert_eq!(slots[0].f0a, 0x6666);
        assert_eq!(slots[0].f0c, 0x7777);
        assert_eq!(slots[0].anim, 0xdead_beef);
        // Untouched by the retail store list.
        assert_eq!(slots[0].f00, 0xffff);
        assert_eq!(slots[0].f08, 0xeeee);
        assert_eq!(slots[0].f0e, 0xdddd);
        assert_eq!(slots[0].f10, 0);
        assert_eq!(slots[0].f12, 0);
    }

    #[test]
    fn pose_copy_remapped_permutes_and_biases() {
        let mut slots = vec![PoseSlot::default(); 2];
        slots[1] = PoseSlot {
            f06: 0x0044,
            f0a: 0x0100,
            f0c: 0x0080,
            anim: 7,
            ..Default::default()
        };
        pose_slot_copy_remapped(&mut slots, 0, 1);
        assert_eq!(slots[0].f02, 0x0100, "dst+2 <- src+0xA");
        assert_eq!(slots[0].f04, 0x0080, "dst+4 <- src+0xC");
        assert_eq!(slots[0].f06, 0x0044);
        assert_eq!(slots[0].f0a, 0x0100u16.wrapping_sub(0x140));
        assert_eq!(slots[0].f0c, 0x0080);
        assert_eq!(slots[0].anim, 7);
    }

    #[test]
    fn widget_teardown_releases_only_flagged_and_pointed_slots() {
        let mut slots = vec![WidgetSlot::default(); WIDGET_SLOTS];
        slots[0] = WidgetSlot {
            live: 1,
            tag: 9,
            widget: Some(WidgetHandle { actor_id: 0x21 }),
        };
        // Flagged but null: retail leaves the flag set.
        slots[1] = WidgetSlot {
            live: 1,
            tag: 3,
            widget: None,
        };
        // Pointed but not flagged: retail leaves the pointer alone.
        slots[2] = WidgetSlot {
            live: 0,
            tag: 0,
            widget: Some(WidgetHandle { actor_id: 0x22 }),
        };
        slots[5] = WidgetSlot {
            live: 0x80,
            tag: 1,
            widget: Some(WidgetHandle { actor_id: 0x23 }),
        };
        let mut scratch = [0xffff_ffffu32; WIDGET_SCRATCH_WORDS];

        let released = release_widget_pool(&mut slots, &mut scratch);

        assert_eq!(released, vec![0x21, 0x23]);
        assert_eq!(slots[0], WidgetSlot::default());
        assert_eq!(slots[1].live, 1, "null pointer leaves the flag set");
        assert!(slots[2].widget.is_some(), "unflagged pointer survives");
        assert_eq!(slots[5], WidgetSlot::default());
        assert!(scratch.iter().all(|&w| w == 0));
    }
}
