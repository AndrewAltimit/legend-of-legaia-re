//! The two voice decisions the battle anim commit `FUN_8004AD80` makes for a
//! party seat: the Tactical-Arts **shout** (`FUN_8004C140`) and the
//! SpecialStarter's **fanfare** (the anim-id-`0x1A` block).
//!
//! Both are pure functions of the commit's inputs plus one `rand()` closure,
//! so the world can run them on the frame the clip installs, with the same
//! BIOS stream every other retail caller advances.
//!
//! # When the shout is asked for
//!
//! The commit tests the queue byte **before** the cursor `ctx[+0x15]` for
//! every staged id `>= 0x10` on a party seat (`0x8004BCF0..0x8004BD6C`):
//!
//! | `queue[cursor - 1]` | `ctx[+0x243]` | call |
//! |---|---|---|
//! | `0x19` (the known-art starter) | any | `FUN_8004C140(char, queue[cursor], queue[cursor + 1])` |
//! | anything else | non-zero | `FUN_8004C140(char, queue[cursor - 1], queue[cursor])` |
//! | anything else | `0` | none |
//!
//! The cursor has already passed the staged byte (the strike loop
//! post-increments it), so the first row is the commit of the `0x19` clip
//! itself, keyed on the art constant behind it - the art's wind-up, after
//! the approach run. A newly learned art is staged behind `0x1A`, not
//! `0x19`, and shouts nothing; its voice is the starter's fanfare instead.
//! `ctx[+0x243]` is read before this commit's own `FUN_8004E13C` store
//! (`0x8004BE28`).
//!
//! The shout's `flag` is the byte after the art constant: `0` when the art
//! ends the string, its connector or the next starter when another follows.
//! That byte picks the first-half pool variant, and on the second half it
//! swaps the voice for a sound cue (see [`select_arts_voice`]).
//!
//! # The fanfare
//!
//! See [`starter_commit_voice_ids`].
//!
//! REF: FUN_8004AD80 (both call sites), FUN_8004FCC8 (the dispatcher every
//! fanfare id goes through), FUN_8003D53C (the clip starter the shout calls
//! directly, with no busy gate).

use legaia_art::arts_voice::{
    ArtsVoiceSelectorTables, DUR_TABLE_VA, FIRST_HALF_VARIANT_BASES, SECOND_HALF_BASE,
};

/// The queue byte that marks a known art's starter (`0x19`).
pub const KNOWN_ART_STARTER: u8 = 0x19;

/// The first formation id that forces the shout onto channel `0xC`
/// (`lbu v1,0x9f4(gp)` / `li v0,0x4f` / `bne` / `li a1,0xc` at
/// `0x8004C3FC..0x8004C414`).
pub const FORCED_SHOUT_FORMATION_ID: u8 = 0x4F;
/// The channel [`FORCED_SHOUT_FORMATION_ID`] forces.
pub const FORCED_SHOUT_CHANNEL: u8 = 0x0C;

/// One `FUN_8004C140(char_id, action, flag)` call and the state it reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ArtsVoiceCall {
    /// The 1-based roster id (`DAT_8007BD10[seat]`): 1 Vahn, 2 Noa, 3 Gala.
    /// Anything else returns at once (`sltiu v0,v1,0x3` on `char - 1`).
    pub char_id: u8,
    /// The `a1` byte - the art constant on the starter row.
    pub action: u8,
    /// The `a2` byte.
    pub flag: u8,
    /// `ctx[+0x243]`, the entry solo latch.
    pub solo_latch: u8,
    /// The first formation id (`gp+0x9F4`).
    pub first_enemy: Option<u8>,
    /// The committing seat (`actor[+0x5A]`), which sits in the caller's `s3`
    /// when the selector is entered. The selector forms the clip slot in its
    /// own `s3` only on a multi-member record (`0x8004C3B0..0x8004C3B8`); a
    /// one-member record leaves the caller's value there, so the clip slot
    /// it plays is the seat. No real art constant reaches a one-member
    /// record (every record inside the decoded spans has two or more), so
    /// this is the modelled hazard, not a path the shipped game takes.
    pub caller_seat: u8,
}

