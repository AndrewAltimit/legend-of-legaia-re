//! The engine side of the field walk film
//! (`scripts/pcsx-redux/autorun_field_walk_film.lua`,
//! `scripts/ci/walk-film.py`): stand the seated session on the state's own
//! camera, hold a pad schedule and write the player and the follow camera one
//! row a tick, so a walk can be compared with retail pass by pass where the
//! corpus compares one still. `LEGAIA_RC_WALK_FILM` names the schedule
//! (`A-B:BTN`, in ticks) and `LEGAIA_RC_WALK_OUT` the CSV.

use super::*;
use std::io::Write;

/// A pad schedule and where its trace goes.
pub(super) struct WalkFilm {
    spans: Vec<(u64, u64, u16)>,
    /// `LEGAIA_RC_WALK_PAD`: one `tick,0xMASK` row per change of the pad word
    /// (a `LEGAIA_DIAG_TICK_DIGEST` run's engine pad word), used instead of
    /// the spans; the same rows retail's `LEGAIA_PAD_FILE` replays.
    pad_rows: Vec<(u64, u16)>,
    ticks: u64,
    out: std::path::PathBuf,
}

/// The engine pad word's bit of a schedule's button name.
fn button(name: &str) -> Option<u16> {
    Some(match name.to_ascii_uppercase().as_str() {
        "UP" => 0x0010,
        "RIGHT" => 0x0020,
        "DOWN" => 0x0040,
        "LEFT" => 0x0080,
        "CROSS" => 0x4000,
        "CIRCLE" => 0x2000,
        "SELECT" => 0x0001,
        "START" => 0x0008,
        "L2" => 0x0100,
        "R2" => 0x0200,
        "L1" => 0x0400,
        "R1" => 0x0800,
        "TRIANGLE" => 0x1000,
        "SQUARE" => 0x8000,
        _ => return None,
    })
}

impl WalkFilm {
    pub(super) fn from_env() -> Option<Self> {
        let script = std::env::var("LEGAIA_RC_WALK_FILM").unwrap_or_default();
        let pad_rows: Vec<(u64, u16)> = std::env::var_os("LEGAIA_RC_WALK_PAD")
            .and_then(|p| std::fs::read_to_string(p).ok())
            .map(|text| {
                text.lines()
                    .filter_map(|l| {
                        let (t, m) = l.trim().split_once(',')?;
                        let m = m.trim().trim_start_matches("0x");
                        Some((t.parse().ok()?, u16::from_str_radix(m, 16).ok()?))
                    })
                    .collect()
            })
            .unwrap_or_default();
        if script.is_empty() && pad_rows.is_empty() {
            return None;
        }
        let out = std::env::var_os("LEGAIA_RC_WALK_OUT")?.into();
        let spans: Vec<_> = script
            .split(',')
            .filter_map(|item| {
                let (range, name) = item.trim().split_once(':')?;
                let (a, b) = range.split_once('-')?;
                Some((a.parse().ok()?, b.parse().ok()?, button(name)?))
            })
            .collect();
        let ticks = std::env::var("LEGAIA_RC_WALK_TICKS")
            .ok()
            .and_then(|t| t.parse().ok())
            .unwrap_or(600);
        Some(Self {
            spans,
            pad_rows,
            ticks,
            out,
        })
    }

    /// The pad word in force on engine tick `t`.
    fn pad_at(&self, t: u64) -> u16 {
        let timeline = self
            .pad_rows
            .iter()
            .take_while(|&&(row, _)| row <= t)
            .last()
            .map_or(0, |&(_, m)| m);
        self.spans
            .iter()
            .filter(|&&(a, b, _)| (a..b).contains(&t))
            .fold(timeline, |p, s| p | s.2)
    }

