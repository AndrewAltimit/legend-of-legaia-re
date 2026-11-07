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
        let script = std::env::var("LEGAIA_RC_WALK_FILM").ok()?;
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
        Some(Self { spans, ticks, out })
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
            "tick,x,y,z,heading,clip,pitch,yaw,h,eye_x,eye_y,eye_z,focus_x,focus_z,offset_ease,cursor,rate,record"
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
        for t in 0..self.ticks {
            let pad = self
                .spans
                .iter()
                .filter(|&&(a, b, _)| (a..b).contains(&t))
                .fold(0, |p, s| p | s.2);
            session.host.world.input.set_pad(pad);
            session.tick()?;
            session.field_vram_tick();
            let world = &session.host.world;
            let Some(ms) = world
                .player_actor_slot
                .and_then(|s| world.actors.get(usize::from(s)))
                .map(|a| &a.move_state)
            else {
                continue;
            };
            for (&slot, &(x, z)) in &world.npcs.positions {
                let h = world.npcs.heading(slot).wrapping_sub(0x800);
                writeln!(a, "{t},{},{x},{z},{h}", n0 + u16::from(slot))?;
            }
            let g = &session.camera.globals.0;
            let anim = world.locomotion.player_anim.as_ref();
            writeln!(
                f,
                "{t},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{}",
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
            )?;
        }
        session.host.world.input.set_pad(0);
        f.flush()?;
        a.flush()?;
        Ok(())
    }
}
