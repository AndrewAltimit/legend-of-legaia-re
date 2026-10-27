//! Disc-gated: the scripted-motion VM's tint fade (op `0x0C`) reaches the
//! draw-tint tables both play hosts read, through `SceneHost::tick`.
//!
//! Retail's field actor draws hand the actor's `+0x74` tint and `+0x78`
//! draw-mode word to the mesh submit `FUN_80043390` (`0x8001B46C`,
//! `0x8001BC7C`), which stages them as the GTE far colour and `IR0`. The
//! census (`engine-scene`'s `ambient_motion_op_census_disc`) finds the op
//! bound to placed objects in `koin3` / `other7` / `ropeway` / `taiku` and to
//! NPCs in `taiku` / `bubu1` / `bubu2` / `garmel` / `edbubu`. This drives three of
//! those scenes from entry and samples what the hosts sample each frame:
//! `World::object_draw_tints` for placed objects and
//! `World::field_npc_draw_tint` for NPC slots.
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

fn open() -> Option<SceneHost> {
    if std::env::var_os("LEGAIA_DISC_BIN").is_none() {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return None;
    }
    let extracted = extracted_dir().or_else(|| {
        eprintln!("[skip] extracted/ missing - run `legaia-extract` first");
        None
    })?;
    Some(SceneHost::open_extracted(&extracted).expect("open extracted"))
}

#[test]
fn koin3_object_streams_fade_their_records() {
    let Some(mut host) = open() else { return };
    host.enter_field_scene("koin3", 0).expect("enter koin3");
    let mut tinted = std::collections::BTreeSet::new();
    let mut weights = std::collections::BTreeSet::new();
    for _ in 0..600 {
        host.tick().expect("tick");
        for (record, (_, blend)) in host.world.object_draw_tints() {
            tinted.insert(record);
            weights.insert(blend);
        }
    }
    eprintln!(
        "[motion tint] koin3 tinted records {tinted:?}, weights seen {}",
        weights.len()
    );
    assert!(
        tinted.iter().any(|r| (9..=12).contains(r)),
        "no op-0x0C-bound record (9..=12) drew tinted: {tinted:?}"
    );
    assert!(weights.len() > 1, "the fade never ramped: {weights:?}");
}

#[test]
fn taiku_npc_streams_tint_their_slots() {
    let Some(mut host) = open() else { return };
    host.enter_field_scene("taiku", 0).expect("enter taiku");
    // The NPC slots `taiku`'s `0x0C` streams bind (records 1..=3).
    let slots = [11usize, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21];
    let mut tinted = std::collections::BTreeSet::new();
    for _ in 0..600 {
        host.tick().expect("tick");
        for &s in &slots {
            if host.world.field_npc_draw_tint(s).is_some() {
                tinted.insert(s);
            }
        }
    }
    eprintln!("[motion tint] taiku tinted NPC slots {tinted:?}");
    assert!(!tinted.is_empty(), "no taiku NPC drew an op-0x0C tint");
}

/// `bubu2`'s NPC 7 carries two scripted-motion variants: a default that only
/// waits, and one gated on system flag `0x3D4` that pulses a blue tint
/// (`0C 50 50 9B 54 0B 06 00`, `05 08`, `0C 50 50 9B 98 08 06 00`, `05 08`,
/// loop): the mode word swings `0xB54 <-> 0x898` over six ticks each way. The
/// entry script tints the NPC on its own (`0x9B5050` at `3000`), so with the
/// flag down the pair stands still; with it up the motion VM's fade takes
/// the pair over and ramps it between the two authored modes.
#[test]
fn bubu2_npc7_pulses_its_blue_tint_once_its_flag_is_up() {
    let Some(mut host) = open() else { return };
    host.enter_field_scene("bubu2", 0).expect("enter bubu2");
    let mut idle = std::collections::BTreeSet::new();
    for _ in 0..120 {
        host.tick().expect("tick");
        if let Some((_, b)) = host.world.field_npc_draw_tint(7) {
            idle.insert(b);
        }
    }
    eprintln!(
        "[motion tint] bubu2 npc7 seat {:?}",
        host.world.npcs.positions.get(&7)
    );
    host.world.system_flag_set(0x3D4);
    let mut pulse = std::collections::BTreeSet::new();
    let mut colours = std::collections::BTreeSet::new();
    for _ in 0..240 {
        host.tick().expect("tick");
        if let Some((c, b)) = host.world.field_npc_draw_tint(7) {
            colours.insert(c);
            pulse.insert(b);
        }
    }
    eprintln!(
        "[motion tint] bubu2 npc7 idle modes {idle:?}, pulse modes {pulse:?}, colours {colours:?}"
    );
    assert!(
        idle.len() <= 1,
        "with 0x3D4 down the default variant only waits: {idle:?}"
    );
    assert_eq!(
        colours,
        [0x9B5050].into(),
        "the pulse keeps the authored blue"
    );
    assert!(
        pulse.contains(&0xB54) && pulse.contains(&0x898),
        "both authored modes reached: {pulse:?}"
    );
    assert!(
        pulse.len() > 4,
        "the mode word ramps between them: {pulse:?}"
    );
    // The first fade ramps from the entry script's standing `3000`, not from
    // an untinted `0`: the actor has one `+0x78`, and op `0x0C` ramps it.
    assert!(
        pulse.iter().all(|&b| b >= 0x898),
        "the pulse never dips below its authored floor: {pulse:?}"
    );
}