    pub(super) fn run(&self, session: &mut NativeSession, retail: &RetailObs) -> Result<()> {
        crate::retail_compare::seat_host_camera(&mut session.camera);
        // The lead's clip players, as a play host binds them.
        legaia_engine_session::scene_actors::bind_scene_actors(&mut session.host);
        // `LEGAIA_RC_PORT_PAD=1` (lane W) runs the port's default pad and
        // walk delivery instead of the retail pipeline the seed pins: the pad
        // on the vsync it is pressed, the overworld step paid out a vsync at
        // a time. A diagnostic, for measuring what the pipeline buys.
        if std::env::var_os("LEGAIA_RC_PORT_PAD").is_some() {
            session.host.world.toggles.immediate_pad = true;
            session.host.world.toggles.smooth_walk = true;
        }
        // What both play hosts raise.
        session.host.world.locomotion.leading_edge_wall_probes = true;
        session.host.world.locomotion.follow_terrain_height = true;
        session.host.world.locomotion.vertical_settle = true;
        session.host.world.npcs.solid = true;
        session.host.world.npcs.animate = true;
        // The Field Move option the state was saved under: a held pad walks
        // at its run speed when the player had picked Run (`0x800846CC`,
        // `World::field_run_active`). `LEGAIA_RC_WALK_RUN=1` forces Run,
        // `=0` Walk.
        session.host.world.locomotion.run_default =
            match std::env::var("LEGAIA_RC_WALK_RUN").as_deref() {
                Ok("0") => false,
                Ok(_) => true,
                Err(_) => retail.field_move_run,
            };
        // The region roll's step counter (`_DAT_8007B5FC`): how many steps
        // the walk has left before a fight is walk history, so a film from a
        // state one step short of an encounter rolls on retail's step. A
        // harness seat (lane W); `LEGAIA_RC_NO_STEP_SEAT=1` keeps the seed's.
        if std::env::var_os("LEGAIA_RC_NO_STEP_SEAT").is_none() {
            session
                .host
                .world
                .set_encounter_step_counter(retail.encounter_step_counter);
        }
        // The wanderers' stream state and the world's seed, so a film's
        // `rng` column and its `rand()`-driven NPCs track retail's.
        super::seat_ambient_streams(session, retail);
        // Every drawn actor's clip where the RAM holds it: a placement's
        // idle phase is the history of the passes since its clip started,
        // which the seed's scene entry does not replay, so a film left on the
        // seed's phases compared two arbitrary cursors. Seated, the clip
        // columns read how the pose moves from retail's. A harness seat;
        // `LEGAIA_RC_NO_CLIP_SEAT=1` leaves the seed's phases.
        if std::env::var_os("LEGAIA_RC_NO_CLIP_SEAT").is_none() {
            for c in &retail.object_clips_ram {
                session.host.world.seed_object_prop_clip(
                    usize::from(c.record),
                    c.clip,
                    c.cursor,
                    c.flags,
                    c.rate,
                );
            }
        }
        if std::env::var_os("LEGAIA_RC_PART_DIAG").is_some() {
            for s in &retail.part_vm {
                eprintln!(
                    "R rec {:#x} pc {} wait {} loops {:?} 5c {}",
                    s.record_off, s.pc, s.wait_timer, s.loops, s.field_5c
                );
            }
            for p in session.host.world.ambient.fx.iter().filter(|p| !p.finished) {
                let st = &p.state;
                eprintln!(
                    "E rec {:#x} pc {} wait {} loops {:?} 5c {}",
                    p.record_off,
                    st.pc,
                    st.wait_timer,
                    [st.field_88, st.field_8a, st.field_8c, st.field_8e],
                    st.field_5c
                );
            }
        }
        // The ambient parts' programs where the state holds them: a part's
        // waits run from its spawn on the game pass, so the pass its next
        // `rand()` op lands on is time since the entry, which the seed's
        // settle does not replay.
        if std::env::var_os("LEGAIA_RC_NO_PART_VM").is_none() {
            let (seated, of) = session.host.world.seat_ambient_part_vm(&retail.part_vm);
            if std::env::var_os("LEGAIA_RC_PART_DIAG").is_some() {
                eprintln!(
                    "part vm seated {seated} of {of} phase {} fog {} live {} table {:?}",
                    retail.vsync_phase,
                    retail.fog_live.len(),
                    retail.fog_live_count,
                    retail.frame_table
                );
            }
        }
        // The fog pool as the state left it: its live count gates the next
        // spawns (`_DAT_8007BCA8`, FUN_801D629C), so a film from an empty
        // pool draws past the cap until it fills. The seed's warm-up ticks
        // accumulated a step the state's records have already taken.
        session.host.world.fog.install_snapshot(&retail.fog_live);
        // The count the spawner reads is the word the last render step wrote,
        // not the records alive at the save: a state saved between a pass's
        // spawns and its render step holds both (`field_walled_collision_pin`:
        // 73 records up, the word 66, and retail's first pass spawns).
        if !retail.fog_live.is_empty() {
            session.host.world.fog.live = retail.fog_live_count;
        }
        session.host.world.fog.pending_dt = 0;
        // The game frame's phase: retail's next pass opens `step - phase`
        // vsyncs into the film, which the engine's accumulators reach from
        // `phase - 1`; a state saved on the pacer's return holds the first
        // vsync on `0`.
        // The pacer's history: a state saved a few frames after a slow
        // stretch runs on at the raised step until those samples leave the
        // ring, and falls to the floor when retail's does. A harness seat;
        // `LEGAIA_RC_NO_FRAME_RING=1` holds the state's step instead.
        let mut anchor_cost = None;
        if std::env::var_os("LEGAIA_RC_NO_FRAME_RING").is_none()
            && let Some((history, cursor, step, cost)) = retail.frame_cost_seat()
        {
            let world = &mut session.host.world;
            world.seed_frame_cost_history(history, cursor, step, cost);
            // A state saved inside the pacer's wait is waiting out the step
            // of the frame before (`0x1F800392`, stored after the wait).
            if let Some((wait, _)) = retail.step_bytes.filter(|b| b.0 > 0) {
                world.clock.pass_wait = Some(wait.max(world.clock.frame_step_floor));
            }
            // The frame-cost model, where it prices the field, carries the
            // change from the state's measured cost, not its own level
            // (`World::anchor_field_frame_cost`), once the camera and the
            // fog pool stand where the state's do (below). A harness seat;
            // `LEGAIA_RC_NO_COST_ANCHOR=1` runs the model as play does.
            if world.toggles.field_frame_cost
                && std::env::var_os("LEGAIA_RC_NO_COST_ANCHOR").is_none()
            {
                anchor_cost = Some(cost);
            }
        }
        let hold_first = seat_frame_phase(&mut session.host.world, retail.vsync_phase);
        let g = &mut session.camera.globals.0;
        let c = &retail.camera;
        g[0] = i32::from(c.pitch);
        g[1] = i32::from(c.yaw);
        g[9] = i32::from(c.h);
        [g[3], g[4], g[5]] = c.eye;
        [g[6], g[8]] = c.focus;
        // A state saved between a frame's actor pass and its render pass
        // holds that frame's spawns unwalked (age `0`, and more records up
        // than the live count): retail's first step past the save is that
        // frame's fog walk, not the next frame's actor pass. Run it here, so
        // the pool ages and frees a pass in step with retail's and the first
        // spawner reads the count that walk writes.
        //
        // A state saved inside the walk itself has walked part of the pool
        // already: the walk counts its survivors into the live word as it
        // goes, slot by slot, so the first `live` records up are the frame's
        // walked ones and only those past them still owe the frame's step.
        let pending = pending_fog_walk(retail);
        if pending != PendingFogWalk::None && std::env::var_os("LEGAIA_RC_NO_FOG_WALK").is_none() {
            let step = u32::from(session.host.world.clock.frame_step.max(1));
            let walked: Vec<usize> = match pending {
                PendingFogWalk::From(rank) => {
                    let pool = &mut session.host.world.fog;
                    let done: Vec<usize> = (0..pool.records.len())
                        .filter(|&i| pool.records[i].alive)
                        .take(rank)
                        .collect();
                    for &i in &done {
                        pool.records[i].alive = false;
                    }
                    done
                }
                _ => Vec::new(),
            };
            session.host.world.fog.pending_dt = step;
            session.fog_render_tick();
            let pool = &mut session.host.world.fog;
            for &i in &walked {
                pool.records[i].alive = true;
            }
            pool.live += walked.len() as u16;
        }
        // The anchor prices the seat with the fog sheets the state's pool
        // draws, which the seat itself has not emitted (an installed
        // snapshot carries no quads): a walk over a copy of the pool counts
        // them, and only the count is kept. Anchored on an empty sheet list,
        // `retona_field_card_boot`'s `45` sheets had read as a cost rise the
        // moment the first walk drew them.
        if let Some(cost) = anchor_cost {
            let saved = session.host.world.fog.clone();
            session.host.world.fog.pending_dt = 1;
            session.fog_render_tick();
            let quads = std::mem::take(&mut session.host.world.fog.quads);
            session.host.world.fog = saved;
            session.host.world.fog.quads = quads;
            session.host.world.anchor_field_frame_cost(cost);
        }
        let mut f = std::io::BufWriter::new(std::fs::File::create(&self.out)?);
        writeln!(
            f,
            "tick,x,y,z,heading,clip,pitch,yaw,h,eye_x,eye_y,eye_z,focus_x,focus_z,offset_ease,cursor,rate,record,scene,held,engaged,tint,rng,talk,enc,menu"
        )?;
        // A placement's flat MAN index is its channel slot past the scene's
        // partition-0 count, which is the scene's own: a film that crosses a
        // scene change re-reads it for the scene it is in.
        let partition0 = |session: &NativeSession| {
            session
                .host
                .scene
                .as_ref()
                .and_then(|s| s.field_man_payload(&session.host.index).ok().flatten())
                .and_then(|man| legaia_scene_formats::man_section::parse(&man).ok())
                .map_or(0, |mf| mf.header.partition_counts[0].max(0) as u16)
        };
        let mut n0 = partition0(session);
        let mut n0_scene = session.host.world.active_scene_label.clone();
        let mut actors = self.out.clone();
        actors.set_extension("actors.csv");
        let mut a = std::io::BufWriter::new(std::fs::File::create(&actors)?);
        writeln!(a, "tick,flat,x,z,heading,clip,cursor,rate")?;
        // Each ambient channel's `rand()` draws, per tick: what a split against
        // retail's `draws.csv` (a breakpoint on `0x80056798`) names.
        let mut draws_path = self.out.clone();
        draws_path.set_extension("draws.csv");
        let mut d = std::io::BufWriter::new(std::fs::File::create(&draws_path)?);
        writeln!(d, "tick,flat,draws")?;
        let mut seen_draws: std::collections::BTreeMap<u8, u32> = session
            .host
            .world
            .npcs
            .ambient
            .iter()
            .map(|(&k, c)| (k, c.vm.rand_draws))
            .collect();
        // `LEGAIA_RC_WALK_FOG=1`: the fog pool's live count and every live
        // record's `slot:age:rate:x:z:y` a tick (`engine.fog.csv`), the rows a
        // PCSX-Redux probe of `_DAT_8007BCA8` and the pool writes.
        let mut fog = match std::env::var_os("LEGAIA_RC_WALK_FOG") {
            Some(_) => {
                let mut p = self.out.clone();
                p.set_extension("fog.csv");
                let mut w = std::io::BufWriter::new(std::fs::File::create(&p)?);
                writeln!(w, "tick,live,cap,recs,box,view")?;
                Some(w)
            }
            None => None,
        };
        // `LEGAIA_RC_WALK_COST=1`: what each tick's field frame draws
        // (`World::field_frame_features`) beside the step it runs at
        // (`engine.cost.csv`), the engine side of a film's `frame_cost` column.
        let mut cost = match std::env::var_os("LEGAIA_RC_WALK_COST") {
            Some(_) => {
                // The state's own frame-time ring beside it: the retail cost
                // a state with no film of its own still carries.
                let mut r = self.out.clone();
                r.set_extension("ring.txt");
                std::fs::write(
                    &r,
                    format!(
                        "history={:?}\nstep_bytes={:?}\n",
                        retail.frame_history, retail.step_bytes
                    ),
                )?;
                let mut p = self.out.clone();
                p.set_extension("cost.csv");
                let mut w = std::io::BufWriter::new(std::fs::File::create(&p)?);
                writeln!(
                    w,
                    "tick,fired,step,dec_draws,dec_prims,dec_verts,ground,placed_draws,placed_prims,placed_verts,npcs,fog,parts,actors,scene,dec_quads,dec_lit,placed_quads,placed_lit,cost,npc_prims,loop_cells,fog_quads"
                )?;
                Some(w)
            }
            None => None,
        };
        for t in 0..self.ticks {
            let pad = self.pad_at(t);
            // The hosts' pad entry, so the retail-faithful pad pipeline
            // (`World::retail_frame_pad`) runs as it does in play.
            session.host.world.set_pad(pad);
            session.tick()?;
            if hold_first && t == 0 {
                seat_frame_accums(&mut session.host.world, 0);
            }
            session.field_vram_tick();
            // The pool's render step a play host runs from its draw pass: it
            // writes the live count the next tick's fog spawns read.
            session.fog_render_tick();
            if let Some(w) = fog.as_mut() {
                let pool = &session.host.world.fog;
                let mut recs: Vec<_> = pool.records.iter().filter(|r| r.alive).collect();
                recs.sort_by_key(|r| r.slot);
                let recs: Vec<String> = recs
                    .iter()
                    .map(|r| {
                        format!(
                            "{}:{}:{}:{}:{}:{}",
                            r.slot, r.age, r.rate, r.x, r.z, r.y as u16
                        )
                    })
                    .collect();
                let b = session.host.world.terrain.region_attributes.box_bytes;
                let v = pool.view_window.unwrap_or_default();
                writeln!(
                    w,
                    "{t},{},{},{},{}:{}:{}:{},{}:{}:{}:{}",
                    pool.live,
                    pool.cap,
                    recs.join(" "),
                    b[0],
                    b[1],
                    b[2],
                    b[3],
                    v[0],
                    v[1],
                    v[2],
                    v[3]
                )?;
            }
            if let Some(w) = cost.as_mut() {
                let world = &session.host.world;
                let f = world.field_frame_features().unwrap_or_default();
                writeln!(
                    w,
                    "{t},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{}",
                    u8::from(world.clock.game_tick_fired),
                    world.clock.frame_step,
                    f.decoration_draws,
                    f.decoration_prims,
                    f.decoration_verts,
                    f.ground_cells,
                    f.placed_draws,
                    f.placed_prims,
                    f.placed_verts,
                    f.npcs,
                    f.fog,
                    f.parts,
                    world.actors.iter().filter(|a| a.active).count(),
                    world.active_scene_label,
                    f.decoration_quads,
                    f.decoration_lit,
                    f.placed_quads,
                    f.placed_lit,
                    world.clock.frame_cost.unwrap_or(-1),
                    f.npc_prims,
                    f.decoration_loop_cells,
                    f.fog_quads,
                )?;
            }
            if session.host.world.active_scene_label != n0_scene {
                n0 = partition0(session);
                n0_scene = session.host.world.active_scene_label.clone();
            }
            let world = &session.host.world;
            let Some(ms) = world
                .player_actor_slot
                .and_then(|s| world.actors.get(usize::from(s)))
                .map(|a| &a.move_state)
            else {
                continue;
            };
            for (&slot, c) in &world.npcs.ambient {
                let was = seen_draws.insert(slot, c.vm.rand_draws).unwrap_or(0);
                if c.vm.rand_draws > was {
                    writeln!(d, "{t},{},{}", n0 + u16::from(slot), c.vm.rand_draws - was)?;
                }
            }
            for (&slot, &(x, z)) in &world.npcs.positions {
                let h = world.npcs.heading(slot).wrapping_sub(0x800);
                // The placement's clip cursor, against retail's `+0x5C` /
                // `+0x68` / `+0x6A`; `-1` for an actor no clip is bound to.
                let (clip, cursor, rate) = world
                    .npcs
                    .clip_cursors
                    .get(&slot)
                    .map_or((-1, -1, -1), |c| {
                        (i32::from(c.anim_id), i32::from(c.cursor), i32::from(c.rate))
                    });
                writeln!(
                    a,
                    "{t},{},{x},{z},{h},{clip},{cursor},{rate}",
                    n0 + u16::from(slot)
                )?;
            }
            // The pause menu: `1` from its open, `2` once the wipe has spawned it.
            let menu = if !session.field_menu_is_open() {
                0
            } else if session.pause_wipe().menu_spawned() {
                2
            } else {
                1
            };
            let g = &session.camera.globals.0;
            let anim = world.locomotion.player_anim.as_ref();
            // The scene change's own timeline: the scene, whether the hold
            // is parked, whether the player is engaged, and the strongest
            // channel of any full-screen wash pushed this tick.
            let tint = world
                .screen_tint_push_args()
                .iter()
                .map(|&(_, _, p)| (p & 0xFF).max((p >> 8) & 0xFF).max((p >> 16) & 0xFF))
                .max()
                .unwrap_or(0);
            writeln!(
                f,
                "{t},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{:08x},{},{},{}",
                ms.world_x,
                ms.world_y,
                ms.world_z,
                ms.render_26,
                0,
                g[0] as i16,
                g[1] as i16,
                g[9] as i16,
                g[3],
                g[4],
                g[5],
                g[6],
                g[8],
                world.camera.offset_ease,
                anim.map_or(-1, |a| a.playing_clip().2 as i32),
                world.locomotion.player_clip_rate,
                anim.map_or(-1, |a| a.playing_clip().1 as i32),
                world.active_scene_label,
                u8::from(world.scene_transition_hold.is_some()),
                u8::from(world.script_context_engages_player()),
                tint,
                world.rng_state,
                u8::from(world.dialogue_owns_input()),
                world.encounter_step_counter(),
                menu,
            )?;
        }
        session.host.world.input.set_pad(0);
        if let Some(w) = cost.as_mut() {
            w.flush()?;
        }
        f.flush()?;
        a.flush()?;
        d.flush()?;
        Ok(())
    }
}

