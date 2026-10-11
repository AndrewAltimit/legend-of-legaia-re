//! Seated traversal: the walk-on doors toward a destination, placing the
//! player on one, talk-carried hops, and the gate report when no door fires.

use super::*;

// ---------------------------------------------------------------------------
// Doors
// ---------------------------------------------------------------------------

/// One way out of the current scene toward `dest`.
#[derive(Debug, Clone)]
pub(super) struct Door {
    pub(super) tile: (u8, u8),
    /// The entry tile the door's scene change lands on in `dest`, when the
    /// door's own record names it.
    pub(super) entry: Option<(u8, u8)>,
}

/// The doors toward `dest` from the loaded scene, and - when there are none -
/// why, in terms of the disc structures.
pub(super) fn doors_to(
    session: &NativeSession,
    graph: &DiscGraph,
    dest: &str,
) -> Result<Vec<Door>, String> {
    let w = &session.host.world;
    if session.host.world.mode == SceneMode::WorldMap {
        let doors: Vec<Door> = w
            .world_map
            .entity_configs
            .iter()
            .zip(w.world_map.entity_positions.iter())
            .filter_map(|(cfg, &(x, z))| match cfg {
                WorldMapEntityConfig::OverworldPortal {
                    scene_name,
                    entry_x,
                    entry_z,
                    ..
                } if scene_name == dest => Some(Door {
                    tile: ((x >> 7) as u8, (z >> 7) as u8),
                    entry: Some((*entry_x & 0x7F, *entry_z & 0x7F)),
                }),
                _ => None,
            })
            .collect();
        if doors.is_empty() {
            let installed: BTreeSet<String> = w
                .world_map
                .entity_configs
                .iter()
                .filter_map(|c| match c {
                    WorldMapEntityConfig::OverworldPortal { scene_name, .. } => {
                        Some(scene_name.clone())
                    }
                    _ => None,
                })
                .collect();
            return Err(format!(
                "no overworld portal to {dest} installed (portals: {installed:?}); {}",
                site_gate_report(session, dest)
            ));
        }
        return Ok(doors);
    }
    let name = scene_name(session);
    let index = &session.host.index;
    let scene = Scene::load(index, &name).map_err(|e| format!("Scene::load: {e:#}"))?;
    let man = scene
        .field_man_payload(index)
        .map_err(|e| format!("MAN: {e:#}"))?
        .ok_or("no MAN")?;
    let mf =
        legaia_scene_formats::man_section::parse(&man).map_err(|e| format!("parse MAN: {e:#}"))?;
    let (primary, fallback) = scene
        .field_tile_triggers(index)
        .map_err(|e| format!(".MAP triggers: {e:#}"))?;
    let triggers: Vec<TileTrigger> = primary.into_iter().chain(fallback).collect();
    let sites = overworld_portal_sites(&mf, &man, &triggers);
    let mut doors: Vec<Door> = sites
        .iter()
        .filter(|s| {
            s.scene_name == dest || s.conditional.as_ref().is_some_and(|c| c.scene_name == dest)
        })
        .map(|s| Door {
            tile: (s.overworld_x, s.overworld_z),
            entry: (s.scene_name == dest).then_some((s.entry_x & 0x7F, s.entry_z & 0x7F)),
        })
        .collect();
    // Sol's warp pads: a band whose record parks on the op-`49 04` floor
    // list and branches on the picked row to one scene change per floor
    // (`kor3` P2[9..12]). With every flag clear the record's path ends in no
    // `0x3F`, so the portal join reads no door off it; [`flag_window_pad`]
    // picks the row that names the hop's destination.
    for t in triggers.iter().filter(|t| t.gate == 1) {
        if doors.iter().any(|d| d.tile == (t.tile_x, t.tile_z))
            || triggers
                .iter()
                .find(|u| (u.tile_x, u.tile_z) == (t.tile_x, t.tile_z))
                .is_some_and(|u| u.record != t.record || u.gate != 1)
        {
            continue;
        }
        if flag_window_destinations(&mf, &man, usize::from(t.record)).contains(dest) {
            doors.push(Door {
                tile: (t.tile_x, t.tile_z),
                entry: None,
            });
        }
    }
    // An FMV hop: the walk-on tiles whose partition-2 record triggers the
    // movie that hands off to `dest`.
    let fmv_doors: Vec<Door> = graph
        .fmv
        .get(&(name.clone(), dest.to_string()))
        .map(|recs| {
            triggers
                .iter()
                .filter(|t| t.gate == 1 && recs.contains(&(2, usize::from(t.record))))
                .map(|t| Door {
                    tile: (t.tile_x, t.tile_z),
                    entry: None,
                })
                .collect()
        })
        .unwrap_or_default();
    if doors.is_empty() && !fmv_doors.is_empty() {
        return Ok(fmv_doors);
    }
    if doors.is_empty() {
        if let Some(recs) = graph.fmv.get(&(name.clone(), dest.to_string())) {
            return Err(format!(
                "{dest} is reached by an FMV hand-off from record(s) {recs:?} (partition, record), none on a walk-on band"
            ));
        }
        let walk_on: BTreeSet<&str> = sites.iter().map(|s| s.scene_name.as_str()).collect();
        let listed = scene_destinations(&mf, &man)
            .iter()
            .any(|d| d.scene_name == dest);
        return Err(format!(
            "no walk-on door to {dest} (walk-on doors lead to {walk_on:?}; {dest} {} in the MAN's 0x3F set - {})",
            if listed { "is" } else { "is not" },
            if listed {
                "reached by a partition-1 / talk / scripted record, not a .MAP band"
            } else {
                "the loaded MAN carrier does not name it"
            }
        ));
    }
    Ok(doors)
}

