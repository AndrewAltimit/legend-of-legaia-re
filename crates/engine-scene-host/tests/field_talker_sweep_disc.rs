//! Disc-gated: every talker still speaks.
//!
//! A talk runs the placement's own record from its live cursor (retail
//! `actor[+0x9E]`), and an executed `0x21` before any box ends it with
//! nothing said (`FUN_80039B7C`). The port once covered such a talk with the
//! record's first text segment; with that net gone, a record whose cursor the
//! port seats wrongly is a silent NPC, and nothing else in the suite would
//! notice (the full-game ladder talks to story NPCs only).
//!
//! The sweep enters every CDNAME field scene under a set of story-flag
//! states - a cold bank, then the bank of a milestone of the playthrough
//! spine (`fixtures/spine_anchor_flags.txt`, flag ids only) - stands the
//! player on each standable tile around each talkable placement and talks,
//! every talk from a fresh entry. A talk that opens no box where the old
//! first-segment net would have opened one is a **lost line**, and each
//! record that loses one is accounted for in [`RETAIL_SILENT`] with the
//! reason its own bytes give. A lost line outside that table fails the test,
//! and so does a table row the sweep no longer finds.
//!
//! The default run is the cold bank and one late milestone, which between
//! them reach every row of the table (about a minute and a half).
//! `LEGAIA_TALKER_SWEEP=full` runs all thirty-six flag states (about
//! twenty-seven minutes, the nightly shape) and `=cold` the cold bank alone.
//! `LEGAIA_TALKER_SWEEP_LOG=<path>` writes one line per silent talk, and
//! `LEGAIA_TALKER_SWEEP_SCENE=<name>` narrows the sweep to one scene.
//! `LEGAIA_TALKER_CURSOR_LOG=<path>` writes every talker's cursor instead
//! (`scene slot state entry_pc live|static`) and talks to nobody: the engine side of a
//! comparison with the `actor[+0x9E]` words of retail save states.
//!
//! Skips without `LEGAIA_DISC_BIN` / `extracted/` (disc-gated convention).

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use legaia_engine_scene::scene::is_world_map_scene;
use legaia_engine_scene_host::scene_host::SceneHost;
use legaia_engine_system::input::PadButton;

const ANCHOR_FLAGS: &str = include_str!("fixtures/spine_anchor_flags.txt");

/// The late milestone the default run adds to the cold bank: every flag-gated
/// row of [`RETAIL_SILENT`] is silent under it.
const LATE_STATE: &str = "rogue_tower";

/// Every record a talk leaves silent where the first-segment net spoke:
/// `(scene, P1 slot, whether the cold bank shows it, why retail says nothing)`.
///
/// In each the cursor a talk starts from is one past the spawn section's
/// executed `0x21`, and the retail states that hold the scene carry the same
/// cursor at `actor[+0x9E]` (`rayman`, `bylon`, `doman`, `station3`,
/// `concnow`, `town0b`, `vell`). What follows is the record's own selector.
const RETAIL_SILENT: &[(&str, u8, bool, &str)] = &[
    // A player box test (`CD F8 x0 z0 x1 z1`) ahead of every line: the talk
    // speaks from inside the box and skips to the closing `0x21` outside it.
    ("rayman", 44, true, "player box (107, 70..=80)"),
    ("rayman", 45, true, "player box (107, 70..=80)"),
    ("rayman", 46, true, "player box (107, 70..=80)"),
    (
        "doman",
        18,
        true,
        "player box (40..=41, 81): the desk's front",
    ),
    (
        "station3",
        4,
        true,
        "player box (80..=81, 37..=38): the poster's front",
    ),
    // No text at all: `31 1D`, `31 00`, `31 1F`, `4C 42 50 ..`, `38 82 00`,
    // then a `21` / `26 FE FF` park. The net's "first segment" was the
    // `1F` operand of `31 1F` read as a text lead.
    (
        "bylon",
        39,
        true,
        "textless record, parked on 21 / 26 FE FF",
    ),
    (
        "bylon",
        41,
        true,
        "textless record, parked on 21 / 26 FE FF",
    ),
    (
        "bylon",
        42,
        true,
        "textless record, parked on 21 / 26 FE FF",
    ),
    (
        "edbylon",
        27,
        true,
        "textless record, parked on 21 / 26 FE FF",
    ),
    (
        "edbylon",
        29,
        true,
        "textless record, parked on 21 / 26 FE FF",
    ),
    (
        "edbylon",
        30,
        true,
        "textless record, parked on 21 / 26 FE FF",
    ),
    // A story-flag test at the cursor that jumps to a bare `21` and back:
    // the record has nothing to say once the flag is up.
    ("concnow", 12, false, "75 F8 -> 21: flag 0x5F8"),
    ("concnow", 13, false, "75 F8 -> 21: flag 0x5F8"),
    ("dolk", 27, false, "71 7B / 71 78 / 71 42 -> 21"),
    ("station3", 6, false, "73 8F -> 21: flag 0x38F"),
    ("station3", 8, false, "73 8F -> 21: flag 0x38F"),
    ("town0b", 36, false, "75 C4 -> 21: flag 0x5C4"),
    ("town0c", 21, false, "75 C4 -> 21: flag 0x5C4"),
    // `72 AF` to the player box test, then `72 B0` (the beat already played)
    // to the closing `21`.
    ("vell", 3, false, "72 B0 -> 21: flag 0x2B0"),
];

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