/// The game-frame accumulators a walk film starts on
/// ([`legaia_engine_vm::frame_clock::FrameClock::actor_vsync_accum`] and the
/// CLUT bank's twin), from the vsync of retail's frame the state was saved on
/// ([`RetailObs::vsync_phase`]). Returns whether the first tick must be held
/// (a phase of `0`, where the accumulators would need `-1`).
///
/// The frame pacer `FUN_80016B6C` waits out each frame with `VSync(step)`
/// (`jal 0x8005FB84` at `0x800171C0`), and libetc's `VSync` stores the vblank
/// count on return (`0x800797D4`, `0x8005FC8C..0x8005FCA0`), so the state's
/// vblank counter `0x8007A894` less that store is how far into its frame the
/// state was saved, and the next pass opens `step - phase` vsyncs on. Left on
/// the seed's own phase, every placement that turns on the pass grid turns a
/// vsync early or late. A phase of `step` was saved on the vblank the wait
/// returns on, its pass opening on the film's first vsync (`conc_field_card_boot`, `dolk2_market_noa`,
/// `koin1_field_route_walk`). A harness seat.
fn seat_frame_phase(world: &mut legaia_engine_core::world::World, phase: u32) -> bool {
    let step = u32::from(world.clock.game_frame_wait());
    if phase > step {
        return false;
    }
    seat_frame_accums(world, phase.saturating_sub(1) as u8);
    phase == 0
}

