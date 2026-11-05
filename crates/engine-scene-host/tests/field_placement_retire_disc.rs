//! Disc-gated: a retired placement leaves the map, and `4C D2` brings it back.
//!
//! `31 03` / `B1 <id> 03` raises a context's retire bit, and `FUN_8002519C`
//! stops walking the node (`0x800251E8`). The disc spends it on story gates in
//! spawn sections (`cave01`'s pickups), on the pickup record itself, and in
//! paired records: `balden` `P2[18]` retires ten placements' contexts and
//! `P2[19]` re-seats them with `4C D2 <id>`.
//!
//! The chain runs through `SceneHost::tick`, the entry point both play hosts
//! call per frame; what is asserted is `World::field_npc_live_anchor`, the
//! read both hosts draw an NPC from.
//!
//! Skips + passes when `LEGAIA_DISC_BIN` / extracted assets are missing.

use legaia_engine_scene_host::scene_host::SceneHost;
use std::path::PathBuf;

fn extracted_dir() -> Option<PathBuf> {
    for c in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(c);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    None
}

fn open_host() -> Option<SceneHost> {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return None;
    }
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing - run `legaia-extract` first");
        return None;
    };
    Some(SceneHost::open_extracted(&extracted).expect("open extracted"))
}

/// The ten channels `balden` `P2[18]` kills and `P2[19]` seats again.
const BALDEN_CHANNELS: [u8; 10] = [0x27, 0x28, 0x29, 0x26, 0x1B, 0x1C, 0x24, 0x2A, 0x2B, 0x2C];

fn drawn(host: &SceneHost, slot: u8) -> bool {
    host.world.field_npc_live_anchor(slot, (0, 0)).is_some()
}

fn run_record(host: &mut SceneHost, man: &[u8], record: usize) {
    let man_file = legaia_scene_formats::man_section::parse(man).expect("parse MAN");
    assert!(
        host.world
            .install_cutscene_timeline_record(&man_file, man, 2, record, false),
        "P2[{record}] installs"
    );
    // Both records wait 16 frames behind their fade and then park on a
    // `21` / `26` spin.
    for _ in 0..120 {
        host.tick().expect("tick");
    }
}

#[test]
fn balden_lamp_records_retire_and_respawn_their_placements() {
    let Some(mut host) = open_host() else {
        return;
    };
    host.enter_field_scene("balden", 0).expect("enter balden");
    let man = {
        let scene = host.scene.as_ref().expect("scene loaded");
        scene
            .field_man_payload(&host.index)
            .expect("MAN readable")
            .expect("balden has a MAN")
    };
    let n0 = legaia_scene_formats::man_section::parse(&man)
        .expect("parse MAN")
        .header
        .partition_counts[0] as u8;
    let slots: Vec<u8> = BALDEN_CHANNELS.iter().map(|c| c - n0).collect();
    let at_entry: Vec<bool> = slots.iter().map(|&s| drawn(&host, s)).collect();
    assert!(
        at_entry.iter().any(|&d| d),
        "the entry stands some of the ten on the map"
    );

    run_record(&mut host, &man, 18);
    for &s in &slots {
        assert!(!drawn(&host, s), "P2[18] retired slot {s}");
    }
    let live = legaia_engine_field::field_channels::resolve_target(
        &host.world.field_vm.channels,
        BALDEN_CHANNELS[0],
    );
    assert_eq!(live, None, "a retired context answers no lookup");

    // The re-seat runs each spawn section afresh, so a record's own story arm
    // may park it again. Everything the entry stood on the map stands, and
    // every context answers.
    run_record(&mut host, &man, 19);
    for ((&s, &was), &channel) in slots.iter().zip(&at_entry).zip(&BALDEN_CHANNELS) {
        assert!(!was || drawn(&host, s), "P2[19] re-seated slot {s}");
        assert!(
            legaia_engine_field::field_channels::resolve_target(
                &host.world.field_vm.channels,
                channel,
            )
            .is_some(),
            "the re-seated context {channel:#x} answers again"
        );
    }
    eprintln!(
        "[retire] balden: {} placements out and back, {} of them drawn at entry",
        slots.len(),
        at_entry.iter().filter(|&&d| d).count()
    );
}

#[test]
fn a_spawn_section_story_gate_retires_at_entry() {
    let Some(mut host) = open_host() else {
        return;
    };
    // `cave01` P1[3]: `71 5D 05 00` takes `31 03` with system flag `0x15D` up.
    host.enter_field_scene("cave01", 0).expect("enter cave01");
    assert!(drawn(&host, 3), "flag clear: the placement stands");

    host.world.system_flag_set(0x15D);
    host.enter_field_scene("cave01", 0)
        .expect("re-enter cave01");
    assert!(!drawn(&host, 3), "flag up: the spawn section retired it");
}
