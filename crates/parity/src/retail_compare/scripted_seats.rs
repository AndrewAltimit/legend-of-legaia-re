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
//! A cross-context run-to-tile (`CC <flat> 51 xb zb depth mv`) is the same
//! history: its seat arm is a teleport onto the `MOVE_TO` tile decode
//! (`0x801E1904..0x801E1940`) with the compass heading `depth & 0xF`. `town0d`
//! `P2[32]`, the Rim Elm revisit arrival the entry spawns, closes with
//! `CC 36 51 60 0F 07 42`: Mei (flat `54`) on tile `(96, 15)`, `(12352, 1984)`,
//! facing index `7`, `0xE00`. Her record `P1[21]` seats her on
//! `4C 51 DF 0F 00 ..`, `(12288, 1984)` facing `0`, so a card load past the
//! beat stands her there; `town0d_house_interior` / `town0d_house_free` were
//! played through the beat. The position seat moves her, and the heading seat
//! below then turns her, since `0xE00` is the beat's own literal for her.
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
//!
//! A beat's last turn is the same history when the beat ends with its cast
//! member on the seat its record stands it on anyway. `son` `P2[5]` turns
//! Luctes (flat `10`) with `B8 0A 83 12`, a budgeted `0x38` rotate onto
//! compass index `3`, `0x600`, and leaves him on `(2432, 7936)`, the tile his
//! record `P1[4]` seats him on with `4C 51 92 BD 07 ..`, facing index `7`
//! (`0xE00`). A card load past the beat stands him there facing `0xE00`, so
//! the position seat has nothing to move and his heading stays the record's.
//! A placement the engine already stands on retail's seat, whose retail
//! heading is a compass literal a partition-2 beat turns it to (a targeted
//! `0x38` CAM_CFG, `table[op0 & 0xF]`, or a `4C 51` seat's `depth & 0xF`;
//! the table is SCUS `0x80073F04`, index `i` = `i * 0x200`), takes retail's
//! heading ([`scripted_heading_seats`], `World::seed_placement_heading`, the
//! image child's `LEGAIA_SEAT_HEADINGS`). A heading no beat names for that
//! flat still scores as a miss.

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
/// `MOVE_TO` and run-to-tile (`CC <flat> 51`) tile target by the flat index it
/// aims at, and every flat a beat parks off-map (`MOVE_TO` or run-to-tile onto
/// [`PARK`]).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BeatMoves {
    pub targets: BTreeMap<u16, Vec<(i16, i16)>>,
    pub parks: std::collections::BTreeSet<u16>,
    /// Every retail-space compass heading a beat turns a flat to: a targeted
    /// `0x38` CAM_CFG's `op0 & 0xF` and a `4C 51` seat's `depth & 0xF`, each
    /// an index into the SCUS `0x80073F04` table.
    pub turns: BTreeMap<u16, std::collections::BTreeSet<u16>>,
    /// Every tile a beat walks a flat onto (`C7 <flat> xb zb mode`, the
    /// walk-to-tile kernel's own tile decode): the walk turns its walker
    /// along the leg, so a placement standing on the landing holds the
    /// leg's heading.
    pub landings: BTreeMap<u16, std::collections::BTreeSet<(i16, i16)>>,
}