/// Seat one tile off `tile` and onto it, so the walk-on dispatch sees a
/// genuine tile change.
pub(super) fn step_onto(session: &mut NativeSession, tile: (u8, u8)) {
    session.host.world.set_pad(0);
    // The approach tile must itself be inert: a kind-1 band there spawns its
    // own record, and a kind-0 teleport there arms a warp whose landing, a
    // few dozen frames later, carries the player off the tile under test
    // (the warp in flight also owns the dispatcher, so the step never fires).
    let claimed = claimed_tiles(session);
    let off = [(-1i16, 0i16), (1, 0), (0, -1), (0, 1)]
        .iter()
        .map(|&(dx, dz)| (i16::from(tile.0) + dx, i16::from(tile.1) + dz))
        .filter(|&(x, z)| (0..128).contains(&x) && (0..128).contains(&z))
        .map(|(x, z)| (x as u8, z as u8))
        .find(|t| !claimed.contains(t))
        .unwrap_or(if tile.0 > 0 {
            (tile.0 - 1, tile.1)
        } else {
            (tile.0 + 1, tile.1)
        });
    session.host.world.seat_player_at_tile(off.0, off.1);
    let _ = session.tick();
    session.host.world.seat_player_at_tile(tile.0, tile.1);
}

/// Tiles of the loaded field scene that carry a `.MAP` trigger of either
/// kind (kind-1 record bands, kind-0 intra-scene teleports).
pub(super) fn claimed_tiles(session: &NativeSession) -> HashSet<(u8, u8)> {
    let mut out = HashSet::new();
    if session.host.world.mode != SceneMode::Field {
        return out;
    }
    let index = &session.host.index;
    let Ok(scene) = Scene::load(index, &scene_name(session)) else {
        return out;
    };
    if let Ok((p, f)) = scene.field_tile_triggers(index) {
        out.extend(p.iter().chain(f.iter()).map(|t| (t.tile_x, t.tile_z)));
    }
    if let Ok((p, f)) = scene.field_intra_scene_teleports(index) {
        out.extend(p.iter().chain(f.iter()).map(|t| (t.tile_x, t.tile_z)));
    }
    out
}

/// Try `doors` by seating; `Ok(entered)` for the first that changed scene.
pub(super) fn seated_hop(
    session: &mut NativeSession,
    graph: &DiscGraph,
    dest: &str,
) -> Result<String, String> {
    let doors = match doors_to(session, graph, dest) {
        Ok(d) => d,
        Err(why) => return talk_hop(session, graph, dest).ok_or(why)?,
    };
    HOP_DEST.with(|d| *d.borrow_mut() = Some(dest.to_string()));
    let mut tried = Vec::new();
    for d in doors.iter().take(MAX_SITES) {
        step_onto(session, d.tile);
        let mut r = run(session, EXIT_TICKS, true);
        if matches!(r, Run::Parked(_)) {
            r = run_while_moving(session, DEEP_EXIT_TICKS);
        }
        match r {
            Run::Entered(s) => return Ok(s),
            other => tried.push(format!("{:?}: {other:?}", d.tile)),
        }
    }
    Err(format!(
        "{} door(s) to {dest}, none fired: {}; {}",
        doors.len(),
        tried.join("; "),
        site_gate_report(session, dest)
    ))
}

