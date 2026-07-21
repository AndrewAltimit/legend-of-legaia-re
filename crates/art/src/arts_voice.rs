//! Arts-voice cue table parser (`FUN_8004C140` in `SCUS_942.54`).
//!
//! When the staged-animation materialiser `FUN_8004AD80` runs a party art
//! action, it calls the arts-voice cue selector `FUN_8004C140(char_id,
//! action_constant, flag)`, which fires the SCUS CD-XA clip player
//! `FUN_8003D53C(clip_slot, channel, dur)` - the per-character arts **shout**.
//! This is a distinct cue from the ordinary directional-attack grunt (that one
//! is `XA30.XA`, fired from the battle-action overlay; see
//! `docs/subsystems/battle-action.md`).
//!
//! ## Clip file - one per character
//!
//! `clip_slot = (char_id - 1) * 2 + 1`, and clip slot `i` = file `XA<i+1>.XA`
//! (the `0x801C6ED8` clip table), so the arts-voice file is:
//!
//! | character (0-based) | clip slot | file |
//! |---|---|---|
//! | Vahn (0) | 1 | `XA2.XA` |
//! | Noa  (1) | 3 | `XA4.XA` |
//! | Gala (2) | 5 | `XA6.XA` |
//!
//! Each of these is a 16-channel short-mono shout bank. (`XA3` / `XA5` are the
//! 8-channel *stereo* Miracle/summon fanfares fired from the separate
//! `FUN_8004FCC8` jingle path - NOT the per-art shout, an easy mis-ID by ear.)
//! This mapping is capture-verified: a live PCSX-Redux trace of Vahn's
//! Tri-Somersault fired `FUN_8003D53C(0x01=XA2, chan 0/6, ...)` and Noa's
//! Miracle fired `FUN_8003D53C(0x03=XA4, ...)`, both from `FUN_8004C140`
//! (`ra = 0x8004C464`).
//!
//! ## Per-art channel pool
//!
//! The `channel` argument is chosen at random (avoiding an immediate repeat)
//! from a **candidate-channel pool** keyed by the art's action constant. The
//! pools live in two SCUS tables per character:
//!
//! * a range table at [`RANGE_TABLE_VA`] (`[lo, hi, second_lo]` per character,
//!   stride 3);
//! * a **first-half** table ([`FIRST_HALF_BASE`]) for `lo <= ac <= hi`: record
//!   `base + (hi - ac) * 0x0F`;
//! * a **second-half** table ([`SECOND_HALF_BASE`]) for `ac >= second_lo`:
//!   record `base + (ac - second_lo) * 0x10`.
//!
//! Each record is a channel list: the first byte is always a member (channel 0
//! is legal), and if the second byte is non-zero the list continues to the
//! next `0` terminator (`FUN_8004C140` counting quirk). The retail selector has
//! non-combat and combat-mode / special-flag table variants; this parser reads
//! the non-combat variant (`mode == 0, flag == 0`), the one relevant to a
//! showcase.
//!
//! ## Duration
//!
//! `dur = (dur_table[channel + char*0x10] * 0x3C + 99) / 100` from the table at
//! [`DUR_TABLE_VA`] (a physical CD read span, not a channel-sector count).
//! Verified: Vahn `dur_table[0] = 0x4B` -> `0x2D`, `dur_table[6] = 0x65` ->
//! `0x3D`, matching the live trace.

use std::collections::BTreeMap;

/// Range table: `[lo, hi, second_lo]` per character (Vahn/Noa/Gala), stride 3.
pub const RANGE_TABLE_VA: u32 = 0x8007_81A4;
/// Per-character first-half candidate-table base (`lo..=hi`), non-combat path.
pub const FIRST_HALF_BASE: [u32; 3] = [0x8007_7B64, 0x8007_7D5C, 0x8007_7F54];
/// Per-character second-half candidate-table base (`>= second_lo`).
pub const SECOND_HALF_BASE: [u32; 3] = [0x8007_80A4, 0x8007_8104, 0x8007_8154];
/// Per-channel duration base table (stride `0x10` per character).
pub const DUR_TABLE_VA: u32 = 0x8007_7A8C;

