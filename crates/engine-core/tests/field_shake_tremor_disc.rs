//! Disc-gated: `tunnelc`'s **hammer tremor** cycles the camera shake the way
//! its script authors it, stops when the script stops it, and never follows
//! the player through a door.
//!
//! The scene-entry system script (`P1[0]`) runs a per-frame timer on slot `1`
//! of the script-counter table `0x801C6460` while system flag `0x360` is set:
//! `+1` a frame, the shake amplitude (`[4C 84 amp]`) raised to `2` on counts
//! `1..=10` and dropped to `0` on `11..=30`, wrapping by `-30`. Xain's
//! interaction script raises the flag with the slot at `30` to start it and
//! writes `80` to stop it: the loop's `79 < slot` exit clears the flag. With
//! the slot table unmodelled (reads `0`, writes dropped) the loop fell to the
//! shake arm every frame - a constant tremor that nothing could stop.
//!
//! The scene reset `FUN_8003A024` zeroes the amplitude on every scene load
//! (`0x8003A07C`), so whatever the script left running ends at the door.
//!
//! Skips silently when `extracted/` or `LEGAIA_DISC_BIN` is missing.

use std::path::PathBuf;

use legaia_engine_core::scene::{DefaultMapIdResolver, SceneHost};

const TREMOR_FLAG: u16 = 0x360;
const TREMOR_SLOT: usize = 1;

fn extracted_dir() -> Option<PathBuf> {
    for p in ["extracted", "../../extracted"] {
        let d = PathBuf::from(p);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    None
}

fn open_host() -> Option<SceneHost> {
    let Some(extracted) = extracted_dir() else {
        eprintln!("[skip] extracted/ missing");
        return None;
    };
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset");
        return None;
    }
    let mut host = SceneHost::open_extracted(&extracted).expect("open SceneHost");
    host.set_map_resolver(Box::new(DefaultMapIdResolver::from_index(&host.index)));
    Some(host)
}

fn settle(host: &mut SceneHost, frames: usize) {
    for _ in 0..frames {
        host.world.set_pad(0);
        let _ = host.world.tick();
    }
}

#[test]
fn tunnelc_tremor_cycles_stops_and_ends_at_the_door() {
    let Some(mut host) = open_host() else {
        return;
    };
    host.enter_field_scene("tunnelc", 0)
        .expect("enter_field_scene('tunnelc')");
    settle(&mut host, 120);
    eprintln!("[ran] tunnelc entered");
    assert_eq!(
        host.world.camera.shake_amplitude, 0,
        "no tremor before the flag is raised"
    );

    // Xain's start arm: `53 60` + `4C CA 01 1E 00`.
    host.world.system_flag_set(TREMOR_FLAG);
    host.world.field_vm.slot_table[TREMOR_SLOT] = 30;
    let mut amps = Vec::new();
    for _ in 0..120 {
        host.world.set_pad(0);
        let _ = host.world.tick();
        amps.push(host.world.camera.shake_amplitude);
    }
    let on = amps.iter().filter(|&&a| a == 2).count();
    let off = amps.iter().filter(|&&a| a == 0).count();
    eprintln!("[tunnelc] tremor over 120 frames: {on} shaking, {off} still");
    assert_eq!(
        on + off,
        amps.len(),
        "the loop only writes 0 or 2: {amps:?}"
    );
    assert!(on > 0, "the tremor must run while the flag is set");
    assert!(
        off > on,
        "10 shaking frames in each 30-frame cycle, not a constant shake: {amps:?}"
    );
    assert!(host.world.system_flag_test(TREMOR_FLAG));

    // Xain's stop arm: `4C CA 01 50 00` - the loop's `79 < slot` exit.
    host.world.field_vm.slot_table[TREMOR_SLOT] = 80;
    settle(&mut host, 4);
    assert!(
        !host.world.system_flag_test(TREMOR_FLAG),
        "the loop's exit clears the tremor flag"
    );
    assert_eq!(host.world.camera.shake_amplitude, 0);
    settle(&mut host, 60);
    assert_eq!(host.world.camera.shake_amplitude, 0, "and it stays stopped");

    // A tremor still running at the door does not follow the player.
    host.world.camera.shake_amplitude = 2;
    host.enter_field_scene("tunnelb", 0)
        .expect("enter_field_scene('tunnelb')");
    assert_eq!(
        host.world.camera.shake_amplitude, 0,
        "FUN_8003A024 zeroes the amplitude on the scene load"
    );
    settle(&mut host, 60);
    assert_eq!(host.world.camera.shake_amplitude, 0);
}
