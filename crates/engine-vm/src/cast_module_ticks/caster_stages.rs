//! The **caster's own clip stages** of the capture-class bodies whose tick
//! port does not write the caster's `+0x1DA`.
//!
//! A capture-class cast never reads the AI picker's `+0x1E0` clip byte: the
//! action SM routes a class-`0x63` record to `0x6E` (`0x801E4490`), and from
//! there the slot-B module stages the caster's clips itself with
//! `sb <clip>,0x1DA(<caster>)`, the caster being the register loaded from
//! `actor_table[ctx[+0x13]]` in the body's prologue. Most bodies stage one
//! literal at arm `0` - the monster's wind-up / cast entry - and close with
//! `sb zero,0x1DA(<caster>)`; a few stage two in sequence, and three pick the
//! literal on a monster id.
//!
//! The per-body ports in this crate carry the phase walk, the damage and the
//! victim's reaction, but for the bodies below none of the caster stage
//! sites: without these rows a monster's special plays its whole band on the
//! idle loop. The engine replays the listed stages, each to its clip's end,
//! at the head of the band (`World::capture_stager_tick`).
//!
//! What this table does **not** carry, and why:
//!
//! * the bodies whose ports already stage the caster - PROT 0938's `0x4E`
//!   (`CHAOS_BREATH_ARM0_CLIP`), 0952's `0xB8` (Astral Slash), 0960's `0x7B`
//!   (Plasma Strike) and the fourteen trampoline arms of
//!   [`crate::cast_arm_ticks`];
//! * the bodies with **no** caster stage at all - PROT 0945's `0x54`
//!   (Water Column), 0946 (Call Wave), 0949 (Water Crystals), 0954 (Fatal
//!   Decision) and 0955's `0x6E` (Kiss of Death) write `+0x1DA` only on the
//!   victim (its `+0x1F1` / `+0x1EF` reaction), so their caster holds idle in
//!   retail too;
//! * the **approach** of the melee bodies (`approach` below, and the ported
//!   bodies of [`CAPTURE_APPROACH_BODIES`]): retail's arm `0` turns the
//!   caster onto its victim, stages the walk (`1`) and holds on the range
//!   poll `FUN_8004E2F0` until the caster is in reach. That leg is the
//!   band's (`World::capture_stager_tick`), ahead of the stages. Nothing
//!   walks the caster home afterwards, as after any action.
//!
//! Provenance: the `sb ...,0x1DA` sites cited per row, in the module images at
//! slot-B base `0x801F69D8` (`see ghidra/scripts/funcs/overlay_cast_<label>_<entry>_<va>.txt`).

/// How a row picks the caster's clip list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CasterClipRule {
    /// The body stages these literals, in this order.
    Fixed(&'static [u8]),
    /// The literal depends on the caster seat's own monster id
    /// (`0x8007BD0C[ctx[+0x13] - 3]`): `hit` when it equals `id`, else `miss`.
    SeatMonster { id: u8, hit: u8, miss: u8 },
    /// The literal depends on the battle's **first** monster id
    /// (`0x8007BD0C[0]`, `lbu -0x42F4(0x80080000)`).
    FirstMonster { id: u8, hit: u8, miss: u8 },
}

/// One body's caster stages.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CasterStageRow {
    /// Owning PROT entry.
    pub prot_entry: u32,
    /// The action ids the row covers; empty = every id the module serves (a
    /// single-body module).
    pub action_ids: &'static [u8],
    /// The clips.
    pub rule: CasterClipRule,
    /// Retail walks the caster into reach before the first stage (see the
    /// module doc).
    pub approach: bool,
}