/// One statically-resolved CD-XA voice-clip cue: a `(clip_id, channel)` pair
/// fired at a fixed call site of the clip player `FUN_8003D53C(clip_id, chan,
/// dur)`. `clip_id` is a slot in the clip descriptor table at `0x801C6ED8`;
/// `channel` is the CD-XA channel inside that clip's interleave; `dur` is the
/// literal read-span operand where the site supplies one (`None` otherwise).
///
/// These are the cues whose `(clip_id, channel)` are compile-time immediates -
/// distinct from the arts-shout path ([`ArtsVoiceTable`], whose channel is a
/// runtime pool pick), the Miracle/summon jingle queue (`FUN_8004FCC8` /
/// `FUN_8004FE5C`, channel from a sound-event id), and the field-VM XA opcode
/// (operands are per-scene MAN script literals, disc-sourced). See
/// `docs/subsystems/audio.md` and `ghidra/scripts/funcs/8003d53c.txt`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StaticXaCue {
    /// Descriptor-table slot (`0x801C6ED8 + clip_id*8`).
    pub clip_id: u8,
    /// CD-XA channel within the clip.
    pub channel: u8,
    /// Literal `dur` operand at the call site, when the site supplies one.
    pub dur: Option<u16>,
    /// Call-site instruction address (in the dump named by [`Self::dump`]).
    pub site: u32,
    /// Short cue context.
    pub context: &'static str,
    /// Committed Ghidra dump file the call site lives in.
    pub dump: &'static str,
}

