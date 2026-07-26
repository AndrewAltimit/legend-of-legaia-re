//! In-field **save/load screen driver** - the actor that carries a field
//! session through the overlay swap into the memory-card UI and back.
//!
//! (The `PORT:` tag sits on [`FieldSaveScreenActor::tick`], the item that
//! implements the body.)
//!
//! The field overlay (PROT 897) cannot host the save UI - that code is in
//! the menu overlay (PROT 899) - so a save point or the pause menu's save row
//! means paging one overlay out and the other in, running the UI, and paging
//! back. `FUN_80024190` is the eleven-state machine that sequences it, and it
//! retires itself once the field overlay is resident again. Its spawn
//! descriptor is `0x800706BC`, whose `+0x8` handler word reads `0x80024190`
//! on the disc; the field/world fade SM `FUN_801EE5D4` allocates from it.
//!
//! ## How it gets ticked
//!
//! State 0 does two things that together form the call chain: it writes its
//! own actor pointer to `_DAT_8007B8E0` and sets the game mode to `0x16`
//! (mode 22, CARD INIT). Mode 23's frame body `FUN_80017978` is
//! `(*_DAT_8007B8E0)[+0x0C]()` - see [`crate::mode::CARD_FRAME_BODY`] - so
//! from the next frame the CARD mode pair ticks *this* actor and nothing
//! else. That is why mode 23 runs no actor passes and no display flip: this
//! handler is the entire frame.
//!
//! Mode 22's own init `FUN_8002574C` would overwrite `_DAT_8007B8E0` with the
//! standalone card actor (descriptor `0x800706D4`, handler `0x801E36A0`), but
//! its whole body sits behind `gp+0x7E8 != 0` (`0x8002576C`), so on the
//! in-field path the registration this actor made survives. The two are
//! different actors for different entry points, not two names for one.
//!
//! ## The eleven states (jump table `0x80010898`)
//!
//! | state | body |
//! |---|---|
//! | 0 | register at `_DAT_8007B8E0`, game mode `0x16`, advance |
//! | 1, 3, 5, 7, 9 | advance when `FUN_8003DE7C(1)` reports the queue idle |
//! | 2 | load overlay slot `4` (menu), advance |
//! | 4 | run the UI: `actor+0x5C == 0` calls the load-side dispatcher `FUN_801DD35C`, non-zero the save-write flow `FUN_801DC6B4`; advance only on a non-zero return |
//! | 6 | load overlay slot `2` (field), advance |
//! | 8 | restore the slot-B pair via `FUN_80025BA0`, advance |
//! | 10 | game mode `3` (field per-frame), clear `DAT_8007B648`, `actor[+0x10] \|= 8` |
//!
//! State 10 does **not** advance - it retires instead, so the machine has no
//! state 11 even though the table is bounded at `< 0xB`.
//!
//! ## The cover fill
//!
//! States 0, 1 and 2 - the frames where an overlay is mid-swap and the
//! screen would otherwise show whatever the outgoing overlay left - also emit
//! a full-screen quad every tick. It is a 24-byte, 5-word primitive
//! (`0x05000000` tag, code word `0x2BFFFFFF` = semi-transparent flat white)
//! spanning `y = -4` to the framebuffer height, with the extents read from
//! the scratchpad rect at `0x1F80038C` / `0x1F80038E`, followed by a 12-byte
//! draw-mode packet from `FUN_80059010(p, 0, 0, 0x4E, 0)`. Both come out of
//! the primitive cursor `0x1F8003A0` and are linked with `FUN_8003D2C4`.
//!
//! **The ordering-table index is `0`, not `ot_size - 1`.** The index
//! computation at `0x800241FC..0x8002420C` is
//!
//! ```text
//! addiu v1, v1, -0x1        ; v1 = ot_size - 1
//! bgez  v1, 0x80024260      ; taken for any non-empty OT ...
//! _clear s1                 ;   ... with s1 = 0 in the delay slot
//! j     0x80024260
//! _move s1, v1              ; only reached when ot_size == 0
//! ```
//!
//! so `s1 = min(0, ot_size - 1)`: zero for every real ordering table, and the
//! negative `ot_size - 1` only in the degenerate empty case. Reading `s1` as
//! `ot_size - 1` (the deepest bucket) inverts the depth the cover is drawn
//! at. [`cover_fill_ot_index`] is the arithmetic.
//!
//! ## NOT WIRED
//!
//! The engine has the save UI ([`crate::save_select`], [`crate::card_flow`])
//! but reaches it as host screen state, not by swapping a code overlay under
//! a running field session - [`crate::overlay_loader`] models the retail
//! loader's bookkeeping without there being separate overlay images to page.
//! So the five queue waits and the two `LoadOverlaySlot` steps have nothing
//! to wait on, and the sequencing this actor exists to provide is exactly the
//! part the clean-room engine does not need. What has to exist first is a
//! host that actually pages overlay code (or a deliberate decision to model
//! the swap latency), not a caller.