/// A hop no walk-on band carries, taken by talking: the talk-NPC placements
/// whose own partition-1 record names `dest` in a `0x3F`, directly or through
/// the partition-2 records it spawns (`0x44`, followed a few levels), or
/// spawns the record whose FMV hands off to `dest` (`town01`'s P1[40] spawns
/// P2[25], the mist-night movie). `None` when no such talk exists;
/// `Some(Err)` when talks ran and none left the scene.
pub(super) fn talk_hop(
    session: &mut NativeSession,
    graph: &DiscGraph,
    dest: &str,
) -> Option<Result<String, String>> {
    use legaia_engine_field::man_field_scripts::partition_record_span;
    use legaia_scene_formats::field_disasm::{InsnInfo, LinearWalker, scene_change_name};
    let (mf, man, _) = scene_man_and_triggers(session)?;
    let n0 = mf.partitions.first().map_or(0, Vec::len);
    let n1 = mf.partitions.get(1).map_or(0, Vec::len);
    let fmv: BTreeSet<usize> = graph
        .fmv
        .get(&(scene_name(session), dest.to_string()))
        .map(|r| r.iter().filter(|(p, _)| *p == 2).map(|&(_, r)| r).collect())
        .unwrap_or_default();
    // Does the record (partition, index) reach `dest` within `depth` spawns?
    fn reaches(
        mf: &legaia_scene_formats::man_section::ManFile,
        man: &[u8],
        part: usize,
        rec: usize,
        dest: &str,
        base: usize,
        depth: usize,
    ) -> bool {
        let Some((start, pc0, len)) = partition_record_span(mf, man, part, rec) else {
            return false;
        };
        let body = &man[start..start + len];
        for insn in LinearWalker::new(body, pc0).flatten() {
            match insn.info {
                InsnInfo::SceneChange { .. }
                    if scene_change_name(body, &insn).as_deref() == Some(dest) =>
                {
                    return true;
                }
                InsnInfo::SpawnRecord { global_index } if depth > 0 => {
                    if let Some(r2) = usize::from(global_index).checked_sub(base)
                        && (r2 != rec || part != 2)
                        && reaches(mf, man, 2, r2, dest, base, depth - 1)
                    {
                        return true;
                    }
                }
                _ => {}
            }
        }
        false
    }
    let w = &session.host.world;
    let slots: Vec<u8> = w
        .npcs
        .positions
        .keys()
        .copied()
        .filter(|s| w.npcs.dialog.contains_key(s) || w.npcs.dialog_prologue.contains_key(s))
        .filter(|&s| {
            reaches(&mf, &man, 1, usize::from(s), dest, n0 + n1, 3)
                || spawned_p2(&mf, &man, 1, usize::from(s), n0 + n1, 3)
                    .iter()
                    .any(|r| fmv.contains(r))
        })
        .collect();
    if slots.is_empty() {
        return None;
    }
    let mut tried = Vec::new();
    for slot in slots {
        match talk_to(session, slot) {
            Run::Entered(s) => return Some(Ok(s)),
            other => tried.push(format!("talk P1[{slot}]: {other:?}")),
        }
    }
    Some(Err(format!(
        "no walk-on door to {dest}; talks that lead there did not leave: {}",
        tried.join("; ")
    )))
}

/// The loaded scene's MAN, parsed, and its `.MAP` triggers.
pub(super) fn scene_man_and_triggers(
    session: &NativeSession,
) -> Option<(
    legaia_scene_formats::man_section::ManFile,
    Vec<u8>,
    Vec<TileTrigger>,
)> {
    let name = scene_name(session);
    let index = &session.host.index;
    let scene = Scene::load(index, &name).ok()?;
    let man = scene.field_man_payload(index).ok()??;
    let mf = legaia_scene_formats::man_section::parse(&man).ok()?;
    let (p, f) = scene.field_tile_triggers(index).ok()?;
    Some((mf, man, p.into_iter().chain(f).collect()))
}

/// For every walk-on site whose record names `dest`: the record, its
/// partition-2 C1 / C2 story gates (retail `FUN_8003BDE0`) and which of them
/// the live flag bank fails - the one-level-down reason a door stays shut.
pub(super) fn site_gate_report(session: &NativeSession, dest: &str) -> String {
    use legaia_engine_field::man_field_scripts::partition2_record_gates;
    let Some((mf, man, triggers)) = scene_man_and_triggers(session) else {
        return "scene MAN / triggers unavailable".into();
    };
    let w = &session.host.world;
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    for s in overworld_portal_sites(&mf, &man, &triggers) {
        let names_dest =
            s.scene_name == dest || s.conditional.as_ref().is_some_and(|c| c.scene_name == dest);
        if !names_dest || !seen.insert(s.record) {
            continue;
        }
        let cond = s.conditional.as_ref().map_or(String::new(), |c| {
            format!(
                " (dest switches to {} when 0x{:03X} {})",
                c.scene_name,
                c.flag,
                if w.system_flag_test(c.flag) {
                    "SET"
                } else {
                    "clear"
                }
            )
        });
        match partition2_record_gates(&mf, &man, usize::from(s.record)) {
            Some((c1, c2)) => {
                let bad1: Vec<String> = c1
                    .iter()
                    .filter(|&&f| w.system_flag_test(f))
                    .map(|f| format!("0x{f:03X} set"))
                    .collect();
                let bad2: Vec<String> = c2
                    .iter()
                    .filter(|&&f| !w.system_flag_test(f))
                    .map(|f| format!("0x{f:03X} clear"))
                    .collect();
                let verdict = if bad1.is_empty() && bad2.is_empty() {
                    "gates pass".to_string()
                } else {
                    format!("gates FAIL: {}", [bad1, bad2].concat().join(", "))
                };
                out.push(format!("P2[{}] {verdict}{cond}", s.record));
            }
            None => out.push(format!("P2[{}] gates undecodable{cond}", s.record)),
        }
    }
    if out.is_empty() {
        format!("no walk-on record in this scene names {dest}")
    } else {
        format!("sites: {}", out.join(" | "))
    }
}