/// How much of its frame's fog walk the state was saved with still to run.
///
/// The table the frame is building holds packets but no fog sheet
/// ([`RetailObs::frame_table`]): the render pass has not reached its fog
/// stage. Or the pool says so itself: a record the spawner seated this frame
/// stands at age `0` until a walk adds `rate * dt` to it (`FUN_8003F3FC`),
/// and the live count `_DAT_8007BCA8` is the last walk's.
///
/// A table that already holds fog sheets was saved inside the walk. The walk
/// zeroes the live word before it starts and adds each survivor as it passes
/// it (`0x8003F398`, `0x8003F3C8`), so the word counts the records behind its
/// cursor: `vell_fog_field` holds `24` up and the word `14`, and its first
/// vsync steps the ten records past the fourteenth and none before them.
fn pending_fog_walk(retail: &RetailObs) -> PendingFogWalk {
    if retail.fog_live.is_empty() {
        return PendingFogWalk::None;
    }
    let live = usize::from(retail.fog_live_count);
    // The walk had started: its sheets are in the table and the live word
    // holds its running count. It walks in slot order (`FUN_8003F348`), so
    // the first `live` records up are behind its cursor.
    if retail.frame_table.is_some_and(|(_, fog)| fog > 0) && retail.fog_live.len() > live {
        let mut slots: Vec<u8> = retail.fog_live.iter().map(|r| r.slot).collect();
        slots.sort_unstable();
        let behind = |s: u8| slots.iter().position(|&x| x == s).is_some_and(|i| i < live);
        // A record behind the cursor has taken the frame's step; one at age
        // `0` there would not have, so that pool is not mid-walk.
        if !retail.fog_live.iter().any(|r| r.age == 0 && behind(r.slot)) {
            return PendingFogWalk::From(live);
        }
    }
    if retail.frame_table.is_some_and(|(n, fog)| n > 0 && fog == 0)
        || retail.fog_live.iter().any(|r| r.age == 0)
        || retail.fog_live.len() > live
    {
        PendingFogWalk::Whole
    } else {
        PendingFogWalk::None
    }
}

