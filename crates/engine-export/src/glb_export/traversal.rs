//! Scene traversal export: floor sampling, spawn / door / trigger boxes and their landings.

use super::*;

/// A floor-height sampler over the scene's `.MAP` collision grid, floor LUT
/// and elevation overrides - the same retail model the play hosts run
/// (`World::sample_field_floor_height`, `FUN_80019278`), so manifest spawn
/// heights match where the engine grounds an actor.
pub struct FloorSampler {
    world: Box<legaia_engine_core::world::World>,
}

impl FloorSampler {
    /// Build from the scene's on-disc geometry; every input is optional (a
    /// missing grid just samples 0, same as retail before the install).
    pub fn build(index: &ProtIndex, scene: &Scene) -> Self {
        let mut world = Box::<legaia_engine_core::world::World>::default();
        if let Ok(Some(grid)) = scene.field_collision_grid(index) {
            world.load_field_collision_grid(&grid);
        }
        if let Some(idx) = scene.field_map_index(index)
            && let Ok(bytes) = index.entry_bytes_extended(idx)
        {
            world.load_field_object_cells(
                bytes
                    .get(legaia_scene_formats::field_objects::OBJECT_GRID_OFFSET..)
                    .unwrap_or_default(),
            );
            world.load_field_elevation_overrides(
                bytes
                    .get(legaia_engine_field::field_regions::MAP_REGION_BLOCK_OFFSET..)
                    .unwrap_or_default(),
                bytes
                    .get(legaia_engine_field::field_regions::MAP_TRIGGER_FALLBACK_OFFSET..)
                    .unwrap_or_default(),
            );
        }
        if let Ok(Some(lut)) = scene.field_floor_height_lut(index) {
            // The MAN header stores POSITIVE tiers; every runtime consumer
            // assumes the negated (PSX Y-down) copy (`FUN_8003AEB0`).
            world.terrain.floor_height_lut = lut.map(|v| v.wrapping_neg());
            world.terrain.floor_height_lut_static = world.terrain.floor_height_lut;
        }
        Self { world }
    }

    /// Floor height at a world (x, z), retail frame (PSX +Y down).
    pub fn height(&self, world_x: i32, world_z: i32) -> i32 {
        self.world.sample_field_floor_height(world_x, world_z)
    }

    /// Retail "the player may stand here": on the authored walk-visible
    /// floor and clear of the collision-grid wall bits - the same pair of
    /// gates the engine's spawn validation composes. Distinguishes street
    /// from the unwalkable inside of an exterior house shell, which pure
    /// mesh analysis cannot.
    pub fn standable(&self, world_x: i16, world_z: i16) -> bool {
        self.world.field_tile_is_walk_visible(world_x, world_z)
            && !self.world.field_tile_is_wall(world_x, world_z)
    }
}

/// Traversal data of one scene, ready for the manifest: the intra-scene
/// teleports a walk-in world needs to make doorways *work*, plus the
/// scene-change portal sites a multi-scene build can wire up.
#[derive(Default)]
pub struct TraversalExport {
    /// Intra-scene teleports (both retail families - see
    /// [`export_scene_traversal`]), as manifest-ready JSON entries.
    pub teleports: Vec<Value>,
    /// Gate-1 walk-on portal sites whose record runs a `0x3F` named scene
    /// change (a town exit / overworld entrance) - useful when several
    /// exported scenes share one world.
    pub portals: Vec<Value>,
}

/// The facing direction of engine heading `h` (12-bit, `0` faces +Z) as a
/// unit XZ vector - unchanged by the export frame's Y re-sign
/// ([`draw_translation`] touches only Y), so it is valid in both.
fn heading_dir_xz(h: i16) -> [f32; 2] {
    let th = (h as f32) / 4096.0 * std::f32::consts::TAU;
    [th.sin(), th.cos()]
}

