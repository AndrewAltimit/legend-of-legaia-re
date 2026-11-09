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
        // The fog pool as the state left it: its live count gates the next
        // spawns (`_DAT_8007BCA8`, FUN_801D629C), so a film from an empty
        // pool draws past the cap until it fills. The seed's warm-up ticks
        // accumulated a step the state's records have already taken.
        session.host.world.fog.install_snapshot(&retail.fog_live);
        session.host.world.fog.pending_dt = 0;
        let g = &mut session.camera.globals.0;
        let c = &retail.camera;
        g[0] = i32::from(c.pitch);
        g[1] = i32::from(c.yaw);
        g[9] = i32::from(c.h);
        [g[3], g[4], g[5]] = c.eye;
        [g[6], g[8]] = c.focus;
        let mut f = std::io::BufWriter::new(std::fs::File::create(&self.out)?);
        writeln!(
            f,
            "tick,x,y,z,heading,clip,pitch,yaw,h,eye_x,eye_y,eye_z,focus_x,focus_z,offset_ease,cursor,rate,record,scene,held,engaged,tint,rng"
        )?;
        let n0 = session
            .host
            .scene
            .as_ref()
            .and_then(|s| s.field_man_payload(&session.host.index).ok().flatten())
            .and_then(|man| legaia_scene_formats::man_section::parse(&man).ok())
            .map_or(0, |mf| mf.header.partition_counts[0].max(0) as u16);
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
        for t in 0..self.ticks {
            let pad = self.pad_at(t);
            session.host.world.input.set_pad(pad);
            session.tick()?;
            session.field_vram_tick();
            // The pool's render step a play host runs from its draw pass: it
            // writes the live count the next tick's fog spawns read.
            session.fog_render_tick();
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