/// `(milestone, flag ids)` rows of the fixture, the cold bank first.
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

/// Enter `scene` with exactly `flags` raised.
fn enter(host: &mut SceneHost, scene: &str, flags: &[u16]) -> bool {
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
    for _ in 0..4 {
        let _ = host.world.tick();
    }
    host.world.mode == legaia_engine_mode::mode::SceneMode::Field
        && host.world.dialog.inline.is_none()
        && host.world.dialog.current.is_none()
}

fn out_of_reach(host: &SceneHost, slot: u8) -> bool {
    let hide = legaia_engine_field::man_field_scripts::FIELD_OFFMAP_HIDE_XZ;
    host.world.field_placement_retired(slot)
        || host.world.npcs.positions.get(&slot) == Some(&(hide, hide))
}

/// The standable tiles around the tile `(tx, tz)`.
fn stands(host: &SceneHost, tx: i32, tz: i32) -> Vec<(i32, i32)> {
    let mut out = Vec::new();
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
                out.push((sx, sz));
            }
        }
    }
    out
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Talk {
    /// A box opened.
    Spoke,
    /// The talk ended with no box.
    Silent,
    /// The talk left the field (a fight, a scene change, a minigame) or
    /// never ended inside the budget: the record did something.
    Acted,
}

/// One talk on `slot` from tile `stand`, run to its first box or its end.
fn talk(host: &mut SceneHost, slot: u8, stand: (i32, i32)) -> Talk {
    let pslot = usize::from(host.world.player_actor_slot.expect("a player"));
    {
        let ms = &mut host.world.actors[pslot].move_state;
        ms.world_x = (stand.0 * 128 + 0x40) as i16;
        ms.world_z = (stand.1 * 128 + 0x40) as i16;
    }
    let scene = host.world.active_scene_label.clone();
    host.world.trigger_field_interact(0, slot);
    let mut started = false;
    for f in 0..3_000 {
        host.world.input.set_pad(if f % 4 == 0 {
            PadButton::Cross.mask()
        } else {
            0
        });
        let _ = host.world.tick();
        if host.world.mode != legaia_engine_mode::mode::SceneMode::Field
            || host.world.active_scene_label != scene
        {
            return Talk::Acted;
        }
        if host
            .world
            .dialog
            .inline
            .as_ref()
            .is_some_and(|id| id.panel.is_some())
        {
            return Talk::Spoke;
        }
        let live = host.world.dialog.inline.is_some() || host.world.dialog.current.is_some();
        started |= live;
        if (started && !live) || (!started && f > 8) {
            return Talk::Silent;
        }
    }
    Talk::Acted
}

/// One silent talk: where, under which flags, from which tile.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct Silent {
    scene: String,
    slot: u8,
    state: String,
    stand: (i32, i32),
    npc: (i32, i32),
    entry_pc: usize,
    first_segment: usize,
    /// The first-talk fallback this wave removed would have opened a box.
    spoke_with_fallback: bool,
}