/// Recover both retail doorway families of a scene as manifest entries (the
/// same disc structures the play hosts dispatch - see
/// `docs/subsystems/field-locomotion.md` § intra-scene doorways):
///
/// - **map doors** (`.MAP` kind-0 intra-scene-teleport table): a plain tile
///   carries a destination; crossing onto it repositions the player. This is
///   where most house *exits* live.
/// - **script doors** (`.MAP` object gate-0 kind-1 binds): touching the
///   object's contact box runs its MAN record, whose taken arm teleports the
///   player channel. The arm is resolved here against the **cold-entry**
///   story-flag state (all clear - the state a fresh walk-in world models);
///   records whose cold arm spawns a cutscene instead of teleporting are
///   skipped.
///
/// Positions are in the export frame (scaled, [`draw_translation`]-signed),
/// with destination heights re-sampled off the floor model exactly as the
/// engine re-seats an arrival. `facing_dir` is the arrival facing as a unit
/// XZ direction (`null` = keep the walked-in facing).
///
/// **Trigger boxes are built for a physical player capsule, not retail's
/// point-player.** Retail dispatches both families on a 2D distance check
/// with no height test, so a literal transcription fails two ways a 3D
/// engine's capsule exposes: a contact tile can sample a different floor
/// *layer* than the approach ground walks on (Rim Elm's cave mouth: contact
/// floor 4 m below the village ground crossing above it), and a recessed
/// door's contact centre can sit deeper into an alcove than a capsule can
/// follow (Vahn's door: the walkable channel is narrower than a player
/// capsule; retail's point slides down it). Hence each trigger's vertical
/// span is the min..max of the floor sampled on rings around the contact
/// (padded a step down / a body height up), and script-door horizontal
/// half-extents carry a capsule allowance over retail's `0x50`.
pub fn export_scene_traversal(
    index: &ProtIndex,
    scene: &Scene,
    floor: &FloorSampler,
    opts: &GlbExportOptions,
    world_glb: Option<&[u8]>,
) -> TraversalExport {
    let s = opts.scale;
    // The baked world's own triangles: what a glTF consumer collides
    // against, used to verify each trigger box is capsule-reachable.
    let soup = world_glb.and_then(GlbTriangles::from_glb);
    let reach_adjust = |pos: &mut [f32; 3], half: &mut [f32; 3], door_y: f32| {
        if let Some(soup) = &soup {
            reach_extend_box(soup, floor, s, door_y, pos, half);
        }
    };
    let door_level = |wx: i16, wz: i16| -> f32 { -(floor.height(wx as i32, wz as i32) as f32) * s };
    let pt = |x: f32, y: f32, z: f32| -> [f32; 3] {
        let t = draw_translation([x, y, z]);
        [t[0] * s, t[1] * s, t[2] * s]
    };
    let floor_pt = |wx: i16, wz: i16| -> [f32; 3] {
        let y = floor.height(wx as i32, wz as i32);
        pt(wx as f32, y as f32, wz as f32)
    };
    // Trigger vertical envelope: sample the floor at the contact and on
    // 8-direction rings out to two tiles, and return `(position, half_y)`
    // such that the box spans `lowest - 0x20 .. highest + 0x90` PSX units
    // (half a step below, a body height above) - so the box overlaps the
    // player wherever the *approach* ground actually is, exactly as
    // retail's height-blind 2D check behaves. Retail frame: +Y is DOWN,
    // so "lowest surface" is the numeric max.
    let floor_box = |wx: i16, wz: i16| -> ([f32; 3], f32) {
        let (cx, cz) = (wx as i32, wz as i32);
        let mut lo = floor.height(cx, cz); // numeric min = highest surface
        let mut hi = lo;
        for r in [64, 128] {
            for (dx, dz) in [
                (0, -1),
                (1, -1),
                (1, 0),
                (1, 1),
                (0, 1),
                (-1, 1),
                (-1, 0),
                (-1, -1),
            ] {
                let h = floor.height(cx + dx * r, cz + dz * r);
                lo = lo.min(h);
                hi = hi.max(h);
            }
        }
        let bottom = hi + 0x20; // retail +Y down: below the lowest floor
        let top = lo - 0x90; // above the highest floor
        let half_y = (bottom - top) as f32 / 2.0 * s;
        (pt(wx as f32, bottom as f32, wz as f32), half_y)
    };
    let mut out = TraversalExport::default();

    // --- Collect both door families first: the boxes are sized in a
    // second pass so every box can be clamped against every OTHER door's
    // landing point (a reach-grown box that swallows a landing would
    // bounce the arriving player straight back).
    // Map doors (kind-0 intra-scene teleports): (trigger world, dest world).
    let mut map_doors: Vec<((i16, i16), (i16, i16))> = Vec::new();
    if let Ok((primary, fallback)) = scene.field_intra_scene_teleports(index) {
        let mut seen: Vec<(u8, u8)> = Vec::new();
        for t in primary.iter().chain(fallback.iter()) {
            if seen.contains(&(t.tile_x, t.tile_z)) {
                continue; // primary block wins, as in the trigger dispatch
            }
            seen.push((t.tile_x, t.tile_z));
            let (tx, tz) = (
                i16::from(t.tile_x) * 128 + 0x40,
                i16::from(t.tile_z) * 128 + 0x40,
            );
            map_doors.push(((tx, tz), t.dest_world()));
        }
    }

    // Script doors (object walk-touch binds): resolve the arm the record
    // takes with every story flag clear - the cold-entry state a fresh
    // world models. A record whose cold arm is a cutscene spawn (or a
    // minigame warp) is not a plain doorway; left out rather than guessed.
    type ScriptDoor = ((i16, i16), (i16, i16), Option<i16>, usize);
    let mut script_doors: Vec<ScriptDoor> = Vec::new();
    let mut portal_sites = Vec::new();
    let map_bytes = scene
        .field_map_index(index)
        .and_then(|i| index.entry_bytes_extended(i).ok());
    let triggers = scene.field_tile_triggers(index).ok().map(|(p, f)| {
        let mut t = p;
        t.extend(f);
        t
    });
    let man = scene.field_man_payload(index).ok().flatten();
    if let (Some(map), Some(triggers), Some(man)) = (map_bytes, triggers, man.as_ref())
        && let Ok(mf) = legaia_scene_formats::man_section::parse(man)
    {
        for bind in legaia_engine_field::man_field_scripts::object_walk_touch_binds(
            &map, &triggers, &mf, man,
        ) {
            let cold = legaia_engine_field::man_field_scripts::resolve_walk_touch_event(
                &mf,
                man,
                bind.record,
                &|_| false,
            );
            let Some(legaia_engine_field::man_field_scripts::WalkTouchEvent::PlayerMoveTo {
                world_x,
                world_z,
                facing,
            }) = cold
            else {
                continue;
            };
            script_doors.push((bind.contact, (world_x, world_z), facing, bind.record));
        }
        portal_sites =
            legaia_engine_field::man_field_scripts::overworld_portal_sites(&mf, man, &triggers);
    }

    // Every landing point, indexed teleport-order (map doors then script
    // doors) so a trigger can skip its own destination.
    let landings: Vec<[f32; 3]> = map_doors
        .iter()
        .map(|&(_, (dx, dz))| floor_pt(dx, dz))
        .chain(
            script_doors
                .iter()
                .map(|&(_, (dx, dz), _, _)| floor_pt(dx, dz)),
        )
        .collect();

    // Box sizing shared by both families: retail footprint + capsule
    // allowance (retail measured a POINT player; a capsule stopped by a
    // mesh doorframe narrower than itself needs the face to come out and
    // meet it), vertical span from the local floor envelope, capsule-reach
    // verified against the baked mesh, then landing-clamped.
    let sized_box = |wx: i16, wz: i16, base_half_units: f32, own: usize| -> ([f32; 3], [f32; 3]) {
        let (mut tpos, thalf_y) = floor_box(wx, wz);
        let mut thalf = [base_half_units * s, thalf_y, base_half_units * s];
        let base = (tpos, thalf);
        reach_adjust(&mut tpos, &mut thalf, door_level(wx, wz));
        clamp_box_from_landings(
            &mut tpos,
            &mut thalf,
            &base.0,
            &base.1,
            &landings,
            Some(own),
        );
        (tpos, thalf)
    };

    for (i, &((tx, tz), (dx, dz))) in map_doors.iter().enumerate() {
        let (tpos, thalf) = sized_box(tx, tz, 88.0, i);
        out.teleports.push(json!({
            "kind": "map",
            "trigger": { "position": tpos, "half_extents": thalf },
            "destination": { "position": floor_pt(dx, dz) },
            "facing_dir": Value::Null,
            "record": Value::Null,
        }));
    }
    for (i, &((cx, cz), (dx, dz), facing, record)) in script_doors.iter().enumerate() {
        let base = (legaia_engine_field::field_regions::MAP_OBJECT_CONTACT_HALF + 48) as f32;
        let (tpos, thalf) = sized_box(cx, cz, base, map_doors.len() + i);
        out.teleports.push(json!({
            "kind": "script",
            "trigger": { "position": tpos, "half_extents": thalf },
            "destination": { "position": floor_pt(dx, dz) },
            "facing_dir": facing.map(heading_dir_xz),
            "record": record,
        }));
    }

    // --- Scene-change portal sites (gate-1 walk-on -> 0x3F). ---
    let entry_half =
        |b: u8| -> f32 { f32::from(b & 0x7F) * 128.0 + if b & 0x80 != 0 { 128.0 } else { 64.0 } };
    for site in &portal_sites {
        let (tx, tz) = (
            i16::from(site.overworld_x) * 128 + 0x40,
            i16::from(site.overworld_z) * 128 + 0x40,
        );
        // Arrival facing: the 0x3F trailing dir byte through the retail
        // compass table (`dir & 7` eighths of the 12-bit circle).
        let facing = heading_dir_xz(i16::from(site.dir & 7) * 0x200);
        let (mut tpos, thalf_y) = floor_box(tx, tz);
        let mut thalf = [88.0 * s, thalf_y, 88.0 * s];
        let base = (tpos, thalf);
        reach_adjust(&mut tpos, &mut thalf, door_level(tx, tz));
        clamp_box_from_landings(&mut tpos, &mut thalf, &base.0, &base.1, &landings, None);
        out.portals.push(json!({
            "target_scene": site.scene_name,
            "trigger": { "position": tpos, "half_extents": thalf },
            // Arrival point in the TARGET scene's export frame (XZ only,
            // scaled; sample the destination scene's floor for Y when
            // wiring a multi-scene world).
            "entry_xz": [entry_half(site.entry_x) * s, entry_half(site.entry_z) * s],
            "facing_dir": facing,
            "record": site.record,
            "conditional": site.conditional.as_ref().map(|c| json!({
                "flag": c.flag,
                "target_scene": c.scene_name,
                "entry_xz": [entry_half(c.entry_x) * s, entry_half(c.entry_z) * s],
            })),
        }));
    }
    out
}

