//! Disc-gated: the field-VM routing census.
//!
//! Retail runs every script the same way: the context runner hands the
//! dispatcher `FUN_801DE840` the executing actor's own context, and the
//! dispatcher's prologue swaps an extended op's resolved target into `s5`
//! before the arm runs (`0x801DE894..0x801DE8B4`, `FUN_8003C83C`). The port's
//! runners route ops onto their targets case by case, and an op a runner does
//! not route runs on the context the runner stands on
//! ([`legaia_engine_core::world::route_census`]).
//!
//! The sweep enters every CDNAME field scene under a cold bank and one late
//! milestone (`fixtures/spine_anchor_flags.txt`), lets each scene's system
//! script and spawned records run, talks every talker to the end of its talk,
//! touches every placed prop, and gathers every fall-through step the
//! runners noted. `LEGAIA_ROUTE_CENSUS_LOG=<path>` writes the table
//! (runner, op family, target kind, count, first site);
//! `LEGAIA_ROUTE_CENSUS=full` runs all thirty-six flag states and
//! `LEGAIA_ROUTE_CENSUS_SCENE=<name>` one scene.
//!
//! Skips without `LEGAIA_DISC_BIN` / `extracted/` (disc-gated convention).

use std::collections::BTreeMap;
use std::path::PathBuf;

use legaia_engine_core::world::route_census::{RouteCensusHit, RouteRunner, RouteTarget};
use legaia_engine_scene::scene::is_world_map_scene;
use legaia_engine_scene_host::scene_host::SceneHost;
use legaia_engine_system::input::PadButton;

const ANCHOR_FLAGS: &str = include_str!("fixtures/spine_anchor_flags.txt");
const LATE_STATE: &str = "rogue_tower";

fn extracted_dir() -> Option<PathBuf> {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return None;
    }
    for c in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(c);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    eprintln!("[skip] extracted/ missing");
    None
}

fn flag_states() -> Vec<(String, Vec<u16>)> {
    let mut out = vec![("cold".to_string(), Vec::new())];
    for line in ANCHOR_FLAGS.lines() {
        let mut it = line.split_whitespace();
        let (Some(name), Some(_scene), flags) = (it.next(), it.next(), it.next()) else {
            continue;
        };
        if name.starts_with('#') {
            continue;
        }
        let flags = flags
            .unwrap_or("")
            .split(',')
            .filter_map(|f| f.parse().ok())
            .collect();
        out.push((name.to_string(), flags));
    }
    out
}

fn enter(host: &mut SceneHost, scene: &str, flags: &[u16], settle: u32) -> bool {
    host.world
        .flags
        .system_flags
        .iter_mut()
        .for_each(|b| *b = 0);
    for &f in flags {
        host.world.system_flag_set(f);
    }
    if host.enter_field_scene(scene, 0).is_err() {
        return false;
    }
    host.world.toggles.use_vm_dialogue = true;
    host.world.field_vm.route_census = Some(Vec::new());
    for f in 0..settle {
        host.world.input.set_pad(if f % 4 == 0 {
            PadButton::Cross.mask()
        } else {
            0
        });
        let _ = host.world.tick();
        if host.world.mode != legaia_engine_mode::mode::SceneMode::Field
            || host.world.active_scene_label != scene
        {
            return false;
        }
    }
    host.world.input.set_pad(0);
    host.world.mode == legaia_engine_mode::mode::SceneMode::Field
}

/// Run frames pressing confirm until the field is free again (or left).
fn run_out(host: &mut SceneHost, scene: &str) {
    let mut started = false;
    for f in 0..2_400 {
        host.world.input.set_pad(if f % 4 == 0 {
            PadButton::Cross.mask()
        } else {
            0
        });
        let _ = host.world.tick();
        if host.world.mode != legaia_engine_mode::mode::SceneMode::Field
            || host.world.active_scene_label != scene
        {
            return;
        }
        let live = host.world.dialog.inline.is_some()
            || host.world.dialog.current.is_some()
            || host.world.cutscene_timeline_active();
        started |= live;
        if (started && !live) || (!started && f > 8) {
            return;
        }
    }
}

fn stand_near(host: &SceneHost, tx: i32, tz: i32) -> Option<(i32, i32)> {
    for dz in -1..=1 {
        for dx in -1..=1 {
            if (dx, dz) == (0, 0) {
                continue;
            }
            let (sx, sz) = (tx + dx, tz + dz);
            if !(0..128).contains(&sx) || !(0..128).contains(&sz) {
                continue;
            }
            let (x, z) = ((sx * 128 + 0x40) as i16, (sz * 128 + 0x40) as i16);
            if host.world.field_tile_is_walk_visible(x, z) && !host.world.field_tile_is_wall(x, z) {
                return Some((sx, sz));
            }
        }
    }
    None
}