/// What one selector call starts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArtsVoiceEmit {
    /// `FUN_8003D53C(slot, channel, dur)`: the shout clip.
    Clip { slot: u8, channel: u8, dur: u32 },
    /// `FUN_8004FCC8(id)` - the second-half sound cue for Vahn (`0x51`) and
    /// Gala (`0x58`).
    Dispatch(u16),
    /// `FUN_8004FE5C(id, ctx[+0x13])` - the second-half sound cue for Noa
    /// (`0xAD`, routed under the acting actor's category).
    Route(u16),
}

/// The channel-pick loop over one candidate record: the membership walk, the
/// re-rolled draw and the party-wide last-pick store. Returns `(clip slot,
/// channel)`.
fn pick_from_record(
    t: &ArtsVoiceSelectorTables,
    rec: u32,
    cslot: u8,
    s3: &mut u8,
    last: &mut u8,
    rand: &mut dyn FnMut() -> u32,
) -> u8 {
    // `lbu v0,0x1(s1); beq v0,zero` - a one-member record skips the walk and
    // the `s3` store. Otherwise the list runs to the next zero from index 2.
    let count = if t.byte(rec + 1) == 0 {
        1u32
    } else {
        *s3 = cslot * 2 + 1;
        let mut k = 2u32;
        while t.byte(rec + k) != 0 {
            k += 1;
        }
        k
    };
    // Retail re-rolls until the member differs from `gp+0xA4A`, which never
    // exits when every member equals it. The port takes the draw instead.
    let all_last = (0..count).all(|k| t.byte(rec + k) == *last);
    let channel = loop {
        let r = rand() as i32;
        let idx = r.rem_euclid(count as i32) as u32;
        let c = t.byte(rec + idx);
        if all_last || c != *last {
            break c;
        }
    };
    *last = channel;
    channel
}

/// The members of the first-half candidate record a call picks from - the
/// record [`select_arts_voice`] walks for `call` - or `None` when the
/// action is outside the character's first-half range. Membership is the
/// pick loop's: byte `+0` alone on a one-member record, else the run up to
/// the next `0` from index `2`.
pub fn first_half_pool(t: &ArtsVoiceSelectorTables, call: &ArtsVoiceCall) -> Option<Vec<u8>> {
    let cslot = call.char_id.checked_sub(1).filter(|&c| c < 3)?;
    let [lo, hi, _] = t.range(cslot);
    if call.action < lo || call.action > hi {
        return None;
    }
    let variant = if call.solo_latch != 0 {
        2
    } else if call.flag != 0 {
        1
    } else {
        0
    };
    let rec =
        FIRST_HALF_VARIANT_BASES[variant][usize::from(cslot)] + u32::from(hi - call.action) * 0x0F;
    let count = if t.byte(rec + 1) == 0 {
        1u32
    } else {
        (2..).find(|&k| t.byte(rec + k) == 0).unwrap_or(2)
    };
    Some((0..count).map(|k| t.byte(rec + k)).collect())
}

/// `dur = (dur_table[cslot * 0x10 + channel] * 0x3C + 99) / 100`.
pub fn shout_dur(t: &ArtsVoiceSelectorTables, cslot: u8, channel: u8) -> u32 {
    let raw = u32::from(t.byte(DUR_TABLE_VA + u32::from(cslot) * 0x10 + u32::from(channel)));
    (raw * 0x3C).div_ceil(100)
}