/// What of the frame's fog walk a state still owes ([`pending_fog_walk`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PendingFogWalk {
    /// The walk ran before the save, or the pool is empty.
    None,
    /// The render pass had not reached its fog stage: every record owes it.
    Whole,
    /// Saved inside the walk: the records past the first `n` up owe it.
    From(usize),
}

fn seat_frame_accums(world: &mut legaia_engine_core::world::World, accum: u8) {
    world.clock.actor_vsync_accum = accum;
    world.ambient.clut_vsync_accum = accum;
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The fog walk a state owes: none on an empty pool or a finished walk,
    /// the whole walk before the render pass reaches its fog stage, and the
    /// records past the live word's count when the table already holds
    /// sheets (`vell_fog_field`: `24` up, the word `14`).
    #[test]
    fn a_state_saved_inside_the_fog_walk_owes_the_records_past_the_count() {
        use legaia_engine_field::fog_particles::FogParticle;
        let mut r = RetailObs::from_ram(&vec![0u8; 0x20_0000], None);
        assert_eq!(pending_fog_walk(&r), PendingFogWalk::None);
        r.fog_live = (0..24u8)
            .map(|i| FogParticle {
                slot: 48 + i,
                age: 1000 + u16::from(i),
                rate: 10,
                alive: true,
                ..FogParticle::default()
            })
            .collect();
        r.fog_live_count = 24;
        r.frame_table = Some((990, 21));
        assert_eq!(pending_fog_walk(&r), PendingFogWalk::None);
        r.fog_live_count = 14;
        assert_eq!(pending_fog_walk(&r), PendingFogWalk::From(14));
        r.frame_table = Some((990, 0));
        assert_eq!(pending_fog_walk(&r), PendingFogWalk::Whole);
        // A record behind the count still at age `0` was never walked.
        r.frame_table = Some((990, 21));
        r.fog_live[3].age = 0;
        assert_eq!(pending_fog_walk(&r), PendingFogWalk::Whole);
    }

    /// The first pass opens `step - phase` vsyncs on: `phase - 1` on the
    /// accumulators, a held first vsync on `0`, and a state saved on the
    /// return itself (`phase == step`) fires on the film's first vsync.
    #[test]
    fn the_film_seats_the_frame_phase_of_the_state() {
        let mut w = legaia_engine_core::world::World::new();
        w.set_frame_step_floor(2);
        assert!(!seat_frame_phase(&mut w, 1));
        assert_eq!(w.clock.actor_vsync_accum, 0);
        assert!(!seat_frame_phase(&mut w, 2));
        assert_eq!(w.clock.actor_vsync_accum, 1);
        assert_eq!(w.ambient.clut_vsync_accum, 1);
        assert!(seat_frame_phase(&mut w, 0));
        assert_eq!(w.clock.actor_vsync_accum, 0);
        // A phase past the step is no frame of the pacer's: left alone.
        w.clock.actor_vsync_accum = 1;
        assert!(!seat_frame_phase(&mut w, 7));
        assert_eq!(w.clock.actor_vsync_accum, 1);
    }
}