/// The retail heading compass index `i` names in the SCUS `0x80073F04`
/// table (`[0, 0x200, .. 0xE00]`); `None` past its eight entries.
fn compass_heading(i: u8) -> Option<u16> {
    (i < 8).then(|| u16::from(i) * 0x200)
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
                InsnInfo::MenuCtrl {
                    kind:
                        MenuCtrlKind::Nibble5NpcRun {
                            x_enc,
                            z_enc,
                            depth,
                            ..
                        },
                    ..
                } => {
                    // The run-to-tile is a teleport seat on the same tile
                    // decode as `MOVE_TO` (`0x801E1904..0x801E1940`), so its
                    // tile is a beat literal too.
                    out.targets
                        .entry(flat)
                        .or_default()
                        .push((move_to_world(*x_enc), move_to_world(*z_enc)));
                    if let Some(h) = compass_heading(depth & 0xF) {
                        out.turns.entry(flat).or_default().insert(h);
                    }
                }
                InsnInfo::Yield {
                    kind: legaia_scene_formats::field_disasm::YieldKind::Wide,
                } if body.get(insn.pc) == Some(&0xC7) => {
                    if let (Some(&xb), Some(&zb)) = (body.get(insn.pc + 2), body.get(insn.pc + 3)) {
                        out.landings
                            .entry(flat)
                            .or_default()
                            .insert((move_to_world(xb), move_to_world(zb)));
                    }
                }
                InsnInfo::CamCfg { op0, .. } => {
                    if let Some(h) = compass_heading(op0 & 0xF) {
                        out.turns.entry(flat).or_default().insert(h);
                    }
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

/// The retail placements standing where their own records seat them when the
/// scene spawns under `flags`, as seats.
///
/// A probe that lowers story flags on a state loaded inside a scene leaves
/// every placement where its spawn section stood it under the flags as they
/// were: the spawn ran once, at the scene entry, and nothing re-runs it.
/// `bylon_mist_gala_goes_ahead` was made from `bylon_field_door_hop` with
/// `0x2A2` / `0x2A6` / `0x2A7` lowered to replay the Mist beat, and flat `65`
/// (`P1[30]`) still stands on `(15232, 13376)`, the `23 76 68` / `41 06 01`
/// arm its record takes with `0x2A7` up; a load with the flag down leaves it
/// on its MAN seat `(14784, 12864)`. The scenario names the flags
/// (`spawned_under_flags`), and a placement whose retail position is exactly
/// where a spawn of the scene under them stands it is stood there. A
/// placement the spawn puts anywhere else still scores as a miss, and one the
/// load already stands on retail's seat is left to the load.
pub fn spawn_history_seats(
    extracted: &std::path::Path,
    retail: &super::RetailObs,
    flags: &[u16],
) -> Vec<WalkerSeed> {
    if flags.is_empty() {
        return Vec::new();
    }
    let Some(plain) = retail.seed_save() else {
        return Vec::new();
    };
    let mut raised = plain.clone();
    for &f in flags {
        super::set_system_flag(&mut raised, f, true);
    }
    let (Some(under), Some(now)) = (
        spawn_seats(extracted, &retail.scene, raised),
        spawn_seats(extracted, &retail.scene, plain),
    ) else {
        return Vec::new();
    };
    retail
        .actor_facings
        .iter()
        .filter(|a| {
            let at = Some(&(a.x, a.z));
            !a.ambient && under.get(&a.flat) == at && now.get(&a.flat) != at
        })
        .map(|a| WalkerSeed {
            flat: a.flat,
            x: a.x,
            z: a.z,
            heading: (a.facing as u16) & 0x0FFF,
        })
        .collect()
}

/// Where each placement stands, by flat index, a few frames after `scene`
/// spawns over `save`.
fn spawn_seats(
    extracted: &std::path::Path,
    scene: &str,
    save: legaia_save::SaveFile,
) -> Option<BTreeMap<u16, (i16, i16)>> {
    use legaia_engine_save_state::WorldSaveState;
    let mut host =
        legaia_engine_scene_host::scene_host::SceneHost::open_extracted(extracted).ok()?;
    host.world.load_full(save);
    host.enter_field_scene(scene, 0).ok()?;
    // A spawn section's compass step lands a few frames after the entry.
    for _ in 0..SPAWN_SETTLE_TICKS {
        host.tick().ok()?;
        let _ = host.step_headless_frame_tail();
    }
    Some(
        host.world
            .field_vm
            .channels
            .iter()
            .filter(|c| !c.object_bind)
            .filter_map(|c| {
                let slot = u8::try_from(c.placement_index).ok()?;
                let at = host
                    .world
                    .npcs
                    .positions
                    .get(&slot)
                    .copied()
                    .unwrap_or((c.ctx.world_x as i16, c.ctx.world_z as i16));
                Some((c.ctx.script_id, at))
            })
            .collect(),
    )
}

/// Frames [`spawn_history_seats`] runs past the entry.
const SPAWN_SETTLE_TICKS: usize = 16;

/// The retail placements, standing on a seat, whose heading is a compass
/// literal a beat turns them to ([`BeatMoves::turns`]), or who stand on a
/// tile a beat walks them onto ([`BeatMoves::landings`]), as heading seats.
/// `kor5` `P2[4]` walks flat `9` onto `(0x99, 0xA8)` with
/// `C7 09 99 A8 32` and `P2[5]` re-seats him there with `A3 09 99 A8`, which
/// moves it without a turn: `kor5_p2_5_pre_raise` holds the walk's heading
/// `0x400`, where a seed that resumes `P2[5]` keeps its record's `0xC00`.
/// The seed stands the heading only where the engine already holds
/// retail's position (`World::seed_placement_heading`).
pub fn scripted_heading_seats(beats: &BeatMoves, facings: &[ActorFacing]) -> Vec<WalkerSeed> {
    facings
        .iter()
        .filter(|a| !a.ambient && !npc_parked(a.x, a.z))
        .filter(|a| {
            beats
                .turns
                .get(&a.flat)
                .is_some_and(|t| t.contains(&((a.facing as u16) & 0x0FFF)))
                || beats
                    .landings
                    .get(&a.flat)
                    .is_some_and(|l| l.contains(&(a.x, a.z)))
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
    fn a_run_to_tile_seat_is_a_beat_target_and_turn() {
        // town0d P2[32]'s closing seat on Mei: `CC 36 51 60 0F 07 42`. A
        // minimal MAN cannot be built here, so the decode the walk applies is
        // pinned on the operand bytes.
        assert_eq!((move_to_world(0x60), move_to_world(0x0F)), (12352, 1984));
        assert_eq!(compass_heading(0x07 & 0xF), Some(0xE00));
        // Her record's own seat, far edge on X.
        assert_eq!((move_to_world(0xDF), move_to_world(0x0F)), (12288, 1984));
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
    fn only_a_heading_a_beat_turns_the_flat_to_is_seated() {
        let mut beats = BeatMoves::default();
        beats.turns.entry(10u16).or_default().insert(0x600);
        let facing = |flat, facing, x, z, ambient| ActorFacing {
            flat,
            facing,
            x,
            z,
            y: 0,
            model: 69,
            flags: 0,
            ambient,
            ambient_op: None,
        };
        let seats = scripted_heading_seats(
            &beats,
            &[
                facing(10, 0x600, 2432, 7936, false),
                // A heading no beat names for the flat.
                facing(10, 0x400, 2432, 7936, false),
                // The literal, on another flat.
                facing(11, 0x600, 2432, 7936, false),
                // A wanderer, and a parked placement.
                facing(10, 0x600, 2432, 7936, true),
                facing(10, 0x600, 16320, 16320, false),
            ],
        );
        assert_eq!(seats.len(), 1);
        assert_eq!((seats[0].flat, seats[0].heading), (10, 0x600));
    }

    #[test]
    fn compass_indices_name_the_eight_table_headings() {
        assert_eq!(compass_heading(3), Some(0x600));
        assert_eq!(compass_heading(7), Some(0xE00));
        assert_eq!(compass_heading(8), None);
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