/// Every capture-class body whose port does not stage the caster.
pub const CAPTURE_CASTER_STAGES: &[CasterStageRow] = &[
    // PROT 0935 Earthquake: `li v0,0x9; sb v0,0x1da(s4)` at `0x801F6B4C`.
    CasterStageRow {
        prot_entry: 935,
        action_ids: &[],
        rule: CasterClipRule::Fixed(&[0x09]),
        approach: false,
    },
    // PROT 0936 Hyper Crush: `0x0D` at `0x801F6BA0`, then `0x0B` at `0x801F6EB0`.
    CasterStageRow {
        prot_entry: 936,
        action_ids: &[],
        rule: CasterClipRule::Fixed(&[0x0D, 0x0B]),
        approach: false,
    },
    // PROT 0937 Hyper Lightning: `0x0D` at `0x801F6B38`, then `0x0C` at `0x801F6E80`.
    CasterStageRow {
        prot_entry: 937,
        action_ids: &[],
        rule: CasterClipRule::Fixed(&[0x0D, 0x0C]),
        approach: false,
    },
    // PROT 0938 `0xB7` (`0x801F69EC`, Mystic Circle): `li v0,0x7;
    // sb v0,0x1da(s4)` at `0x801F6CCC`.
    CasterStageRow {
        prot_entry: 938,
        action_ids: &[0xB7],
        rule: CasterClipRule::Fixed(&[0x07]),
        approach: false,
    },
    // PROT 0939 Spore Gas: `li v1,0xa; sb v1,0x1da(s2)` at `0x801F6C54`.
    CasterStageRow {
        prot_entry: 939,
        action_ids: &[],
        rule: CasterClipRule::Fixed(&[0x0A]),
        approach: false,
    },
    // PROT 0940 `0x3C`: `0x8007BD0C[seat - 3] == 0xA9` stages `9`
    // (`0x801F6B44`), anything else `8` (`0x801F6B64`).
    CasterStageRow {
        prot_entry: 940,
        action_ids: &[0x3C],
        rule: CasterClipRule::SeatMonster {
            id: 0xA9,
            hit: 0x09,
            miss: 0x08,
        },
        approach: false,
    },
    // PROT 0947 V-Windhash: `li v0,0xa; sb v0,0x1da(s2)` at `0x801F6B20`.
    CasterStageRow {
        prot_entry: 947,
        action_ids: &[],
        rule: CasterClipRule::Fixed(&[0x0A]),
        approach: false,
    },
    // PROT 0948 Cross Beam: `li v0,0x8; sb v0,0x1da(s3)` at `0x801F6AD0`.
    CasterStageRow {
        prot_entry: 948,
        action_ids: &[],
        rule: CasterClipRule::Fixed(&[0x08]),
        approach: false,
    },
    // PROT 0951 `0x36` (`0x801F6A20`): walk, then `8` at `0x801F6D64`.
    CasterStageRow {
        prot_entry: 951,
        action_ids: &[0x36],
        rule: CasterClipRule::Fixed(&[0x08]),
        approach: true,
    },
    // PROT 0951 `0x5B` (`0x801F77E8`): walk, then `9` at `0x801F7B6C`.
    CasterStageRow {
        prot_entry: 951,
        action_ids: &[0x5B],
        rule: CasterClipRule::Fixed(&[0x09]),
        approach: true,
    },
    // PROT 0952 `0x5C` (`0x801F7118`): walk, then `9` at `0x801F7494` and
    // `0x0A` at `0x801F7538`.
    CasterStageRow {
        prot_entry: 952,
        action_ids: &[0x5C],
        rule: CasterClipRule::Fixed(&[0x09, 0x0A]),
        approach: true,
    },
    // PROT 0953 Terio Punch: `li v1,0xa; sb v1,0x1da(s4)` at `0x801F6CAC`.
    CasterStageRow {
        prot_entry: 953,
        action_ids: &[],
        rule: CasterClipRule::Fixed(&[0x0A]),
        approach: false,
    },
    // PROT 0955 `0x6F` (`0x801F7FA4`): walk, then `7` at `0x801F826C`.
    CasterStageRow {
        prot_entry: 955,
        action_ids: &[0x6F],
        rule: CasterClipRule::Fixed(&[0x07]),
        approach: true,
    },
    // PROT 0955 `0x70` (`0x801F767C`): `8` at `0x801F79FC`.
    CasterStageRow {
        prot_entry: 955,
        action_ids: &[0x70],
        rule: CasterClipRule::Fixed(&[0x08]),
        approach: false,
    },
    // PROT 0955 `0x72` (`0x801F7158`): `0x0A` at `0x801F72AC`.
    CasterStageRow {
        prot_entry: 955,
        action_ids: &[0x72],
        rule: CasterClipRule::Fixed(&[0x0A]),
        approach: false,
    },
    // PROT 0957 `0x77` (`0x801F6A14`): `0x0A` at `0x801F6C50`.
    CasterStageRow {
        prot_entry: 957,
        action_ids: &[0x77],
        rule: CasterClipRule::Fixed(&[0x0A]),
        approach: false,
    },
    // PROT 0957 `0x76` (`0x801F798C`): `9` at `0x801F7CF8`.
    CasterStageRow {
        prot_entry: 957,
        action_ids: &[0x76],
        rule: CasterClipRule::Fixed(&[0x09]),
        approach: false,
    },
    // PROT 0960 `0xA6` (`0x801F69D8`, Neo Star Slash): `li v0,0xa;
    // sb v0,0x1da(s4)` at `0x801F6B98`.
    CasterStageRow {
        prot_entry: 960,
        action_ids: &[0xA6],
        rule: CasterClipRule::Fixed(&[0x0A]),
        approach: false,
    },
    // PROT 0961 `0xA1` / `0xB4`: first monster `0xB5` stages `6`
    // (`0x801F6D58`), anything else `8` (`0x801F6DE8`).
    CasterStageRow {
        prot_entry: 961,
        action_ids: &[0xA1, 0xB4],
        rule: CasterClipRule::FirstMonster {
            id: 0xB5,
            hit: 0x06,
            miss: 0x08,
        },
        approach: false,
    },
    // PROT 0962 `0xA5` (`0x801F69D8`): first monster `0xB5` stages `6`
    // (`0x801F6AEC`), anything else `7` (`0x801F6B00`).
    CasterStageRow {
        prot_entry: 962,
        action_ids: &[0xA5],
        rule: CasterClipRule::FirstMonster {
            id: 0xB5,
            hit: 0x06,
            miss: 0x07,
        },
        approach: false,
    },
    // PROT 0963 Genocidal Cannon `0xB3`: `8` at `0x801F6EC8`.
    CasterStageRow {
        prot_entry: 963,
        action_ids: &[0xB3],
        rule: CasterClipRule::Fixed(&[0x08]),
        approach: false,
    },
    // PROT 0965 Doomsday `0xB6`: `7` at `0x801F6B30`.
    CasterStageRow {
        prot_entry: 965,
        action_ids: &[0xB6],
        rule: CasterClipRule::Fixed(&[0x07]),
        approach: false,
    },
    // PROT 0966 Evil Seru Magic: `li v0,0x6; sb v0,0x1da(s2)` at `0x801F6DE4`.
    CasterStageRow {
        prot_entry: 966,
        action_ids: &[],
        rule: CasterClipRule::Fixed(&[0x06]),
        approach: false,
    },
];

