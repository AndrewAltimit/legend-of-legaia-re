//! Per-frame **animation cue track** walker - the producer that turns a battle
//! action's baked `(frame, cue_id)` list into arts-voice XA requests and SPU
//! ring cues.
//!
//! PORT: FUN_800508dc - the walker itself.
//!
//! NOT WIRED: nothing on the engine's frame path owns the two inputs this
//! needs. The track lives at `entry + 0x54` of a **playing battle action
//! entry** (the LZS-decoded per-character `data\battle\PLAYERn` record the
//! battle-form assembler seats, see `docs/formats/battle-data-pack.md`), and
//! the cursor is a field of the retail battle actor (`+0x1F6`). Neither the
//! entry nor the actor is modelled in `legaia-engine-audio`, and the engine's
//! battle actor (`legaia_engine_core`) carries neither field - so there is no
//! value to pass. Two things close the gap, both outside this crate:
//!
//! * the battle-form assembler keeping the playing entry's cue track alongside
//!   the mesh it already splices, and
//! * the battle actor growing the `+0x1F6` cursor so [`AnimCueState`] can be
//!   ticked once per animation frame.
//!
//! The arithmetic here is the part that is reused unchanged once those exist;
//! [`AnimCueEmit::xa_shout`] already lands on the same `(clip_slot, channel)`
//! pair [`crate::ArtsShoutBank`] is keyed on, and [`AnimCueEmit::Dispatch`]
//! lands on [`crate::classify_cue`].
//!
//! REF: FUN_8004fe5c - the battle SFX-cue router a [`AnimCueEmit::Route`]
//! feeds (ported as `legaia_engine_core::sfx_cue`; it is the arm that splits
//! `id >= 0x100` off to the CD-XA clip player and everything below it into the
//! 4-slot ring).
//! REF: FUN_8004fcc8 - the menu-cue / voice dispatcher an
//! [`AnimCueEmit::Dispatch`] feeds ([`crate::classify_cue`]).
//! REF: FUN_80056798 - the BIOS `rand()` thunk; injected here as a closure so
//! the *consumption* of the draw is what is ported, not the BIOS LCG.
//!
//! # What the track is
//!
//! A playing action entry carries an 8-slot cue track at `+0x54`, stride 4:
//!
//! ```text
//! +0  u16  frame   trigger frame (compared against the low byte of the clip key)
//! +2  u16  cue     cue id; 0 terminates the track
//! ```
//!
//! The actor's own byte at `+0x1F6` is a **persistent cursor** into it, so the
//! walker is resumable: each call fires every cue whose trigger frame the clip
//! has already reached and leaves the cursor on the first one it has not.
//!
//! # The `0xC8..=0xFF` band is the arts voice
//!
//! For a **party** slot (battle slot `< 3`) a cue id in `0xC8..=0xFF`, except
//! `0xFA`, is re-based by `+0x38` into the `>= 0x100` namespace that
//! `FUN_8004FE5C` routes to the CD-XA clip player instead of the SPU ring -
//! `0xC8 + 0x38 = 0x100`, so the whole band maps onto arts-voice clips.
//!
//! Three ids in that band are the per-character **shout**:
//!
//! | cue id | `+0x38` | XA clip slot | character |
//! |---|---|---|---|
//! | `0xD7` | `0x10F` | `26` | Vahn |
//! | `0xE7` | `0x11F` | `27` | Noa |
//! | `0xF7` | `0x12F` | `28` | Gala |
//!
//! and they get three things the rest of the band does not:
//!
//! 1. a **two-take coin flip** - one BIOS `rand()` draw, `id + 0x38 - (r % 2)`,
//!    so the shout alternates between XA channels `7` and `6` of the
//!    character's bank;
//! 2. a **per-character tally**, `+1` into the live character record's `+0x98`
//!    word, bumped before any gate - so it counts requests, not playbacks; and
//! 3. a **mute bit** - record `+0xF8 & 0x2000` suppresses the shout outright.
//!
//! The coin flip is also conditional on the CD being **free**. While a load is
//! in flight (`_DAT_8007BC20 != 0`) no XA stream can start, so the shout falls
//! back to a fixed SPU ring cue through `FUN_8004FCC8`, picked by roster id -
//! and the mapping is *not* monotonic: Vahn `0x56`, Noa `0x62`, Gala `0x5C`.
//!
//! # Everything else
//!
//! Cue ids below `0xC8` (and `0xFA`, and every id on a monster slot) route
//! through `FUN_8004FE5C` unchanged except for a `+1` nudge when the entry's
//! staged anim id is exactly `0x12`. On a party slot whose record has the
//! `0x2000` bit set that nudge becomes a **suppression** for ids `>= 0x4D`.
//!
//! Source: `ghidra/scripts/funcs/800508dc.txt` (disassembly; the Ghidra C for
//! this body inverts the `id != 0xF7` fall-through and loses the delay-slot
//! `sltiu a0,a0,1`, so it is not the reference).