/// The static `(clip_id, channel)` voice-clip cues recovered by a byte-level
/// `jal FUN_8003D53C` census over `SCUS_942.54` + the static-overlay corpus
/// (deduplicated against PROT-entry over-read; see
/// `docs/subsystems/audio.md` "One-shot cue census") - only the sites whose
/// `(clip_id, channel)` are compile-time immediates. The Baka Fighter duel
/// (`clip_id 0x20` = `XA33.XA`, PROT 0976) supplies the bulk; the three `0x1D`
/// (`XA30.XA`) cues are the battle normal-move grunt, and `0x10` (`XA17.XA`)
/// is a scripted-scene/dialog cue. (One further duel site takes a runtime
/// channel - `clip_id 0x1F` at `0x801D5CC4`, dur `0x48` - and so is not a
/// `(clip_id, channel)` pair.)
pub const STATIC_XA_CUES: &[StaticXaCue] = &[
    StaticXaCue {
        clip_id: 0x10,
        channel: 7,
        dur: Some(0x135),
        site: 0x801d_509c,
        context: "scripted-scene / dialog fixed voice",
        dump: "PROT 0897 file +0x6884 @0x801CE818",
    },
    StaticXaCue {
        clip_id: 0x1d,
        channel: 0,
        dur: Some(0x26),
        site: 0x801e_eb44,
        context: "battle normal-move grunt",
        dump: "PROT 0898 file +0x2032C @0x801CE818",
    },
    StaticXaCue {
        clip_id: 0x1d,
        channel: 4,
        dur: Some(0x2e),
        site: 0x801e_eb44,
        context: "battle normal-move grunt",
        dump: "PROT 0898 file +0x2032C @0x801CE818",
    },
    StaticXaCue {
        clip_id: 0x1d,
        channel: 6,
        dur: Some(0x1a),
        site: 0x801e_eb44,
        context: "battle normal-move grunt",
        dump: "PROT 0898 file +0x2032C @0x801CE818",
    },
    // Baka Fighter announcer / duel lines - descriptor slot 0x20, twelve
    // fixed channels (PROT 0976 at slot-A base 0x801CE818).
    StaticXaCue {
        clip_id: 0x20,
        channel: 1,
        dur: Some(0x36),
        site: 0x801d_04ec,
        context: "Baka Fighter duel line",
        dump: "PROT 0976 file +0x1CD4 @0x801CE818",
    },
    StaticXaCue {
        clip_id: 0x20,
        channel: 2,
        dur: Some(0x45),
        site: 0x801d_3968,
        context: "Baka Fighter announcer",
        dump: "PROT 0976 file +0x5150 @0x801CE818",
    },
    StaticXaCue {
        clip_id: 0x20,
        channel: 3,
        dur: Some(0x6d),
        site: 0x801d_38e4,
        context: "Baka Fighter announcer",
        dump: "PROT 0976 file +0x50CC @0x801CE818",
    },
    StaticXaCue {
        clip_id: 0x20,
        channel: 4,
        dur: Some(0x35),
        site: 0x801d_38a0,
        context: "Baka Fighter announcer",
        dump: "PROT 0976 file +0x5088 @0x801CE818",
    },
    StaticXaCue {
        clip_id: 0x20,
        channel: 5,
        dur: Some(0x39),
        site: 0x801d_39bc,
        context: "Baka Fighter announcer",
        dump: "PROT 0976 file +0x51A4 @0x801CE818",
    },
    StaticXaCue {
        clip_id: 0x20,
        channel: 8,
        dur: Some(0x4a),
        site: 0x801d_1264,
        context: "Baka Fighter announcer",
        dump: "PROT 0976 file +0x2A4C @0x801CE818",
    },
    StaticXaCue {
        clip_id: 0x20,
        channel: 9,
        dur: Some(0x4e),
        site: 0x801d_0df4,
        context: "Baka Fighter announcer",
        dump: "PROT 0976 file +0x25DC @0x801CE818",
    },
    StaticXaCue {
        clip_id: 0x20,
        channel: 0xa,
        dur: Some(0x46),
        site: 0x801d_2220,
        context: "Baka Fighter announcer",
        dump: "PROT 0976 file +0x3A08 @0x801CE818",
    },
    StaticXaCue {
        clip_id: 0x20,
        channel: 0xb,
        dur: Some(0x4d),
        site: 0x801d_2258,
        context: "Baka Fighter announcer",
        dump: "PROT 0976 file +0x3A40 @0x801CE818",
    },
    StaticXaCue {
        clip_id: 0x20,
        channel: 0xc,
        dur: Some(0x5a),
        site: 0x801d_22fc,
        context: "Baka Fighter announcer",
        dump: "PROT 0976 file +0x3AE4 @0x801CE818",
    },
    StaticXaCue {
        clip_id: 0x20,
        channel: 0xe,
        dur: Some(0x3f),
        site: 0x801d_5a50,
        context: "Baka Fighter announcer",
        dump: "PROT 0976 file +0x7238 @0x801CE818",
    },
    StaticXaCue {
        clip_id: 0x20,
        channel: 0xf,
        dur: Some(0x76),
        site: 0x801d_5a98,
        context: "Baka Fighter announcer",
        dump: "PROT 0976 file +0x7280 @0x801CE818",
    },
];

/// Arts-voice clip file per 0-based character slot (Vahn/Noa/Gala). `None`
/// for Terra (slot 3) or out of range.
pub fn clip_file(cslot: usize) -> Option<&'static str> {
    ["XA2.XA", "XA4.XA", "XA6.XA"].get(cslot).copied()
}