/// The capture-class bodies that write **no** caster `+0x1DA` at all - their
/// only stage sites are the victim's reaction (`+0x1F1` / `+0x1EF` /
/// `+0x1F0`) - as `(PROT entry, action ids)`, empty ids = the whole module.
/// A monster casting one of these holds its idle in retail; the effect is the
/// whole of the visual.
///
/// * PROT 0945 `0x54` Water Column - the one stage, `0x801F74FC`, is on the
///   victim `s2`.
/// * PROT 0946 Call Wave - `0x801F7468`, the victim's `+0x1F1`.
/// * PROT 0949 Water Crystals - `0x801F7370`, the victim's `+0x1F1`.
/// * PROT 0954 Fatal Decision - `0x801F83CC` / `0x801F83E8`, the victim's
///   reaction run.
/// * PROT 0955 `0x6E` Kiss of Death - `0x801F8DB4`, the victim's `+0x1F1` /
///   `+0x1EF`.
///
/// The same holds - no caster store; any `+0x1DA` write is a victim's
/// reaction - for PROT 0940's
/// `0x801F78B8` (`0x50` / `0xAE`) and `0x801F7240` (`0xAC`), PROT 0941's
/// `0x801F6A04` (`0xB9`, whose one stage is the victim's), PROT 0942's
/// `0x801F7D34` (`0x52`), PROT 0943's `0x801F6A04` (`0xB5`), PROT 0945's
/// `0x801F69F8` (`0xBA`), PROT 0955's `0x801F8F0C` (`0x60`) and
/// `0x801F6A28` (`0x73`), PROT 0956's `0x801F7298` (`0x71`) and
/// `0x801F69D8` (`0x75`), and PROT 0964's `0x801F88EC` (`0xAF`).
pub const CAPTURE_BODIES_WITHOUT_CASTER_STAGE: &[(u32, &[u8])] = &[
    (940, &[0x50, 0xAC, 0xAE]),
    (941, &[0xB9]),
    (942, &[0x52]),
    (943, &[0xB5]),
    (945, &[0x54, 0xBA]),
    (946, &[]),
    (949, &[]),
    (954, &[]),
    (955, &[0x60, 0x6E, 0x73]),
    (956, &[0x71, 0x75]),
    (964, &[0xAF]),
];

/// Whether the capture-class body PROT `prot_entry` runs for `action_id`
/// stages nothing on its caster ([`CAPTURE_BODIES_WITHOUT_CASTER_STAGE`]).
pub fn capture_body_idles_caster(prot_entry: u32, action_id: u8) -> bool {
    CAPTURE_BODIES_WITHOUT_CASTER_STAGE
        .iter()
        .any(|(e, ids)| *e == prot_entry && (ids.is_empty() || ids.contains(&action_id)))
}