fn seat_player(host: &mut SceneHost, stand: (i32, i32)) {
    let pslot = usize::from(host.world.player_actor_slot.expect("a player"));
    let ms = &mut host.world.actors[pslot].move_state;
    ms.world_x = (stand.0 * 128 + 0x40) as i16;
    ms.world_z = (stand.1 * 128 + 0x40) as i16;
}

/// `(runner, family, target kind)` the census groups by.
type Key = (RouteRunner, String, String);

fn family(h: &RouteCensusHit) -> String {
    let b = h.bytes;
    let ext = b[0] & 0x80 != 0;
    let op = b[0] & 0x7F;
    let sub = if ext { b[2] } else { b[1] };
    match op {
        0x4C | 0x43 | 0x49 | 0x4E => format!("{op:02X} {sub:02X}"),
        0x34 => format!("34 {:X}x", sub >> 4),
        0x2B..=0x33 => format!("{op:02X} {:02X}", sub & 0x1F),
        _ => format!("{op:02X}"),
    }
}

fn kind(h: &RouteCensusHit) -> String {
    match h.target {
        RouteTarget::Own => "own".into(),
        RouteTarget::Player => "player".into(),
        RouteTarget::System => "system".into(),
        RouteTarget::Placement(s) if h.who == Some(u16::from(s)) => "self".into(),
        RouteTarget::Placement(_) => "placement".into(),
        RouteTarget::Object(k) if h.who == Some(k) => "self-object".into(),
        RouteTarget::Object(_) => "object".into(),
        RouteTarget::Unresolved(_) => "unresolved".into(),
    }
}

#[derive(Default)]
struct Row {
    count: usize,
    sites: std::collections::BTreeSet<String>,
}

fn drain(host: &mut SceneHost, scene: &str, state: &str, table: &mut BTreeMap<Key, Row>) {
    let hits = host.world.field_vm.route_census.replace(Vec::new());
    for h in hits.unwrap_or_default() {
        let row = table.entry((h.runner, family(&h), kind(&h))).or_default();
        row.count += 1;
        if row.sites.len() < 12 {
            row.sites.insert(format!(
                "{scene}/{state} who={:?} pc={:#x} [{:02X} {:02X} {:02X} {:02X}]",
                h.who, h.pc, h.bytes[0], h.bytes[1], h.bytes[2], h.bytes[3]
            ));
        }
    }
}

#[test]
fn field_route_census() {
    let Some(dir) = extracted_dir() else {
        return;
    };
    let mut host = SceneHost::open_extracted(&dir).expect("open the host");
    let mode = std::env::var("LEGAIA_ROUTE_CENSUS").unwrap_or_default();
    let states: Vec<_> = flag_states()
        .into_iter()
        .filter(|(n, _)| mode == "full" || n == "cold" || n == LATE_STATE)
        .collect();
    let only = std::env::var("LEGAIA_ROUTE_CENSUS_SCENE").ok();
    let mut names = host.index.cdname_scene_names();
    names.sort();
    names.dedup();
    names.retain(|n| !is_world_map_scene(n));
    let mut table: BTreeMap<Key, Row> = BTreeMap::new();
    let t0 = std::time::Instant::now();
    let mut scenes = 0usize;
    for (state, flags) in &states {
        for scene in &names {
            if only.as_deref().is_some_and(|o| o != scene) {
                continue;
            }
            if !enter(&mut host, scene, flags, 240) {
                if let Some(h) = host.world.field_vm.route_census.as_ref()
                    && !h.is_empty()
                {
                    drain(&mut host, scene, state, &mut table);
                }
                continue;
            }
            scenes += 1;
            drain(&mut host, scene, state, &mut table);
            let mut slots: Vec<u8> = host.world.npcs.dialog_prologue.keys().copied().collect();
            slots.sort_unstable();
            for slot in slots {
                if !enter(&mut host, scene, flags, 4) {
                    break;
                }
                let Some(&(nx, nz)) = host.world.npcs.positions.get(&slot) else {
                    continue;
                };
                let npc = (i32::from(nx - 0x40) >> 7, i32::from(nz - 0x40) >> 7);
                let Some(stand) = stand_near(&host, npc.0, npc.1) else {
                    continue;
                };
                seat_player(&mut host, stand);
                host.world.trigger_field_interact(0, slot);
                run_out(&mut host, scene);
                drain(&mut host, scene, state, &mut table);
            }
            let anchors: Vec<(u8, u8)> = host.world.props.bank.props.keys().copied().collect();
            for anchor in anchors {
                if !enter(&mut host, scene, flags, 4) {
                    break;
                }
                if let Some(stand) = stand_near(&host, i32::from(anchor.0), i32::from(anchor.1)) {
                    seat_player(&mut host, stand);
                }
                if host.world.start_prop_interaction(anchor) {
                    run_out(&mut host, scene);
                }
                drain(&mut host, scene, state, &mut table);
            }
        }
    }
    eprintln!(
        "[route census] {} state(s), {scenes} scene entries, {} rows, {:.0?}",
        states.len(),
        table.len(),
        t0.elapsed()
    );
    if let Ok(path) = std::env::var("LEGAIA_ROUTE_CENSUS_LOG") {
        let mut text = String::new();
        for ((runner, fam, kind), row) in &table {
            text.push_str(&format!("{runner:?}\t{fam}\t{kind}\t{}\n", row.count));
            for s in &row.sites {
                text.push_str(&format!("\t\t{s}\n"));
            }
        }
        std::fs::write(path, text).expect("write the census log");
    }
    assert!(scenes > 100, "the census entered the field scenes");
}