/// The arts-voice selector, branch for branch.
///
/// 1. **First half**, `lo <= action <= hi`: the record at `base + (hi -
///    action) * 0x0F`, `base` by variant - `ctx[+0x243] != 0` first, then
///    `flag != 0`, then the plain table ([`FIRST_HALF_VARIANT_BASES`]). The
///    pick may be replaced by [`FORCED_SHOUT_CHANNEL`] after the last-pick
///    store, and the duration is read for the channel played.
/// 2. **Second half**, `action >= second_lo`, reached after the first half
///    whatever it did: with `flag == 0` the record at `SECOND_HALF_BASE +
///    (action - second_lo) * 0x10` is picked the same way, with no forced
///    channel; with `flag != 0` no clip plays and a sound cue goes out
///    instead ([`ArtsVoiceEmit::Dispatch`] / [`ArtsVoiceEmit::Route`]).
///
/// `last` is `gp+0xA4A`, shared by every character.
///
/// PORT: FUN_8004C140
pub fn select_arts_voice(
    t: &ArtsVoiceSelectorTables,
    call: &ArtsVoiceCall,
    last: &mut u8,
    rand: &mut dyn FnMut() -> u32,
) -> Vec<ArtsVoiceEmit> {
    let mut out = Vec::new();
    let Some(cslot) = call.char_id.checked_sub(1).filter(|&c| c < 3) else {
        return out;
    };
    let [lo, hi, second_lo] = t.range(cslot);
    let ac = call.action;
    let mut s3 = call.caller_seat;
    if ac >= lo && ac <= hi {
        let variant = if call.solo_latch != 0 {
            2
        } else if call.flag != 0 {
            1
        } else {
            0
        };
        let base = FIRST_HALF_VARIANT_BASES[variant][usize::from(cslot)];
        let rec = base + u32::from(hi - ac) * 0x0F;
        let mut channel = pick_from_record(t, rec, cslot, &mut s3, last, rand);
        if call.first_enemy == Some(FORCED_SHOUT_FORMATION_ID) {
            channel = FORCED_SHOUT_CHANNEL;
        }
        out.push(ArtsVoiceEmit::Clip {
            slot: s3,
            channel,
            dur: shout_dur(t, cslot, channel),
        });
    }
    if ac >= second_lo {
        if call.flag == 0 {
            let rec = SECOND_HALF_BASE[usize::from(cslot)] + u32::from(ac - second_lo) * 0x10;
            let channel = pick_from_record(t, rec, cslot, &mut s3, last, rand);
            out.push(ArtsVoiceEmit::Clip {
                slot: s3,
                channel,
                dur: shout_dur(t, cslot, channel),
            });
        } else {
            out.push(match cslot {
                0 => ArtsVoiceEmit::Dispatch(0x51),
                1 => ArtsVoiceEmit::Route(0xAD),
                _ => ArtsVoiceEmit::Dispatch(0x58),
            });
        }
    }
    out
}

/// The commit's shout call site: whether this commit calls the selector,
/// and with which `(action, flag)`.
///
/// `queue` is `actor[+0x1DF..]`, `cursor` is `ctx[+0x15]` (already past the
/// staged byte), `category` is `actor[+0x1DE]` - the byte retail reads at
/// `queue[-1]` when the cursor is `0`. `solo_latch` is `ctx[+0x243]` before
/// this commit's own store.
///
/// REF: FUN_8004AD80 (`0x8004BCF0..0x8004BD6C`)
pub fn shout_call_args(queue: &[u8], cursor: u8, category: u8, solo_latch: u8) -> Option<(u8, u8)> {
    let at = |i: isize| -> u8 {
        if i < 0 {
            category
        } else {
            queue.get(i as usize).copied().unwrap_or(0)
        }
    };
    let c = isize::from(cursor);
    if at(c - 1) == KNOWN_ART_STARTER {
        Some((at(c), at(c + 1)))
    } else if solo_latch != 0 {
        Some((at(c - 1), at(c)))
    } else {
        None
    }
}