/// States the machine dispatches (`sltiu v0, v1, 0xb` at `0x800241AC`).
pub const STATE_COUNT: u16 = 11;

/// `_DAT_8007B83C` value state 0 writes: mode 22, CARD INIT.
pub const CARD_INIT_MODE: u16 = 0x16;

/// `_DAT_8007B83C` value state 10 writes: mode 3, field per-frame.
pub const FIELD_FRAME_MODE: u16 = 3;

/// `FUN_8003EBE4(4, 0)` - the menu overlay (PROT 899).
pub const MENU_OVERLAY_SLOT: i32 = 4;

/// `FUN_8003EBE4(2, 0)` - the field overlay (PROT 897).
pub const FIELD_OVERLAY_SLOT: i32 = 2;

/// `actor[+0x10]` bit state 10 sets to retire.
pub const ACTOR_KILL_BIT: u32 = 8;

/// Cover-fill OT tag: 5 words of primitive.
pub const COVER_FILL_TAG: u32 = 0x0500_0000;

/// Cover-fill GP0 code word - semi-transparent flat white.
pub const COVER_FILL_CODE: u32 = 0x2BFF_FFFF;

/// Cover-fill top edge, four pixels above the framebuffer.
pub const COVER_FILL_TOP: i16 = -4;

/// Bytes the cover-fill quad takes out of the primitive cursor.
pub const COVER_FILL_PRIM_BYTES: usize = 0x18;

/// Bytes the draw-mode packet takes out of the primitive cursor.
pub const DRAW_MODE_PRIM_BYTES: usize = 0x0C;

/// `FUN_80059010`'s fourth argument for the cover fill's draw-mode packet.
pub const DRAW_MODE_PARAM: i32 = 0x4E;

/// The ordering-table bucket the cover fill is linked into.
///
/// `min(0, ot_size - 1)` - bucket `0` for any non-empty table. See the
/// module docs for why this is not `ot_size - 1`.
pub fn cover_fill_ot_index(ot_size: u16) -> i32 {
    let n = i32::from(ot_size) - 1;
    if n < 0 { n } else { 0 }
}

/// One emitted cover fill.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CoverFill {
    /// Bucket from [`cover_fill_ot_index`].
    pub ot_index: i32,
    /// `0x2BFFFFFF`.
    pub code: u32,
    /// The four screen-space corners, in the order retail stores them:
    /// top-left, top-right, bottom-left, bottom-right.
    pub corners: [(i16, i16); 4],
    /// `FUN_80059010`'s parameter for the trailing draw-mode packet.
    pub draw_mode_param: i32,
}

/// The globals one tick reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SaveScreenInput {
    /// `FUN_8003DE7C(1) != 0` - the CD / overlay queue is still working.
    pub queue_busy: bool,
    /// `actor+0x5C != 0` - this is a save, not a load.
    pub is_save: bool,
    /// The UI dispatcher returned non-zero, i.e. the player is done.
    pub ui_done: bool,
    /// `0x1F80038C` - framebuffer width.
    pub screen_width: u16,
    /// `0x1F80038E` - framebuffer height.
    pub screen_height: u16,
    /// `*(u16 *)0x1F8003A6` - ordering-table length.
    pub ot_size: u16,
}

/// What a tick asks the host to do, in emission order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SaveScreenEffect {
    /// `_DAT_8007B8E0 = actor` - become the actor mode 23's frame body ticks.
    RegisterCardActor,
    /// `_DAT_8007B83C = mode`.
    EnterGameMode(u16),
    /// `FUN_8003EBE4(slot, 0)`.
    LoadOverlaySlot(i32),
    /// State 4, load side: `FUN_801DD35C(0, 0)`.
    RunLoadSelect,
    /// State 4, save side: `FUN_801DC6B4()`.
    RunSaveWrite,
    /// `FUN_80025BA0()`.
    RestoreSlotBPair,
    /// The mid-swap screen cover.
    EmitCoverFill(CoverFill),
    /// `DAT_8007B648 = 0` - clears the byte
    /// [`crate::scene_transition_actor`] sets during a transition.
    ClearTransitionScreenByte,
    /// `actor[+0x10] |= 8`.
    Retire,
}