/// Every talker's cursor after the spawn pre-run, per flag state.
fn cursors(states: &[(String, Vec<u16>)]) -> Option<String> {
    let dir = extracted_dir()?;
    let mut host = SceneHost::open_extracted(&dir).ok()?;
    let mut names = host.index.cdname_scene_names();
    names.sort();
    names.dedup();
    names.retain(|n| !is_world_map_scene(n));
    let mut out = String::new();
    for (state, flags) in states {
        for scene in &names {
            if !enter(&mut host, scene, flags) {
                continue;
            }
            let mut recs: Vec<_> = host.world.npcs.dialog_prologue.iter().collect();
            recs.sort_by_key(|(slot, _)| **slot);
            for (slot, rec) in recs {
                out.push_str(&format!(
                    "{scene} {slot} {state} {:#x} {}\n",
                    rec.entry_pc,
                    if rec.resume { "live" } else { "static" }
                ));
            }
        }
    }
    Some(out)
}

/// Every talker but one starts its talk from the cursor the spawn pre-run
/// left, which is the cursor retail's load slice leaves (`FUN_8003A1E4`); the
/// retail save states of the library hold the same `actor[+0x9E]` for every
/// placement nobody has talked to yet.
///
/// The one is `dolk2` `P1[13]`. Its record opens on `38 87 00` (face the
/// compass point), not on `24` / `25`, so retail's load slice does not run it
/// (`0x8003A480..0x8003A48C`) and its cursor stays on the record's first
/// opcode: `dolk2_field_door_hop` holds `+0x9E = 9`. A first talk there runs
/// the `38` and the `21` behind it and ends with nothing said, and the second
/// speaks. The port starts it one past that `21` (the static entry,
/// `placement_interaction_entry_pc`) and speaks on the first press: a
/// deliberate kindness over a retail oversight, pinned here so a second such
/// record cannot arrive unseen.
#[test]
fn one_talker_alone_keeps_a_static_cursor() {
    let Some(dir) = extracted_dir() else {
        return;
    };
    let mut host = SceneHost::open_extracted(&dir).expect("open the host");
    let mut names = host.index.cdname_scene_names();
    names.sort();
    names.dedup();
    names.retain(|n| !is_world_map_scene(n));
    let (mut talkers, mut statics) = (0usize, Vec::new());
    for scene in &names {
        if !enter(&mut host, scene, &[]) {
            continue;
        }
        for (&slot, rec) in &host.world.npcs.dialog_prologue {
            talkers += 1;
            if !rec.resume {
                statics.push((scene.clone(), slot, rec.entry_pc));
            }
        }
    }
    assert!(talkers > 1_500, "only {talkers} talkers");
    assert_eq!(statics, vec![("dolk2".to_string(), 13, 0xD)]);
}

fn sweep(states: &[(String, Vec<u16>)]) -> Option<(Vec<Silent>, usize, usize)> {
    let dir = extracted_dir()?;
    let mut host = SceneHost::open_extracted(&dir).ok()?;
    let mut names = host.index.cdname_scene_names();
    names.sort();
    names.dedup();
    names.retain(|n| !is_world_map_scene(n));
    let only = std::env::var("LEGAIA_TALKER_SWEEP_SCENE").ok();
    let mut silent = Vec::new();
    let (mut talks, mut spoke) = (0usize, 0usize);
    for (state, flags) in states {
        for scene in &names {
            if only.as_deref().is_some_and(|o| o != scene) {
                continue;
            }
            if !enter(&mut host, scene, flags) {
                continue;
            }
            let mut slots: Vec<u8> = host.world.npcs.dialog_prologue.keys().copied().collect();
            slots.sort_unstable();
            for slot in slots {
                let mut fresh = true;
                let Some(&(nx, nz)) = host.world.npcs.positions.get(&slot) else {
                    continue;
                };
                if out_of_reach(&host, slot) {
                    continue;
                }
                let npc = (i32::from(nx - 0x40) >> 7, i32::from(nz - 0x40) >> 7);
                for stand in stands(&host, npc.0, npc.1) {
                    // Every talk starts from the scene's entry state.
                    if !fresh && !enter(&mut host, scene, flags) {
                        break;
                    }
                    fresh = false;
                    if out_of_reach(&host, slot) {
                        break;
                    }
                    let entry_pc = host.world.npcs.dialog_prologue[&slot].entry_pc;
                    let first_segment = host.world.npcs.dialog_prologue[&slot].first_segment;
                    talks += 1;
                    match talk(&mut host, slot, stand) {
                        Talk::Spoke => spoke += 1,
                        Talk::Acted => {}
                        Talk::Silent => {
                            // The same talk as the port ran it before: a
                            // first talk took the first text segment.
                            let before = enter(&mut host, scene, flags) && {
                                if let Some(rec) = host.world.npcs.dialog_prologue.get_mut(&slot) {
                                    rec.resume = false;
                                }
                                talk(&mut host, slot, stand) == Talk::Spoke
                            };
                            silent.push(Silent {
                                scene: scene.clone(),
                                slot,
                                state: state.clone(),
                                stand,
                                npc,
                                entry_pc,
                                first_segment,
                                spoke_with_fallback: before,
                            });
                        }
                    }
                }
                // The next placement starts clean too.
                if !fresh && !enter(&mut host, scene, flags) {
                    break;
                }
            }
        }
    }
    Some((silent, talks, spoke))
}