/// One `(frame, cue)` pair of a playing entry's cue track (`entry + 0x54`,
/// stride 4).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AnimCueSlot {
    /// Trigger frame, compared against the zero-extended clip key.
    pub frame: u16,
    /// Cue id; `0` terminates the track.
    pub cue: u16,
}

/// Retail track length - the walker refuses to advance past cursor `8`.
pub const ANIM_CUE_TRACK_LEN: u8 = 8;

/// Cue id band that re-bases into the CD-XA arts-voice namespace on a party
/// slot (`id + ARTS_VOICE_REBASE >= 0x100`).
pub const ARTS_VOICE_BAND_START: u16 = 0xC8;
/// Offset added to a party slot's `0xC8..=0xFF` cue before routing.
pub const ARTS_VOICE_REBASE: u16 = 0x38;
/// The one id inside the band that is *not* an arts-voice cue.
pub const ARTS_VOICE_BAND_HOLE: u16 = 0xFA;
/// Per-character shout cue ids, in roster order (Vahn, Noa, Gala).
pub const SHOUT_CUE_IDS: [u16; 3] = [0xD7, 0xE7, 0xF7];
/// SPU ring cue each character falls back to while the CD is busy, indexed by
/// `char_id - 1`. Deliberately non-monotonic - this is the retail order.
pub const SHOUT_RING_FALLBACK: [u16; 3] = [0x56, 0x62, 0x5C];
/// Character-record bit (`record + 0xF8`) that mutes the shout.
pub const SHOUT_MUTE_BIT: u32 = 0x2000;
/// Staged anim id that nudges a sub-band cue by `+1`.
pub const ANIM_ID_NUDGE: u8 = 0x12;
/// Sub-band cue ids at or above this are suppressed (rather than nudged) when
/// the record's [`SHOUT_MUTE_BIT`] is set.
pub const MUTE_SUPPRESS_FROM: u16 = 0x4D;

/// The retail actor + record fields the walk reads.
#[derive(Debug, Clone, Copy)]
pub struct AnimCueActor {
    /// Battle slot. `< 3` is a party seat; `3..=6` are the monster seats.
    pub slot: u8,
    /// `DAT_8007BD10[slot]` - the 1-based roster id (Vahn / Noa / Gala /
    /// Terra) that selects the live `0x414`-stride character record.
    pub char_id: u8,
    /// The playing entry's staged anim id (`entry + 0x77`).
    pub anim_id: u8,
    /// Character record `+0xF8 & 0x2000` - the shout mute bit.
    pub voice_muted: bool,
    /// `_DAT_8007BC20 != 0` - a CD load is in flight, so no XA stream can
    /// start and the shout degrades to a ring cue.
    pub cd_busy: bool,
}

