//! The run itself: the rungs of the chapter-1 route, in order, each scored
//! by what the pad walker reached.

use super::*;

// ---------------------------------------------------------------------------
// The run
// ---------------------------------------------------------------------------

/// One rung's verdict, for the printed table.
pub(super) struct Rung {
    pub(super) label: &'static str,
    pub(super) detail: String,
    pub(super) cleared: bool,
}

/// Drive the ladder, returning the rungs attempted. Stops at the first
/// failure - later rungs depend on earlier ones having landed.
pub(super) fn run_ladder(host: &mut SceneHost) -> Vec<Rung> {
    let mut rungs = Vec::new();
    let fight = FightPolicy::default();

    // The fighting model. Without this the live loop auto-commits a two-swing
    // basic attack for every party turn and nothing else - see
    // [`drain_battle`] - so the ladder's fighter could not choose a command,
    // could not use an item, and could not run. `SceneHost::open_extracted`
    // is the bare scene host, not `NativeSession`, so the catalogs the play
    // hosts install (`NativeSession::enter_field_live`) have to be installed
    // here too: **an empty item catalog makes every bag row inadmissible**,
    // which is a silent way for a healing policy to do nothing at all.
    if fight.driven {
        host.world.battle.player_driven = true;
        // Arm the **field** side of the loop as well, which is what both play
        // hosts do (`NativeSession::enter_field_live`) and what makes a dungeon
        // roll its own encounters. It is inert on this route today - see the
        // `keikoku encounter model` line below - and arming it anyway means
        // the day the region decode is fixed, rung 5 starts fighting in the
        // Ravine instead of quietly continuing not to. Set directly rather
        // than through `arm_live_loop`, whose `encounter.is_none()` arm would
        // install the *fabricated* vanilla formation table over the scene's
        // own.
        host.world.toggles.live_gameplay_loop = true;
        host.world
            .set_item_catalog(legaia_engine_menus::items::ItemCatalog::vanilla());
        // Leave Rim Elm with the purse spent on healing. See [`BAG_HEAL_COUNT`]
        // for why five, and why this is the model's one non-pad input.
        if fight.bag {
            host.world
                .party
                .inventory
                .insert(BAG_HEAL_ITEM, BAG_HEAL_COUNT);
            host.world.party.money -= BAG_HEAL_PRICE * i32::from(BAG_HEAL_COUNT);
        }
    }

    // Run the collision model **both shipped hosts run**. `play-window`
    // (`window/run.rs`) and the browser play page (`web-play/runtime.rs`)
    // each set this pair; a bare `World` defaults both off, so a ladder that
    // left them alone would be scoring a third model no player ever meets.
    //
    // It is also what makes the planner honest. `plan_path` certifies its
    // edges with `field_dir_blocked`, and its doc calls that "the same probe
    // the locomotion step runs" - true only under `leading_edge_wall_probes`.
    // With the flag off, `advance_with_collision` tests the *candidate centre*
    // (`field_tile_is_wall`) instead, so planner and mover disagree about
    // where the walls are, and the disagreement surfaces as a stall whose site
    // reports "walls none" while the player does not move.
    //
    // `solid_field_npcs` only became survivable once `plan_path` started
    // consulting `field_actor_dir_blocked` as well: with solid NPCs and a
    // wall-only planner, a Rim Elm townsperson parked in the route stalls
    // rung 2 at tile `(25, 22)`. The two edits belong together.
    host.world.locomotion.leading_edge_wall_probes = true;
    host.world.npcs.solid = true;

    // Rim Elm's south exit is **story-locked**, and correctly so - but the
    // lock is the gate's own collision, not a script gate on the `0x3F`. The
    // exit record P2[0] (tiles (24..26, 46)) has empty C1/C2; what stops a
    // cold boot leaving is grid row 47, which `P0[20]` only cuts through on
    // its `327`+`321` arm. See `GATE_OPEN_FLAGS`.
    for flag in GATE_OPEN_FLAGS {
        host.world.system_flag_set(flag);
    }
    // The overworld's own story gates. See `MIST_WALL_FLAG` (the Drake mist
    // walls) and `WATER_GATE_FLAG` (which `suimon` chamber the crossing lands
    // in - without it the crossing is a dead end by design).
    host.world.system_flag_set(MIST_WALL_FLAG);
    host.world.system_flag_set(WATER_GATE_FLAG);

    // --- 1. Cold boot into Rim Elm free-roam with control released. -------
    host.enter_field_scene("town01", 0).expect("enter town01");
    let field = host.world.mode == SceneMode::Field;
    let held = if field {
        held_frames_before_input(host)
    } else {
        None
    };
    let released = held.is_some();
    // The rung's one *positive* fact about the load. "Input is free" is a
    // statement about what is NOT happening, and a scene that loaded nothing
    // at all satisfies it perfectly - which is how this rung came to be read
    // as "the opening played" when the opening had not. So also require that
    // the scene-entry system script (`P1[0]`, retail ctx `0xFB`) is resident
    // and executed: it is what raises the scene's story flags, cues its BGM,
    // arms the arrival fade, and drives the per-frame camera-zone tests, and
    // if it silently stopped running every one of those would go with it
    // while every rung below still passed.
    //
    // A few neutral frames, because the entry script is stepped by the frame
    // tick and `held_frames_before_input` returns without ticking at all when
    // nothing holds input.
    let entry_pc0 = host.world.field_pc;
    let mut entry_script_ran = false;
    for _ in 0..4 {
        host.world.set_pad(0);
        let _ = host.tick();
        entry_script_ran |=
            !host.world.field_bytecode.is_empty() && host.world.field_pc != entry_pc0;
    }
    rungs.push(Rung {
        label: "town01 loads into free-roam Field, entry script running",
        detail: if let Some(held) = held {
            let (x, z) = player_world(host);
            format!(
                "player at tile {:?}; entry script pc {entry_pc0} -> {}; the opening \
                 held input for {held} frame(s){}",
                tile_of(x, z),
                host.world.field_pc,
                if held == 0 {
                    " - i.e. no opening choreography ran. That is correct for this \
                     door: Rim Elm's opening is partition-2 record 3, gated on system \
                     flag 0x225 and installed only on the New Game hand-off, and this \
                     ladder enters as a returning player does"
                } else {
                    ""
                }
            )
        } else if field {
            "control never released (cutscene timeline or dialogue held input)".into()
        } else {
            format!("scene mode is {:?}, expected Field", host.world.mode)
        },
        cleared: released && entry_script_ran,
    });
    if !(released && entry_script_ran) {
        return rungs;
    }

    // --- 2. Walk out of the south gate onto the overworld. ----------------
    let goal = (
        i16::from(TOWN01_SOUTH_GATE.0),
        i16::from(TOWN01_SOUTH_GATE.1),
    );
    // No hazards in a town: a field scene's exits are walk-on trigger bands,
    // not entity portals, and the only one on the route is the goal.
    let leg = walk_to(
        host,
        goal,
        &HashSet::new(),
        &mut LegStats::default(),
        &fight,
    );
    let cleared = matches!(&leg, Leg::Transitioned(n) if n == "map01");
    rungs.push(Rung {
        label: "pad-walk town01 south gate -> map01",
        detail: format!("{leg}"),
        cleared,
    });
    if !cleared {
        return rungs;
    }

    // --- 3. Walk the overworld toward the Ravine. -------------------------
    //
    // Three legs, not one - see [`RAVINE_CROSSING`]. Leg A crosses the
    // northern component to the `suimon` door, leg B walks `suimon` to its
    // southern door, leg C crosses the southern component to a `keikoku`
    // mouth. Rung 3 is scored on leg A, which is the long overworld walk.
    let arrival = {
        let (x, z) = player_world(host);
        tile_of(x, z)
    };
    if std::env::var_os("LEGAIA_CPR_ABLATE").is_some() {
        let hazards = portal_hazards(host, "keikoku");
        ablate_rung4_inputs(host, &hazards);
    }
    if std::env::var_os("LEGAIA_CPR_LATTICE").is_some() {
        probe_rung4_lattice(host);
    }

    let a_hazards = portal_hazards(host, "suimon");
    let a_goal = portal_tile(host, "suimon", &a_hazards);
    let mut stats = LegStats::default();
    let leg_a = a_goal.map(|goal| walk_to(host, goal, &a_hazards, &mut stats, &fight));

    // --- 3. …and the crossing itself, which is leg A. ---------------------
    let crossed = stats.fought > 0 && stats.reach >= OVERWORLD_CROSSING_TILES;
    rungs.push(Rung {
        label: "pad-walk map01 across the continent",
        detail: match &leg_a {
            None => "no suimon overworld portal on map01".to_string(),
            Some(_) => format!(
                "reached {} tiles from the {arrival:?} arrival, {} random encounter(s) fought \
                 (needs {OVERWORLD_CROSSING_TILES} and 1)",
                stats.reach, stats.fought
            ),
        },
        cleared: crossed,
    });
    if !crossed {
        return rungs;
    }

    // --- 4. Through `suimon` and on to a Ravine mouth. --------------------
    let mut trail = vec![format!("map01 -> suimon: {}", fmt_leg(leg_a.as_ref()))];
    let mut arrived = matches!(&leg_a, Some(Leg::Transitioned(n)) if n == "suimon");
    if arrived {
        if std::env::var_os("LEGAIA_CPR_DEBUG").is_some() {
            let (x, z) = player_world(host);
            eprintln!(
                "[dbg] suimon entry world ({x},{z}) tile {:?} dispatch {:?}",
                tile_of(x, z),
                dispatch_tile(x, z)
            );
        }
        let leg_b = walk_to(
            host,
            SUIMON_SOUTH_EXIT,
            &suimon_north_doors(),
            &mut LegStats::default(),
            &fight,
        );
        if std::env::var_os("LEGAIA_CPR_DEBUG").is_some() {
            let (x, z) = player_world(host);
            eprintln!(
                "[dbg] after leg B: {leg_b} at world ({x},{z}) dispatch {:?}",
                dispatch_tile(x, z)
            );
        }
        trail.push(format!("suimon -> map01: {leg_b}"));
        arrived = matches!(&leg_b, Leg::Transitioned(n) if n == "map01");
    }
    if arrived {
        let c_hazards = portal_hazards(host, "keikoku");
        let leg_c = portal_tile(host, "keikoku", &c_hazards)
            .map(|goal| walk_to(host, goal, &c_hazards, &mut LegStats::default(), &fight));
        trail.push(format!("map01 -> keikoku: {}", fmt_leg(leg_c.as_ref())));
        arrived = matches!(&leg_c, Some(Leg::Transitioned(n)) if n == "keikoku");
    }
    rungs.push(Rung {
        label: "pad-walk map01 -> suimon -> map01 -> keikoku (Ravine)",
        detail: trail.join(" | "),
        cleared: arrived,
    });
    if !arrived {
        return rungs;
    }

    // --- 5. Walk the Ravine itself, entrance to far exit. -----------------
    //
    // Every rung up to here ends the moment a door fires; none of them walks a
    // *dungeon interior*, and that is a different surface. A dungeon's exits
    // are `.MAP` walk-on bands rather than overworld entity portals, its
    // encounter table is the scene's own rather than the overworld region
    // set, and its corridors are the narrow geometry the leading-edge probe is
    // hardest on. Under the four-rung ladder "the Ravine loads" and "the Ravine
    // can be walked" were the same score.
    //
    // Success is leaving by a **different scene-change record** than the one
    // the player arrived through, after walking at least
    // [`DUNGEON_TRAVERSE_TILES`]. See [`ExitRecord`] for why the unit is a
    // record and not a tile, and [`DUNGEON_TRAVERSE_TILES`] for why a bare
    // `Transitioned` event is not enough on a scene whose every door returns
    // to `map01`.
    let entry_tile = {
        let (x, z) = player_world(host);
        tile_of(x, z)
    };
    // Why rung 5 fights nothing, printed where a reader will meet the claim.
    //
    // The rung's own header says its encounters "come from the scene's own
    // table rather than the overworld region set". They do not - not because
    // the loop is unarmed (it is, above) but because `keikoku`'s MAN encounter
    // section decodes to exactly ONE region: the whole map (`x 0..128`,
    // `z 0..128`) at `rate 0` with a one-row formation range. `any_rollable`
    // therefore answers `false` and no step ever rolls, while the same MAN
    // registers 37 formation rows that nothing can reach. A 128x128 AABB with
    // a zero rate does not read like authored dungeon data; the suspect is
    // `man_section::region_records` finding a header or terminator row rather
    // than the region array. Until that is settled, rung 5 measures dungeon
    // *locomotion* and its exit bands, and nothing about its encounters.
    if std::env::var_os("LEGAIA_CPR_FIGHT").is_some() {
        eprintln!(
            "[fight] keikoku encounter model: live_loop {} rollable {} region_tracker {} \
             session {} formations {:?}",
            host.world.toggles.live_gameplay_loop,
            host.world.encounters.scene_rollable,
            host.world.terrain.region_tracker.is_some(),
            host.world.encounters.session.is_some(),
            host.world.registered_formation_ids(),
        );
        if let Some(t) = host.world.terrain.region_tracker.as_ref() {
            for r in &t.table().regions {
                eprintln!(
                    "[fight]   region x{}..{} z{}..{} rate {} formations {}..+{}",
                    r.tile_x_min,
                    r.tile_x_max,
                    r.tile_z_min,
                    r.tile_z_max,
                    r.rate_increment,
                    r.formation_base,
                    r.formation_count
                );
            }
        }
    }
    let records = exit_records(host);
    // The door the player came in through is the record whose band the arrival
    // is standing on the approach to. On `keikoku` that is unambiguous by a
    // factor of twenty: the arriving chamber's own record is 2 tiles away and
    // the next-nearest is 41.
    let arrival_rec = records
        .iter()
        .min_by_key(|r| r.near(entry_tile))
        .map(|r| r.record);
    let hazards5 = arrival_rec
        .and_then(|rec| records.iter().find(|r| r.record == rec))
        .map(ExitRecord::hazard)
        .unwrap_or_default();
    // Rank the *other* records' band tiles by the residual `plan_path` leaves,
    // the same measure `portal_tile` picks an overworld door with - a band
    // across a wall is a band the follower cannot use, however close it looks.
    let from5 = {
        let (x, z) = player_world(host);
        cell_of(x, z)
    };
    // Non-vacuity control (`LEGAIA_CPR_RUNG5_BACKOUT=1`): aim the leg at the
    // arrival record's **own** band and hazard nothing, i.e. walk straight
    // back out the door the player came in through. That reproduces the exact
    // observable the first draft of this rung passed on - a clean
    // `Transitioned("map01")` a couple of tiles into the scene - so a run
    // under this flag must report the rung **unclear**. If it ever clears, the
    // rung is measuring the event again instead of the traverse.
    let backout = std::env::var_os("LEGAIA_CPR_RUNG5_BACKOUT").is_some();
    let hazards5 = if backout { HashSet::new() } else { hazards5 };
    let mut candidates: Vec<(u8, (i16, i16), i32)> = records
        .iter()
        .filter(|r| (Some(r.record) == arrival_rec) == backout)
        .filter_map(|r| {
            let mut tiles = r.tiles.clone();
            tiles.sort_by_key(|&(tx, tz)| {
                ((tx - entry_tile.0).abs() + (tz - entry_tile.1).abs()) as i32
            });
            tiles.truncate(MAX_EXIT_CANDIDATES);
            tiles
                .into_iter()
                .map(|t| {
                    let (wx, wz) = tile_center(t);
                    let goal_c = cell_of(wx, wz);
                    let residual = plan_path(host, from5, t, &hazards5)
                        .and_then(|p| p.last().copied())
                        .map_or(i32::MAX, |end| {
                            i32::from((end.0 - goal_c.0).abs() + (end.1 - goal_c.1).abs())
                        });
                    (r.record, t, residual)
                })
                .min_by_key(|&(_, _, residual)| residual)
        })
        .collect();
    candidates.sort_by_key(|&(_, _, residual)| residual);
    if std::env::var_os("LEGAIA_CPR_DEBUG").is_some() {
        for r in &records {
            eprintln!(
                "[dbg] keikoku exit record {} -> {} @ {:?}: {} band tile(s), nearest {} tiles",
                r.record,
                r.dest,
                r.dest_tile,
                r.tiles.len(),
                r.near(entry_tile)
            );
        }
        eprintln!(
            "[dbg] keikoku entry tile {entry_tile:?}, arrival record {arrival_rec:?}, \
             candidates {candidates:?}"
        );
    }
    let mut stats5 = LegStats::default();
    let leg_d = candidates
        .first()
        .map(|&(_, goal, _)| walk_to(host, goal, &hazards5, &mut stats5, &fight));
    // Where the transition put the player names **which door** it was, because
    // every `keikoku` record returns to `map01` at its own tile. That is the
    // check a step back through the entrance cannot pass, and it comes off the
    // disc rather than off a distance heuristic. The reach threshold is kept
    // alongside it: together they say "walked the canyon" rather than "a door
    // fired".
    let landed = matches!(&leg_d, Some(Leg::Transitioned(_))).then(|| {
        let (x, z) = player_world(host);
        tile_of(x, z)
    });
    let arrival_return = arrival_rec
        .and_then(|rec| records.iter().find(|r| r.record == rec))
        .map(|r| r.dest_tile);
    let traversed =
        left_by_another_door(landed, arrival_return) && stats5.reach >= DUNGEON_TRAVERSE_TILES;
    rungs.push(Rung {
        label: "pad-walk the Ravine out of a different door",
        detail: match (&leg_d, candidates.first()) {
            (None, _) => format!(
                "keikoku carries no scene-change record other than the {arrival_rec:?} the \
                 {entry_tile:?} arrival came through ({} record(s) found)",
                records.len()
            ),
            (Some(leg), Some(&(rec, goal, residual))) => format!(
                "from arrival {entry_tile:?} (record {arrival_rec:?}, returns to {arrival_return:?}) \
                 toward record {rec}'s band at {goal:?} (route residual {residual}): {leg} at \
                 {landed:?}; reached {} tiles (needs {DUNGEON_TRAVERSE_TILES}), \
                 {} random encounter(s) fought",
                stats5.reach, stats5.fought,
            ),
            (Some(leg), None) => format!("{leg}"),
        },
        cleared: traversed,
    });

    rungs
}

/// Did the dungeon leg come out somewhere other than where its entrance
/// record returns to?
///
/// `landed` is the tile the transition seated the player on, `arrival_return`
/// the tile the record the player came in through returns to. Both are `None`
/// when there was no transition / no identifiable entrance record, and a
/// missing half is **not** a pass: an unknown door is not a different one.
/// Split out so this can be exercised without a two-minute disc run - the
/// clause is the half of rung 5 that the `LEGAIA_CPR_RUNG5_BACKOUT` control
/// cannot isolate, because a backed-out leg also fails the reach threshold.
pub(super) fn left_by_another_door(
    landed: Option<(i16, i16)>,
    arrival_return: Option<(i16, i16)>,
) -> bool {
    matches!((landed, arrival_return), (Some(l), Some(a)) if l != a)
}

/// Render an optional leg, so a missing goal reads as a leg outcome rather
/// than as an empty string.
pub(super) fn fmt_leg(leg: Option<&Leg>) -> String {
    leg.map_or_else(|| "no reachable portal".to_string(), |l| format!("{l}"))
}
