//! Hyper / Super / Miracle Art **fanfare** cue selector (`FUN_8004AD80`).
//!
//! An art whose action constant sits below the shout-pool range table's `lo`
//! bound (the Hyper constants) fires **no** `XA2`/`XA4`/`XA6` pool shout
//! ([`crate::arts_voice`]). Its cue comes from a different block of the
//! staged-animation materialiser `FUN_8004AD80`: when the staged anim id
//! (`actor+0x1DA`) is the Hyper class byte `0x1A`, the materialiser fires a
//! CD-XA **fanfare** through the jingle queue (`FUN_8004FCC8`, one-shot clip
//! player `FUN_8003D53C` behind it) instead.
//!
//! ## The selector (disassembly, `ghidra/scripts/funcs/8004ad80.txt`)
//!
//! Two paths, gated on the queue-builder's Super/Miracle marks:
//!
//! * **Per-art pair** (a plain Hyper): the materialiser reads the queued art's
//!   action constant (`actor[0x1DF + cursor]`, cursor = `ctx+0x15`) and fires
//!   `jingle_id = rand() % 2 * 3 + base` - a **coin flip between two fixed
//!   channels** of the character's fanfare bank. The per-(character, constant)
//!   `base` ids are compile-time immediates in three per-character switch
//!   blocks (`0x8004B8D4` Vahn / `0x8004B9A0` Noa / `0x8004BA6C` Gala; the
//!   fire is `jal FUN_8004FCC8` at `0x8004BB34`). See [`HYPER_FANFARES`].
//! * **Generic character fanfare** (a Super or Miracle expansion): when the
//!   per-seat Super mark `ctx[0x28D + seat]` or the queue-builder scratch word
//!   `0x801F6990[cursor-1]` (written by `FUN_801EED1C`) is set, the id is the
//!   fixed per-character `0x101` / `0x111` / `0x121` instead - **channel 1**
//!   of the same bank (sites `0x8004B7D0` and `0x8004B840..0x8004B868`). A
//!   latch at `ctx+0x28B` keeps one `0x1A` stage from firing twice.
//!
//! The jingle decode (`FUN_8004FCC8` / `FUN_8004FE5C`, identical `>= 0x100`
//! halves): `n = id - 0x100`, clip slot `n >> 3` (slots 1/3/5 - the shout
//! banks - remap to `0x1A`/`0x1B`/`0x1C` = `XA27`/`XA28`/`XA29`; the even
//! slots pass through), channel `n & 7`, and
//! `dur = (u16[0x800788B8 + n*2] * 0x3C + 99) / 100`.
//! So the Hyper fanfare banks are the **even** clip slots:
//! Vahn = `XA1.XA` (slot 0), Noa = `XA3.XA` (slot 2), Gala = `XA5.XA`
//! (slot 4), the 8-channel stereo siblings of the mono shout banks.
//!
//! A Miracle additionally fires its **finisher** fanfare from the animation
//! cue track (`entry+0x54`, walker `FUN_800508DC`): party cue ids
//! `0xC8..=0xFF` re-base by `+0x38` into the same jingle namespace (witnessed:
//! Gala's Miracle finisher fired id `0x12D` = `XA29.XA` channel 5).
//!
//! ## Capture verification
//!
//! Every row of [`CAPTURED_FANFARES`] is a live recomp-runtime battle fire
//! captured off the `FUN_8003D53C` cue globals
//! (`scripts/recomp/xa_cue_capture.py`): all nine per-art cues, both members
//! of a pair for two arts (Tornado Flame 2/5, Explosive Fist 4/7), a repeat
//! fire landing the same member twice (Frost Breath 2, 2 - the coin flip is
//! plain `rand() % 2`, with **no** avoid-repeat memory, unlike the shout
//! pool), and the generic channel-1 fire on all three characters (Vahn/Noa
//! Super, Gala Miracle). Every captured `dur` reproduced the `0x800788B8`
//! arithmetic ([`FanfareDurTable`]).

/// Per-character fanfare bank, indexed by 0-based character slot
/// (Vahn/Noa/Gala): CD-XA clip-table slots `0` / `2` / `4`.
pub const FANFARE_XA_FILE: [&str; 3] = ["XA1.XA", "XA3.XA", "XA5.XA"];