/// What one fired cue asks the host to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnimCueEmit {
    /// `FUN_8004FE5C(id, slot)` - the battle SFX-cue router. `id >= 0x100` is
    /// an arts-voice XA request; below that it is an SPU ring cue.
    Route {
        /// Already-rebased cue id.
        id: u16,
        /// Battle slot passed through as the router's second argument.
        slot: u8,
    },
    /// `FUN_8004FCC8(id)` - the menu-cue dispatcher, the CD-busy shout
    /// fallback.
    Dispatch {
        /// Ring cue id.
        id: u16,
    },
    /// The cue was reached but deliberately fired nothing.
    Suppressed {
        /// The raw track id, for tracing.
        id: u16,
    },
}

impl AnimCueEmit {
    /// The `(clip_slot, channel)` an arts-voice [`Self::Route`] resolves to,
    /// mirroring the `id >= 0x100` arm of `FUN_8004FE5C`: slot
    /// `(id - 0x100) >> 3` with the `1 / 3 / 5` remap onto `26 / 27 / 28`, and
    /// channel `id & 7`. `None` for anything that is not an XA request.
    pub fn xa_shout(self) -> Option<(u8, u8)> {
        let Self::Route { id, .. } = self else {
            return None;
        };
        if id < 0x100 {
            return None;
        }
        let raw = ((id - 0x100) >> 3) as u8;
        let clip = match raw {
            1 => 26,
            3 => 27,
            5 => 28,
            other => other,
        };
        Some((clip, (id & 7) as u8))
    }
}

/// Result of one walk: the cursor to store back and the cues that fired.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AnimCueWalk {
    /// The value retail stores into actor `+0x1F6`.
    pub cursor: u8,
    /// `false` when the cursor's slot was already the terminator, which is the
    /// one path that leaves `+0x1F6` untouched.
    pub cursor_committed: bool,
    /// Cues fired, in track order.
    pub emits: Vec<AnimCueEmit>,
    /// How many times the character record's `+0x98` shout tally was bumped.
    pub shout_tally: u32,
}

/// Walk `track` from `cursor` and fire every cue the clip has reached.
///
/// `key` is the low byte of the clip's frame key (the caller's `param_3`);
/// `rng` stands in for the BIOS `rand()` thunk and is drawn **once per shout**,
/// only on the branch that reaches it.
pub fn walk_anim_cues(
    actor: &AnimCueActor,
    track: &[AnimCueSlot],
    cursor: u8,
    key: u8,
    rng: &mut impl FnMut() -> i32,
) -> AnimCueWalk {
    let at = |i: u8| -> AnimCueSlot { track.get(i as usize).copied().unwrap_or_default() };

    let mut out = AnimCueWalk {
        cursor,
        ..Default::default()
    };

    // Pre-loop guards. A terminator under the cursor returns without ever
    // touching `+0x1F6`; a not-yet-reached frame commits the cursor unchanged
    // (retail lands mid-way through the write-back sequence).
    let mut cue = at(cursor).cue;
    if cue == 0 {
        return out;
    }
    out.cursor_committed = true;
    if u16::from(key) < at(cursor).frame {
        return out;
    }

    let mut cursor = cursor;
    loop {
        if cursor >= ANIM_CUE_TRACK_LEN {
            break;
        }
        out.emits
            .push(fire_one(actor, cue, rng, &mut out.shout_tally));
        // The increment sits in the loop-tail branch's delay slot, so it lands
        // even on the iteration that breaks.
        cursor += 1;
        out.cursor = cursor;
        let next = at(cursor);
        cue = next.cue;
        if u16::from(key) < next.frame || cue == 0 {
            break;
        }
    }
    out
}

