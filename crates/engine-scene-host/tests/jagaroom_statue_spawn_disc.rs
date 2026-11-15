//! Disc-gated: `jagaroom`'s six statue triggers (`P1[2..=7]`) finish their
//! spawn sections inside the load slice.
//!
//! Each record seats itself with a run-to-tile (`4C 51 <x> <z> 00 23`, which
//! raises its halt bit `0x400`), then aims the cross-context halt-acquire at
//! its own id (`CC <self> 85 00 00 26`: budget `0`, facing flat `0x26`, the
//! figure the six stand around) and raises its no-shadow bit
//! (`B1 <self> 15`). Both pokes name a context that is mid-seat, which the
//! dispatcher refuses outside a spawn (`0x801DE90C..0x801DE944`) and lets
//! through while `FUN_8003AEB0` holds the spawn bracket
//! (`0x8003B73C..0x8003B928`).
//!
//! `jagaroom_field_door_hop` holds flats 23..=28 with `+0x10` bits 21 and 22
//! up, `0x400` clear, on the six tiles below, each heading the bearing to
//! flat `0x26` at `(5056, 1472)`.
//!
//! Skips + passes when `LEGAIA_DISC_BIN` / extracted assets are missing.

use legaia_engine_scene_host::scene_host::SceneHost;
use std::path::PathBuf;

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
    eprintln!("[skip] extracted/ missing - run `legaia-extract` first");
    None
}

/// Placement, world seat and retail heading (`+0x26 & 0xFFF`) of each statue
/// in `jagaroom_field_door_hop`.
const STATUES: [(usize, (u16, u16), i16); 6] = [
    (2, (6336, 5184), 0x0D8),
    (3, (4288, 5184), 0xF7C),
    (4, (3648, 3520), 0xE78),
    (5, (6592, 320), 0x5A3),
    (6, (8000, 1344), 0x41C),
    (7, (8000, 3264), 0x29C),
];

#[test]
fn jagaroom_statues_finish_their_spawn_in_the_load_slice() {
    let Some(ex) = extracted_dir() else { return };
    let mut host = SceneHost::open_extracted(&ex).expect("open");
    host.enter_field_scene("jagaroom", 0)
        .expect("enter jagaroom");
    let chan = |host: &SceneHost, p: usize| {
        host.world
            .field_vm
            .channels
            .iter()
            .find(|c| !c.object_bind && c.placement_index == p)
            .map(|c| (c.pc, c.ctx.flags, (c.ctx.world_x, c.ctx.world_z)))
            .expect("statue channel")
    };
    for (p, _, _) in STATUES {
        let (pc, flags, _) = chan(&host, p);
        assert_eq!(pc, 0x56, "P1[{p}] ran to its idle loop in the load slice");
        assert_eq!(
            flags & 0x0060_0000,
            0x0060_0000,
            "P1[{p}]: `31 16` and `B1 <self> 15` both landed"
        );
    }
    for _ in 0..8 {
        host.tick().expect("tick");
        let _ = host.step_headless_frame_tail();
    }
    for (p, seat, heading) in STATUES {
        let (_, flags, at) = chan(&host, p);
        assert_eq!(flags & 0x400, 0, "P1[{p}]: the face leg dropped the halt");
        assert_eq!(at, seat, "P1[{p}] stands on its run-to-tile seat");
        assert_eq!(
            host.world.npcs.headings.get(&(p as u8)).map(|h| h & 0xFFF),
            Some((heading + 0x800) & 0xFFF),
            "P1[{p}] faces flat 0x26"
        );
    }
}
