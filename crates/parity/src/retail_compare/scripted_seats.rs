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
//!
//! The off-map park literal is the same history. A beat that ends by parking
//! its cast (`A3 <flat> 7F 7F`, or the run-to-tile `CC <flat> 51 7F 7F`)
//! leaves them parked for the rest of the visit, and a card load spawns them
//! from their records over the story flags the beat raised: `town01` `P2[4]`
//! sets `0x226` as its first op and ends by parking flats `69` / `70`, whose
//! record `P1[33]` stands them at tile `(98, 13)` on a load with `0x226` up
//! (`town01_tetsu_topic_prompt`); `nilboa` `P2[0]` parks Gala (flat `50`) and
//! `P2[26]` the two pushed blocks (flats `39` / `40`), which their own
//! records stand on the bridge tiles on a load (`nivora_cutscene_delilas_intro`).
//! A placement retail holds on the park seat ([`npc_parked`]) whose flat a
//! beat parks is stood on retail's park coordinates, with retail's heading:
//! the beat's last walk turned it, and nothing draws it there.

use std::collections::BTreeMap;

use legaia_engine_capture::field_seeds::WalkerSeed;
use legaia_scene_formats::field_disasm::{InsnInfo, LinearWalker, MenuCtrlKind};

use super::ActorFacing;
use super::scoring::npc_parked;

/// The off-map park literal a `MOVE_TO` / run-to-tile grid pair names.
const PARK: (u8, u8) = (0x7F, 0x7F);

/// The world position a `MOVE_TO` grid byte names: the tile centre
/// `(b & 0x7F) * 0x80 + 0x40`, and the far edge (`+0x40` more) when bit 7 is
/// set (`docs/subsystems/script-vm.md`, op `0x23`; the walk kernel
/// `FUN_8003774C` decodes its walk-to-tile bytes the same way at
/// `0x80037B90..0x80037C04`).
pub fn move_to_world(b: u8) -> i16 {
    i16::from(b & 0x7F) * 0x80 + 0x40 + if b & 0x80 != 0 { 0x40 } else { 0 }
}

/// What a scene's partition-2 records do to their cast: every cross-context
/// `MOVE_TO` tile target by the flat index it aims at, and every flat a beat
/// parks off-map (`MOVE_TO` or run-to-tile onto [`PARK`]).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BeatMoves {
    pub targets: BTreeMap<u16, Vec<(i16, i16)>>,
    pub parks: std::collections::BTreeSet<u16>,
}

/// Every cross-context `MOVE_TO` target and park a scene's partition-2
/// records hold ([`BeatMoves`]).
pub fn beat_move_targets(man: &[u8]) -> BeatMoves {
    let mut out = BeatMoves::default();
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
            let Some(flat) = insn.extended else {
                continue;
            };
            let flat = u16::from(flat);
            match &insn.info {
                InsnInfo::MoveTo { xb, zb } if (*xb, *zb) == PARK => {
                    out.parks.insert(flat);
                }
                InsnInfo::MoveTo { xb, zb } => out
                    .targets
                    .entry(flat)
                    .or_default()
                    .push((move_to_world(*xb), move_to_world(*zb))),
                InsnInfo::MenuCtrl {
                    kind: MenuCtrlKind::Nibble5NpcRun { x_enc, z_enc, .. },
                    ..
                } if (*x_enc, *z_enc) == PARK => {
                    out.parks.insert(flat);
                }
                _ => {}
            }
        }
    }
    out
}

/// The retail placements standing exactly on a beat's `MOVE_TO` target for
/// them, or parked where a beat parks them ([`beat_move_targets`]), as seats.
pub fn scripted_seats(beats: &BeatMoves, facings: &[ActorFacing]) -> Vec<WalkerSeed> {
    facings
        .iter()
        .filter(|a| !a.ambient)
        .filter(|a| {
            beats
                .targets
                .get(&a.flat)
                .is_some_and(|t| t.contains(&(a.x, a.z)))
                || (npc_parked(a.x, a.z) && beats.parks.contains(&a.flat))
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
        let mut beats = BeatMoves::default();
        beats.targets.insert(14u16, vec![(4160i16, 11968i16)]);
        let facing = |flat, x, z, ambient| ActorFacing {
            flat,
            facing: 0x0800,
            x,
            z,
            y: 0,
            model: 66,
            flags: 0,
            ambient,
            ambient_op: None,
        };
        let seats = scripted_seats(
            &beats,
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

    #[test]
    fn a_parked_placement_is_seated_only_where_a_beat_parks_it() {
        let mut beats = BeatMoves::default();
        beats.parks.insert(69u16);
        let facing = |flat, x, z| ActorFacing {
            flat,
            facing: 0x0400,
            x,
            z,
            y: 0,
            model: 100,
            flags: 0,
            ambient: false,
            ambient_op: None,
        };
        let seats = scripted_seats(
            &beats,
            &[
                // Parked, and a beat parks it: seated on retail's seat.
                facing(69, 16320, 16320),
                // A beat parks it, but retail draws it: the engine's own.
                facing(69, 12608, 1728),
                // Parked, but no beat parks it: a spawn-section park.
                facing(36, 16320, 16256),
            ],
        );
        assert_eq!(seats.len(), 1);
        assert_eq!((seats[0].flat, seats[0].x, seats[0].z), (69, 16320, 16320));
    }
}
