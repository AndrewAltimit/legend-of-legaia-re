//! Disc-gated: the overworld **map display** end to end, on every kingdom.
//!
//! Enters each kingdom map through `SceneHost::enter_world_map_scene`, presses
//! L1 through the engine's pad, and follows the retail chain: the fade-up
//! ramp (`FUN_800196A4`), the hand-off, PROT 0981's display tick
//! (`FUN_801CE850`, `legaia_engine_field::map_display`). It then asserts at
//! the output - the label list a host draws - rather than at the call site:
//!
//! - the scene load installed the kingdom MAN's location table
//!   (`DAT_80073EE0`), all 29 records of it;
//! - with no discovery flag set the display draws no label;
//! - raising one record's flag draws exactly the labels sharing that flag,
//!   on the display, with the record's own name;
//! - with every flag raised the labels are exactly this kingdom's records;
//! - the mini-map panel is on the screen-primitive list, and Circle leaves.
//!
//! Skip-passes without disc data / extracted assets (CLAUDE.md convention).

use std::path::PathBuf;

use legaia_engine_field::map_display::MODE_INTERACTIVE;
use legaia_engine_scene_host::scene_host::SceneHost;
use legaia_engine_screens::world_map_labels::{
    map_display_compass_marks, map_display_labels_now, map_display_prims,
};
use legaia_engine_system::input::PadButton;

fn extracted_dir() -> Option<PathBuf> {
    for p in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(p);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    None
}

fn frame(host: &mut SceneHost, pad: u16) {
    host.world.set_pad(pad);
    host.tick().expect("tick");
}

#[test]
fn l1_opens_the_map_display_and_labels_the_discovered_places() {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return;
    }
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing");
        return;
    };
    let mut host = SceneHost::open_extracted(&extracted).expect("open SceneHost");
    for (kingdom, scene) in ["map01", "map02", "map03"].into_iter().enumerate() {
        host.enter_world_map_scene(scene).expect("enter kingdom");
        // Let the entry script settle before the pad is read.
        for _ in 0..30 {
            frame(&mut host, 0);
        }
        let locations = host.world.world_map.locations.clone();
        assert_eq!(locations.len(), 29, "{scene}: the whole table is installed");
        let mine: Vec<usize> = locations
            .iter()
            .enumerate()
            .filter(|(_, l)| usize::from(l.region) == kingdom)
            .map(|(i, _)| i)
            .collect();
        assert!(!mine.is_empty(), "{scene}: the kingdom has places");
        for l in &locations {
            host.world.system_flag_clear(l.discovery_flag);
        }

        // L1 (raw 0x0400), then the ramp and the display's own fade-in.
        frame(&mut host, PadButton::L1 as u16);
        let mut opened = false;
        for _ in 0..120 {
            frame(&mut host, 0);
            if host
                .world
                .world_map
                .map_display
                .as_ref()
                .is_some_and(|d| d.mode == MODE_INTERACTIVE)
            {
                opened = true;
                break;
            }
        }
        assert!(opened, "{scene}: L1 never reached the interactive display");
        frame(&mut host, 0);

        assert!(
            map_display_labels_now(&host.world).is_empty(),
            "{scene}: an undiscovered kingdom draws no label"
        );

        // One place discovered: its label (and any sharing its flag) draws.
        let first = mine[0];
        let flag = locations[first].discovery_flag;
        host.world.system_flag_set(flag);
        let labels = map_display_labels_now(&host.world);
        let want: Vec<usize> = mine
            .iter()
            .copied()
            .filter(|&i| locations[i].discovery_flag == flag)
            .collect();
        assert_eq!(
            labels.iter().map(|l| l.record).collect::<Vec<_>>(),
            want,
            "{scene}: exactly the discovered records draw"
        );
        assert_eq!(labels[0].name, locations[first].name);
        assert!(!labels[0].name.is_empty());

        // Everything discovered: this kingdom's records, no other's.
        for l in &locations {
            host.world.system_flag_set(l.discovery_flag);
        }
        let labels = map_display_labels_now(&host.world);
        assert_eq!(
            labels.iter().map(|l| l.record).collect::<Vec<_>>(),
            mine,
            "{scene}: the kingdom filter selects the visible subset"
        );
        // The survey frames the player's surroundings: at least one label
        // lands on the 320x240 display.
        let on_screen = labels
            .iter()
            .filter(|l| (0..320).contains(&l.sx) && (0..240).contains(&l.sy))
            .count();
        eprintln!(
            "{scene}: {} labels, {on_screen} on screen: {:?}",
            labels.len(),
            labels.iter().map(|l| (l.sx, l.sy)).collect::<Vec<_>>()
        );

        // The mini-map panel: 3 sprites + the backing + 4 compass plates.
        assert_eq!(map_display_prims(&host.world).len(), 8, "{scene}");
        assert_eq!(map_display_compass_marks(&host.world).len(), 4, "{scene}");

        // Step the cursor to a place (Square): from the player (`-1`) the
        // location stepper `FUN_801CF5E8` wraps backward to this kingdom's
        // last record (`0x801CEE8C..0x801CEEC4`), the survey seats on its
        // cell, and its label lands at the display's centre column.
        frame(&mut host, PadButton::Square as u16);
        frame(&mut host, 0);
        frame(&mut host, 0);
        let d = host.world.world_map.map_display.as_ref().expect("display");
        assert_eq!(
            d.cursor,
            *mine.last().expect("kingdom places") as i32,
            "{scene}: Square steps from the player to the kingdom's last place"
        );
        let rec = d.cursor as usize;
        let want = (
            -(i32::from(locations[rec].map_x) << 7),
            -(i32::from(locations[rec].map_y) << 7),
        );
        // The survey camera is clamped to the kingdom interior
        // (`FUN_801CF678`), so a place near the edge seats short of its cell.
        let (mut cx, mut cz) = want;
        legaia_engine_field::map_display::clamp_camera(&mut cx, &mut cz);
        assert_eq!(
            (d.cam_x, d.cam_z),
            (cx, cz),
            "{scene}: the survey seats on record {rec}'s cell"
        );
        let labels = map_display_labels_now(&host.world);
        let l = labels.iter().find(|l| l.record == rec).expect("its label");
        if (cx, cz) == want {
            assert!(
                (l.sx - 160).abs() <= 2,
                "{scene}: record {rec} under the camera projects to the centre column, got {l:?}"
            );
        }
        eprintln!("[ok] {scene}: Square stepped to record {rec}");

        // Circle leaves, and the walk frame comes back.
        frame(&mut host, PadButton::Circle as u16);
        let mut closed = false;
        for _ in 0..32 {
            frame(&mut host, 0);
            if host.world.world_map.map_display.is_none() {
                closed = true;
                break;
            }
        }
        assert!(closed, "{scene}: Circle never closed the display");
        assert!(map_display_labels_now(&host.world).is_empty());
    }
}