/// CD-XA clip-table slot (`file XA<slot+1>.XA`) for a 0-based character slot.
pub fn clip_slot(cslot: usize) -> Option<u8> {
    (cslot < 3).then(|| (cslot as u8) * 2 + 1)
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

/// Decode one candidate record into its channel list, replicating the
/// `FUN_8004C140` membership walk: `record[0]` is always a member (channel 0 is
/// legal); if `record[1] != 0` the list runs to the next `0`. Returns `None`
/// if any member is not a valid channel (`> 15`) - the guard that bounds the
/// second-half table's tail against adjacent data.
fn record_members(rec: &[u8]) -> Option<Vec<u8>> {
    if rec.len() < 2 {
        return None;
    }
    let count = if rec[1] == 0 {
        1
    } else {
        let mut k = 2usize;
        while k < rec.len() && rec[k] != 0 {
            k += 1;
        }
        k
    };
    let members = &rec[..count];
    if members.iter().any(|&c| c > 15) {
        return None;
    }
    Some(members.to_vec())
}

/// The decoded arts-voice tables: per 0-based character (Vahn/Noa/Gala), a map
/// from art **action constant** to its candidate voice-channel pool.
#[derive(Debug, Clone, Default)]
pub struct ArtsVoiceTable {
    pools: [BTreeMap<u8, Vec<u8>>; 3],
    /// Per-character `[channel] -> raw duration byte` (16 entries each).
    dur: [[u8; 16]; 3],
}

impl ArtsVoiceTable {
    /// Parse the non-combat arts-voice cue tables out of `SCUS_942.54`.
    pub fn parse_from_scus(scus: &[u8]) -> Option<Self> {
        let range_off = scus_off(scus, RANGE_TABLE_VA)?;
        let mut out = Self::default();
        for c in 0..3usize {
            let lo = *scus.get(range_off + c * 3)?;
            let hi = *scus.get(range_off + c * 3 + 1)?;
            let second_lo = *scus.get(range_off + c * 3 + 2)?;
            // First-half: lo..=hi, record base + (hi - ac)*0x0F.
            if hi >= lo {
                let base = scus_off(scus, FIRST_HALF_BASE[c])?;
                for ac in lo..=hi {
                    let ro = base + (hi - ac) as usize * 0x0F;
                    if let Some(rec) = scus.get(ro..ro + 0x0F)
                        && let Some(ch) = record_members(rec)
                    {
                        out.pools[c].insert(ac, ch);
                    }
                }
            }
            // Second-half: ac >= second_lo, record base + (ac - second_lo)*0x10.
            // No hard upper bound in the selector; stop at the first record that
            // fails the channel-validity guard (bounds against adjacent data).
            let base2 = scus_off(scus, SECOND_HALF_BASE[c])?;
            for i in 0..16u8 {
                let ac = second_lo.saturating_add(i);
                let ro = base2 + i as usize * 0x10;
                match scus.get(ro..ro + 0x10).and_then(record_members) {
                    Some(ch) => {
                        out.pools[c].entry(ac).or_insert(ch);
                    }
                    None => break,
                }
            }
            // Duration base table (16 channels per character).
            if let Some(doff) = scus_off(scus, DUR_TABLE_VA)
                && let Some(row) = scus.get(doff + c * 0x10..doff + c * 0x10 + 16)
            {
                out.dur[c].copy_from_slice(row);
            }
        }
        Some(out)
    }

    /// Iterate every `(action_constant, candidate channel pool)` pair for a
    /// character slot - the full decoded cue table, for consumers that stage
    /// it into a runtime bank. Empty for out-of-range slots.
    pub fn pools(&self, cslot: usize) -> impl Iterator<Item = (u8, &[u8])> {
        self.pools
            .get(cslot)
            .into_iter()
            .flat_map(|m| m.iter().map(|(k, v)| (*k, v.as_slice())))
    }

    /// The candidate voice-channel pool for `(character, action_constant)`.
    /// `None` when the character slot is out of range or the art has no
    /// arts-voice entry (an art the retail build plays silent).
    pub fn channels(&self, cslot: usize, action_constant: u8) -> Option<&[u8]> {
        self.pools
            .get(cslot)?
            .get(&action_constant)
            .map(Vec::as_slice)
    }

    /// A deterministic member of the art's candidate pool - the site's stable
    /// per-art pick (retail chooses a random member each fire). Keyed on the
    /// action constant so distinct arts get distinct channels within the pool.
    pub fn pick_channel(&self, cslot: usize, action_constant: u8) -> Option<u8> {
        let pool = self.channels(cslot, action_constant)?;
        (!pool.is_empty()).then(|| pool[action_constant as usize % pool.len()])
    }

    /// The `FUN_8003D53C` `dur` argument for `(character, channel)`:
    /// `(dur_table * 0x3C + 99) / 100`.
    pub fn duration(&self, cslot: usize, channel: u8) -> Option<u32> {
        let row = self.dur.get(cslot)?;
        let base = *row.get(channel as usize)? as u32;
        // Retail: (base * 0x3C + 99) / 100 - i.e. ceil-divide by 100.
        Some((base * 0x3C).div_ceil(100))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clip_files_map_char_to_xa2_4_6() {
        assert_eq!(clip_file(0), Some("XA2.XA"));
        assert_eq!(clip_file(1), Some("XA4.XA"));
        assert_eq!(clip_file(2), Some("XA6.XA"));
        assert_eq!(clip_file(3), None);
        assert_eq!(clip_slot(0), Some(1));
        assert_eq!(clip_slot(1), Some(3));
        assert_eq!(clip_slot(2), Some(5));
        assert_eq!(clip_slot(3), None);
    }

    #[test]
    fn record_membership_matches_fun_8004c140_walk() {
        // Single channel: record[1] == 0 -> just record[0] (channel 0 legal).
        assert_eq!(record_members(&[0, 0, 0]), Some(vec![0]));
        assert_eq!(record_members(&[6, 0, 0]), Some(vec![6]));
        // Multi-channel: runs from record[0] to the first 0 at index >= 2.
        assert_eq!(
            record_members(&[0, 2, 3, 5, 6, 0, 0]),
            Some(vec![0, 2, 3, 5, 6])
        );
        assert_eq!(
            record_members(&[2, 3, 5, 6, 9, 13, 14, 15, 0]),
            Some(vec![2, 3, 5, 6, 9, 13, 14, 15])
        );
        // Any member > 15 is not a valid channel -> reject (bounds the tail).
        assert_eq!(record_members(&[31, 41, 43, 0]), None);
    }

    #[test]
    fn static_xa_cue_census_is_internally_consistent() {
        // 4 non-Baka cues (1x scripted voice + 3x grunt) + 12 Baka channels.
        assert_eq!(STATIC_XA_CUES.len(), 16);

        // Every channel is a valid CD-XA channel (<= 15), as FUN_8004C140's
        // membership guard requires for the pool tables.
        assert!(STATIC_XA_CUES.iter().all(|c| c.channel <= 15));

        // The normal-move grunt fires clip 0x1D on channels 0 / 4 / 6
        // (durs 0x26 / 0x2E / 0x1A read off the 0x801EEB44 block's literals).
        let grunt: Vec<(u8, u16)> = STATIC_XA_CUES
            .iter()
            .filter(|c| c.clip_id == 0x1d)
            .map(|c| (c.channel, c.dur.unwrap()))
            .collect();
        assert_eq!(grunt, vec![(0, 0x26), (4, 0x2e), (6, 0x1a)]);

        // Baka Fighter fires slot 0x20 across exactly its twelve fixed
        // channels (no channel 0xD site exists in PROT 0976's true extent;
        // channel 1 is the literal-s4 duel line at 0x801D04EC).
        let mut baka: Vec<u8> = STATIC_XA_CUES
            .iter()
            .filter(|c| c.clip_id == 0x20)
            .map(|c| c.channel)
            .collect();
        baka.sort_unstable();
        assert_eq!(baka, vec![1, 2, 3, 4, 5, 8, 9, 0xa, 0xb, 0xc, 0xe, 0xf]);

        // Every callsite supplies a literal duration operand.
        assert!(STATIC_XA_CUES.iter().all(|c| c.dur.is_some()));

        // Every entry cites its PROT-entry byte provenance.
        assert!(STATIC_XA_CUES.iter().all(|c| c.dump.contains("PROT")));
    }
}