/// The per-character voice id the SpecialStarter's generic arms dispatch:
/// `0x101` / `0x111` / `0x121` = `XA1` / `XA3` / `XA5` channel 1
/// (`0x8004B784..0x8004B7CC`, the same switch again at
/// `0x8004B81C..0x8004B868`). `None` for any other roster id.
pub fn generic_starter_voice_id(char_id: u8) -> Option<u16> {
    match char_id {
        1 => Some(0x101),
        2 => Some(0x111),
        3 => Some(0x121),
        _ => None,
    }
}

/// The Hyper Art fanfare's base id for `(roster id, the constant after the
/// starter)` - the coin flip adds `0` or `3` (`0x8004B8D4..0x8004BB30`).
pub fn hyper_fanfare_base(char_id: u8, next: u8) -> Option<u16> {
    match (char_id, next) {
        (1, 0x1C) => Some(0x104),
        (1, 0x1D) => Some(0x103),
        (1, 0x1E) => Some(0x102),
        (2, 0x1D) => Some(0x114),
        (2, 0x1F) => Some(0x113),
        (2, 0x20) => Some(0x112),
        (3, 0x1C) => Some(0x124),
        (3, 0x1D) => Some(0x123),
        (3, 0x1E) => Some(0x122),
        _ => None,
    }
}

/// The voice ids a party SpecialStarter (`0x1A`) commit dispatches through
/// `FUN_8004FCC8`, in order, with the banner stage it leaves
/// (`0x8004B74C..0x8004BB44`).
///
/// * `seat_flag` (`ctx[+0x28D + seat]`, the Miracle seat): stage `3` and the
///   generic voice.
/// * `mark` (`0x801F6990[ctx[+0x15] - 1]`, non-zero - a newly learned art or
///   a Super's starter): stage = its low byte and the generic voice again.
/// * otherwise, only while the stage is still `0`: stage `2` and the **Hyper
///   fanfare**, picked by the constant after the starter (`queue[cursor]`),
///   with one `rand()` draw - `base + (rand() % 2) * 3` - taken only when
///   that constant names a Hyper Art. Any other constant plays nothing.
///
/// `stage` is `ctx[+0x28B]` as the commit found it (after the prologue's
/// cancel). Both generic arms dispatch when both conditions hold; the
/// dispatcher's busy gate is what drops the second.
///
/// REF: FUN_8004AD80
pub fn starter_commit_voice_ids(
    char_id: u8,
    seat_flag: bool,
    mark: Option<u32>,
    stage: u8,
    next: u8,
    rand: &mut dyn FnMut() -> u32,
) -> (Vec<u16>, u8) {
    let mut ids = Vec::new();
    let mut stage = stage;
    if seat_flag {
        stage = 3;
        ids.extend(generic_starter_voice_id(char_id));
    }
    if let Some(w) = mark.filter(|&w| w != 0) {
        stage = w as u8;
        ids.extend(generic_starter_voice_id(char_id));
    } else if stage == 0 {
        stage = 2;
        if let Some(base) = hyper_fanfare_base(char_id, next) {
            let r = rand() as i32;
            ids.push(base + (r % 2) as u16 * 3);
        }
    }
    (ids, stage)
}

#[cfg(test)]
mod tests {
    use super::*;
    use legaia_art::arts_voice::{RANGE_TABLE_VA, SELECTOR_SPAN_END, SELECTOR_SPAN_START};

    /// A snapshot with the range table and one record per table written.
    fn tables() -> ArtsVoiceSelectorTables {
        let mut b = vec![0u8; (SELECTOR_SPAN_END - SELECTOR_SPAN_START) as usize];
        let mut put = |va: u32, bytes: &[u8]| {
            let o = (va - SELECTOR_SPAN_START) as usize;
            b[o..o + bytes.len()].copy_from_slice(bytes);
        };
        // Vahn: lo 0x1F, hi 0x29, second_lo 0x2B.
        put(RANGE_TABLE_VA, &[0x1F, 0x29, 0x2B]);
        // Somersault 0x27 in each variant: record (hi - ac) = 2.
        put(FIRST_HALF_VARIANT_BASES[0][0] + 2 * 0x0F, &[0, 6, 14]);
        put(FIRST_HALF_VARIANT_BASES[1][0] + 2 * 0x0F, &[4, 7]);
        put(FIRST_HALF_VARIANT_BASES[2][0] + 2 * 0x0F, &[9, 13]);
        // A one-member record at 0x28.
        put(FIRST_HALF_VARIANT_BASES[0][0] + 0x0F, &[5]);
        // Second half 0x2B.
        put(SECOND_HALF_BASE[0], &[2, 3]);
        // Durations: channel c -> 100 + c.
        let d: Vec<u8> = (0..16).map(|c| 100 + c).collect();
        put(DUR_TABLE_VA, &d);
        ArtsVoiceSelectorTables::from_span_bytes(b)
    }