/// The actor's own state: `actor+0x1A` and the save/load discriminator at
/// `actor+0x5C`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FieldSaveScreenActor {
    /// `actor+0x1A`.
    pub state: u16,
}

impl FieldSaveScreenActor {
    /// One frame.
    ///
    /// PORT: FUN_80024190
    ///
    /// NOT WIRED: the engine reaches the save UI as host screen state, so
    /// there is no code-overlay swap for the five queue waits and the two
    /// `LoadOverlaySlot` steps to sequence. A host that actually pages
    /// overlay images is the prerequisite; see the module docs.
    pub fn tick(&mut self, input: SaveScreenInput) -> Vec<SaveScreenEffect> {
        use SaveScreenEffect as E;
        let mut out = Vec::new();
        if self.state >= STATE_COUNT {
            return out;
        }

        // The three mid-swap states share a tail: advance (conditionally),
        // then draw the cover. Everything else either advances or retires.
        match self.state {
            0 => {
                out.push(E::RegisterCardActor);
                out.push(E::EnterGameMode(CARD_INIT_MODE));
                self.state += 1;
                out.push(E::EmitCoverFill(self.cover_fill(input)));
            }
            1 => {
                if !input.queue_busy {
                    self.state += 1;
                }
                out.push(E::EmitCoverFill(self.cover_fill(input)));
            }
            2 => {
                out.push(E::LoadOverlaySlot(MENU_OVERLAY_SLOT));
                self.state += 1;
                out.push(E::EmitCoverFill(self.cover_fill(input)));
            }
            3 | 5 | 7 | 9 => {
                if !input.queue_busy {
                    self.state += 1;
                }
            }
            4 => {
                out.push(if input.is_save {
                    E::RunSaveWrite
                } else {
                    E::RunLoadSelect
                });
                if input.ui_done {
                    self.state += 1;
                }
            }
            6 => {
                out.push(E::LoadOverlaySlot(FIELD_OVERLAY_SLOT));
                self.state += 1;
            }
            8 => {
                out.push(E::RestoreSlotBPair);
                self.state += 1;
            }
            _ => {
                // State 10 - hand back to the field and retire. No advance.
                out.push(E::EnterGameMode(FIELD_FRAME_MODE));
                out.push(E::ClearTransitionScreenByte);
                out.push(E::Retire);
            }
        }
        out
    }

