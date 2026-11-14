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
        // What both play hosts raise.
        session.host.world.locomotion.leading_edge_wall_probes = true;
        session.host.world.locomotion.follow_terrain_height = true;
        session.host.world.locomotion.vertical_settle = true;
        session.host.world.npcs.solid = true;
        session.host.world.npcs.animate = true;
        session.host.world.locomotion.run_default =
            std::env::var_os("LEGAIA_RC_WALK_RUN").is_some();
        // The wanderers' stream state and the world's seed, so a film's
        // `rng` column and its `rand()`-driven NPCs track retail's.
        super::seat_ambient_streams(session, retail);
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
        if pending_fog_walk(retail) && std::env::var_os("LEGAIA_RC_NO_FOG_WALK").is_none() {
            let step = u32::from(session.host.world.clock.frame_step.max(1));
            session.host.world.fog.pending_dt = step;
            session.fog_render_tick();
        }
        let mut f = std::io::BufWriter::new(std::fs::File::create(&self.out)?);
        writeln!(
            f,
            "tick,x,y,z,heading,clip,pitch,yaw,h,eye_x,eye_y,eye_z,focus_x,focus_z,offset_ease,cursor,rate,record,scene,held,engaged,tint,rng"
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
        writeln!(a, "tick,flat,x,z,heading")?;
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
                writeln!(w, "tick,live,cap,recs")?;
                Some(w)
            }
            None => None,
        };
        for t in 0..self.ticks {
            let pad = self.pad_at(t);
            session.host.world.input.set_pad(pad);
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
                writeln!(w, "{t},{},{},{}", pool.live, pool.cap, recs.join(" "))?;
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
                writeln!(a, "{t},{},{x},{z},{h}", n0 + u16::from(slot))?;
            }
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
                "{t},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{:08x}",
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
            )?;
        }
        session.host.world.input.set_pad(0);
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
    let step = u32::from(world.clock.frame_step.max(1));
    if phase > step {
        return false;
    }
    seat_frame_accums(world, phase.saturating_sub(1) as u8);
    phase == 0
}

/// Whether the state was saved with its frame's fog walk still to run.
///
/// The table the frame is building holds packets but no fog sheet
/// ([`RetailObs::frame_table`]): the render pass has not reached its fog
/// stage. Or the pool says so itself: a record the spawner seated this frame
/// stands at age `0` until a walk adds `rate * dt` to it (`FUN_8003F3FC`),
/// and the live count `_DAT_8007BCA8` is the last walk's.
fn pending_fog_walk(retail: &RetailObs) -> bool {
    if retail.fog_live.is_empty() {
        return false;
    }
    retail.frame_table.is_some_and(|(n, fog)| n > 0 && fog == 0)
        || retail.fog_live.iter().any(|r| r.age == 0)
        || retail.fog_live.len() > usize::from(retail.fog_live_count)
}

fn seat_frame_accums(world: &mut legaia_engine_core::world::World, accum: u8) {
    world.clock.actor_vsync_accum = accum;
    world.ambient.clut_vsync_accum = accum;
}

#[cfg(test)]
mod tests {
    use super::*;

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