/// The generic (Super / Miracle) fanfare channel - every character's jingle
/// id `0x1?1` decodes to channel 1 of its bank.
pub const GENERIC_FANFARE_CHANNEL: u8 = 1;

/// SCUS VA of the jingle duration table (`u16` per jingle id `- 0x100`);
/// `dur = (entry * 0x3C + 99) / 100` (see `FUN_8004FCC8`).
pub const FANFARE_DUR_TABLE_VA: u32 = 0x8007_88B8;

/// One Hyper art's fanfare selector row: the `FUN_8004AD80` per-art switch
/// maps `(character, action constant)` to a **pair** of channels
/// `{base_channel, base_channel + 3}` in the character's fanfare bank
/// ([`FANFARE_XA_FILE`]), coin-flipped per fire.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HyperFanfare {
    /// 0-based character slot (0 Vahn / 1 Noa / 2 Gala).
    pub cslot: u8,
    /// The Hyper art's action constant.
    pub action_constant: u8,
    /// Jingle id of the pair's first member (`rand() % 2 == 0`); the other
    /// member is `base_id + 3`. `(base_id - 0x100) & 7` is the base channel.
    pub base_id: u16,
}

impl HyperFanfare {
    /// The two channels this art coin-flips between.
    pub fn channel_pair(&self) -> (u8, u8) {
        let base = ((self.base_id - 0x100) & 7) as u8;
        (base, base + 3)
    }
}

/// The nine Hyper-art fanfare rows - the complete per-art switch of
/// `FUN_8004AD80` (there are no other cases; any other constant reaching the
/// per-art path plays nothing). Base ids are the disassembly immediates.
pub const HYPER_FANFARES: &[HyperFanfare] = &[
    // Vahn (switch block 0x8004B8D4) - bank XA1.XA.
    HyperFanfare {
        cslot: 0,
        action_constant: 0x1C, // Burning Flare
        base_id: 0x104,
    },
    HyperFanfare {
        cslot: 0,
        action_constant: 0x1D, // Fire Blow
        base_id: 0x103,
    },
    HyperFanfare {
        cslot: 0,
        action_constant: 0x1E, // Tornado Flame (Hyper)
        base_id: 0x102,
    },
    // Noa (switch block 0x8004B9A0) - bank XA3.XA.
    HyperFanfare {
        cslot: 1,
        action_constant: 0x1D, // Hurricane Kick (stages `1A 1D 1E`)
        base_id: 0x114,
    },
    HyperFanfare {
        cslot: 1,
        action_constant: 0x1F, // Vulture Blade
        base_id: 0x113,
    },
    HyperFanfare {
        cslot: 1,
        action_constant: 0x20, // Frost Breath
        base_id: 0x112,
    },
    // Gala (switch block 0x8004BA6C) - bank XA5.XA.
    HyperFanfare {
        cslot: 2,
        action_constant: 0x1C, // Explosive Fist
        base_id: 0x124,
    },
    HyperFanfare {
        cslot: 2,
        action_constant: 0x1D, // Lightning Storm
        base_id: 0x123,
    },
    HyperFanfare {
        cslot: 2,
        action_constant: 0x1E, // Thunder Punch (Hyper)
        base_id: 0x122,
    },
];

/// One capture-witnessed fanfare fire: running the Hyper art with this action
/// constant in a live recomp-runtime battle staged this XA channel through the
/// `FUN_8003D53C` cue globals. A witnessed channel is one member of the art's
/// coin-flip pair - both fields of evidence: it proves the row's decoded pair,
/// and gives deterministic consumers a member retail verifiably plays.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CapturedFanfare {
    /// 0-based character slot.
    pub cslot: u8,
    /// The Hyper art's action constant.
    pub action_constant: u8,
    /// The witnessed channel (a member of the row's [`HyperFanfare::channel_pair`]).
    pub channel: u8,
}

