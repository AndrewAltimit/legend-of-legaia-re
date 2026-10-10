//! Diagnostics for the rung-4 crossing: the input ablation and the lattice
//! probe.

use super::*;

/// Ablation over the four inputs that can seal the overworld route, printed
/// under `LEGAIA_CPR_ABLATE=1`.
///
/// Rung 4 stalls with the flood unable to reach any `keikoku` mouth, and
/// `plan_path` rejects a step for four independent reasons: the walkability
/// grid ([`World::field_dir_blocked`]), placed-prop boxes and live NPC boxes
/// (both inside [`World::field_actor_dir_blocked`]), and the caller's portal
/// hazard set. A residual measured with all four live cannot say which one is
/// the seal, and a table that reports "walls only" is wrong: `plan_path` in
/// fact consults every one of them - so the ablation is run rather than
/// reasoned about.
///
/// Each row clears exactly one input and re-floods to every `keikoku` mouth.
/// A row whose residual collapses names the seal.
pub(super) fn ablate_rung4_inputs(host: &mut SceneHost, hazards: &HashSet<(i32, i32)>) {
    let from = {
        let (x, z) = player_world(host);
        cell_of(x, z)
    };
    let goals: Vec<(i16, i16)> = host
        .world
        .world_map
        .entity_configs
        .iter()
        .zip(host.world.world_map.entity_positions.iter())
        .filter_map(|(cfg, &(x, z))| match cfg {
            WorldMapEntityConfig::OverworldPortal { scene_name, .. } if scene_name == "keikoku" => {
                Some(tile_of(x, z))
            }
            _ => None,
        })
        .collect();

    let residuals = |host: &SceneHost, avoid: &HashSet<(i32, i32)>| -> Vec<i32> {
        goals
            .iter()
            .map(|&goal| {
                let goal_w = tile_center(goal);
                let goal_c = cell_of(goal_w.0, goal_w.1);
                plan_path(host, from, goal, avoid)
                    .and_then(|p| p.last().copied())
                    .map_or(i32::MAX, |end| {
                        i32::from((end.0 - goal_c.0).abs() + (end.1 - goal_c.1).abs())
                    })
            })
            .collect()
    };

    let none: HashSet<(i32, i32)> = HashSet::new();
    let n_props = host.world.props.colliders.len();
    let n_npcs = host.world.npcs.positions.len();
    eprintln!(
        "[ablate] map01 from {from:?}: {} keikoku mouths, {n_props} props, {n_npcs} npcs, \
         {} hazards",
        goals.len(),
        hazards.len()
    );
    eprintln!("[ablate] mouths {goals:?}");

    let row = |label: &str, host: &SceneHost, avoid: &HashSet<(i32, i32)>| {
        let r = residuals(host, avoid);
        let best = r.iter().copied().min().unwrap_or(i32::MAX);
        eprintln!("[ablate] {label:<28} best {best:>6}  all {r:?}");
    };

    // How big is the pocket the player is actually standing in? A bounded
    // reachable set is the difference between "a terrain barrier the route has
    // to go round" and "the grid decode put the player inside a sealed room".
    let flood_stats = |host: &SceneHost| -> (usize, (i32, i32), (i32, i32)) {
        let mut seen: HashSet<Cell> = HashSet::new();
        let mut queue = VecDeque::new();
        seen.insert(from);
        queue.push_back(from);
        let (mut lo, mut hi) = (from, from);
        while let Some(cur) = queue.pop_front() {
            lo = (lo.0.min(cur.0), lo.1.min(cur.1));
            hi = (hi.0.max(cur.0), hi.1.max(cur.1));
            let (cx, cz) = cell_center(cur);
            for ((dx, dz), dir) in STEPS {
                let next = (cur.0 + dx, cur.1 + dz);
                if next.0 < 0 || next.1 < 0 || seen.contains(&next) {
                    continue;
                }
                if host.world.field_dir_blocked(cx, cz, dir)
                    || host.world.field_actor_dir_blocked(cx, cz, dir)
                {
                    continue;
                }
                seen.insert(next);
                queue.push_back(next);
            }
        }
        (
            seen.len(),
            (i32::from(lo.0), i32::from(lo.1)),
            (i32::from(hi.0), i32::from(hi.1)),
        )
    };
    let reached: HashSet<Cell> = {
        let mut seen: HashSet<Cell> = HashSet::new();
        let mut queue = VecDeque::new();
        seen.insert(from);
        queue.push_back(from);
        while let Some(cur) = queue.pop_front() {
            let (cx, cz) = cell_center(cur);
            for ((dx, dz), dir) in STEPS {
                let next = (cur.0 + dx, cur.1 + dz);
                if next.0 < 0 || next.1 < 0 || seen.contains(&next) {
                    continue;
                }
                if host.world.field_dir_blocked(cx, cz, dir)
                    || host.world.field_actor_dir_blocked(cx, cz, dir)
                {
                    continue;
                }
                seen.insert(next);
                queue.push_back(next);
            }
        }
        seen
    };
    let (n, lo, hi) = flood_stats(host);
    eprintln!(
        "[ablate] reachable pocket: {n} sub-cells, bbox cells {lo:?}..{hi:?} \
         = tiles ({},{})..({},{})",
        lo.0 / 4,
        lo.1 / 4,
        hi.0 / 4,
        hi.1 / 4
    );

    // The wall bits themselves across the pocket's southern edge, at the
    // 64-unit sub-cell granularity the bits are authored in. A boundary that
    // is authored terrain is ragged; one that is an indexing artifact is a
    // straight line.
    eprintln!(
        "[ablate] x tiles 45..110, z tiles 56..72  ('#' wall, 'o' flood-reached, '.' open+unreached):"
    );
    for sz in (56 * 2)..(72 * 2) {
        let zw = sz * 64 + 32;
        let mut line = String::new();
        for sx in (45 * 2)..(110 * 2) {
            let xw = sx * 64 + 32;
            let hit = (0..2).any(|qz| {
                (0..2).any(|qx| reached.contains(&(((sx * 2 + qx) as i16), ((sz * 2 + qz) as i16))))
            });
            line.push(if host.world.field_tile_is_wall(xw as i16, zw as i16) {
                '#'
            } else if hit {
                'o'
            } else {
                '.'
            });
        }
        eprintln!("[ablate] z{:>3}.{} {line}", sz / 2, sz % 2);
    }

    // The x=64 corridor, cell by cell: what the *tile* read says vs what the
    // *directional* probe the planner actually consults says. They are
    // different functions and only the second one gates the flood.
    eprintln!("[ablate] x=64 corridor, tile-read vs dir-probe (Z+ = south):");
    for sz in (62 * 4)..(70 * 4) {
        let zw = sz * 32 + 16;
        let xw = 64 * 128 + 64; // tile centre
        let wall = host.world.field_tile_is_wall(xw as i16, zw as i16);
        let dir_z = host.world.field_dir_blocked(xw as i16, zw as i16, 2);
        let act_z = host.world.field_actor_dir_blocked(xw as i16, zw as i16, 2);
        eprintln!(
            "[ablate]   z {:>3}.{}  world({xw},{zw})  tile_is_wall {:<5}  dir_blocked(Z+) {:<5}  actor {}",
            sz / 4,
            sz % 4,
            wall,
            dir_z,
            act_z
        );
    }

    // Where does the *directional* probe disagree with the *tile* read?
    // Retail's own grid for this scene is byte-identical to the port's decode
    // (read offline from a `map01` save state), and retail parks the player
    // inside the x=64 corridor this flood never reaches - so the seal is not
    // the data. Every frontier cell whose neighbour is an open tile the flood
    // refused to enter is a place `field_dir_blocked` is stricter than the
    // walls it is derived from.
    let mut disagreements: Vec<(Cell, usize)> = Vec::new();
    for &cur in &reached {
        let (cx, cz) = cell_center(cur);
        for ((dx, dz), dir) in STEPS {
            let next = (cur.0 + dx, cur.1 + dz);
            if next.0 < 0 || next.1 < 0 || reached.contains(&next) {
                continue;
            }
            let (nx, nz) = cell_center(next);
            if host.world.field_tile_is_wall(nx, nz) {
                continue; // genuinely walled - not a disagreement
            }
            if host.world.field_actor_dir_blocked(cx, cz, dir) {
                continue; // a prop/NPC, already ablated above
            }
            disagreements.push((cur, dir));
        }
    }
    eprintln!(
        "[ablate] probe-vs-tile disagreements on the flood frontier: {}",
        disagreements.len()
    );
    if let (Some(lo_x), Some(hi_x), Some(lo_z), Some(hi_z)) = (
        disagreements.iter().map(|(c, _)| c.0).min(),
        disagreements.iter().map(|(c, _)| c.0).max(),
        disagreements.iter().map(|(c, _)| c.1).min(),
        disagreements.iter().map(|(c, _)| c.1).max(),
    ) {
        eprintln!(
            "[ablate]   spread: cells x {lo_x}..{hi_x} z {lo_z}..{hi_z} \
             = tiles ({},{})..({},{})",
            lo_x / 4,
            lo_z / 4,
            hi_x / 4,
            hi_z / 4
        );
        for (c, dir) in disagreements.iter().take(8) {
            let (wx, wz) = cell_center(*c);
            eprintln!(
                "[ablate]   at cell {c:?} tile ({},{}) world ({wx},{wz}) dir {dir}",
                c.0 / 4,
                c.1 / 4
            );
        }
    }

    // Is the probe *asymmetric*? It samples points ahead of the source, so
    // `A -> B` can block while `B -> A` is clear, and a BFS from the arrival
    // then cannot enter a region a BFS from the destination would leave. Flood
    // backward from the tile retail actually parks the player on
    // (`map01` save state, world (8266, 8700) = tile (64, 67)) and see whether
    // it reaches the arrival the forward flood started from.
    let retail_cell = cell_of(8266, 8700);
    let back: HashSet<Cell> = {
        let mut seen: HashSet<Cell> = HashSet::new();
        let mut queue = VecDeque::new();
        seen.insert(retail_cell);
        queue.push_back(retail_cell);
        while let Some(cur) = queue.pop_front() {
            let (cx, cz) = cell_center(cur);
            for ((dx, dz), dir) in STEPS {
                let next = (cur.0 + dx, cur.1 + dz);
                if next.0 < 0 || next.1 < 0 || seen.contains(&next) {
                    continue;
                }
                if host.world.field_dir_blocked(cx, cz, dir)
                    || host.world.field_actor_dir_blocked(cx, cz, dir)
                {
                    continue;
                }
                seen.insert(next);
                queue.push_back(next);
            }
        }
        seen
    };
    eprintln!(
        "[ablate] backward flood from retail's tile (64,67): {} sub-cells; \
         reaches the arrival cell {:?}: {}; forward flood reached retail's cell: {}",
        back.len(),
        from,
        back.contains(&from),
        reached.contains(&retail_cell)
    );

    row("as-is (all inputs live)", host, hazards);
    row("hazards cleared", host, &none);

    let props = std::mem::take(&mut host.world.props.colliders);
    row("props cleared", host, hazards);
    row("props+hazards cleared", host, &none);

    let npcs = std::mem::take(&mut host.world.npcs.positions);
    row("props+npcs+hazards (walls)", host, &none);

    host.world.npcs.positions = npcs;
    host.world.props.colliders = props;
    row("restored (sanity, == as-is)", host, hazards);
}