/// Ported capture-class bodies whose arm `0` walks the caster into reach
/// (the walk entry `1` staged and held on the range poll `FUN_8004E2F0`, the
/// pairwise separation `FUN_80050BB8` x `0x20` on arrival) but whose port
/// runs the strike from wherever the caster stands, as `(PROT entry, action
/// ids)`. Their approach is the band's, like the [`CAPTURE_CASTER_STAGES`]
/// rows marked `approach`.
///
/// * PROT 0941 `0x51` - the polls at `0x801F75E0` / `0x801F76D8`, the walk
///   stage `0x801F756C`;
/// * PROT 0950 `0x5A` - the poll at `0x801F7B54`, the walk stage `0x801F7BA4`;
/// * PROT 0952 `0xB8` - the poll and the walk stage `0x801F6BBC` (the
///   stage's `li v1,0x1` sits in the poll branch's delay slot at
///   `0x801F6B7C`);
/// * PROT 0962 `0xA2` / `0xA3` / `0xA4` - the polls at `0x801F7C04` /
///   `0x801F7D00` and their siblings in the other two bodies.
pub const CAPTURE_APPROACH_BODIES: &[(u32, &[u8])] = &[
    // PROT 0941 `0x51` (Steal): the polls at `0x801F75E0` / `0x801F76D8` and
    // the walk stage `0x801F756C` (`FUN_80050E2C(table, 1, count)` - the
    // record's walk entry by tag). It stages no other caster clip.
    (941, &[0x51]),
    (950, &[0x5A]),
    (952, &[0xB8]),
    (962, &[0xA2, 0xA3, 0xA4]),
];

/// Whether the capture-class body PROT `prot_entry` runs for `action_id`
/// walks its caster into reach before it strikes.
pub fn capture_body_approaches(prot_entry: u32, action_id: u8) -> bool {
    let matches =
        |e: u32, ids: &[u8]| e == prot_entry && (ids.is_empty() || ids.contains(&action_id));
    CAPTURE_APPROACH_BODIES
        .iter()
        .any(|(e, ids)| matches(*e, ids))
        || CAPTURE_CASTER_STAGES
            .iter()
            .any(|r| r.approach && matches(r.prot_entry, r.action_ids))
}

/// The caster clips a capture-class cast of `action_id` through PROT
/// `prot_entry` stages, in order, for a caster seat whose monster id is
/// `seat_monster` in a battle whose first monster is `first_monster`.
/// `None` when the body's port stages the caster itself or the body stages
/// nothing on it.
///
/// REF: FUN_801F2160 (the capture-class dispatch the rows hang off)
pub fn capture_caster_stages(
    prot_entry: u32,
    action_id: u8,
    seat_monster: u8,
    first_monster: u8,
) -> Option<Vec<u8>> {
    let row = CAPTURE_CASTER_STAGES.iter().find(|r| {
        r.prot_entry == prot_entry && (r.action_ids.is_empty() || r.action_ids.contains(&action_id))
    })?;
    Some(match row.rule {
        CasterClipRule::Fixed(clips) => clips.to_vec(),
        CasterClipRule::SeatMonster { id, hit, miss } => {
            vec![if seat_monster == id { hit } else { miss }]
        }
        CasterClipRule::FirstMonster { id, hit, miss } => {
            vec![if first_monster == id { hit } else { miss }]
        }
    })
}

#[cfg(test)]
mod caster_stage_tests {
    use super::*;

    #[test]
    fn rows_resolve_by_entry_and_id() {
        assert_eq!(capture_caster_stages(948, 0x58, 0x74, 0x74), Some(vec![8]));
        assert_eq!(
            capture_caster_stages(937, 0x4C, 0x88, 0x88),
            Some(vec![0x0D, 0x0C])
        );
        // A two-body module answers only for the body the row names.
        assert_eq!(capture_caster_stages(951, 0x5B, 0, 0), Some(vec![9]));
        assert_eq!(capture_caster_stages(952, 0xB8, 0, 0), None);
        // The monster-id rules.
        assert_eq!(capture_caster_stages(940, 0x3C, 0xA9, 0), Some(vec![9]));
        assert_eq!(capture_caster_stages(940, 0x3C, 0x83, 0), Some(vec![8]));
        assert_eq!(capture_caster_stages(961, 0xB4, 0, 0xB5), Some(vec![6]));
        assert_eq!(capture_caster_stages(961, 0xA1, 0, 0xA1), Some(vec![8]));
        // Bodies that stage nothing on the caster.
        assert_eq!(capture_caster_stages(945, 0x54, 0, 0), None);
        assert_eq!(capture_caster_stages(946, 0x56, 0, 0), None);
        // The approach: table rows and the ported melee bodies alike.
        assert!(capture_body_approaches(952, 0x5C));
        assert!(capture_body_approaches(952, 0xB8));
        assert!(capture_body_approaches(962, 0xA3));
        assert!(!capture_body_approaches(962, 0xA5));
        assert!(!capture_body_approaches(948, 0x58));
    }
}