/// Capture-witnessed per-art fanfare fires (`scripts/recomp/xa_cue_capture.py`
/// over slot-resumed `jou ene` battles; cue = staged `CdlLOC` resolved against
/// the `0x801C6ED8` clip table + channel word `0x8007BC6C`). Arts fired twice
/// keep both witnesses.
pub const CAPTURED_FANFARES: &[CapturedFanfare] = &[
    // Vahn - all three Hypers; Tornado Flame twice (both pair members).
    CapturedFanfare {
        cslot: 0,
        action_constant: 0x1C,
        channel: 4,
    },
    CapturedFanfare {
        cslot: 0,
        action_constant: 0x1D,
        channel: 6,
    },
    CapturedFanfare {
        cslot: 0,
        action_constant: 0x1E,
        channel: 2,
    },
    CapturedFanfare {
        cslot: 0,
        action_constant: 0x1E,
        channel: 5,
    },
    // Noa - all three Hypers; Frost Breath twice (same member both fires -
    // the coin flip has no avoid-repeat).
    CapturedFanfare {
        cslot: 1,
        action_constant: 0x1D,
        channel: 4,
    },
    CapturedFanfare {
        cslot: 1,
        action_constant: 0x1F,
        channel: 3,
    },
    CapturedFanfare {
        cslot: 1,
        action_constant: 0x20,
        channel: 2,
    },
    // Gala - all three Hypers; Explosive Fist twice (both pair members).
    CapturedFanfare {
        cslot: 2,
        action_constant: 0x1C,
        channel: 4,
    },
    CapturedFanfare {
        cslot: 2,
        action_constant: 0x1C,
        channel: 7,
    },
    CapturedFanfare {
        cslot: 2,
        action_constant: 0x1D,
        channel: 3,
    },
    CapturedFanfare {
        cslot: 2,
        action_constant: 0x1E,
        channel: 5,
    },
];

/// The fanfare bank file for a 0-based character slot. `None` for Terra
/// (slot 3) or out of range - the selector has no case for them.
pub fn fanfare_file(cslot: usize) -> Option<&'static str> {
    FANFARE_XA_FILE.get(cslot).copied()
}

/// The Hyper fanfare selector row for `(character, action constant)`, when
/// the `FUN_8004AD80` per-art switch has a case for it. `None` otherwise -
/// including every regular-art constant (those play the pool shout instead)
/// and the Super/Miracle constants (those take the generic path).
pub fn hyper_fanfare(cslot: usize, action_constant: u8) -> Option<&'static HyperFanfare> {
    HYPER_FANFARES
        .iter()
        .find(|f| f.cslot as usize == cslot && f.action_constant == action_constant)
}

/// The capture-witnessed channel for `(character, action constant)` - the
/// first witnessed fire, when the art has one.
pub fn captured_fanfare_channel(cslot: usize, action_constant: u8) -> Option<u8> {
    CAPTURED_FANFARES
        .iter()
        .find(|c| c.cslot as usize == cslot && c.action_constant == action_constant)
        .map(|c| c.channel)
}

/// A deterministic channel for the art's fanfare: the capture-witnessed
/// member where one is pinned (and it belongs to the decoded pair - the
/// membership check keeps a capture row honest against the selector), else
/// the pair's base member. Retail coin-flips between the two per fire.
/// `None` when the selector has no case for the art.
pub fn pick_fanfare_channel(cslot: usize, action_constant: u8) -> Option<u8> {
    let row = hyper_fanfare(cslot, action_constant)?;
    let (a, b) = row.channel_pair();
    match captured_fanfare_channel(cslot, action_constant) {
        Some(ch) if ch == a || ch == b => Some(ch),
        _ => Some(a),
    }
}

/// The generic Super / Miracle fanfare jingle id for a character slot
/// (`0x101` / `0x111` / `0x121`) - channel [`GENERIC_FANFARE_CHANNEL`] of the
/// same bank. `None` for Terra / out of range.
pub fn generic_fanfare_id(cslot: usize) -> Option<u16> {
    (cslot < 3).then(|| 0x101 + (cslot as u16) * 0x10)
}