/// Lattice-pitch sweep + portal-reachability report for the rung-4 flood,
/// printed under `LEGAIA_CPR_LATTICE=1`.
///
/// [`ablate_rung4_inputs`] eliminates the flood's *inputs* (grid, props, NPCs,
/// hazards) one at a time; this eliminates the flood's *discretisation*, then
/// says which overworld portals the surviving component actually contains.
///
/// **Pitch.** The planner walks a [`SUBCELL`]-pitch lattice while retail's
/// locomotion is continuous in 2-unit sub-steps - the world-map-walk copy of
/// `FUN_801d01b0` increments the actor's `+0x14`/`+0x18` by 2 and re-runs
/// `FUN_801cfe4c` each time (`ghidra/scripts/funcs/overlay_world_map_walk_801d01b0.txt`).
/// A lattice can only ever *under*-report reachability against that, because
/// [`World::field_dir_blocked`]'s three probe points spread +/-16 units
/// laterally and which sub-cells that footprint straddles depends on the
/// position's phase modulo 64. Pitch 2 is therefore the ceiling: a node set
/// closed under 2-unit cardinal steps is exactly the set of positions the
/// retail stepper can reach, and nothing coarser can beat it.
///
/// The measured answer is that the pitch buys nothing - the reachable area is
/// flat from 32 down to 2 and never contains a `keikoku` mouth - so the seal
/// is not the lattice. What the portal listing then shows is that the
/// component *does* contain `suimon`, which is the scene that crosses to the
/// component the `keikoku` mouths are on. See
/// [`docs/subsystems/world-map.md`].
///
/// Walls only: props / NPCs are already falsified by the ablation, and the
/// per-node actor test is a linear scan the fine pitches cannot afford.
pub(super) fn probe_rung4_lattice(host: &SceneHost) {
    /// World extent of the collision grid: 128 tiles x 128 units.
    const SPAN: i32 = 128 * 128;
    /// Pitches to sweep, coarsest first; the last is the retail step unit.
    const PITCHES: [i32; 5] = [32, 16, 8, 4, 2];
    /// A `map01` save state parks retail's own player here, just outside the
    /// `keikoku` entrance - a ground-truth standable point on the far side.
    const RETAIL_STAND: (i16, i16) = (8266, 8700);

    let (px, pz) = player_world(host);
    let portals: Vec<(String, (i16, i16))> = host
        .world
        .world_map
        .entity_configs
        .iter()
        .zip(host.world.world_map.entity_positions.iter())
        .filter_map(|(cfg, &(x, z))| match cfg {
            WorldMapEntityConfig::OverworldPortal { scene_name, .. } => {
                Some((scene_name.clone(), (x, z)))
            }
            _ => None,
        })
        .collect();
    eprintln!(
        "[lattice] arrival world ({px},{pz}); retail stand {RETAIL_STAND:?}; \
         {} overworld portals",
        portals.len()
    );

    for pitch in PITCHES {
        let stride = (SPAN / pitch) as usize;
        let snap = |w: i16| -> i32 { (i32::from(w) + pitch / 2).div_euclid(pitch) };
        let inside = |nx: i32, nz: i32| -> bool {
            nx >= 0 && nz >= 0 && (nx as usize) < stride && (nz as usize) < stride
        };
        let at = |nx: i32, nz: i32| -> usize { nz as usize * stride + nx as usize };

        let (sx, sz) = (snap(px), snap(pz));
        if !inside(sx, sz) {
            eprintln!("[lattice] pitch {pitch:>3}: the arrival is off the lattice");
            continue;
        }
        let mut seen = vec![false; stride * stride];
        let mut queue = VecDeque::new();
        seen[at(sx, sz)] = true;
        queue.push_back((sx, sz));
        let mut nodes = 0u64;
        while let Some((cx, cz)) = queue.pop_front() {
            nodes += 1;
            let (wx, wz) = ((cx * pitch) as i16, (cz * pitch) as i16);
            for ((dx, dz), dir) in STEPS {
                let (nx, nz) = (cx + i32::from(dx), cz + i32::from(dz));
                if !inside(nx, nz) || seen[at(nx, nz)] {
                    continue;
                }
                if host.world.field_dir_blocked(wx, wz, dir) {
                    continue;
                }
                seen[at(nx, nz)] = true;
                queue.push_back((nx, nz));
            }
        }

        // Reached "near" = within 128 world units, so a portal tile that is
        // itself a wall (a door reads as one) still reports as arrived-at.
        let near = |wx: i16, wz: i16| -> bool {
            let (gx, gz) = (snap(wx), snap(wz));
            let span = 128 / pitch;
            (-span..=span).any(|dz| {
                (-span..=span).any(|dx| inside(gx + dx, gz + dz) && seen[at(gx + dx, gz + dz)])
            })
        };
        // Node count as tile-equivalents, so the pitches compare directly.
        let tiles = nodes * (pitch as u64) * (pitch as u64) / (128 * 128);
        let (rx, rz) = (snap(RETAIL_STAND.0), snap(RETAIL_STAND.1));
        eprintln!(
            "[lattice] pitch {pitch:>3}: {nodes:>9} nodes = {tiles:>5} tile-equivalents; \
             reaches retail's stand: {}",
            inside(rx, rz) && seen[at(rx, rz)]
        );
        if pitch == *PITCHES.last().expect("non-empty") {
            for (scene, (wx, wz)) in &portals {
                eprintln!(
                    "[lattice]   portal -> {scene:<10} world ({wx:>5},{wz:>5}) \
                     tile {:?}  reached: {}",
                    tile_of(*wx, *wz),
                    near(*wx, *wz)
                );
            }
        }
    }
}