#[test]
fn every_talker_speaks_or_is_silent_in_retail() {
    let all = flag_states();
    let mode = std::env::var("LEGAIA_TALKER_SWEEP").unwrap_or_default();
    let states: Vec<_> = match mode.as_str() {
        "full" => all,
        "cold" => all.into_iter().take(1).collect(),
        _ => all
            .into_iter()
            .filter(|(n, _)| n == "cold" || n == LATE_STATE)
            .collect(),
    };
    if let Ok(path) = std::env::var("LEGAIA_TALKER_CURSOR_LOG") {
        if let Some(text) = cursors(&states) {
            std::fs::write(path, text).expect("write the cursor log");
        }
        return;
    }
    let t0 = std::time::Instant::now();
    let Some((silent, talks, spoke)) = sweep(&states) else {
        return;
    };
    if let Ok(path) = std::env::var("LEGAIA_TALKER_SWEEP_LOG") {
        let mut text = String::new();
        for s in &silent {
            text.push_str(&format!(
                "{} P1[{}] state={} stand=({},{}) npc=({},{}) entry_pc={:#x} first_segment={:#x} fallback_spoke={}\n",
                s.scene,
                s.slot,
                s.state,
                s.stand.0,
                s.stand.1,
                s.npc.0,
                s.npc.1,
                s.entry_pc,
                s.first_segment,
                s.spoke_with_fallback
            ));
        }
        std::fs::write(path, text).expect("write the sweep log");
    }
    let mut lost: BTreeMap<(String, u8), BTreeSet<String>> = BTreeMap::new();
    for s in silent.iter().filter(|s| s.spoke_with_fallback) {
        lost.entry((s.scene.clone(), s.slot))
            .or_default()
            .insert(s.state.clone());
    }
    eprintln!(
        "[talker sweep] {} flag state(s), {talks} talks, {spoke} spoke, {} silent, {} record(s) lost the net's line, {:.0?}",
        states.len(),
        silent.len(),
        lost.len(),
        t0.elapsed()
    );
    assert!(talks > 4_000 && spoke * 10 > talks * 9, "the sweep talked");
    if std::env::var_os("LEGAIA_TALKER_SWEEP_SCENE").is_some() {
        return;
    }
    let unexplained: Vec<String> = lost
        .iter()
        .filter(|((scene, slot), _)| {
            !RETAIL_SILENT
                .iter()
                .any(|(s, p, _, _)| s == scene && p == slot)
        })
        .map(|((scene, slot), states)| format!("{scene} P1[{slot}] under {states:?}"))
        .collect();
    assert!(
        unexplained.is_empty(),
        "talkers that say nothing where the record's first line used to open, and that no \
         RETAIL_SILENT row explains (read the record: retail-silent, or the port's cursor): \
         {unexplained:#?}"
    );
    // The table stays honest: a row the sweep no longer finds is stale.
    let cold_only = mode == "cold";
    let stale: Vec<String> = RETAIL_SILENT
        .iter()
        .filter(|(_, _, cold, _)| *cold || !cold_only)
        .filter(|(s, p, _, _)| !lost.contains_key(&(s.to_string(), *p)))
        .map(|(s, p, _, why)| format!("{s} P1[{p}] ({why})"))
        .collect();
    assert!(
        stale.is_empty(),
        "RETAIL_SILENT rows the sweep finds speaking: {stale:#?}"
    );
}