/// Decode a jingle-queue id (`>= 0x100`) the way `FUN_8004FCC8` /
/// `FUN_8004FE5C` do: `(clip_slot, channel)`, with the odd (shout-bank) clip
/// slots 1/3/5 remapped to `0x1A`/`0x1B`/`0x1C` (`XA27`/`XA28`/`XA29`).
pub fn jingle_decode(id: u16) -> Option<(u8, u8)> {
    let n = id.checked_sub(0x100)?;
    let clip = match n >> 3 {
        1 => 0x1A,
        3 => 0x1B,
        5 => 0x1C,
        c => c as u8,
    };
    Some((clip, (n & 7) as u8))
}

/// PSX-EXE `t_addr` data-segment VA -> file offset (data loads at file `0x800`).
fn scus_off(scus: &[u8], va: u32) -> Option<usize> {
    if scus.len() < 0x800 || &scus[0..8] != b"PS-X EXE" {
        return None;
    }
    let t_addr = u32::from_le_bytes(scus[0x18..0x1C].try_into().ok()?);
    let t_size = u32::from_le_bytes(scus[0x1C..0x20].try_into().ok()?);
    if va < t_addr || va >= t_addr.checked_add(t_size)? {
        return None;
    }
    Some((va - t_addr) as usize + 0x800)
}

/// The jingle duration base table at [`FANFARE_DUR_TABLE_VA`]: one `u16` per
/// jingle id above `0x100`. The `FUN_8003D53C` `dur` (CD read-span) argument
/// is `(entry * 0x3C + 99) / 100`.
#[derive(Debug, Clone)]
pub struct FanfareDurTable {
    /// Raw table entries, indexed by `id - 0x100` (`0x40` ids = every id the
    /// jingle decode can map onto clip slots `0..=7`).
    entries: [u16; 0x40],
}

/// File offset of the jingle duration table inside `SCUS_942.54` (for
/// patching an entry in place - the `u16` for jingle id `0x100 + n`
/// sits at `offset + n*2`).
pub fn dur_table_file_offset(scus: &[u8]) -> Option<usize> {
    scus_off(scus, FANFARE_DUR_TABLE_VA)
}

impl FanfareDurTable {
    /// Parse the table out of `SCUS_942.54`.
    pub fn parse_from_scus(scus: &[u8]) -> Option<Self> {
        let off = scus_off(scus, FANFARE_DUR_TABLE_VA)?;
        let raw = scus.get(off..off + 0x40 * 2)?;
        let mut entries = [0u16; 0x40];
        for (i, e) in entries.iter_mut().enumerate() {
            *e = u16::from_le_bytes([raw[i * 2], raw[i * 2 + 1]]);
        }
        Some(Self { entries })
    }