    fn call(action: u8, flag: u8, solo: u8) -> ArtsVoiceCall {
        ArtsVoiceCall {
            char_id: 1,
            action,
            flag,
            solo_latch: solo,
            first_enemy: None,
            caller_seat: 0,
        }
    }

    #[test]
    fn plain_art_picks_from_the_plain_pool_on_xa2() {
        let t = tables();
        let mut last = 0xFF;
        let mut draws = [1u32].into_iter();
        let out = select_arts_voice(&t, &call(0x27, 0, 0), &mut last, &mut || {
            draws.next().unwrap()
        });
        assert_eq!(
            out,
            vec![ArtsVoiceEmit::Clip {
                slot: 1,
                channel: 6,
                dur: (106u32 * 60).div_ceil(100)
            }]
        );
        assert_eq!(last, 6);
    }

    #[test]
    fn flag_and_latch_select_the_other_variants() {
        let t = tables();
        let mut last = 0xFF;
        let mut r = || 0u32;
        let out = select_arts_voice(&t, &call(0x27, 0x0F, 0), &mut last, &mut r);
        assert!(matches!(out[0], ArtsVoiceEmit::Clip { channel: 4, .. }));
        let out = select_arts_voice(&t, &call(0x27, 0x0F, 1), &mut last, &mut r);
        assert!(matches!(out[0], ArtsVoiceEmit::Clip { channel: 9, .. }));
    }

    #[test]
    fn the_last_pick_is_rerolled() {
        let t = tables();
        let mut last = 0;
        let mut draws = [0u32, 3, 2].into_iter();
        let out = select_arts_voice(&t, &call(0x27, 0, 0), &mut last, &mut || {
            draws.next().unwrap()
        });
        // 0 -> channel 0 (the last pick, re-rolled), 3 % 3 = 0 again, 2 -> 14.
        assert!(matches!(out[0], ArtsVoiceEmit::Clip { channel: 14, .. }));
    }

    #[test]
    fn tetsu_forces_channel_twelve_after_the_store() {
        let t = tables();
        let mut last = 0xFF;
        let mut c = call(0x27, 0, 0);
        c.first_enemy = Some(FORCED_SHOUT_FORMATION_ID);
        let out = select_arts_voice(&t, &c, &mut last, &mut || 1);
        assert_eq!(
            out,
            vec![ArtsVoiceEmit::Clip {
                slot: 1,
                channel: 12,
                dur: (112u32 * 60).div_ceil(100)
            }]
        );
        assert_eq!(last, 6, "the stored pick is the drawn member");
    }

    #[test]
    fn a_one_member_record_plays_from_the_callers_seat() {
        let t = tables();
        let mut last = 0xFF;
        let mut c = call(0x28, 0, 0);
        c.caller_seat = 2;
        let out = select_arts_voice(&t, &c, &mut last, &mut || 7);
        assert!(matches!(
            out[0],
            ArtsVoiceEmit::Clip {
                slot: 2,
                channel: 5,
                ..
            }
        ));
    }

