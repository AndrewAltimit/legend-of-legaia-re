//! Disc-gated: the Water Gate Controller (`suimon` `P1[4]`) is an undrawn
//! interaction trigger, and its spawn section hides it inside the load slice.
//!
//! The record's spawn section, with flags `0x2C7` / `0x27B` clear, runs
//! `53 66`, two `34 30` triggers, a compass walk on its own actor
//! (`41 05 01`, which raises the actor's halt bit `0x400`) and then
//! `CC 0E 40 00 00 00 00` (render scale `+0x72 = 0`), `B1 0E 15` (`+0x10 |=
//! 0x200000`, no drop shadow) and the `0x21` that ends the slice. Both pokes
//! name the record's own id, `0x0E`, on a context that is mid-walk. Outside
//! a spawn the dispatcher refuses that (`0x801DE90C..0x801DE944`), but the
//! test reads the scene word `*(_DAT_801C6EA4) + 8` first, and
//! `FUN_8003AEB0` holds it at `1` around the whole spawn loop
//! (`0x8003B73C..0x8003B928`). So retail's slice runs on, the trigger never
//! draws, and the talk starts at the record's text (`+0x28`).
//!
//! The port once refused the poke, which parked the section on it: the
//! trigger's marker mesh stood at the controller at full size until the key
//! was used (the flags then skip the walk), and the talk entered on the hide.
//!
//! Skip-passes when `LEGAIA_DISC_BIN` / `extracted/` are missing.

use std::path::PathBuf;

use legaia_engine_scene_host::scene_host::SceneHost;

fn extracted_root() -> Option<PathBuf> {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return None;
    }
    let candidates = std::env::var_os("LEGAIA_EXTRACTED_DIR")
        .map(PathBuf::from)
        .into_iter()
        .chain(["extracted", "../extracted", "../../extracted"].map(PathBuf::from));
    for d in candidates {
        if d.join("CDNAME.TXT").exists() {
            return Some(d);
        }
    }
    eprintln!("[skip] extracted/ missing - run `legaia-extract` first");
    None
}

/// `suimon`'s partition-1 controller record.
const CONTROLLER: usize = 4;

#[test]
fn water_gate_controller_spawns_hidden_or_skip() {
    let Some(root) = extracted_root() else { return };
    let mut host = SceneHost::open_extracted(&root).expect("open SceneHost");
    host.enter_field_scene("suimon", 0).expect("enter suimon");
    let chan = |host: &SceneHost| {
        host.world
            .field_vm
            .channels
            .iter()
            .find(|c| !c.object_bind && c.placement_index == CONTROLLER)
            .map(|c| (c.pc, c.ctx.field_72, c.ctx.flags))
            .expect("P1[4] has a channel")
    };
    let (pc, scale, flags) = chan(&host);
    assert_eq!(
        scale, 0,
        "the controller trigger draws at scale 0 from the load frame"
    );
    assert_ne!(flags & 0x0020_0000, 0, "B1 0E 15 ran: no drop shadow");
    assert_eq!(
        pc, 0x28,
        "the load slice ended on the 0x21 before the talk text"
    );
    assert_eq!(
        host.world.field_npc_render_scale(CONTROLLER),
        Some(0),
        "both hosts skip a zero-scale placement"
    );
    // It stays hidden after the walk lands and the halt bit drops.
    for _ in 0..8 {
        host.tick().expect("tick");
    }
    let (_, scale, flags) = chan(&host);
    assert_eq!(scale, 0);
    assert_eq!(flags & 0x400, 0, "the compass walk landed");
}
