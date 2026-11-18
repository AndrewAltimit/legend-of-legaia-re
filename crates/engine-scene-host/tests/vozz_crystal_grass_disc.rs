//! Disc-gated: `vozz`'s Crystal Grass leaves the frame when the Weed Hammer
//! cuts it.
//!
//! Every thicket across East Voz's paths is a placed object bound to
//! partition-0 record 1. Examined while story flag `0x079` (the Weed Hammer
//! grant, `vozz` `P1[1]`) is up, the record plays its cue, waits 32 frames and
//! runs its own `23 7F 7F`: the non-player arm of op `0x23`
//! (`FUN_801DE840`, `0x801DECAC`) stores the hide tile into the executing
//! actor's `+0x14` / `+0x18`, and retail's actor - the one that draws and the
//! one that collides - is that object. The port ran the record on a context
//! of its own and moved only the collider rows, so the player walked through
//! a thicket that stayed drawn.
//!
//! Skip-passes when `LEGAIA_DISC_BIN` / `extracted/` are missing.

use legaia_engine_scene_host::scene_host::SceneHost;
use std::path::PathBuf;

fn open_host() -> Option<SceneHost> {
    let Some(disc) = std::env::var_os("LEGAIA_DISC_BIN") else {
        eprintln!("[skip] LEGAIA_DISC_BIN unset (disc-gated convention)");
        return None;
    };
    for c in ["extracted", "../extracted", "../../extracted"] {
        let d = PathBuf::from(c);
        if d.join("PROT.DAT").exists() && d.join("CDNAME.TXT").exists() {
            return SceneHost::open_extracted(&d).ok();
        }
    }
    SceneHost::open_disc(PathBuf::from(disc)).ok()
}

/// The thickets: every prop whose bind record is `P0[1]`, by anchor.
fn thickets(host: &SceneHost) -> Vec<(u8, u8)> {
    host.world
        .props
        .bank
        .props
        .iter()
        .filter(|(_, p)| p.record == 1)
        .map(|(&a, _)| a)
        .collect()
}

#[test]
fn the_weed_hammer_hides_the_crystal_grass_it_cuts_or_skip() {
    let Some(mut host) = open_host() else {
        return;
    };
    host.world.stage_picker_entry("vozz", false);
    host.enter_field_scene("vozz", 0).expect("enter vozz");
    for _ in 0..30 {
        host.tick().expect("tick");
    }
    let grass = thickets(&host);
    assert!(grass.len() >= 2, "vozz binds several thickets to P0[1]");
    let before = host.world.hidden_object_records();

    // Holding the Weed Hammer, cut the first thicket.
    host.world.system_flag_set(0x079);
    let anchor = grass[0];
    assert!(
        host.world.start_prop_interaction(anchor),
        "the examine starts"
    );
    let mut ended = false;
    for _ in 0..600 {
        host.world.set_pad(0x0020); // confirm through any panel
        host.tick().expect("tick");
        host.world.set_pad(0);
        host.tick().expect("tick");
        if host.world.dialog.inline.is_none() {
            ended = true;
            break;
        }
    }
    assert!(ended, "the cut ran to its park");

    let hide = i32::from(legaia_engine_field::man_field_scripts::FIELD_OFFMAP_HIDE_XZ);
    assert_eq!(
        host.world.props.bank.props[&anchor].world,
        (hide, hide),
        "the cut thicket's collision is on the hide tile"
    );
    let after = host.world.hidden_object_records();
    let cut: Vec<usize> = after.difference(&before).copied().collect();
    assert!(
        !cut.is_empty(),
        "the cut thicket leaves the draw with its collision"
    );
    // One object leaves (its channel key and its per-object key), not the
    // record's every thicket.
    assert!(cut.len() <= 2, "only the cut thicket hides: {cut:?}");
    for &a in &grass[1..] {
        assert_ne!(
            host.world.props.bank.props[&a].world,
            (hide, hide),
            "thicket {a:?} was not examined and still blocks"
        );
    }
    // No later tick carries the collider a second time.
    for _ in 0..10 {
        host.tick().expect("tick");
    }
    assert_eq!(
        host.world.props.bank.props[&anchor].world,
        (hide, hide),
        "the collider stays on the hide tile"
    );
    eprintln!("[ran] vozz thicket at {anchor:?} cut; hidden keys {cut:?}");
}