/// The per-cue decision, factored out of the walk.
fn fire_one(
    actor: &AnimCueActor,
    cue: u16,
    rng: &mut impl FnMut() -> i32,
    shout_tally: &mut u32,
) -> AnimCueEmit {
    let party = actor.slot < 3;

    // Arts-voice band: party seats only, `0xC8..` minus the `0xFA` hole.
    if party && cue >= ARTS_VOICE_BAND_START && cue != ARTS_VOICE_BAND_HOLE {
        if !SHOUT_CUE_IDS.contains(&cue) {
            // Rest of the band: straight re-base, no tally, no gate.
            return AnimCueEmit::Route {
                id: cue + ARTS_VOICE_REBASE,
                slot: actor.slot,
            };
        }
        // A shout. The tally is bumped before either gate.
        *shout_tally += 1;
        if actor.voice_muted {
            return AnimCueEmit::Suppressed { id: cue };
        }
        if actor.cd_busy {
            // No XA stream can start; degrade to the roster's ring cue.
            return match SHOUT_RING_FALLBACK.get(actor.char_id.wrapping_sub(1) as usize) {
                Some(&id) => AnimCueEmit::Dispatch { id },
                // char_id 4 (Terra) has no shout bank and no fallback.
                None => AnimCueEmit::Suppressed { id: cue },
            };
        }
        // Two-take coin flip: `-(r % 2)` picks channel 7 or 6.
        let take = (rng() % 2) as u16;
        return AnimCueEmit::Route {
            id: cue + ARTS_VOICE_REBASE - take,
            slot: actor.slot,
        };
    }

    // Sub-band (and every monster-seat cue). A monster seat takes no nudge at
    // all; a party seat nudges by one when the staged anim id is exactly 0x12.
    let mut nudge: u16 = 0;
    if party {
        nudge = u16::from(actor.anim_id == ANIM_ID_NUDGE);
        if actor.voice_muted && cue >= MUTE_SUPPRESS_FROM {
            return AnimCueEmit::Suppressed { id: cue };
        }
    }
    AnimCueEmit::Route {
        id: cue + nudge,
        slot: actor.slot,
    }
}

/// Convenience wrapper that owns the cursor across frames.
#[derive(Debug, Clone, Copy, Default)]
pub struct AnimCueState {
    /// Retail actor `+0x1F6`.
    pub cursor: u8,
}

impl AnimCueState {
    /// Reset for a freshly staged clip (retail zeroes `+0x1F6` at stage time).
    pub fn rewind(&mut self) {
        self.cursor = 0;
    }

