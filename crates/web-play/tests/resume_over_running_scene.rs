//! A save that names no scene, loaded over the scene the page is running,
//! re-enters that scene under the save's flags - as the window does, which
//! lands the same file through `--scene` - so the scene's spawn scripts read
//! the loaded flags. Loaded over the running scene without an entry, two of
//! `town01`'s villagers kept the clips they had spawned on before the load
//! (`31` / `62` where the save seats `29` / `60`), from the first frame.
//!
//! Disc-gated, and keyed on the port-written `saves/slot_00.bin` (a save
//! with no scene label): skips and passes without either.

use legaia_web_play::runtime::LegaiaRuntime;

/// The NPC clips of the host trace's binding line: each slot's clip and
/// its length, not its playhead (a clip-phase draw takes `rand()`, which the
/// cold entry ahead of the landing has already drawn from).
fn clip_line(rt: &LegaiaRuntime) -> String {
    let t = rt.debug_host_trace_text();
    let line = t
        .lines()
        .find(|l| l.starts_with("bindings_parts"))
        .unwrap_or_default();
    let Some(i) = line.find("PropAnim") else {
        return String::new();
    };
    line[i..]
        .split(", cursor: ")
        .enumerate()
        .map(|(k, part)| {
            if k == 0 {
                part
            } else {
                part.split_once(',').map_or(part, |(_, rest)| rest)
            }
        })
        .collect::<Vec<_>>()
        .join(",")
}

#[test]
fn a_scene_less_save_over_the_running_scene_spawns_as_its_entry() {
    let Some(disc) = std::env::var("LEGAIA_DISC_BIN").ok() else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated)");
        return;
    };
    let save_path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../saves/slot_00.bin");
    let (Ok(bytes), Ok(save)) = (std::fs::read(&disc), std::fs::read(save_path)) else {
        eprintln!("[skip] disc or saves/slot_00.bin unreadable");
        return;
    };
    let land = |over_town: bool| {
        let mut rt = LegaiaRuntime::new();
        rt.load_disc(bytes.clone(), String::new())
            .expect("load_disc");
        if over_town {
            rt.enter_field("town01").expect("enter town01");
        }
        let cold = clip_line(&rt);
        rt.import_save(save.clone()).expect("import");
        let landing: serde_json::Value =
            serde_json::from_str(&rt.play_resume_save().expect("resume")).expect("json");
        (cold, landing, clip_line(&rt))
    };
    let (_, fresh, want) = land(false);
    let (cold, over, got) = land(true);
    assert_eq!(fresh["scene"], "town01", "the fresh landing enters town01");
    assert_eq!(over["landing"], "current");
    assert_eq!(over["entered"], true, "the running scene re-entered");
    // Non-vacuity: town01 entered cold seats other clips than the save does.
    assert_ne!(cold, want, "the cold entry's clips differ from the save's");
    assert_eq!(got, want, "the landing seats the save's clips");
}
