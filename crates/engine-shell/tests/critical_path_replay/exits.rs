//! Finding the way out of a scene: the portal tile toward a destination,
//! the hazards around it, and the scene's exit records.

use super::*;

/// Tile of the overworld portal to `dest` the player can actually **walk to**.
///
/// Two things make "the first row naming that scene" the wrong answer, and the
/// second one is not obvious:
///
/// 1. `world_map_entity_positions` is in **world** units; [`walk_to`] navigates
///    in tiles. Handing the raw world pair straight through overflows
///    `tile_center`'s `t * 128` on an `i16` and aims the follower at a wrapped
///    coordinate off the map - which, because map01's collision leaves the sea
///    open, it then walks to without ever hitting a wall.
/// 2. **A scene has several overworld doors and they are not interchangeable.**
///    `map01` carries six `keikoku` portals - the Ravine is a corridor with
///    entrances on both sides of the continent. From Rim Elm's exit the
///    table's *first* one is on the far side of terrain the walk cannot cross,
///    and the follower's best-effort push toward it runs it over a **different
///    scene's** portal on the way (`suimon`), which auto-engages and ends the
///    leg in the wrong place. That reads as a navigation failure and is really
///    a goal-selection failure.
///
/// So the goal is chosen by flooding the walkability grid once per candidate
/// and keeping the one the flood gets closest to - the same
/// [`plan_path`] the follower will use, so "reachable" means reachable to the
/// thing that has to do the walking, not to a straight line.
pub(super) fn portal_tile(
    host: &SceneHost,
    dest: &str,
    avoid: &HashSet<(i32, i32)>,
) -> Option<(i16, i16)> {
    let from = {
        let (x, z) = player_world(host);
        cell_of(x, z)
    };
    host.world
        .world_map
        .entity_configs
        .iter()
        .zip(host.world.world_map.entity_positions.iter())
        .filter_map(|(cfg, &(x, z))| match cfg {
            WorldMapEntityConfig::OverworldPortal { scene_name, .. } if scene_name == dest => {
                Some(tile_of(x, z))
            }
            _ => None,
        })
        .map(|goal| {
            let goal_w = tile_center(goal);
            let goal_c = cell_of(goal_w.0, goal_w.1);
            // Residual = how close the flood gets. `None` (nothing reachable
            // at all) scores worse than any route.
            let residual = plan_path(host, from, goal, avoid)
                .and_then(|p| p.last().copied())
                .map_or(i32::MAX, |end| {
                    i32::from((end.0 - goal_c.0).abs() + (end.1 - goal_c.1).abs())
                });
            if std::env::var_os("LEGAIA_CPR_DEBUG").is_some() {
                eprintln!("[dbg] candidate {dest} @ {goal:?} residual {residual}");
            }
            (residual, goal)
        })
        .min_by_key(|&(residual, _)| residual)
        .map(|(_, goal)| goal)
}

/// Dispatch tiles of every overworld portal that is **not** a door to `dest`.
///
/// These are hazards, not obstacles in the collision sense: the grid says they
/// are walkable and the engine agrees, but stepping on one ends the leg in
/// another scene. The route has to go round them, so they are handed to
/// [`plan_path`] as blocked cells. Keyed in the dispatch frame (`world >> 7`)
/// because that is the frame `World::auto_engage_world_map_portals` compares
/// in - matching it in the planner's `(world - 0x40) >> 7` frame would leave a
/// half-tile band of the hazard open.
pub(super) fn portal_hazards(host: &SceneHost, dest: &str) -> HashSet<(i32, i32)> {
    host.world
        .world_map
        .entity_configs
        .iter()
        .zip(host.world.world_map.entity_positions.iter())
        .filter_map(|(cfg, &(x, z))| match cfg {
            WorldMapEntityConfig::OverworldPortal { scene_name, .. } if scene_name != dest => {
                Some(dispatch_tile(x, z))
            }
            _ => None,
        })
        .collect()
}

/// One of the current field scene's **scene-change records**: a partition-2
/// record whose script carries a `0x3F`, together with every `.MAP` gate-1
/// trigger tile that spawns it and the arrival tile it lands on.
///
/// A field scene has no entity portals - the overworld's [`portal_tile`] has
/// nothing to read here. Its doors are `.MAP` kind-1 gate-1 trigger bands
/// joined to their partition-2 records, which is exactly the join
/// `man_field_scripts::overworld_portal_sites` performs for the overworld, so
/// this asks the disc where the scene's doors are rather than probing tiles.
pub(super) struct ExitRecord {
    /// Partition-2 record index.
    pub(super) record: u8,
    /// Destination CDNAME scene label from the record's `0x3F`.
    pub(super) dest: String,
    /// Arrival tile at the destination - the coordinate that tells two doors
    /// of the same scene apart, since every `keikoku` door returns to `map01`.
    pub(super) dest_tile: (i16, i16),
    /// Every trigger tile that spawns this record.
    pub(super) tiles: Vec<(i16, i16)>,
}

impl ExitRecord {
    /// Manhattan tile distance from `t` to the nearest tile of this band.
    pub(super) fn near(&self, t: (i16, i16)) -> i32 {
        self.tiles
            .iter()
            .map(|&(tx, tz)| ((tx - t.0).abs() + (tz - t.1).abs()) as i32)
            .min()
            .unwrap_or(i32::MAX)
    }

    /// This band as a [`plan_path`] `avoid` set.
    pub(super) fn hazard(&self) -> HashSet<(i32, i32)> {
        self.tiles
            .iter()
            .map(|&(tx, tz)| (i32::from(tx), i32::from(tz)))
            .collect()
    }
}

/// Every scene-change record of the loaded field scene, grouped by record.
///
/// Grouping by **record** rather than by tile is what makes a dungeon leg
/// scoreable. `keikoku` is one connected canyon with four such records - 0, 1,
/// 2, 3, returning to `map01` tiles `(52, 94)`, `(64, 67)`, `(77, 68)` and
/// `(82, 83)` - plus 31 further gate-1 tiles that are story *beats* and change
/// no scene at all. A tile-keyed view cannot tell those three populations
/// apart, so a leg aimed at "the nearest trigger tile 12+ away" routinely aims
/// at a beat band while treating the corridor onward as an obstacle.
pub(super) fn exit_records(host: &SceneHost) -> Vec<ExitRecord> {
    let Some(scene) = host.scene.as_ref() else {
        return Vec::new();
    };
    let Ok(Some(man)) = scene.field_man_payload(&host.index) else {
        return Vec::new();
    };
    let Ok(mf) = legaia_scene_formats::man_section::parse(&man) else {
        return Vec::new();
    };
    let Ok((primary, fallback)) = scene.field_tile_triggers(&host.index) else {
        return Vec::new();
    };
    let mut triggers = primary;
    triggers.extend(fallback);
    let mut by_record: HashMap<u8, ExitRecord> = HashMap::new();
    for site in legaia_engine_field::man_field_scripts::overworld_portal_sites(&mf, &man, &triggers)
    {
        by_record
            .entry(site.record)
            .or_insert_with(|| ExitRecord {
                record: site.record,
                dest: site.scene_name.clone(),
                dest_tile: (i16::from(site.entry_x), i16::from(site.entry_z)),
                tiles: Vec::new(),
            })
            .tiles
            .push((i16::from(site.overworld_x), i16::from(site.overworld_z)));
    }
    let mut out: Vec<ExitRecord> = by_record.into_values().collect();
    out.sort_by_key(|r| r.record);
    out
}