    /// Tick one animation frame, storing the walk's cursor back the way retail
    /// does (only when the walk committed one).
    pub fn tick(
        &mut self,
        actor: &AnimCueActor,
        track: &[AnimCueSlot],
        key: u8,
        rng: &mut impl FnMut() -> i32,
    ) -> AnimCueWalk {
        let walk = walk_anim_cues(actor, track, self.cursor, key, rng);
        if walk.cursor_committed {
            self.cursor = walk.cursor;
        }
        walk
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn party(char_id: u8) -> AnimCueActor {
        AnimCueActor {
            slot: char_id - 1,
            char_id,
            anim_id: 0,
            voice_muted: false,
            cd_busy: false,
        }
    }

    fn track(pairs: &[(u16, u16)]) -> Vec<AnimCueSlot> {
        pairs
            .iter()
            .map(|&(frame, cue)| AnimCueSlot { frame, cue })
            .collect()
    }

    fn no_rng() -> impl FnMut() -> i32 {
        || panic!("rng drawn on a branch that must not reach it")
    }

    #[test]
    fn terminator_under_cursor_leaves_the_field_untouched() {
        let t = track(&[(0, 0)]);
        let w = walk_anim_cues(&party(1), &t, 0, 200, &mut no_rng());
        assert!(!w.cursor_committed);
        assert!(w.emits.is_empty());
    }

    #[test]
    fn unreached_frame_commits_the_cursor_unchanged() {
        let t = track(&[(40, 0x10)]);
        let w = walk_anim_cues(&party(1), &t, 0, 39, &mut no_rng());
        assert!(w.cursor_committed);
        assert_eq!(w.cursor, 0);
        assert!(w.emits.is_empty());
    }

    #[test]
    fn fires_every_reached_cue_and_parks_on_the_first_unreached() {
        let t = track(&[(0, 0x10), (5, 0x11), (60, 0x12), (0, 0)]);
        let w = walk_anim_cues(&party(1), &t, 0, 10, &mut no_rng());
        assert_eq!(w.cursor, 2);
        assert_eq!(
            w.emits,
            vec![
                AnimCueEmit::Route { id: 0x10, slot: 0 },
                AnimCueEmit::Route { id: 0x11, slot: 0 },
            ]
        );
    }

    #[test]
    fn track_terminator_stops_the_walk_but_still_advances_the_cursor() {
        let t = track(&[(0, 0x10), (0, 0)]);
        let w = walk_anim_cues(&party(1), &t, 0, 200, &mut no_rng());
        assert_eq!(w.cursor, 1);
        assert_eq!(w.emits.len(), 1);
    }

    #[test]
    fn cursor_never_walks_past_eight() {
        let t: Vec<AnimCueSlot> = (0..12)
            .map(|i| AnimCueSlot {
                frame: 0,
                cue: 0x20 + i,
            })
            .collect();
        let w = walk_anim_cues(&party(1), &t, 0, 255, &mut no_rng());
        assert_eq!(w.cursor, ANIM_CUE_TRACK_LEN);
        assert_eq!(w.emits.len(), ANIM_CUE_TRACK_LEN as usize);
    }

    #[test]
    fn arts_voice_band_rebases_by_0x38() {
        let t = track(&[(0, 0xC8), (0, 0)]);
        let w = walk_anim_cues(&party(1), &t, 0, 255, &mut no_rng());
        assert_eq!(w.emits, vec![AnimCueEmit::Route { id: 0x100, slot: 0 }]);
        assert_eq!(w.shout_tally, 0);
    }

    #[test]
    fn band_hole_0xfa_falls_through_to_the_sub_band_path() {
        let t = track(&[(0, 0xFA), (0, 0)]);
        let w = walk_anim_cues(&party(1), &t, 0, 255, &mut no_rng());
        // No re-base: 0xFA routes as-is.
        assert_eq!(w.emits, vec![AnimCueEmit::Route { id: 0xFA, slot: 0 }]);
    }

    #[test]
    fn shout_coin_flip_picks_channel_seven_then_six() {
        for (draw, want_id, want_chan) in [(0i32, 0x10Fu16, 7u8), (1, 0x10E, 6)] {
            let t = track(&[(0, 0xD7), (0, 0)]);
            let mut rng = move || draw;
            let w = walk_anim_cues(&party(1), &t, 0, 255, &mut rng);
            assert_eq!(
                w.emits,
                vec![AnimCueEmit::Route {
                    id: want_id,
                    slot: 0
                }]
            );
            assert_eq!(w.shout_tally, 1);
            assert_eq!(w.emits[0].xa_shout(), Some((26, want_chan)));
        }
    }

    #[test]
    fn each_character_shout_lands_on_its_own_clip_slot() {
        for (char_id, cue, clip) in [(1u8, 0xD7u16, 26u8), (2, 0xE7, 27), (3, 0xF7, 28)] {
            let t = track(&[(0, cue), (0, 0)]);
            let mut rng = || 0;
            let w = walk_anim_cues(&party(char_id), &t, 0, 255, &mut rng);
            assert_eq!(w.emits[0].xa_shout(), Some((clip, 7)));
        }
    }

    #[test]
    fn mute_bit_suppresses_the_shout_but_still_tallies_it() {
        let t = track(&[(0, 0xD7), (0, 0)]);
        let mut a = party(1);
        a.voice_muted = true;
        let w = walk_anim_cues(&a, &t, 0, 255, &mut no_rng());
        assert_eq!(w.emits, vec![AnimCueEmit::Suppressed { id: 0xD7 }]);
        assert_eq!(w.shout_tally, 1);
    }

    #[test]
    fn cd_busy_degrades_the_shout_to_the_roster_ring_cue() {
        for (char_id, cue, ring) in [(1u8, 0xD7u16, 0x56u16), (2, 0xE7, 0x62), (3, 0xF7, 0x5C)] {
            let t = track(&[(0, cue), (0, 0)]);
            let mut a = party(char_id);
            a.cd_busy = true;
            let w = walk_anim_cues(&a, &t, 0, 255, &mut no_rng());
            assert_eq!(w.emits, vec![AnimCueEmit::Dispatch { id: ring }]);
            assert_eq!(w.shout_tally, 1);
        }
    }

    #[test]
    fn cd_busy_fallback_is_reachable_through_classify_cue() {
        // The Dispatch arm's whole point is that it lands on FUN_8004FCC8.
        for &id in &SHOUT_RING_FALLBACK {
            match crate::classify_cue(u32::from(id)) {
                crate::CueDispatch::Ring { .. } => {}
                other => panic!("{id:#x} should be a ring cue, got {other:?}"),
            }
        }
    }

    #[test]
    fn anim_id_0x12_nudges_a_sub_band_cue_by_one() {
        let t = track(&[(0, 0x20), (0, 0)]);
        let mut a = party(1);
        a.anim_id = ANIM_ID_NUDGE;
        let w = walk_anim_cues(&a, &t, 0, 255, &mut no_rng());
        assert_eq!(w.emits, vec![AnimCueEmit::Route { id: 0x21, slot: 0 }]);
    }

    #[test]
    fn muted_party_record_suppresses_sub_band_cues_from_0x4d_up() {
        let mut a = party(1);
        a.voice_muted = true;
        let hi = track(&[(0, MUTE_SUPPRESS_FROM), (0, 0)]);
        let w = walk_anim_cues(&a, &hi, 0, 255, &mut no_rng());
        assert_eq!(
            w.emits,
            vec![AnimCueEmit::Suppressed {
                id: MUTE_SUPPRESS_FROM
            }]
        );
        let lo = track(&[(0, MUTE_SUPPRESS_FROM - 1), (0, 0)]);
        let w = walk_anim_cues(&a, &lo, 0, 255, &mut no_rng());
        assert_eq!(
            w.emits,
            vec![AnimCueEmit::Route {
                id: MUTE_SUPPRESS_FROM - 1,
                slot: 0
            }]
        );
    }

    #[test]
    fn monster_seat_takes_no_nudge_and_no_rebase() {
        let t = track(&[(0, 0xD7), (0, 0)]);
        let a = AnimCueActor {
            slot: 3,
            char_id: 0,
            anim_id: ANIM_ID_NUDGE,
            voice_muted: true,
            cd_busy: false,
        };
        let w = walk_anim_cues(&a, &t, 0, 255, &mut no_rng());
        assert_eq!(w.emits, vec![AnimCueEmit::Route { id: 0xD7, slot: 3 }]);
        assert_eq!(w.shout_tally, 0);
    }

    #[test]
    fn state_wrapper_resumes_across_frames() {
        let t = track(&[(0, 0x10), (5, 0x11), (9, 0x12), (0, 0)]);
        let mut st = AnimCueState::default();
        let mut rng = || 0;
        let a = party(1);
        assert_eq!(st.tick(&a, &t, 0, &mut rng).emits.len(), 1);
        assert_eq!(st.cursor, 1);
        assert_eq!(st.tick(&a, &t, 4, &mut rng).emits.len(), 0);
        assert_eq!(st.tick(&a, &t, 9, &mut rng).emits.len(), 2);
        assert_eq!(st.cursor, 3);
        // Past the end the track terminator holds the cursor put.
        let w = st.tick(&a, &t, 255, &mut rng);
        assert!(!w.cursor_committed);
        assert_eq!(st.cursor, 3);
    }
}