    #[test]
    fn second_half_plays_with_flag_zero_and_cues_a_sound_otherwise() {
        let t = tables();
        let mut last = 0xFF;
        let out = select_arts_voice(&t, &call(0x2B, 0, 0), &mut last, &mut || 1);
        assert!(matches!(
            out[0],
            ArtsVoiceEmit::Clip {
                slot: 1,
                channel: 3,
                ..
            }
        ));
        let out = select_arts_voice(&t, &call(0x2B, 0x2B, 1), &mut last, &mut || 1);
        assert_eq!(out, vec![ArtsVoiceEmit::Dispatch(0x51)]);
    }

    #[test]
    fn below_the_range_and_out_of_roster_play_nothing() {
        let t = tables();
        let mut last = 0xFF;
        assert!(select_arts_voice(&t, &call(0x1A, 0, 1), &mut last, &mut || 0).is_empty());
        let mut c = call(0x27, 0, 0);
        c.char_id = 4;
        assert!(select_arts_voice(&t, &c, &mut last, &mut || 0).is_empty());
    }

    #[test]
    fn the_call_site_keys_on_the_starter_before_the_cursor() {
        // `0F 0E 19 27 00`: the 0x19 clip commits with the cursor at 3.
        let q = [0x0F, 0x0E, 0x19, 0x27, 0x00];
        assert_eq!(shout_call_args(&q, 3, 3, 0), Some((0x27, 0x00)));
        // The 0x27 clip itself (cursor 4) is not a starter row.
        assert_eq!(shout_call_args(&q, 4, 3, 0), None);
        // Under the solo latch every commit calls, keyed on the staged byte.
        assert_eq!(shout_call_args(&q, 4, 3, 1), Some((0x27, 0x00)));
        // A chained art carries its connector as the flag.
        let q = [0x19, 0x27, 0x0F, 0x19, 0x1F, 0x0E];
        assert_eq!(shout_call_args(&q, 1, 3, 0), Some((0x27, 0x0F)));
        // A newly learned art is staged behind 0x1A: no shout.
        let q = [0x1A, 0x27, 0x00];
        assert_eq!(shout_call_args(&q, 1, 3, 0), None);
    }

    #[test]
    fn starter_voice_order_matches_the_three_arms() {
        let mut r = || 1u32;
        // Hyper: no seat flag, no mark, idle stage.
        assert_eq!(
            starter_commit_voice_ids(1, false, None, 0, 0x1C, &mut r),
            (vec![0x107], 2)
        );
        assert_eq!(
            starter_commit_voice_ids(3, false, None, 0, 0x1E, &mut || 0),
            (vec![0x122], 2)
        );
        // A newly learned / Super starter: the mark and the generic voice.
        assert_eq!(
            starter_commit_voice_ids(2, false, Some(4), 0, 0x2E, &mut r),
            (vec![0x111], 4)
        );
        // Miracle seat, no mark: generic once, and the stage blocks the Hyper arm.
        assert_eq!(
            starter_commit_voice_ids(1, true, None, 0, 0x1C, &mut r),
            (vec![0x101], 3)
        );
        // Both: two dispatches.
        assert_eq!(
            starter_commit_voice_ids(1, true, Some(1), 0, 0, &mut r),
            (vec![0x101, 0x101], 1)
        );
        // A live banner: no Hyper fanfare and no draw.
        let mut drew = false;
        assert_eq!(
            starter_commit_voice_ids(1, false, None, 5, 0x1C, &mut || {
                drew = true;
                0
            }),
            (vec![], 5)
        );
        assert!(!drew);
        // A non-Hyper constant draws nothing.
        let mut drew = false;
        let _ = starter_commit_voice_ids(1, false, None, 0, 0x27, &mut || {
            drew = true;
            0
        });
        assert!(!drew);
    }

    #[test]
    fn hyper_bases_match_the_fanfare_table() {
        for f in legaia_art::hyper_fanfare::HYPER_FANFARES {
            assert_eq!(
                hyper_fanfare_base(f.cslot + 1, f.action_constant),
                Some(f.base_id),
                "{f:?}"
            );
        }
    }
}