    fn cover_fill(&self, input: SaveScreenInput) -> CoverFill {
        let w = input.screen_width as i16;
        let h = input.screen_height as i16;
        CoverFill {
            ot_index: cover_fill_ot_index(input.ot_size),
            code: COVER_FILL_CODE,
            corners: [(0, COVER_FILL_TOP), (w, COVER_FILL_TOP), (0, h), (w, h)],
            draw_mode_param: DRAW_MODE_PARAM,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::SaveScreenEffect as E;
    use super::*;

    fn input() -> SaveScreenInput {
        SaveScreenInput {
            screen_width: 320,
            screen_height: 240,
            ot_size: 0x200,
            ..Default::default()
        }
    }

    /// What the actor is for: it must get from a running field session into
    /// the menu overlay, run the UI for as long as the player is in it, page
    /// the field overlay back, and hand the game to mode 3 - in that order,
    /// visiting every state.
    #[test]
    fn a_full_save_visits_every_state_and_hands_back_to_the_field() {
        let mut a = FieldSaveScreenActor::default();
        let mut visited = vec![a.state];
        let mut effects = Vec::new();
        for frame in 0..64 {
            // The queue takes a frame to clear; the player leaves the UI on
            // frame 8.
            let out = a.tick(SaveScreenInput {
                queue_busy: frame % 2 == 0,
                is_save: true,
                ui_done: frame >= 8,
                ..input()
            });
            let retired = out.contains(&E::Retire);
            effects.extend(out);
            if a.state != *visited.last().unwrap() {
                visited.push(a.state);
            }
            if retired {
                break;
            }
        }
        assert_eq!(visited, (0..=10).collect::<Vec<_>>(), "every state runs");
        assert_eq!(effects.first(), Some(&E::RegisterCardActor));
        assert!(effects.contains(&E::EnterGameMode(CARD_INIT_MODE)));
        assert!(effects.contains(&E::LoadOverlaySlot(MENU_OVERLAY_SLOT)));
        assert!(effects.contains(&E::RunSaveWrite));
        assert!(!effects.contains(&E::RunLoadSelect));
        assert!(effects.contains(&E::LoadOverlaySlot(FIELD_OVERLAY_SLOT)));
        assert!(effects.contains(&E::RestoreSlotBPair));
        // The terminal hand-off is the documented pair, and it is last.
        let tail = &effects[effects.len() - 3..];
        assert_eq!(
            tail,
            [
                E::EnterGameMode(FIELD_FRAME_MODE),
                E::ClearTransitionScreenByte,
                E::Retire,
            ]
        );
    }

    /// `actor+0x5C` is the only thing that picks the flow, and it picks one
    /// of the two - never both, never neither.
    #[test]
    fn the_save_load_discriminator_picks_exactly_one_flow() {
        for is_save in [false, true] {
            let mut a = FieldSaveScreenActor { state: 4 };
            let out = a.tick(SaveScreenInput { is_save, ..input() });
            let want = if is_save {
                E::RunSaveWrite
            } else {
                E::RunLoadSelect
            };
            assert_eq!(out, vec![want]);
            assert_eq!(a.state, 4, "an unfinished UI does not advance");
        }
    }

    /// State 4 idles for as long as the player is in the UI - that is the
    /// state the actor spends nearly all its life in.
    #[test]
    fn the_ui_state_idles_until_the_dispatcher_returns_non_zero() {
        let mut a = FieldSaveScreenActor { state: 4 };
        for _ in 0..30 {
            a.tick(input());
            assert_eq!(a.state, 4);
        }
        a.tick(SaveScreenInput {
            ui_done: true,
            ..input()
        });
        assert_eq!(a.state, 5);
    }

    /// The cover is drawn exactly while an overlay is mid-swap - states 0, 1
    /// and 2 - and never after the menu overlay is resident.
    #[test]
    fn the_cover_fill_covers_only_the_swap_frames() {
        for state in 0..STATE_COUNT {
            let mut a = FieldSaveScreenActor { state };
            let out = a.tick(input());
            let covered = out.iter().any(|e| matches!(e, E::EmitCoverFill(_)));
            assert_eq!(covered, state <= 2, "state {state}");
        }
    }

    /// The quad spans the whole framebuffer plus a four-pixel bleed above it,
    /// with the extents taken from the scratchpad rect rather than baked in.
    #[test]
    fn the_cover_fill_spans_the_framebuffer_with_a_top_bleed() {
        let mut a = FieldSaveScreenActor::default();
        let out = a.tick(SaveScreenInput {
            screen_width: 512,
            screen_height: 256,
            ..input()
        });
        let E::EmitCoverFill(fill) = out
            .iter()
            .find(|e| matches!(e, E::EmitCoverFill(_)))
            .copied()
            .unwrap()
        else {
            unreachable!()
        };
        assert_eq!(fill.code, COVER_FILL_CODE);
        assert_eq!(fill.corners, [(0, -4), (512, -4), (0, 256), (512, 256)]);
        assert_eq!(fill.draw_mode_param, DRAW_MODE_PARAM);
    }

    /// The bucket is `0` for every real ordering table. A port that read the
    /// index as `ot_size - 1` would link the cover at the far end of the
    /// table and invert the depth it is drawn at.
    #[test]
    fn the_cover_fill_links_into_bucket_zero() {
        for ot_size in [1u16, 2, 0x100, 0x200, 0xFFFF] {
            assert_eq!(cover_fill_ot_index(ot_size), 0, "ot_size {ot_size}");
        }
        // Only the degenerate empty table produces anything else.
        assert_eq!(cover_fill_ot_index(0), -1);
    }

    /// State 10 retires instead of advancing, so the machine never reaches a
    /// twelfth state and a stuck actor cannot re-enter the field mode twice.
    #[test]
    fn the_terminal_state_retires_rather_than_advancing() {
        let mut a = FieldSaveScreenActor { state: 10 };
        let first = a.tick(input());
        assert!(first.contains(&E::Retire));
        assert_eq!(a.state, 10);
        // A host that ticks it again before reaping gets the same tail, not
        // a fall-through into an undefined state.
        assert_eq!(a.tick(input()), first);
    }

    /// Beyond the table bound the handler is a no-op.
    #[test]
    fn out_of_range_states_do_nothing() {
        let mut a = FieldSaveScreenActor { state: 11 };
        assert!(a.tick(input()).is_empty());
        assert_eq!(a.state, 11);
    }
}