    /// The `FUN_8003D53C` `dur` argument for a jingle id (`0x100..=0x13F`).
    pub fn dur(&self, id: u16) -> Option<u32> {
        let n = id.checked_sub(0x100)? as usize;
        let base = *self.entries.get(n)? as u32;
        // Retail: (base * 0x3C + 99) / 100 - i.e. ceil-divide by 100.
        Some((base * 0x3C).div_ceil(100))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nine_rows_cover_three_hypers_per_character() {
        for cslot in 0..3usize {
            let n = HYPER_FANFARES
                .iter()
                .filter(|f| f.cslot as usize == cslot)
                .count();
            assert_eq!(n, 3, "character {cslot} has three Hyper arts");
        }
        assert_eq!(HYPER_FANFARES.len(), 9);
    }

    #[test]
    fn pairs_partition_channels_2_to_7() {
        // Per character, the three pairs are exactly {2,5},{3,6},{4,7} -
        // six per-art channels + channel 1 (generic) per 8-channel bank.
        for cslot in 0..3usize {
            let mut chans: Vec<u8> = HYPER_FANFARES
                .iter()
                .filter(|f| f.cslot as usize == cslot)
                .flat_map(|f| {
                    let (a, b) = f.channel_pair();
                    [a, b]
                })
                .collect();
            chans.sort_unstable();
            assert_eq!(chans, vec![2, 3, 4, 5, 6, 7]);
        }
    }

    #[test]
    fn base_ids_decode_into_the_even_fanfare_clip_slots() {
        for f in HYPER_FANFARES {
            let (clip, chan) = jingle_decode(f.base_id).unwrap();
            // Even slots 0/2/4 = XA1/XA3/XA5 - never the remapped shout slots.
            assert_eq!(clip, f.cslot * 2, "clip slot for {f:?}");
            assert_eq!(chan, f.channel_pair().0);
        }
        for cslot in 0..3usize {
            let (clip, chan) = jingle_decode(generic_fanfare_id(cslot).unwrap()).unwrap();
            assert_eq!(clip, (cslot as u8) * 2);
            assert_eq!(chan, GENERIC_FANFARE_CHANNEL);
        }
        assert_eq!(generic_fanfare_id(3), None, "Terra has no fanfare bank");
    }

    #[test]
    fn jingle_decode_remaps_the_shout_slots() {
        // The odd slots remap to XA27/28/29 - the cue-track namespace the
        // Miracle finisher uses (witnessed: Gala id 0x12D = XA29 chan 5).
        assert_eq!(jingle_decode(0x10F), Some((0x1A, 7)));
        assert_eq!(jingle_decode(0x12D), Some((0x1C, 5)));
        assert_eq!(jingle_decode(0x0FF), None, "below the jingle namespace");
    }

    #[test]
    fn every_captured_fanfare_is_a_pair_member() {
        for c in CAPTURED_FANFARES {
            let row = hyper_fanfare(c.cslot as usize, c.action_constant)
                .unwrap_or_else(|| panic!("no selector row for {c:?}"));
            let (a, b) = row.channel_pair();
            assert!(
                c.channel == a || c.channel == b,
                "{c:?} not in pair ({a},{b})"
            );
        }
        // Both members witnessed for Tornado Flame and Explosive Fist.
        let both = |cslot: u8, ac: u8| {
            let mut chans: Vec<u8> = CAPTURED_FANFARES
                .iter()
                .filter(|c| c.cslot == cslot && c.action_constant == ac)
                .map(|c| c.channel)
                .collect();
            chans.sort_unstable();
            chans
        };
        assert_eq!(both(0, 0x1E), vec![2, 5]);
        assert_eq!(both(2, 0x1C), vec![4, 7]);
    }

    #[test]
    fn pick_prefers_the_witnessed_member() {
        // Witnessed rows win.
        assert_eq!(pick_fanfare_channel(0, 0x1C), Some(4));
        assert_eq!(pick_fanfare_channel(1, 0x1D), Some(4));
        assert_eq!(pick_fanfare_channel(2, 0x1E), Some(5));
        // Honesty guard: constants outside the selector return None - a
        // regular art (pool shout), a Super constant, or Terra.
        assert_eq!(pick_fanfare_channel(0, 0x22), None, "Spin Combo is pooled");
        assert_eq!(pick_fanfare_channel(0, 0x2B), None, "Super takes generic");
        assert_eq!(pick_fanfare_channel(3, 0x1C), None, "Terra");
    }

    #[test]
    fn fanfare_files_are_the_even_xa_banks() {
        assert_eq!(fanfare_file(0), Some("XA1.XA"));
        assert_eq!(fanfare_file(1), Some("XA3.XA"));
        assert_eq!(fanfare_file(2), Some("XA5.XA"));
        assert_eq!(fanfare_file(3), None);
    }

    #[test]
    fn dur_table_arithmetic() {
        // Synthetic SCUS: id 0x102's entry = 594 -> (594*0x3C+99)/100 = 0x165,
        // the capture-witnessed dur for Vahn's Tornado Flame fanfare.
        let t_addr: u32 = 0x8007_8000;
        let mut scus = vec![0u8; 0x800 + 0x2000];
        scus[0..8].copy_from_slice(b"PS-X EXE");
        scus[0x18..0x1C].copy_from_slice(&t_addr.to_le_bytes());
        scus[0x1C..0x20].copy_from_slice(&0x2000u32.to_le_bytes());
        let off = (FANFARE_DUR_TABLE_VA - t_addr) as usize + 0x800 + 2 * 2;
        scus[off..off + 2].copy_from_slice(&594u16.to_le_bytes());
        let table = FanfareDurTable::parse_from_scus(&scus).unwrap();
        assert_eq!(table.dur(0x102), Some(0x165));
        assert_eq!(table.dur(0x0FF), None);
    }
}
