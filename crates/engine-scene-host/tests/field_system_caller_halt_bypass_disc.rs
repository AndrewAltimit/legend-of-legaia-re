//! Disc-gated: the scene's system script lands its cross-context ops on a
//! **halted** target.
//!
//! The field dispatcher refuses an extended op whose target holds the halt bit
//! `0x400` - unless the *calling* context is the system script
//! (`+0x50 == 0xFB`, `0x801DE938..0x801DE940` in `FUN_801DE840`). The port used
//! to test the target's id instead, so a system-script op aimed at a placed
//! object mid-walk was skipped.
//!
//! `conc2` is where it showed. Partition-0 record 11 (ひかり, the light shaft
//! over the plaza, mesh 54) opens its bind prologue with a compass walk
//! (`37 02 01`), which halts it. The entry script `P1[0]` then seats it
//! (`A3 0B 61 70`) and, outside the award-ceremony / heroes'-return beats,
//! parks both it and record 13 (貝, the clam on its pedestal, mesh 12) with
//! `A3 0D 7F 7F` / `A3 0B 7F 7F`. Skipping the light's two ops left the shaft
//! standing over an empty floor while the clam was parked - "an NPC drawn as
//! a pipe, the clam missing". A live retail `conc2` field state
//! (`conc2_field_route_walk`, flags `0x3E1` / `0x4EE` / `0x422` clear) holds
//! the clam at the hide box `(16320, 16320)` and the light at
//! `(16192, 16320)`: parked, then walked its two `-64` steps.
//!
//! Skips (and passes) when `LEGAIA_DISC_BIN` is unset or the disc cannot be
//! opened.

use legaia_engine_scene_host::scene_host::SceneHost;
use std::path::PathBuf;

fn open_host() -> Option<SceneHost> {
    std::env::var_os("LEGAIA_DISC_BIN")?;
    for c in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(c);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return SceneHost::open_extracted(&d).ok();
        }
    }
    let disc = std::env::var_os("LEGAIA_DISC_BIN")?;
    SceneHost::open_disc(PathBuf::from(disc)).ok()
}

/// Flat partition-0 records: the light shaft and the clam.
const LIGHT: usize = 11;
const CLAM: usize = 13;

/// `(x, z)` of an object-bind channel after `ticks` field ticks of a `conc2`
/// entry with `flags` raised.
fn conc2_objects(flags: &[u16], ticks: usize) -> Option<[(i16, i16); 2]> {
    let mut host = open_host()?;
    host.load_scene("conc2").expect("load conc2");
    for &f in flags {
        host.world.system_flag_set(f);
    }
    host.enter_field_scene("conc2", 0).expect("enter conc2");
    host.world.mode = legaia_engine_mode::mode::SceneMode::Field;
    for _ in 0..ticks {
        let _ = host.world.tick();
    }
    let pos = |record: usize| {
        let c = host
            .world
            .field_vm
            .channels
            .iter()
            .find(|c| c.object_bind && c.placement_index == record)
            .expect("conc2 binds the record");
        (c.ctx.world_x as i16, c.ctx.world_z as i16)
    };
    Some([pos(LIGHT), pos(CLAM)])
}

#[test]
fn conc2_entry_parks_the_light_shaft_while_it_walks() {
    let Some([light, clam]) = conc2_objects(&[], 30) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset or disc/extracted tree missing");
        return;
    };
    assert_eq!(clam, (16320, 16320), "the clam is parked at the hide box");
    assert_eq!(
        light,
        (16192, 16320),
        "the light is parked, then its compass walk steps it twice - retail RAM's seat"
    );
}

#[test]
fn conc2_award_ceremony_keeps_the_light_over_the_clam() {
    // `0x3E1` takes the entry script's skip arm: the light is seated on the
    // clam's tile and neither is parked.
    let Some([light, clam]) = conc2_objects(&[0x3E1], 30) else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset or disc/extracted tree missing");
        return;
    };
    assert_eq!(clam, (12352, 14400), "the clam stays on its pedestal seat");
    // `A3 0B 61 70` seats tile (97, 112) = (12480, 14400); the walk then
    // steps it `-128` along X onto the clam.
    assert_eq!(
        light,
        (12352, 14400),
        "the light shaft stands over the clam"
    );
}