/// Shrink a reach-grown trigger box so no OTHER teleport's landing point
/// (plus a standing-capsule clearance) sits inside it - an arriving player
/// inside a trigger is bounced straight back where they came from. Only
/// the grown margin is ever given up: faces never pull past the retail
/// baseline box, which is verified landing-clear by construction.
fn clamp_box_from_landings(
    pos: &mut [f32; 3],
    half: &mut [f32; 3],
    base_pos: &[f32; 3],
    base_half: &[f32; 3],
    landings: &[[f32; 3]],
    own: Option<usize>,
) {
    const CLEAR: f32 = 0.5;
    for (li, d) in landings.iter().enumerate() {
        if own == Some(li) {
            continue;
        }
        let (x0, x1) = (pos[0] - half[0], pos[0] + half[0]);
        let (z0, z1) = (pos[2] - half[2], pos[2] + half[2]);
        let (y0, y1) = (pos[1], pos[1] + 2.0 * half[1]);
        let overlaps = d[0] > x0 - CLEAR
            && d[0] < x1 + CLEAR
            && d[2] > z0 - CLEAR
            && d[2] < z1 + CLEAR
            && d[1] + 1.7 > y0
            && d[1] < y1;
        if !overlaps {
            continue;
        }
        let (bx0, bx1) = (base_pos[0] - base_half[0], base_pos[0] + base_half[0]);
        let (bz0, bz1) = (base_pos[2] - base_half[2], base_pos[2] + base_half[2]);
        // Four candidate face pulls: (new coord, feasible, cost).
        let cands = [
            (0usize, d[0] + CLEAR, d[0] + CLEAR <= bx0, d[0] + CLEAR - x0),
            (1, d[0] - CLEAR, d[0] - CLEAR >= bx1, x1 - (d[0] - CLEAR)),
            (2, d[2] + CLEAR, d[2] + CLEAR <= bz0, d[2] + CLEAR - z0),
            (3, d[2] - CLEAR, d[2] - CLEAR >= bz1, z1 - (d[2] - CLEAR)),
        ];
        let Some(&(face, coord, _, _)) = cands
            .iter()
            .filter(|&&(_, _, ok, cost)| ok && cost > 0.0)
            .min_by(|a, b| a.3.total_cmp(&b.3))
        else {
            continue; // no feasible pull - leave it (base was clear)
        };
        match face {
            0 => {
                half[0] = (x1 - coord) / 2.0;
                pos[0] = coord + half[0];
            }
            1 => {
                half[0] = (coord - x0) / 2.0;
                pos[0] = x0 + half[0];
            }
            2 => {
                half[2] = (z1 - coord) / 2.0;
                pos[2] = coord + half[2];
            }
            _ => {
                half[2] = (coord - z0) / 2.0;
                pos[2] = z0 + half[2];
            }
        }
    }
}
