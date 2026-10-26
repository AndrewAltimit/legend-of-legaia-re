//! Placements a finished cutscene beat left on a script-literal tile.
//!
//! A partition-2 record is a walk-on cutscene beat: it runs once, when the
//! player crosses its trigger tile, and its own story latch keeps it from
//! running again. Its cross-context `MOVE_TO` (`A3 <flat> xb zb`) puts a cast
//! member on a tile that no entry path reproduces: a card load spawns the
//! placement from its record head instead. `kor5` `P2[3]` ends by standing
//! Old Deez (flat `14`) back in front of the statue at `A3 0E 20 5D`, tile
//! `(32, 93)`, which is world `(4160, 11968)`; his record head spawns him at
//! `(0x20, 0xDC)`, world `(4160, 11904)`. `kor5_post_43a_checkpoint` was
//! played through that beat, not loaded from a card past it, so its RAM holds
//! the beat's tile.
//!
//! That position is history the seed does not run, like a walker's `rand()`
//! seat, so a placement whose retail position is exactly the decoded target
//! of such a `MOVE_TO` aimed at it is stood there, position only - its
//! heading stays the engine's for the facing channel
//! (`World::seed_placement_seat`, the image child's `LEGAIA_SEAT_PLACEMENTS`).
//! Only a literal a partition-2 beat holds counts, so a placement the engine's
//! own entry misplaces still scores as a miss.

use std::collections::BTreeMap;

use legaia_engine_capture::field_seeds::WalkerSeed;
use legaia_scene_formats::field_disasm::{InsnInfo, LinearWalker};

use super::ActorFacing;

/// The world position a `MOVE_TO` grid byte names: the tile centre
/// `(b & 0x7F) * 0x80 + 0x40`, and the far edge (`+0x40` more) when bit 7 is
/// set (`docs/subsystems/script-vm.md`, op `0x23`; the walk kernel
/// `FUN_8003774C` decodes its walk-to-tile bytes the same way at
/// `0x80037B90..0x80037C04`).
pub fn move_to_world(b: u8) -> i16 {
    i16::from(b & 0x7F) * 0x80 + 0x40 + if b & 0x80 != 0 { 0x40 } else { 0 }
}

/// Every cross-context `MOVE_TO` target a scene's partition-2 records hold,
/// by the flat index they aim at.
pub fn beat_move_targets(man: &[u8]) -> BTreeMap<u16, Vec<(i16, i16)>> {
    let mut out: BTreeMap<u16, Vec<(i16, i16)>> = BTreeMap::new();
    let Ok(mf) = legaia_scene_formats::man_section::parse(man) else {
        return out;
    };
    let n2 = mf.header.partition_counts[2].max(0) as usize;
    for rec in 0..n2 {
        let Some((start, pc0, len)) =
            legaia_engine_field::man_field_scripts::partition_record_span(&mf, man, 2, rec)
        else {
            continue;
        };
        let Some(body) = man.get(start..start + len) else {
            continue;
        };
        for insn in LinearWalker::new(body, pc0).flatten() {
            if let (Some(flat), InsnInfo::MoveTo { xb, zb }) = (insn.extended, &insn.info) {
                // The off-map park literal is not a tile.
                if (*xb, *zb) != (0x7F, 0x7F) {
                    out.entry(u16::from(flat))
                        .or_default()
                        .push((move_to_world(*xb), move_to_world(*zb)));
                }
            }
        }
    }
    out
}

/// The retail placements standing exactly on a beat's `MOVE_TO` target for
/// them ([`beat_move_targets`]), as seats.
pub fn scripted_seats(
    targets: &BTreeMap<u16, Vec<(i16, i16)>>,
    facings: &[ActorFacing],
) -> Vec<WalkerSeed> {
    facings
        .iter()
        .filter(|a| !a.ambient)
        .filter(|a| {
            targets
                .get(&a.flat)
                .is_some_and(|t| t.contains(&(a.x, a.z)))
        })
        .map(|a| WalkerSeed {
            flat: a.flat,
            x: a.x,
            z: a.z,
            heading: (a.facing as u16) & 0x0FFF,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grid_bytes_decode_to_tile_centres_and_far_edges() {
        assert_eq!(move_to_world(0x20), 4160);
        assert_eq!(move_to_world(0x5D), 11968);
        // The record head's `0xDC`: tile 92, far edge.
        assert_eq!(move_to_world(0xDC), 11904);
        assert_eq!(move_to_world(0xAC), 0x2C * 0x80 + 0x80);
    }

    #[test]
    fn only_a_placement_on_a_beat_literal_is_seated() {
        let mut targets = BTreeMap::new();
        targets.insert(14u16, vec![(4160i16, 11968i16)]);
        let facing = |flat, x, z, ambient| ActorFacing {
            flat,
            facing: 0x0800,
            x,
            z,
            model: 66,
            flags: 0,
            ambient,
        };
        let seats = scripted_seats(
            &targets,
            &[
                facing(14, 4160, 11968, false),
                facing(14, 4160, 11904, false),
                facing(15, 4160, 11968, false),
                facing(14, 4160, 11968, true),
            ],
        );
        assert_eq!(seats.len(), 1);
        assert_eq!((seats[0].flat, seats[0].x, seats[0].z), (14, 4160, 11968));
        assert_eq!(seats[0].heading, 0x0800);
    }
}