/// `LEGAIA_ROUTE_DUMP=<scene>:<partition>:<record>` prints that record's
/// linear disassembly, PCs relative to the record start (the runners' PC
/// space), for reading a census site.
#[test]
fn field_route_dump() {
    let Ok(spec) = std::env::var("LEGAIA_ROUTE_DUMP") else {
        return;
    };
    let Some(dir) = extracted_dir() else {
        return;
    };
    let mut it = spec.split(':');
    let (Some(name), Some(part), Some(rec)) = (it.next(), it.next(), it.next()) else {
        panic!("LEGAIA_ROUTE_DUMP=<scene>:<partition>:<record>");
    };
    let (part, rec): (usize, usize) = (part.parse().unwrap(), rec.parse().unwrap());
    let index = legaia_engine_scene::scene::ProtIndex::open_extracted(&dir).expect("index");
    let scene = legaia_engine_scene::scene::Scene::load(&index, name).expect("scene");
    for carrier in legaia_engine_scene::man_census::scene_man_carriers(&index, &scene) {
        let man = &carrier.payload;
        let Ok(mf) = legaia_scene_formats::man_section::parse(man) else {
            continue;
        };
        let Some((start, pc0, len)) =
            legaia_engine_field::man_field_scripts::partition_record_span(&mf, man, part, rec)
        else {
            continue;
        };
        let body = &man[start..start + len];
        let mut out = format!("{name} P{part}[{rec}] start={start:#x} pc0={pc0:#x} len={len:#x}\n");
        for insn in legaia_scene_formats::field_disasm::LinearWalker::new(body, pc0) {
            match insn {
                Ok(i) => out.push_str(&format!(
                    "{:#06x}  {}\n",
                    i.pc,
                    legaia_scene_formats::field_disasm::format_instruction(&i, body)
                )),
                Err((pc, _)) => out.push_str(&format!("{pc:#06x}  ?? {:02X}\n", body[pc])),
            }
        }
        eprintln!("{out}");
        break;
    }
}

/// Probe: `LEGAIA_ROUTE_PROBE=<scene>:<slot>:<target>` talks to `slot` and
/// prints the target's clip cursor each frame.
#[test]
fn field_route_probe() {
    let Ok(spec) = std::env::var("LEGAIA_ROUTE_PROBE") else {
        return;
    };
    let Some(dir) = extracted_dir() else {
        return;
    };
    let mut it = spec.split(':');
    let scene = it.next().unwrap().to_string();
    let slot: u8 = it.next().unwrap().parse().unwrap();
    let target = u8::from_str_radix(it.next().unwrap(), 16).unwrap();
    let mut host = SceneHost::open_extracted(&dir).expect("open the host");
    assert!(enter(&mut host, &scene, &[], 4));
    let tslot = {
        let v = host.world.channel_view();
        legaia_engine_field::field_channels::resolve_target(v, target)
            .map(|ci| (v[ci].object_bind, v[ci].placement_index))
    };
    eprintln!("target {target:#x} -> {tslot:?}");
    let &(nx, nz) = host.world.npcs.positions.get(&slot).expect("talker");
    let npc = (i32::from(nx - 0x40) >> 7, i32::from(nz - 0x40) >> 7);
    let stand = stand_near(&host, npc.0, npc.1).expect("stand");
    seat_player(&mut host, stand);
    host.world.trigger_field_interact(0, slot);
    let mut last = None;
    for f in 0..3000 {
        host.world.input.set_pad(if f % 4 == 0 {
            PadButton::Cross.mask()
        } else {
            0
        });
        let _ = host.world.tick();
        let pc = host.world.dialog.inline.as_ref().map(|i| i.pc);
        let clip = tslot.and_then(|(_, s)| {
            host.world
                .npcs
                .clip_cursors
                .get(&(s as u8))
                .map(|c| (c.anim_id, c.cursor))
        });
        let cue = tslot.and_then(|(_, s)| host.world.npcs.anim_cues.get(&(s as u8)).cloned());
        let now = (clip, cue.clone());
        if last.as_ref() != Some(&now) || f % 60 == 0 {
            eprintln!("f{f} pc={pc:x?} clip={clip:?} cue={cue:?}");
            last = Some(now);
        }
        if pc.is_none() && f > 20 {
            break;
        }
    }
}
